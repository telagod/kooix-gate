//! Anthropic Messages API 适配器 — tool calling + vision support.

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::types::*;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use serde::{Deserialize, Serialize};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

// ADR-0002 fast-path: 暴露给 custom_provider 复用，避免在两处实现 Anthropic 协议。
pub(crate) const FASTPATH_ANTHROPIC_VERSION: &str = ANTHROPIC_VERSION;

#[derive(Clone)]
pub struct AnthropicProvider {
    client: std::sync::Arc<reqwest::Client>,
    base_url: String,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        Self::new_with_opts(base_url, api_key, crate::ProviderOpts::default())
    }

    pub fn new_with_opts(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let client = crate::shared_http_client(&opts)?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        })
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

// ── Request types ────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,

    /// 0.4.67：透传 ChatRequest.extra 中尚未被识别的字段（例：`top_k`,
    /// `metadata`, `service_tier`, `thinking`, `system_prompt_caching_*`），
    /// 不让 gate 充当语法过滤器。flatten + Map<String,Value> 保证调用方写新
    /// 字段不需要 gate 升级。
    #[serde(flatten, skip_serializing_if = "serde_json::Map::is_empty")]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    r#type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: serde_json::Value,
}

fn to_anthropic_request(req: &ChatRequest) -> AnthropicRequest {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => {
                system_parts.push(msg.content_text().to_string());
            }
            Role::User | Role::Assistant => {
                let content = if let Some(tool_calls) = &msg.tool_calls {
                    let mut blocks = Vec::new();
                    let text = msg.content_text();
                    if !text.is_empty() {
                        blocks.push(AnthropicBlock::Text {
                            text: text.to_string(),
                        });
                    }
                    for tc in tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                        blocks.push(AnthropicBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            input,
                        });
                    }
                    AnthropicContent::Blocks(blocks)
                } else {
                    match &msg.content {
                        Some(MessageContent::Parts(parts)) => {
                            let blocks = parts
                                .iter()
                                .map(|p| match p {
                                    ContentPart::Text { text, .. } => {
                                        AnthropicBlock::Text { text: text.clone() }
                                    }
                                    ContentPart::ImageUrl { image_url, .. } => {
                                        if image_url.url.starts_with("data:") {
                                            let parts: Vec<&str> =
                                                image_url.url.splitn(2, ',').collect();
                                            let media_type = parts
                                                .first()
                                                .and_then(|h| h.strip_prefix("data:"))
                                                .and_then(|h| h.split(';').next())
                                                .unwrap_or("image/png")
                                                .to_string();
                                            let data = parts.get(1).unwrap_or(&"").to_string();
                                            AnthropicBlock::Image {
                                                source: AnthropicImageSource {
                                                    r#type: "base64".to_string(),
                                                    media_type,
                                                    data,
                                                },
                                            }
                                        } else {
                                            AnthropicBlock::Text {
                                                text: format!("[Image: {}]", image_url.url),
                                            }
                                        }
                                    }
                                })
                                .collect();
                            AnthropicContent::Blocks(blocks)
                        }
                        _ => AnthropicContent::Text(msg.content_text().to_string()),
                    }
                };

                messages.push(AnthropicMessage {
                    role: if msg.role == Role::User {
                        "user"
                    } else {
                        "assistant"
                    }
                    .to_string(),
                    content,
                });
            }
            Role::Tool => {
                let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![AnthropicBlock::ToolResult {
                        tool_use_id,
                        content: msg.content_text().to_string(),
                    }]),
                });
            }
        }
    }

    let tools = req.tools.as_ref().map(|t| {
        t.iter()
            .map(|td| AnthropicTool {
                name: td.function.name.clone(),
                description: td.function.description.clone(),
                input_schema: td
                    .function
                    .parameters
                    .clone()
                    .unwrap_or(serde_json::json!({"type": "object"})),
            })
            .collect()
    });

    AnthropicRequest {
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        messages,
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n"))
        },
        temperature: req.temperature,
        stream: if req.stream { Some(true) } else { None },
        tools,
        // 0.4.67: 透传 extra（top_k / metadata / service_tier / thinking 等）
        extra: req.extra.clone(),
    }
}

