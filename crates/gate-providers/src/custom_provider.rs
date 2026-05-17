//! Runtime-configurable HTTP provider plugin.
//!
//! A `provider_type` of `plugin` / `custom` / `http` uses channel `model_mapping`
//! as the plugin manifest. The manifest can reshape requests, map strange JSON
//! responses, and normalize arbitrary SSE frames back into OpenAI-compatible chunks.

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::openai::check_status;
use crate::sse::{SseEvent, SseLineDecoder};
use crate::types::*;
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

const DEFAULT_CHAT_PATH: &str = "/chat/completions";

#[derive(Clone)]
pub struct CustomHttpProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    manifest: Arc<PluginManifest>,
}

impl CustomHttpProvider {
    pub fn new_with_opts(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        manifest: Value,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let manifest = PluginManifest::from_value(manifest)?;
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            manifest: Arc::new(manifest),
        })
    }

    fn endpoint_url(&self) -> String {
        let path = self
            .manifest
            .request
            .chat_path
            .as_deref()
            .unwrap_or(DEFAULT_CHAT_PATH);
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        format!("{}{}", self.base_url, slash_path(path))
    }

    fn request_headers(&self) -> ProviderResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        for (k, v) in &self.manifest.request.headers {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| ProviderError::Config(format!("invalid plugin header {k:?}: {e}")))?;
            let rendered = render_template(v, &json!({ "api_key": self.api_key }));
            let value = HeaderValue::from_str(&rendered).map_err(|e| {
                ProviderError::Config(format!("invalid plugin header value for {k}: {e}"))
            })?;
            headers.insert(name, value);
        }

        if !self.api_key.is_empty() && !headers.contains_key(reqwest::header::AUTHORIZATION) {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| ProviderError::Config(format!("invalid authorization: {e}")))?,
            );
        }
        Ok(headers)
    }

    #[cfg(test)]
    fn build_body(&self, req: &ChatRequest) -> ProviderResult<Value> {
        self.build_body_with_extra(req, &json!({}))
    }

    fn build_body_with_extra(&self, req: &ChatRequest, extra: &Value) -> ProviderResult<Value> {
        let ctx = request_context(req, extra);
        let mut body = match &self.manifest.request.body {
            Some(template) => render_value(template, &ctx),
            None => serde_json::to_value(req)?,
        };

        if self.manifest.request.force_stream_field {
            set_path(&mut body, "stream", Value::Bool(req.stream));
        }
        Ok(body)
    }

    fn parse_chat_response(
        &self,
        value: Value,
        requested_model: &str,
    ) -> ProviderResult<ChatResponse> {
        if self.manifest.response.openai_compatible {
            return Ok(serde_json::from_value(value)?);
        }

        let id = self
            .manifest
            .response
            .id_path
            .as_deref()
            .and_then(|p| get_path(&value, p))
            .and_then(value_to_string)
            .unwrap_or_else(|| format!("chatcmpl-{}", uuid::Uuid::now_v7()));
        let model = self
            .manifest
            .response
            .model_path
            .as_deref()
            .and_then(|p| get_path(&value, p))
            .and_then(value_to_string)
            .unwrap_or_else(|| requested_model.to_string());
        let content = self
            .manifest
            .response
            .content_path
            .as_deref()
            .and_then(|p| get_path(&value, p))
            .and_then(value_to_string)
            .unwrap_or_default();
        let finish_reason = self
            .manifest
            .response
            .finish_reason_path
            .as_deref()
            .and_then(|p| get_path(&value, p))
            .and_then(value_to_string)
            .and_then(|s| map_finish_reason(&s));
        let usage = self.manifest.response.usage.extract(&value);

        Ok(ChatResponse {
            id,
            model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, content),
                finish_reason,
            }],
            usage,
        })
    }
}

#[async_trait]
impl Provider for CustomHttpProvider {
    fn name(&self) -> &'static str {
        "plugin"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let body = self.build_body_with_extra(&req, &json!({ "api_key": self.api_key }))?;
        let resp = self
            .client
            .post(self.endpoint_url())
            .headers(self.request_headers()?)
            .json(&body)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let body: Value = resp.json().await?;
        self.parse_chat_response(body, &req.model)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        let body = self.build_body_with_extra(&req, &json!({ "api_key": self.api_key }))?;
        let resp = self
            .client
            .post(self.endpoint_url())
            .headers(self.request_headers()?)
            .json(&body)
            .send()
            .await?;
        check_status(&resp)?;

