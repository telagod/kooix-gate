//! Plugin SSE replay / normalizer — 把任意 vendor SSE 帧归一为 OpenAI-compatible `ChatStreamChunk`。
//!
//! 入口：`replay_plugin_sse` （供 admin / kgctl plugin replay 用）
//! 核心：`StreamMapper` + `map_plugin_event`
//! Helper：event_name_matches / vendor_done_object / json_values_equal / merge_usage_state / merge_reasoning_content

use super::{enforce_size, map_finish_reason, map_role, value_to_string};
use crate::error::{ProviderError, ProviderResult};
use crate::plugin_manifest::{
    DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES, PluginManifest,
};
use crate::plugin_preset::{StreamManifest, eval_path_value};
use crate::sse::{SseEvent, SseLineDecoder};
use crate::types::*;
use futures::stream::StreamExt;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct StreamMapper {
    pub(super) stream: StreamManifest,
    pub(super) fallback_id: String,
    pub(super) fallback_model: String,
    pub(super) max_response_bytes: usize,
    pub(super) max_sse_event_bytes: usize,
}

pub(super) fn normalize_plugin_sse<S>(
    byte_stream: S,
    mapper: StreamMapper,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    let decoder = SseLineDecoder::new();
    let state = Arc::new(parking_lot::Mutex::new(StreamState {
        id: mapper.fallback_id.clone(),
        model: mapper.fallback_model.clone(),
        response_bytes: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
    }));

    byte_stream.flat_map(move |item| {
        let state = state.clone();
        let mapper = mapper.clone();
        let item = match item {
            Ok(bytes) => {
                {
                    let mut st = state.lock();
                    st.response_bytes = st.response_bytes.saturating_add(bytes.len());
                    if st.response_bytes > mapper.max_response_bytes {
                        return futures::stream::iter(vec![Err(ProviderError::Decode(format!(
                            "plugin response body too large: more than {} bytes",
                            mapper.max_response_bytes
                        )))]);
                    }
                }
                Ok(bytes)
            }
            Err(e) => Err(e),
        };
        let events = match decoder.push(item) {
            Ok(events) => events,
            Err(e) => return futures::stream::iter(vec![Err(e)]),
        };
        let chunks = events
            .into_iter()
            .filter_map(|event| map_plugin_event(event, &mapper, &state))
            .collect::<Vec<_>>();
        futures::stream::iter(chunks)
    })
}

pub fn replay_plugin_sse(
    manifest: Value,
    base_url: &str,
    raw_sse: impl AsRef<[u8]>,
    fallback_model: &str,
) -> ProviderResult<Vec<ChatStreamChunk>> {
    let manifest = PluginManifest::from_value(manifest, base_url)?;
    replay_plugin_sse_with_manifest(manifest.stream, raw_sse, fallback_model)
}

pub(super) fn replay_plugin_sse_with_manifest(
    stream: StreamManifest,
    raw_sse: impl AsRef<[u8]>,
    fallback_model: &str,
) -> ProviderResult<Vec<ChatStreamChunk>> {
    let mut buf = raw_sse.as_ref().to_vec();
    let events = crate::sse::drain_sse_events(&mut buf);
    if !buf.is_empty() {
        return Err(ProviderError::Decode(
            "raw SSE sample ended with an incomplete event; add a blank line terminator".into(),
        ));
    }
    let mapper = StreamMapper {
        stream,
        fallback_id: format!("chatcmpl-{}", uuid::Uuid::now_v7()),
        fallback_model: fallback_model.to_string(),
        max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
    };
    let state = parking_lot::Mutex::new(StreamState {
        id: mapper.fallback_id.clone(),
        model: mapper.fallback_model.clone(),
        response_bytes: raw_sse.as_ref().len(),
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
    });

    let mut chunks = Vec::new();
    for event in events {
        if let Some(mapped) = map_plugin_event(event, &mapper, &state) {
            chunks.push(mapped?);
        }
    }
    Ok(chunks)
}

#[derive(Debug)]
pub(super) struct StreamState {
    id: String,
    model: String,
    response_bytes: usize,
    prompt_tokens: u32,
    completion_tokens: u32,
    cached_tokens: u32,
}

