//! custom_provider 内部 helper：模板渲染 / JSON 路径 / header 插入 / 错误判定。
//! 从 mod.rs 拆出（M1.3 T3.2 收尾）。

use crate::error::{ProviderError, ProviderResult};
use crate::types::*;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

pub(super) fn request_context(req: &ChatRequest, extra: &Value) -> Value {
    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(ChatMessage::content_text)
        .unwrap_or_default();
    let mut ctx = json!({
        "request": req,
        "extra": req.extra,
        "model": req.model,
        "messages": req.messages,
        "last_user_message": last_user,
        "stream": req.stream,
        "temperature": req.temperature,
        "top_p": req.top_p,
        "max_tokens": req.max_tokens,
        "tools": req.tools.clone(),
        "tool_choice": req.tool_choice.clone(),
        "metadata": req.extra.get("metadata").cloned().unwrap_or(Value::Null),
    });
    if let (Some(dst), Some(src)) = (ctx.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    ctx
}

pub(super) fn embedding_request_context(req: &EmbeddingRequest, extra: &Value) -> Value {
    let input_texts = match &req.input {
        EmbeddingInput::Single(value) => vec![value.clone()],
        EmbeddingInput::Multiple(values) => values.clone(),
    };
    let mut ctx = json!({
        "request": req,
        "model": req.model,
        "input": req.input,
        "input_texts": input_texts,
        "encoding_format": req.encoding_format,
        "dimensions": req.dimensions,
        "metadata": Value::Null,
        "extra": Value::Null,
    });
    if let (Some(dst), Some(src)) = (ctx.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    ctx
}

pub(super) fn render_value(template: &Value, ctx: &Value) -> Value {
    render_value_optional(template, ctx)
        .map(|rendered| rendered.value)
        .unwrap_or(Value::Null)
}

pub(super) struct RenderedValue {
    pub(super) value: Value,
    pub(super) conditional: bool,
}

pub(super) fn render_value_optional(template: &Value, ctx: &Value) -> Option<RenderedValue> {
    match template {
        Value::String(s) => {
            if let Some(path) = whole_placeholder(s) {
                let value = get_path(ctx, path)?;
                if is_empty_placeholder_value(value) {
                    None
                } else {
                    Some(RenderedValue {
                        value: value.clone(),
                        conditional: true,
                    })
                }
            } else {
                Some(RenderedValue {
                    value: Value::String(render_template_str(s, ctx)),
                    conditional: false,
                })
            }
        }
        Value::Array(arr) => {
            let mut conditional = false;
            let mut values = Vec::with_capacity(arr.len());
            for value in arr {
                match render_value_optional(value, ctx) {
                    Some(rendered) => {
                        conditional |= rendered.conditional;
                        values.push(rendered.value);
                    }
                    None => conditional = true,
                }
            }
            if conditional && values.is_empty() {
                None
            } else {
                Some(RenderedValue {
                    value: Value::Array(values),
                    conditional,
                })
            }
        }
        Value::Object(obj) => {
            let mut conditional = false;
            let mut values = serde_json::Map::new();
            for (key, value) in obj {
                match render_value_optional(value, ctx) {
                    Some(rendered) => {
                        conditional |= rendered.conditional;
                        values.insert(key.clone(), rendered.value);
                    }
                    None => conditional = true,
                }
            }
            if conditional && values.is_empty() {
                None
            } else {
                Some(RenderedValue {
                    value: Value::Object(values),
                    conditional,
                })
            }
        }
        other => Some(RenderedValue {
            value: other.clone(),
            conditional: false,
        }),
    }
}

pub(super) fn render_template(template: &Value, ctx: &Value) -> Option<String> {
    let rendered = render_value_optional(template, ctx)?.value;
    if is_empty_placeholder_value(&rendered) {
        None
    } else {
        value_to_string(&rendered)
    }
}

pub(super) fn is_empty_placeholder_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(super) fn render_template_str(template: &str, ctx: &Value) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let (head, after_start) = rest.split_at(start);
        out.push_str(head);
        let after_start = &after_start[2..];
        let Some(end) = after_start.find("}}") else {
            out.push_str("{{");
            out.push_str(after_start);
            return out;
        };
        let (expr, after_expr) = after_start.split_at(end);
        let path = expr.trim();
        if let Some(v) = get_path(ctx, path) {
            out.push_str(&value_to_string(v).unwrap_or_else(|| v.to_string()));
        }
        rest = &after_expr[2..];
    }
    out.push_str(rest);
    out
}

