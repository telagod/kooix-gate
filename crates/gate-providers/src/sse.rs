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

#[derive(Clone, Default)]
pub struct SseLineDecoder {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, item: Result<Bytes, reqwest::Error>) -> ProviderResult<Vec<SseEvent>> {
        let bytes = item.map_err(|e| ProviderError::Network(e.to_string()))?;
        let mut buf = self.buf.lock();
        buf.extend_from_slice(&bytes);
        Ok(drain_sse_events(&mut buf))
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
    let mut i = 0;
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
}
