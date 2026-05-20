//! Shared SSE line decoder.
//!
//! It accepts fragmented byte chunks, CRLF/LF delimiters, comments, multi-line
//! `data:` fields, and emits complete events. Provider-specific modules then
//! map event data into normalized OpenAI-compatible chunks.

use crate::error::{ProviderError, ProviderResult};
use bytes::Bytes;
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
struct SseDecoderState {
    buf: Vec<u8>,
    scan_from: usize,
}

#[derive(Clone, Default)]
pub struct SseLineDecoder {
    state: Arc<Mutex<SseDecoderState>>,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, item: Result<Bytes, reqwest::Error>) -> ProviderResult<Vec<SseEvent>> {
        let bytes = item.map_err(|e| ProviderError::Network(e.to_string()))?;
        let mut state = self.state.lock();
        state.buf.extend_from_slice(&bytes);
        Ok(drain_decoder_events(&mut state))
    }
}

pub fn drain_sse_events(buf: &mut Vec<u8>) -> Vec<SseEvent> {
    let mut out = Vec::new();
    while let Some((idx, delim_len)) = find_event_boundary(buf) {
        let event_bytes: Vec<u8> = buf.drain(..idx + delim_len).collect();
        if let Some(event) = parse_event(&event_bytes[..idx]) {
            out.push(event);
        }
    }
    out
}

fn drain_decoder_events(state: &mut SseDecoderState) -> Vec<SseEvent> {
    let mut out = Vec::new();
    loop {
        let start = state.scan_from.min(state.buf.len().saturating_sub(2));
        match find_event_boundary_from(&state.buf, start) {
            Some((idx, delim_len)) => {
                let event_bytes: Vec<u8> = state.buf.drain(..idx + delim_len).collect();
                state.scan_from = 0;
                if let Some(event) = parse_event(&event_bytes[..idx]) {
                    out.push(event);
                }
            }
            None => {
                // Keep a small overlap so boundaries split across chunks are still
                // detected: `\n\n` needs one previous byte, `\r\n\r\n` needs three.
                state.scan_from = state.buf.len().saturating_sub(3);
                break;
            }
        }
    }
    out
}

fn parse_event(bytes: &[u8]) -> Option<SseEvent> {
    let s = String::from_utf8_lossy(bytes);
    let mut event = None;
    let mut data = Vec::new();

    for line in s.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim_start().to_string());
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.trim_start().to_string());
        }
    }

    if event.is_none() && data.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

fn find_event_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    find_event_boundary_from(buf, 0)
}

fn find_event_boundary_from(buf: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start.min(buf.len().saturating_sub(2));
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

pub fn sse_to_json_values<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<serde_json::Value>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    let decoder = SseLineDecoder::new();
    byte_stream.flat_map(move |item| {
        let events = match decoder.push(item) {
            Ok(events) => events,
            Err(e) => return futures::stream::iter(vec![Err(e)]),
        };
        let values = events
            .into_iter()
            .filter_map(|event| {
                let data = event.data.trim();
                if data.is_empty() || data == "[DONE]" {
                    None
                } else {
                    Some(
                        serde_json::from_str(data)
                            .map_err(|e| ProviderError::Decode(format!("sse data {data:?}: {e}"))),
                    )
                }
            })
            .collect::<Vec<_>>();
        futures::stream::iter(values)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fragmented_crlf_multiline_events() {
        let decoder = SseLineDecoder::new();
        let first = decoder
            .push(Ok(Bytes::from_static(
                b": ping\r\nevent: token\r\ndata: {\"a\":1}\r\n",
            )))
            .unwrap();
        assert!(first.is_empty());
        let second = decoder
            .push(Ok(Bytes::from_static(b"data: {\"b\":2}\r\n\r\n")))
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].event.as_deref(), Some("token"));
        assert_eq!(second[0].data, "{\"a\":1}\n{\"b\":2}");
    }

    #[test]
    fn decodes_many_small_frames() {
        let decoder = SseLineDecoder::new();
        let mut total = 0usize;

        for i in 0..2_048 {
            let frame = format!("data: {{\"i\":{i},\"delta\":\"x\"}}\n\n");
            let events = decoder.push(Ok(Bytes::from(frame))).unwrap();
            assert_eq!(events.len(), 1);
            total += events.len();
        }

        assert_eq!(total, 2_048);
    }

    #[test]
    fn decodes_large_frame() {
        let decoder = SseLineDecoder::new();
        let payload = "x".repeat(128 * 1024);
        let frame = format!("event: chunk\ndata: {{\"blob\":\"{payload}\"}}\n\n");
        let events = decoder.push(Ok(Bytes::from(frame))).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("chunk"));
        assert_eq!(
            events[0].data.len(),
            payload.len() + "{\"blob\":\"\"}".len()
        );
        assert!(events[0].data.ends_with("\"}"));
    }

    #[test]
    fn decodes_fragmented_utf8_byte_by_byte() {
        let decoder = SseLineDecoder::new();
        let frame = "data: {\"delta\":\"星辰🚀\"}\n\n";
        let mut events = Vec::new();

        for byte in frame.as_bytes() {
            events.extend(decoder.push(Ok(Bytes::copy_from_slice(&[*byte]))).unwrap());
        }

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"delta\":\"星辰🚀\"}");
    }

    #[test]
    fn does_not_emit_incomplete_event_before_long_connection_cancel() {
        let decoder = SseLineDecoder::new();

        for i in 0..512 {
            let partial = format!("data: {{\"i\":{i},\"delta\":\"still-open\"}}\n");
            let events = decoder.push(Ok(Bytes::from(partial))).unwrap();
            assert!(
                events.is_empty(),
                "decoder must not emit an event before blank-line boundary"
            );
        }

        drop(decoder);
    }
}