pub(super) fn enforce_size(name: &str, actual: usize, limit: usize) -> ProviderResult<()> {
    if actual > limit {
        return Err(ProviderError::Decode(format!(
            "{name} too large: {actual} bytes > {limit} bytes"
        )));
    }
    Ok(())
}

/// 从 `catch_unwind` 拿到的 `Box<dyn Any + Send>` payload 里提取人话信息。
/// panic payload 通常是 `&'static str` 或 `String`；其他类型显示 `<non-string panic>`.
pub(super) fn panic_message(payload: &Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic>".to_string()
}

pub(super) fn enforce_response_length_hint(
    resp: &reqwest::Response,
    limit: usize,
) -> ProviderResult<()> {
    let Some(value) = resp.headers().get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let Some(len) = value.to_str().ok().and_then(|s| s.parse::<usize>().ok()) else {
        return Ok(());
    };
    enforce_size("plugin response body", len, limit)
}

pub(super) fn insert_named_header(
    headers: &mut HeaderMap,
    name: &str,
    value: String,
) -> ProviderResult<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| ProviderError::Config(format!("invalid plugin header {name:?}: {e}")))?;
    insert_header(headers, name, value)
}

pub(super) fn insert_header(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: String,
) -> ProviderResult<()> {
    let value = HeaderValue::from_str(&value)
        .map_err(|e| ProviderError::Config(format!("invalid plugin header value: {e}")))?;
    headers.insert(name, value);
    Ok(())
}

pub(super) fn whole_placeholder(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") && trimmed.matches("{{").count() == 1 {
        Some(
            trimmed
                .trim_start_matches("{{")
                .trim_end_matches("}}")
                .trim(),
        )
    } else {
        None
    }
}

pub(super) fn slash_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub(super) fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path == "." || path == "$" {
        return Some(value);
    }
    let mut cur = value;
    for segment in path.trim_start_matches("$.").split('.') {
        if segment.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(map) => map.get(segment)?,
            Value::Array(arr) => arr.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub(super) fn set_path(value: &mut Value, path: &str, new_value: Value) {
    let segments: Vec<_> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        *value = new_value;
        return;
    }
    let mut cur = value;
    for segment in &segments[..segments.len() - 1] {
        if !cur.is_object() {
            *cur = json!({});
        }
        cur = cur
            .as_object_mut()
            .expect("object just created")
            .entry((*segment).to_string())
            .or_insert_with(|| json!({}));
    }
    if !cur.is_object() {
        *cur = json!({});
    }
    cur.as_object_mut()
        .expect("object just created")
        .insert(segments[segments.len() - 1].to_string(), new_value);
}

pub(super) fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

pub(super) fn value_to_u16(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|value| u16::try_from(value).ok()),
        Value::String(s) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
}

pub(super) fn value_to_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

pub(super) fn parse_embedding_vector(value: &Value) -> ProviderResult<Vec<f32>> {
    let arr = value
        .as_array()
        .ok_or_else(|| ProviderError::Decode("plugin embedding value is not an array".into()))?;
    arr.iter()
        .map(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
                .map(|n| n as f32)
                .ok_or_else(|| {
                    ProviderError::Decode("plugin embedding vector contains non-number".into())
                })
        })
        .collect()
}

pub(super) fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000))
}

pub(super) fn status_in(status: u16, statuses: &[u16]) -> bool {
    statuses.contains(&status)
}

pub(super) fn code_matches(code: Option<&str>, values: &[&str]) -> bool {
    let Some(code) = code else {
        return false;
    };
    values.iter().any(|value| code.eq_ignore_ascii_case(value))
}

pub(super) fn message_contains_any(message: &str, needles: &[&str]) -> bool {
    let message = message.to_ascii_lowercase();
    needles.iter().any(|needle| message.contains(needle))
}

pub(super) fn map_role(s: &str) -> Option<Role> {
    match s {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" | "bot" | "model" => Some(Role::Assistant),
        "tool" => Some(Role::Tool),
        _ => None,
    }
}

pub(super) fn map_finish_reason(s: &str) -> Option<FinishReason> {
    match s {
        "stop" | "stopped" | "stop_sequence" | "end_turn" | "done" => Some(FinishReason::Stop),
        "length" | "max_tokens" => Some(FinishReason::Length),
        "tool_calls" | "tool_use" => Some(FinishReason::ToolCalls),
        "content_filter" | "safety" => Some(FinishReason::ContentFilter),
        "" => None,
        _ => Some(FinishReason::Other),
    }
}