        let mapper = StreamMapper {
            stream: self.manifest.stream.clone(),
            fallback_id: format!("chatcmpl-{}", uuid::Uuid::now_v7()),
            fallback_model: req.model.clone(),
        };
        Ok(normalize_plugin_sse(resp.bytes_stream(), mapper).boxed())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct PluginManifest {
    request: RequestManifest,
    response: ResponseManifest,
    stream: StreamManifest,
}

impl PluginManifest {
    fn from_value(value: Value) -> ProviderResult<Self> {
        let manifest_value = value
            .get("plugin")
            .or_else(|| value.get("adapter"))
            .or_else(|| value.get("protocol"))
            .cloned()
            .unwrap_or(value);

        if manifest_value.is_null() || manifest_value == json!({}) {
            return Ok(Self::default());
        }
        serde_json::from_value(manifest_value)
            .map_err(|e| ProviderError::Config(format!("invalid plugin manifest: {e}")))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct RequestManifest {
    chat_path: Option<String>,
    headers: Map<String, Value>,
    body: Option<Value>,
    force_stream_field: bool,
}

impl Default for RequestManifest {
    fn default() -> Self {
        Self {
            chat_path: Some(DEFAULT_CHAT_PATH.to_string()),
            headers: Map::new(),
            body: None,
            force_stream_field: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct ResponseManifest {
    openai_compatible: bool,
    id_path: Option<String>,
    model_path: Option<String>,
    content_path: Option<String>,
    finish_reason_path: Option<String>,
    usage: UsageManifest,
}

impl Default for ResponseManifest {
    fn default() -> Self {
        Self {
            openai_compatible: true,
            id_path: None,
            model_path: None,
            content_path: None,
            finish_reason_path: None,
            usage: UsageManifest::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct StreamManifest {
    openai_compatible: bool,
    event_path: Option<String>,
    done: Vec<String>,
    id_path: Option<String>,
    model_path: Option<String>,
    role_path: Option<String>,
    content_path: Option<String>,
    finish_reason_path: Option<String>,
    usage: UsageManifest,
}

impl Default for StreamManifest {
    fn default() -> Self {
        Self {
            openai_compatible: true,
            event_path: None,
            done: Vec::new(),
            id_path: None,
            model_path: None,
            role_path: None,
            content_path: None,
            finish_reason_path: None,
            usage: UsageManifest::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct UsageManifest {
    prompt_tokens_path: Option<String>,
    completion_tokens_path: Option<String>,
    total_tokens_path: Option<String>,
    cached_tokens_path: Option<String>,
}

impl Default for UsageManifest {
    fn default() -> Self {
        Self {
            prompt_tokens_path: Some("usage.prompt_tokens".to_string()),
            completion_tokens_path: Some("usage.completion_tokens".to_string()),
            total_tokens_path: Some("usage.total_tokens".to_string()),
            cached_tokens_path: Some("usage.cached_tokens".to_string()),
        }
    }
}

impl UsageManifest {
    fn extract(&self, value: &Value) -> Usage {
        let prompt = self
            .prompt_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_default();
        let completion = self
            .completion_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_default();
        let total = self
            .total_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_else(|| prompt + completion);
        let cached = self
            .cached_tokens_path
            .as_deref()
            .and_then(|p| get_path(value, p))
            .and_then(value_to_u32)
            .unwrap_or_default();

        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: total,
            cached_tokens: cached,
        }
    }

    fn extract_optional(&self, value: &Value) -> Option<Usage> {
        let usage = self.extract(value);
        (usage.prompt_tokens > 0 || usage.completion_tokens > 0 || usage.total_tokens > 0)
            .then_some(usage)
    }
}

#[derive(Clone)]
struct StreamMapper {
    stream: StreamManifest,
    fallback_id: String,
    fallback_model: String,
}

fn normalize_plugin_sse<S>(
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
    }));

    byte_stream.flat_map(move |item| {
        let state = state.clone();
        let mapper = mapper.clone();
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

#[derive(Debug)]
struct StreamState {
    id: String,
    model: String,
}

fn map_plugin_event(
    event: SseEvent,
    mapper: &StreamMapper,
    state: &parking_lot::Mutex<StreamState>,
) -> Option<ProviderResult<ChatStreamChunk>> {
    let raw = event.data.trim();
    if raw.is_empty() || raw == ":" {
        return None;
    }
    let done_tokens = if mapper.stream.done.is_empty() {
        vec!["[DONE]".to_string()]
    } else {
        mapper.stream.done.clone()
    };
    if done_tokens.iter().any(|d| d == raw) {
        return None;
    }

    if mapper.stream.openai_compatible {
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
        .and_then(|p| get_path(&value, p))
        .unwrap_or(&value);

    let mut st = state.lock();
    if let Some(id) = mapper
        .stream
        .id_path
        .as_deref()
        .and_then(|p| get_path(event_value, p))
        .and_then(value_to_string)
    {
        st.id = id;
    }
    if let Some(model) = mapper
        .stream
        .model_path
        .as_deref()
        .and_then(|p| get_path(event_value, p))
        .and_then(value_to_string)
    {
        st.model = model;
    }
    let content = mapper
        .stream
        .content_path
        .as_deref()
        .and_then(|p| get_path(event_value, p))
        .and_then(value_to_string);
    let role = mapper
        .stream
        .role_path
        .as_deref()
        .and_then(|p| get_path(event_value, p))
        .and_then(value_to_string)
        .and_then(|s| map_role(&s));
    let finish_reason = mapper
        .stream
        .finish_reason_path
        .as_deref()
        .and_then(|p| get_path(event_value, p))
        .and_then(value_to_string)
        .and_then(|s| map_finish_reason(&s));
    let usage = mapper.stream.usage.extract_optional(event_value);

    if role.is_none() && content.is_none() && finish_reason.is_none() && usage.is_none() {
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
                tool_calls: None,
            },
            finish_reason,
        }],
        usage,
    }))
}

fn request_context(req: &ChatRequest, extra: &Value) -> Value {
    let last_user = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(ChatMessage::content_text)
        .unwrap_or_default();
    let mut ctx = json!({
        "request": req,
        "model": req.model,
        "messages": req.messages,
        "last_user_message": last_user,
        "stream": req.stream,
        "temperature": req.temperature,
        "top_p": req.top_p,
        "max_tokens": req.max_tokens,
    });
    if let (Some(dst), Some(src)) = (ctx.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    ctx
}

fn render_value(template: &Value, ctx: &Value) -> Value {
    match template {
        Value::String(s) => {
            if let Some(path) = whole_placeholder(s) {
                get_path(ctx, path).cloned().unwrap_or(Value::Null)
            } else {
                Value::String(render_template_str(s, ctx))
            }
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render_value(v, ctx)).collect()),
        Value::Object(obj) => Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), render_value(v, ctx)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn render_template(template: &Value, ctx: &Value) -> String {
    match template {
        Value::String(s) => render_template_str(s, ctx),
        other => value_to_string(other).unwrap_or_default(),
    }
}

fn render_template_str(template: &str, ctx: &Value) -> String {
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

fn whole_placeholder(s: &str) -> Option<&str> {
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

fn slash_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
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

fn set_path(value: &mut Value, path: &str, new_value: Value) {
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

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn value_to_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse::<u32>().ok()))
}

fn map_role(s: &str) -> Option<Role> {
    match s {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" | "bot" | "model" => Some(Role::Assistant),
        "tool" => Some(Role::Tool),
        _ => None,
    }
}

fn map_finish_reason(s: &str) -> Option<FinishReason> {
    match s {
        "stop" | "stopped" | "end_turn" | "done" => Some(FinishReason::Stop),
        "length" | "max_tokens" => Some(FinishReason::Length),
        "tool_calls" | "tool_use" => Some(FinishReason::ToolCalls),
        "content_filter" | "safety" => Some(FinishReason::ContentFilter),
        "" => None,
        _ => Some(FinishReason::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn make_req(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "odd-model".into(),
            messages: vec![ChatMessage::text(Role::User, "Hi plugin")],
            max_tokens: Some(16),
            stream,
            ..Default::default()
        }
    }

    #[test]
    fn request_template_preserves_native_json_values() {
        let manifest = json!({
            "request": {
                "body": {
                    "m": "{{model}}",
                    "prompt": "{{last_user_message}}",
                    "streaming": "{{stream}}",
                    "limit": "{{max_tokens}}"
                }
            }
        });
        let provider = CustomHttpProvider::new_with_opts(
            "http://x",
            "k",
            manifest,
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let body = provider.build_body(&make_req(true)).unwrap();
        assert_eq!(body["m"], "odd-model");
        assert_eq!(body["prompt"], "Hi plugin");
        assert_eq!(body["streaming"], true);
        assert_eq!(body["limit"], 16);
    }

    #[tokio::test]
    async fn maps_weird_sse_frames_to_openai_chunks() {
        let manifest = PluginManifest::from_value(json!({
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "id_path": "rid",
                "model_path": "model_name",
                "role_path": "speaker",
                "content_path": "token",
                "finish_reason_path": "reason",
                "done": ["EOF"],
                "usage": {
                    "prompt_tokens_path": "usage.in",
                    "completion_tokens_path": "usage.out"
                }
            }
        }))
        .unwrap();
        let mapper = StreamMapper {
            stream: manifest.stream,
            fallback_id: "fallback".into(),
            fallback_model: "odd-model".into(),
        };
        let sse = concat!(
            "event: token\n",
            "data: {\"payload\":{\"rid\":\"r1\",\"model_name\":\"m1\",\"speaker\":\"assistant\"}}\n\n",
            "data: {\"payload\":{\"token\":\"he\"}}\n\n",
            "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
            "data: {\"payload\":{\"reason\":\"done\",\"usage\":{\"in\":3,\"out\":2}}}\n\n",
            "data: EOF\n\n"
        );
        let stream =
            futures::stream::once(async move { Ok(bytes::Bytes::from_static(sse.as_bytes())) });
        let chunks: Vec<_> = normalize_plugin_sse(stream, mapper)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("he"));
        assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("llo"));
        assert_eq!(chunks[3].choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(chunks[3].usage.unwrap().total_tokens, 5);
    }
}