// ── Response types ───────────────────────────────

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicResponseBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
    /// 0.4.68: cache 写入 tokens（首次填 prompt cache 时计入，定价 ~1.25× input）。
    #[serde(default)]
    cache_creation_input_tokens: u32,
}

fn map_stop_reason(reason: Option<&str>) -> Option<FinishReason> {
    reason.map(|r| match r {
        "end_turn" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    })
}

fn from_anthropic_response(resp: AnthropicResponse) -> ChatResponse {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &resp.content {
        match block {
            AnthropicResponseBlock::Text { text } => text_parts.push(text.as_str()),
            AnthropicResponseBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    r#type: "function".to_string(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
            }
        }
    }

    let content_text = text_parts.join("");
    ChatResponse {
        id: resp.id,
        model: resp.model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: Role::Assistant,
                content: if content_text.is_empty() {
                    None
                } else {
                    Some(MessageContent::Text(content_text))
                },
                name: None,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
            },
            finish_reason: map_stop_reason(resp.stop_reason.as_deref()),
        }],
        usage: Usage {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
            cached_tokens: resp.usage.cache_read_input_tokens,
            cache_creation_input_tokens: resp.usage.cache_creation_input_tokens,
            raw: Some(serde_json::json!({
                "input_tokens": resp.usage.input_tokens,
                "output_tokens": resp.usage.output_tokens,
                "cache_read_input_tokens": resp.usage.cache_read_input_tokens,
                "cache_creation_input_tokens": resp.usage.cache_creation_input_tokens
            })),
            ..Default::default()
        },
        request_id: None,
        upstream_metadata: None,
    }
}

// ── Provider impl ────────────────────────────────

fn check_status(resp: &reqwest::Response) -> ProviderResult<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let code = status.as_u16();
    if code == 401 || code == 403 {
        return Err(ProviderError::Auth(format!("upstream returned {code}")));
    }
    if code == 429 {
        // 0.4.103（followup §3.1）：用 parse_retry_after 兼容 HTTP-date 格式。
        let retry = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(crate::retry::parse_retry_after);
        return Err(ProviderError::RateLimited {
            retry_after_ms: retry,
        });
    }
    if code == 404 {
        return Err(ProviderError::ModelNotFound(format!(
            "upstream returned {code}"
        )));
    }
    Ok(())
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        let body = to_anthropic_request(&req);
        let resp = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let parsed: AnthropicResponse = resp.json().await.map_err(ProviderError::from)?;
        Ok(from_anthropic_response(parsed))
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        let mut body = to_anthropic_request(&req);
        body.stream = Some(true);

        let resp = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;
        check_status(&resp)?;

        let byte_stream = resp.bytes_stream();
        let parsed = anthropic_sse_to_chunks(byte_stream);
        Ok(parsed.boxed())
    }
}

// ── Streaming ────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicStreamMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        #[serde(default)]
        #[allow(dead_code)]
        index: u32,
        #[serde(default)]
        content_block: Option<AnthropicContentBlockInfo>,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: AnthropicDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {},
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDelta,
        #[serde(default)]
        usage: Option<AnthropicDeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: AnthropicErrorEvent },
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    id: String,
    model: String,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlockInfo {
    r#type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicDeltaUsage {
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorEvent {
    message: String,
}

fn anthropic_sse_to_chunks<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let state = std::sync::Arc::new(parking_lot::Mutex::new(StreamState::default()));

    byte_stream.flat_map(move |item| {
        let buf = buf.clone();
        let state = state.clone();
        let chunks = match item {
            Ok(bytes) => {
                let mut g = buf.lock();
                g.extend_from_slice(&bytes);
                drain_anthropic_events(&mut g, &state)
            }
            Err(e) => vec![Err(ProviderError::Network(e.to_string()))],
        };
        futures::stream::iter(chunks)
    })
}