pub(super) fn map_plugin_event(
    event: SseEvent,
    mapper: &StreamMapper,
    state: &parking_lot::Mutex<StreamState>,
) -> Option<ProviderResult<ChatStreamChunk>> {
    if event_name_matches(event.event.as_deref(), &mapper.stream.ignore_events) {
        return None;
    }
    if event_name_matches(event.event.as_deref(), &mapper.stream.done_events) {
        return None;
    }
    let raw = event.data.trim();
    if raw.is_empty() || raw == ":" {
        return None;
    }
    if let Err(e) = enforce_size("plugin SSE event", raw.len(), mapper.max_sse_event_bytes) {
        return Some(Err(e));
    }
    let done_tokens = if mapper.stream.done.is_empty() {
        vec!["[DONE]".to_string()]
    } else {
        mapper.stream.done.clone()
    };
    if done_tokens.iter().any(|d| d == raw) {
        return None;
    }

    if mapper.stream.is_openai_compatible() {
        let value: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => return Some(Err(ProviderError::Decode(format!("sse data {raw:?}: {e}")))),
        };
        return Some(serde_json::from_value(value).map_err(ProviderError::from));
    }

    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => Value::String(raw.to_string()),
    };

    let event_value = mapper
        .stream
        .event_path
        .as_deref()
        .and_then(|p| eval_path_value(&value, p).ok().flatten())
        .unwrap_or_else(|| value.clone());

    let is_vendor_done = vendor_done_object(&event_value, &mapper.stream);

    let mut st = state.lock();
    if let Some(id) = mapper
        .stream
        .id_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
    {
        st.id = id;
    }
    if let Some(model) = mapper
        .stream
        .model_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
    {
        st.model = model;
    }
    let content = mapper
        .stream
        .content_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string);
    let role = mapper
        .stream
        .role_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
        .and_then(|s| map_role(&s));
    let tool_calls = match mapper
        .stream
        .tool_calls_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
    {
        Some(value) => match serde_json::from_value::<Vec<ToolCallDelta>>(value) {
            Ok(tool_calls) => Some(tool_calls),
            Err(e) => {
                return Some(Err(ProviderError::Decode(format!(
                    "plugin stream tool_calls_path is not a valid tool call delta array: {e}"
                ))));
            }
        },
        None => None,
    };
    let finish_reason = mapper
        .stream
        .finish_reason_path
        .as_deref()
        .and_then(|p| eval_path_value(&event_value, p).ok().flatten())
        .as_ref()
        .and_then(value_to_string)
        .and_then(|s| map_finish_reason(&s));
    let usage = match mapper.stream.usage.extract_optional(&event_value) {
        Ok(Some(usage)) => {
            let emit = mapper
                .stream
                .usage
                .should_emit_stream_usage(&usage, finish_reason);
            let merged = merge_usage_state(usage.usage, &mut st);
            emit.then_some(merged)
        }
        Ok(None) => None,
        Err(e) => return Some(Err(e)),
    };

    if is_vendor_done
        && role.is_none()
        && content.is_none()
        && tool_calls.is_none()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return None;
    }

    if role.is_none()
        && content.is_none()
        && tool_calls.is_none()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return None;
    }

    Some(Ok(ChatStreamChunk {
        id: st.id.clone(),
        model: st.model.clone(),
        choices: vec![ChatStreamChoice {
            index: 0,
            delta: ChatDelta {
                role,
                content,
                tool_calls,
            },
            finish_reason,
        }],
        usage,
    }))
}

pub(super) fn event_name_matches(event: Option<&str>, patterns: &[String]) -> bool {
    let Some(event) = event.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    patterns.iter().any(|p| p.trim() == event)
}

pub(super) fn vendor_done_object(event_value: &Value, stream: &StreamManifest) -> bool {
    let Some(path) = stream.done_path.as_deref() else {
        return false;
    };
    let Some(value) = eval_path_value(event_value, path).ok().flatten() else {
        return false;
    };
    stream
        .done_values
        .iter()
        .any(|done| json_values_equal(&value, done))
}

pub(super) fn json_values_equal(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    match (left, right) {
        (Value::String(a), Value::String(b)) => a == b,
        (Value::String(a), other) => value_to_string(other).is_some_and(|b| a == &b),
        (other, Value::String(b)) => value_to_string(other).is_some_and(|a| &a == b),
        _ => false,
    }
}

pub(super) fn merge_usage_state(usage: Usage, state: &mut StreamState) -> Usage {
    if usage.prompt_tokens > 0 {
        state.prompt_tokens = usage.prompt_tokens;
    }
    if usage.completion_tokens > 0 {
        state.completion_tokens = usage.completion_tokens;
    }
    if usage.cached_tokens > 0 {
        state.cached_tokens = usage.cached_tokens;
    }

    let prompt_tokens = if usage.prompt_tokens > 0 {
        usage.prompt_tokens
    } else {
        state.prompt_tokens
    };
    let completion_tokens = if usage.completion_tokens > 0 {
        usage.completion_tokens
    } else {
        state.completion_tokens
    };
    let cached_tokens = if usage.cached_tokens > 0 {
        usage.cached_tokens
    } else {
        state.cached_tokens
    };
    let inferred_total = prompt_tokens + completion_tokens;
    let total_tokens = usage.total_tokens.max(inferred_total);

    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cached_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        audio_tokens: usage.audio_tokens,
        accepted_prediction_tokens: usage.accepted_prediction_tokens,
        rejected_prediction_tokens: usage.rejected_prediction_tokens,
        image_units: usage.image_units,
        audio_seconds: usage.audio_seconds,
        raw: usage.raw,
    }
}

pub(super) fn merge_reasoning_content(content: String, reasoning: Option<String>) -> String {
    match (reasoning, content) {
        (Some(reasoning), content) if !reasoning.is_empty() && !content.is_empty() => {
            format!("{reasoning}\n{content}")
        }
        (Some(reasoning), _) if !reasoning.is_empty() => reasoning,
        (_, content) => content,
    }
}
