//! Runtime-configurable HTTP provider plugin.
//!
//! A `provider_type` of `plugin` / `custom` / `http` uses channel `model_mapping`
//! as the plugin manifest. The manifest can reshape requests, map strange JSON
//! responses, and normalize arbitrary SSE frames back into OpenAI-compatible chunks.

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::openai::check_status;
use crate::plugin_manifest::{AuthStrategy, DEFAULT_CHAT_PATH, PluginManifest};
use crate::plugin_preset::{StreamManifest, adapt_chat_request};
use crate::sse::{SseEvent, SseLineDecoder};
use crate::types::*;
use async_trait::async_trait;
use base64::Engine as _;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Url;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};
use std::net::IpAddr;
use std::sync::Arc;

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
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let manifest = PluginManifest::from_value(manifest, &base_url)?;
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            base_url,
            api_key: api_key.into(),
            manifest: Arc::new(manifest),
        })
    }

    fn endpoint_url_for(&self, req: &ChatRequest) -> ProviderResult<String> {
        let ctx = self.request_context_for(req)?;
        self.endpoint_url_with_context(&ctx)
    }

    fn endpoint_url_with_context(&self, ctx: &Value) -> ProviderResult<String> {
        let path = self
            .manifest
            .request
            .path
            .as_deref()
            .unwrap_or(DEFAULT_CHAT_PATH);
        let rendered = render_template_str(path, ctx);
        if rendered.starts_with("http://") || rendered.starts_with("https://") {
            if !self.manifest.security.allow_absolute_chat_path {
                return Err(ProviderError::Config(
                    "plugin request.chat_path must be relative; absolute URLs are disabled by default"
                        .into(),
                ));
            }
            validate_http_endpoint(&rendered, true)?;
            return self.url_with_query(rendered, ctx);
        }
        let endpoint = format!("{}{}", self.base_url, slash_path(&rendered));
        validate_http_endpoint(&endpoint, false)?;
        self.url_with_query(endpoint, ctx)
    }

    fn url_with_query(&self, endpoint: String, ctx: &Value) -> ProviderResult<String> {
        let mut url = Url::parse(&endpoint)
            .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in &self.manifest.request.query {
                if value.is_null() {
                    continue;
                }
                pairs.append_pair(name, &render_template(value, ctx));
            }
            if self.manifest.auth.strategy == AuthStrategy::ApiKeyQuery
                && let Some(name) = self.manifest.auth.query_name()
            {
                pairs.append_pair(
                    name,
                    &self.secret_for_slot(self.manifest.auth.secret_slot()),
                );
            }
        }
        Ok(url.to_string())
    }

    fn request_headers_for(&self, req: &ChatRequest) -> ProviderResult<HeaderMap> {
        let ctx = self.request_context_for(req)?;
        self.request_headers_with_context(&ctx)
    }

    fn request_headers_with_context(&self, ctx: &Value) -> ProviderResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        self.apply_auth_headers(&mut headers, ctx)?;
        for (k, v) in &self.manifest.request.headers {
            if v.is_null() {
                continue;
            }
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| ProviderError::Config(format!("invalid plugin header {k:?}: {e}")))?;
            let rendered = render_template(v, ctx);
            let value = HeaderValue::from_str(&rendered).map_err(|e| {
                ProviderError::Config(format!("invalid plugin header value for {k}: {e}"))
            })?;
            headers.insert(name, value);
        }

        Ok(headers)
    }

    fn plugin_context(&self) -> Value {
        json!({
            "api_key": self.api_key,
            "aws_secret_key": std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
        })
    }

    fn apply_auth_headers(&self, headers: &mut HeaderMap, ctx: &Value) -> ProviderResult<()> {
        match self.manifest.auth.strategy {
            AuthStrategy::Bearer => {
                if !self.api_key.is_empty() {
                    insert_header(
                        headers,
                        reqwest::header::AUTHORIZATION,
                        format!(
                            "Bearer {}",
                            self.secret_for_slot(self.manifest.auth.secret_slot())
                        ),
                    )?;
                }
            }
            AuthStrategy::ApiKeyHeader => {
                if let Some(name) = self.manifest.auth.header_name() {
                    insert_named_header(
                        headers,
                        name,
                        self.secret_for_slot(self.manifest.auth.secret_slot()),
                    )?;
                }
            }
            AuthStrategy::Basic => {
                let username = self
                    .manifest
                    .auth
                    .username_slot()
                    .map(|slot| self.secret_for_slot(slot))
                    .unwrap_or_default();
                let password = self
                    .manifest
                    .auth
                    .password_slot()
                    .map(|slot| self.secret_for_slot(slot))
                    .unwrap_or_else(|| self.secret_for_slot(self.manifest.auth.secret_slot()));
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{username}:{password}"));
                insert_header(
                    headers,
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {encoded}"),
                )?;
            }
            AuthStrategy::CustomHeaders => {
                for (name, value) in &self.manifest.auth.headers {
                    if value.is_null() {
                        continue;
                    }
                    insert_named_header(headers, name, render_template(value, ctx))?;
                }
            }
            AuthStrategy::ApiKeyQuery | AuthStrategy::None => {}
        }
        Ok(())
    }

    fn secret_for_slot(&self, slot: &str) -> String {
        match slot {
            "primary" | "api_key" | "" => self.api_key.clone(),
            "aws_secret_key" => std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            other => {
                let env_key = format!(
                    "KOOIX_PLUGIN_SECRET_{}",
                    other
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() {
                                c.to_ascii_uppercase()
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>()
                );
                std::env::var(env_key).unwrap_or_default()
            }
        }
    }

    fn request_context_for(&self, req: &ChatRequest) -> ProviderResult<Value> {
        let effective_req = adapt_chat_request(req, self.manifest.preset.adapter)?;
        Ok(request_context(&effective_req, &self.plugin_context()))
    }

    #[cfg(test)]
    fn build_body(&self, req: &ChatRequest) -> ProviderResult<Value> {
        self.build_body_with_extra(req, &json!({}))
    }

    fn build_body_with_extra(&self, req: &ChatRequest, extra: &Value) -> ProviderResult<Value> {
        let effective_req = adapt_chat_request(req, self.manifest.preset.adapter)?;
        let ctx = request_context(&effective_req, extra);
        let mut body = match &self.manifest.request.body {
            Some(template) => render_value(template, &ctx),
            None => serde_json::to_value(&effective_req)?,
        };

        if self.manifest.request.force_stream_field {
            set_path(
                &mut body,
                &self.manifest.request.stream_path,
                Value::Bool(effective_req.stream),
            );
        }
        Ok(body)
    }

    fn request_json_body(&self, req: &ChatRequest) -> ProviderResult<Vec<u8>> {
        let body = self.build_body_with_extra(req, &self.plugin_context())?;
        let bytes = serde_json::to_vec(&body)?;
        enforce_size(
            "plugin request body",
            bytes.len(),
            self.manifest.security.max_request_bytes(),
        )?;
        Ok(bytes)
    }

    async fn limited_json_response(&self, resp: reqwest::Response) -> ProviderResult<Value> {
        let limit = self.manifest.security.max_response_bytes();
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(ProviderError::Decode(format!(
                    "plugin response body too large: more than {limit} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }

    fn parse_chat_response(
        &self,
        value: Value,
        requested_model: &str,
    ) -> ProviderResult<ChatResponse> {
        if self.manifest.response.is_openai_compatible() {
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
        let body = self.request_json_body(&req)?;
        let mut headers = self.request_headers_for(&req)?;
        headers
            .entry(reqwest::header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .post(self.endpoint_url_for(&req)?)
            .headers(headers)
            .body(body)
            .send()
            .await?;
        check_status(&resp)?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let body = self.limited_json_response(resp).await?;
        self.parse_chat_response(body, &req.model)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        let body = self.request_json_body(&req)?;
        let mut headers = self.request_headers_for(&req)?;
        headers
            .entry(reqwest::header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .post(self.endpoint_url_for(&req)?)
            .headers(headers)
            .body(body)
            .send()
            .await?;
        check_status(&resp)?;
        enforce_response_length_hint(&resp, self.manifest.security.max_response_bytes())?;

        let mapper = StreamMapper {
            stream: self.manifest.stream.clone(),
            fallback_id: format!("chatcmpl-{}", uuid::Uuid::now_v7()),
            fallback_model: req.model.clone(),
            max_response_bytes: self.manifest.security.max_response_bytes(),
            max_sse_event_bytes: self.manifest.security.max_sse_event_bytes(),
        };
        Ok(normalize_plugin_sse(resp.bytes_stream(), mapper).boxed())
    }
}

#[derive(Clone)]
struct StreamMapper {
    stream: StreamManifest,
    fallback_id: String,
    fallback_model: String,
    max_response_bytes: usize,
    max_sse_event_bytes: usize,
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

#[derive(Debug)]
struct StreamState {
    id: String,
    model: String,
    response_bytes: usize,
    prompt_tokens: u32,
    completion_tokens: u32,
    cached_tokens: u32,
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
    let usage = mapper
        .stream
        .usage
        .extract_optional(event_value)
        .and_then(|usage| {
            let emit = usage.completion_present || usage.total_present || finish_reason.is_some();
            let merged = merge_usage_state(usage.usage, &mut st);
            emit.then_some(merged)
        });

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

fn merge_usage_state(usage: Usage, state: &mut StreamState) -> Usage {
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
    }
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

fn enforce_size(name: &str, actual: usize, limit: usize) -> ProviderResult<()> {
    if actual > limit {
        return Err(ProviderError::Decode(format!(
            "{name} too large: {actual} bytes > {limit} bytes"
        )));
    }
    Ok(())
}

fn enforce_response_length_hint(resp: &reqwest::Response, limit: usize) -> ProviderResult<()> {
    let Some(value) = resp.headers().get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let Some(len) = value.to_str().ok().and_then(|s| s.parse::<usize>().ok()) else {
        return Ok(());
    };
    enforce_size("plugin response body", len, limit)
}

fn insert_named_header(headers: &mut HeaderMap, name: &str, value: String) -> ProviderResult<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| ProviderError::Config(format!("invalid plugin header {name:?}: {e}")))?;
    insert_header(headers, name, value)
}

fn insert_header(headers: &mut HeaderMap, name: HeaderName, value: String) -> ProviderResult<()> {
    let value = HeaderValue::from_str(&value)
        .map_err(|e| ProviderError::Config(format!("invalid plugin header value: {e}")))?;
    headers.insert(name, value);
    Ok(())
}

fn validate_http_endpoint(endpoint: &str, deny_internal_host: bool) -> ProviderResult<()> {
    let parsed = reqwest::Url::parse(endpoint)
        .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(ProviderError::Config(format!(
                "plugin endpoint scheme must be http/https, got {other}"
            )));
        }
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| ProviderError::Config("plugin endpoint URL missing host".into()))?;
    if deny_internal_host && is_internal_or_metadata_host(host) {
        return Err(ProviderError::Config(format!(
            "plugin absolute chat_path targets forbidden host {host}"
        )));
    }
    Ok(())
}

fn is_internal_or_metadata_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "localhost" | "metadata" | "metadata.google.internal"
    ) {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
            }
        };
    }
    false
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
        "stop" | "stopped" | "stop_sequence" | "end_turn" | "done" => Some(FinishReason::Stop),
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
    use crate::plugin_manifest::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES};
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

    #[test]
    fn openai_compatible_preset_expands_defaults_and_usage_stream_options() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.deepseek.com/v1",
            "sk-test",
            json!({ "plugin": { "preset": { "provider": "deepseek" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        assert_eq!(
            provider.endpoint_url_for(&make_req(true)).unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        let body = provider.build_body(&make_req(true)).unwrap();
        assert_eq!(body["model"], "odd-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert!(provider.manifest.response.is_openai_compatible());
        assert!(provider.manifest.stream.is_openai_compatible());
    }

    #[test]
    fn azure_preset_templates_deployment_path_and_api_key_header() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://example.openai.azure.com",
            "azure-key",
            json!({
                "plugin": {
                    "preset": { "provider": "azure_openai", "api_version": "2024-02-15-preview" }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let req = make_req(false);
        let body = provider.build_body(&req).unwrap();
        assert_eq!(body["model"], "odd-model");
        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://example.openai.azure.com/openai/deployments/odd-model/chat/completions?api-version=2024-02-15-preview"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("api-key").unwrap(), "azure-key");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn anthropic_preset_adapts_openai_request_to_messages_api() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.anthropic.com",
            "anthropic-key",
            json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let req = ChatRequest {
            model: "claude-sonnet".into(),
            messages: vec![
                ChatMessage::text(Role::System, "You are terse"),
                ChatMessage::text(Role::User, "Hi"),
            ],
            max_tokens: Some(32),
            stream: true,
            ..Default::default()
        };

        assert_eq!(
            provider.endpoint_url_for(&req).unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
        let headers = provider.request_headers_for(&req).unwrap();
        assert_eq!(headers.get("x-api-key").unwrap(), "anthropic-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(headers.get("authorization").is_none());
        let body = provider.build_body(&req).unwrap();
        assert_eq!(body["model"], "claude-sonnet");
        assert_eq!(body["max_tokens"], 32);
        assert_eq!(body["system"], "You are terse");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hi");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn plugin_auth_api_key_query_appends_secret_to_url() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com/v1",
            "query-key",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": { "strategy": "api_key_query", "query_name": "key" },
                    "request": {
                        "path": "/private/chat",
                        "query": { "model": "{{model}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let url = provider.endpoint_url_for(&make_req(false)).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/v1/private/chat?model=odd-model&key=query-key"
        );
        let headers = provider.request_headers_for(&make_req(false)).unwrap();
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn plugin_auth_basic_and_custom_headers_use_secret_slots() {
        // SAFETY: unit test only needs process env for a synthetic plugin secret.
        unsafe {
            std::env::set_var("KOOIX_PLUGIN_SECRET_USER", "basic-user");
        }

        let basic = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "basic-pass",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "basic",
                        "username_slot": "user",
                        "password_slot": "primary"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let headers = basic.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Basic YmFzaWMtdXNlcjpiYXNpYy1wYXNz"
        );

        let custom = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "primary-key",
            json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "custom_headers",
                        "headers": {
                            "X-Api-Key": "{{api_key}}",
                            "X-Model": "{{model}}"
                        }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();
        let headers = custom.request_headers_for(&make_req(false)).unwrap();
        assert_eq!(headers.get("x-api-key").unwrap(), "primary-key");
        assert_eq!(headers.get("x-model").unwrap(), "odd-model");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn preset_allows_request_overrides_without_losing_response_defaults() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://proxy.internal",
            "sk-test",
            json!({
                "plugin": {
                    "preset": { "provider": "openai_compatible" },
                    "request": {
                        "chat_path": "/custom/chat",
                        "headers": { "X-Proxy-Key": "{{api_key}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        assert_eq!(
            provider.endpoint_url_for(&make_req(false)).unwrap(),
            "https://proxy.internal/custom/chat"
        );
        assert!(provider.manifest.response.is_openai_compatible());
        assert_eq!(
            provider
                .request_headers_for(&make_req(false))
                .unwrap()
                .get("x-proxy-key")
                .unwrap(),
            "sk-test"
        );
    }

    #[test]
    fn plugin_manifest_blocks_absolute_chat_path_by_default() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "chat_path": "http://169.254.169.254/latest/meta-data"
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("absolute URLs are disabled"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_manifest_rejects_internal_absolute_url_even_when_enabled() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "chat_path": "http://localhost/admin"
                    },
                    "security": {
                        "allow_absolute_chat_path": true
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("forbidden host localhost"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_manifest_rejects_unknown_header_template_variable() {
        let err = match CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "headers": { "X-Leak": "{{request.messages}}" }
                    }
                }
            }),
            crate::ProviderOpts::default(),
        ) {
            Ok(_) => panic!("manifest should reject unsupported header template variable"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("unsupported template variable"),
            "err={err}"
        );
    }

    #[test]
    fn plugin_request_body_size_limit_is_enforced() {
        let provider = CustomHttpProvider::new_with_opts(
            "https://api.example.com",
            "sk-test",
            json!({
                "plugin": {
                    "request": {
                        "body": { "payload": "{{last_user_message}}" }
                    },
                    "security": {
                        "max_request_bytes": 32
                    }
                }
            }),
            crate::ProviderOpts::default(),
        )
        .unwrap();

        let err = provider.request_json_body(&make_req(false)).unwrap_err();
        assert!(
            err.to_string().contains("plugin request body too large"),
            "err={err}"
        );
    }

    #[tokio::test]
    async fn maps_weird_sse_frames_to_openai_chunks() {
        let manifest = PluginManifest::from_value(
            json!({
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
            }),
            "http://x",
        )
        .unwrap();
        let mapper = StreamMapper {
            stream: manifest.stream,
            fallback_id: "fallback".into(),
            fallback_model: "odd-model".into(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
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