#[derive(Default)]
struct StreamState {
    id: String,
    model: String,
    input_tokens: u32,
    cached_tokens: u32,
    /// 0.4.68: anthropic stream message_start 携带的 cache_creation_input_tokens。
    cache_creation_tokens: u32,
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
}

fn drain_anthropic_events(
    buf: &mut Vec<u8>,
    state: &parking_lot::Mutex<StreamState>,
) -> Vec<ProviderResult<ChatStreamChunk>> {
    let mut out = Vec::new();

    while let Some(idx) = find_double_newline(buf) {
        let event_bytes: Vec<u8> = buf.drain(..idx + 2).collect();
        let s = String::from_utf8_lossy(&event_bytes);
        let mut data_line = None;
        for line in s.lines() {
            if let Some(d) = line.strip_prefix("data:") {
                data_line = Some(d.trim().to_string());
            }
        }
        let Some(data) = data_line else {
            continue;
        };
        let event: AnthropicEvent = match serde_json::from_str(&data) {
            Ok(e) => e,
            Err(e) => {
                out.push(Err(ProviderError::Decode(format!("{data:?}: {e}"))));
                continue;
            }
        };

        match event {
            AnthropicEvent::MessageStart { message } => {
                let mut st = state.lock();
                st.id = message.id;
                st.model = message.model;
                if let Some(u) = message.usage {
                    st.input_tokens = u.input_tokens;
                    st.cached_tokens = u.cache_read_input_tokens;
                    st.cache_creation_tokens = u.cache_creation_input_tokens;
                }
                out.push(Ok(ChatStreamChunk {
                    id: st.id.clone(),
                    model: st.model.clone(),
                    choices: vec![ChatStreamChoice {
                        index: 0,
                        delta: ChatDelta {
                            role: Some(Role::Assistant),
                            content: None,
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                }));
            }
            AnthropicEvent::ContentBlockStart {
                content_block: Some(cb),
                ..
            } if cb.r#type.as_deref() == Some("tool_use") => {
                let mut st = state.lock();
                st.current_tool_id = cb.id.clone();
                st.current_tool_name = cb.name.clone();
                out.push(Ok(ChatStreamChunk {
                    id: st.id.clone(),
                    model: st.model.clone(),
                    choices: vec![ChatStreamChoice {
                        index: 0,
                        delta: ChatDelta {
                            role: None,
                            content: None,
                            tool_calls: Some(vec![ToolCallDelta {
                                index: Some(0),
                                id: cb.id,
                                r#type: Some("function".to_string()),
                                function: Some(FunctionCallDelta {
                                    name: cb.name,
                                    arguments: None,
                                }),
                            }]),
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                }));
            }
            AnthropicEvent::ContentBlockStart { .. } => {}
            AnthropicEvent::ContentBlockDelta { delta } => {
                let st = state.lock();
                if let Some(text) = delta.text {
                    out.push(Ok(ChatStreamChunk {
                        id: st.id.clone(),
                        model: st.model.clone(),
                        choices: vec![ChatStreamChoice {
                            index: 0,
                            delta: ChatDelta {
                                role: None,
                                content: Some(text),
                                tool_calls: None,
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    }));
                }
                if let Some(json) = delta.partial_json {
                    out.push(Ok(ChatStreamChunk {
                        id: st.id.clone(),
                        model: st.model.clone(),
                        choices: vec![ChatStreamChoice {
                            index: 0,
                            delta: ChatDelta {
                                role: None,
                                content: None,
                                tool_calls: Some(vec![ToolCallDelta {
                                    index: Some(0),
                                    id: None,
                                    r#type: None,
                                    function: Some(FunctionCallDelta {
                                        name: None,
                                        arguments: Some(json),
                                    }),
                                }]),
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    }));
                }
            }
            AnthropicEvent::MessageDelta { delta, usage } => {
                let st = state.lock();
                let finish = map_stop_reason(delta.stop_reason.as_deref());
                let final_usage = usage.map(|u| Usage {
                    prompt_tokens: st.input_tokens,
                    completion_tokens: u.output_tokens,
                    total_tokens: st.input_tokens + u.output_tokens,
                    cached_tokens: st.cached_tokens,
                    cache_creation_input_tokens: st.cache_creation_tokens,
                    raw: Some(serde_json::json!({
                        "input_tokens": st.input_tokens,
                        "output_tokens": u.output_tokens,
                        "cache_read_input_tokens": st.cached_tokens,
                        "cache_creation_input_tokens": st.cache_creation_tokens
                    })),
                    ..Default::default()
                });
                out.push(Ok(ChatStreamChunk {
                    id: st.id.clone(),
                    model: st.model.clone(),
                    choices: vec![ChatStreamChoice {
                        index: 0,
                        delta: ChatDelta::default(),
                        finish_reason: finish,
                    }],
                    usage: final_usage,
                }));
            }
            AnthropicEvent::Error { error } => {
                out.push(Err(ProviderError::Decode(format!(
                    "anthropic: {}",
                    error.message
                ))));
            }
            _ => {}
        }
    }
    out
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

// ─── ADR-0002 fast-path entry points ────────────────────────────────────────
//
// CustomHttpProvider 内部 fast-path 复用这些 wrapper。函数体只是把内部辅助
// 暴露给同 crate 调用方，**不重复实现协议**。任何 Anthropic 协议的演进都在
// 上面的 to_/from_/sse 函数里改一次，两条路径同步。

/// Convert ChatRequest to Anthropic Messages API request body (serde_json::Value).
pub(crate) fn fastpath_anthropic_request_body(req: &ChatRequest) -> serde_json::Value {
    serde_json::to_value(to_anthropic_request(req)).expect("AnthropicRequest serializable")
}

/// Convert Anthropic Messages response JSON back to OpenAI-compatible ChatResponse.
pub(crate) fn fastpath_anthropic_response_from_json(
    value: serde_json::Value,
) -> ProviderResult<ChatResponse> {
    let parsed: AnthropicResponse = serde_json::from_value(value)
        .map_err(|e| ProviderError::Decode(format!("anthropic response decode: {e}")))?;
    Ok(from_anthropic_response(parsed))
}

/// Stream wrapper exposing the existing Anthropic SSE → OpenAI chunk parser.
pub(crate) fn fastpath_anthropic_sse_stream<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    anthropic_sse_to_chunks(byte_stream)
}

/// Anthropic-specific status check (401/403/429/404 → typed errors).
pub(crate) fn fastpath_anthropic_check_status(resp: &reqwest::Response) -> ProviderResult<()> {
    check_status(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_chat_req() -> ChatRequest {
        ChatRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::Text("hi".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn extra_fields_passthrough_into_anthropic_body() {
        let mut req = base_chat_req();
        // 模拟用户在 ChatRequest 顶层传了 anthropic 特有字段
        req.extra.insert("top_k".to_string(), serde_json::json!(40));
        req.extra.insert(
            "thinking".to_string(),
            serde_json::json!({"type": "enabled", "budget_tokens": 1000}),
        );
        req.extra.insert(
            "metadata".to_string(),
            serde_json::json!({"user_id": "abc"}),
        );

        let body = to_anthropic_request(&req);
        let v = serde_json::to_value(&body).expect("serialize");

        // 已识别字段仍正确
        assert_eq!(v["model"], "claude-3-5-sonnet");
        assert_eq!(v["max_tokens"], DEFAULT_MAX_TOKENS);
        // extra 字段 flatten 到顶层
        assert_eq!(v["top_k"], 40);
        assert_eq!(v["thinking"]["type"], "enabled");
        assert_eq!(v["thinking"]["budget_tokens"], 1000);
        assert_eq!(v["metadata"]["user_id"], "abc");
    }

    #[test]
    fn empty_extra_does_not_emit_keys() {
        let req = base_chat_req();
        let body = to_anthropic_request(&req);
        let v = serde_json::to_value(&body).expect("serialize");
        let map = v.as_object().expect("object");
        // 不应出现 "extra" 字面 key —— flatten 应该是透明的
        assert!(!map.contains_key("extra"));
        // 也不应出现 anthropic 不识别的随机字段
        assert!(!map.contains_key("top_k"));
    }
}
