//! Anthropic Messages API 适配器。
//!
//! 把统一 [`ChatRequest`] 翻译成 Anthropic 格式，响应翻回 OpenAI 形状。
//!
//! 配置：
//! - `base_url`：默认 `https://api.anthropic.com`
//! - `api_key`：通过 `x-api-key` header 传递
//! - Anthropic 版本固定为 `2023-06-01`

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::types::{
    ChatChoice, ChatDelta, ChatMessage, ChatRequest, ChatResponse, ChatStreamChoice,
    ChatStreamChunk, FinishReason, Role, Usage,
};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
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

// ============================================================================
// Anthropic 请求格式
// ============================================================================

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
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

/// 把统一 ChatRequest → Anthropic 格式。
///
/// - system 消息提到顶层 `system` 参数
/// - `max_tokens` 必填，默认 4096
fn to_anthropic_request(req: &ChatRequest) -> AnthropicRequest {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => {
                system_parts.push(msg.content.clone());
            }
            Role::User | Role::Assistant => {
                messages.push(AnthropicMessage {
                    role: match msg.role {
                        Role::User => "user".to_string(),
                        Role::Assistant => "assistant".to_string(),
                        _ => unreachable!(),
                    },
                    content: msg.content.clone(),
                });
            }
            Role::Tool => {
                // Tool 消息当 user 消息传（简化处理，完整 tool_use 在后续版本支持）
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: msg.content.clone(),
                });
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n"))
    };

    AnthropicRequest {
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        messages,
        system,
        temperature: req.temperature,
        stream: if req.stream { Some(true) } else { None },
    }
}

// ============================================================================
// Anthropic 响应格式
// ============================================================================

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

/// stop_reason → FinishReason 映射。
fn map_stop_reason(reason: Option<&str>) -> Option<FinishReason> {
    reason.map(|r| match r {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        "stop_sequence" => FinishReason::Stop,
        _ => FinishReason::Other,
    })
}

/// Anthropic 响应 → 统一 ChatResponse。
fn from_anthropic_response(resp: AnthropicResponse) -> ChatResponse {
    let content = resp
        .content
        .iter()
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    ChatResponse {
        id: resp.id,
        model: resp.model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: Role::Assistant,
                content,
                name: None,
            },
            finish_reason: map_stop_reason(resp.stop_reason.as_deref()),
        }],
        usage: Usage {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
        },
    }
}

// ============================================================================
// Provider impl
// ============================================================================

/// 上游非 2xx 时映射出业务化错误。
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
        let retry = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| s * 1000);
        return Err(ProviderError::RateLimited {
            retry_after_ms: retry,
        });
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

// ============================================================================
// Streaming SSE → ChatStreamChunk
// ============================================================================

/// Anthropic SSE event 类型。
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicStreamMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {},
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
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
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

/// Anthropic SSE → ChatStreamChunk 流。
fn anthropic_sse_to_chunks<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    // 累积状态：id, model, input_tokens
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
}

fn drain_anthropic_events(
    buf: &mut Vec<u8>,
    state: &parking_lot::Mutex<StreamState>,
) -> Vec<ProviderResult<ChatStreamChunk>> {
    let mut out = Vec::new();

    while let Some(idx) = find_double_newline(buf) {
        let event_bytes: Vec<u8> = buf.drain(..idx + 2).collect();
        let s = String::from_utf8_lossy(&event_bytes);

        let mut event_type = None;
        let mut data_line = None;

        for line in s.lines() {
            if let Some(et) = line.strip_prefix("event:") {
                event_type = Some(et.trim().to_string());
            } else if let Some(d) = line.strip_prefix("data:") {
                data_line = Some(d.trim().to_string());
            }
        }

        let Some(data) = data_line else {
            continue;
        };
        let _ = event_type; // event type is embedded in JSON `type` field

        // Parse as Anthropic event
        let event: AnthropicEvent = match serde_json::from_str(&data) {
            Ok(e) => e,
            Err(e) => {
                out.push(Err(ProviderError::Decode(format!(
                    "anthropic event {data:?}: {e}"
                ))));
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
                }
                // Emit initial chunk with role
                let st_id = st.id.clone();
                let st_model = st.model.clone();
                out.push(Ok(ChatStreamChunk {
                    id: st_id,
                    model: st_model,
                    choices: vec![ChatStreamChoice {
                        index: 0,
                        delta: ChatDelta {
                            role: Some(Role::Assistant),
                            content: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                }));
            }
            AnthropicEvent::ContentBlockDelta { delta } => {
                if let Some(text) = delta.text {
                    let st = state.lock();
                    out.push(Ok(ChatStreamChunk {
                        id: st.id.clone(),
                        model: st.model.clone(),
                        choices: vec![ChatStreamChoice {
                            index: 0,
                            delta: ChatDelta {
                                role: None,
                                content: Some(text),
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
                    "anthropic stream error: {}",
                    error.message
                ))));
            }
            // ping / content_block_start / content_block_stop / message_stop → 不产出 chunk
            _ => {}
        }
    }
    out
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_chat_request() -> ChatRequest {
        ChatRequest {
            model: "claude-3-sonnet-20240229".into(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "You are helpful.".into(),
                    name: None,
                },
                ChatMessage {
                    role: Role::User,
                    content: "Hi".into(),
                    name: None,
                },
            ],
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(1024),
            stream: false,
            extra: Default::default(),
        }
    }

    #[test]
    fn request_conversion_extracts_system() {
        let req = make_chat_request();
        let body = to_anthropic_request(&req);

        assert_eq!(body.model, "claude-3-sonnet-20240229");
        assert_eq!(body.max_tokens, 1024);
        assert_eq!(body.system.as_deref(), Some("You are helpful."));
        assert_eq!(body.temperature, Some(0.7));
        // Only user/assistant messages in the messages array
        assert_eq!(body.messages.len(), 1);
        assert_eq!(body.messages[0].role, "user");
        assert_eq!(body.messages[0].content, "Hi");
    }

    #[test]
    fn request_conversion_default_max_tokens() {
        let req = ChatRequest {
            model: "claude-3-haiku-20240307".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "Hello".into(),
                name: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None, // not set
            stream: false,
            extra: Default::default(),
        };
        let body = to_anthropic_request(&req);
        assert_eq!(body.max_tokens, DEFAULT_MAX_TOKENS);
        assert!(body.system.is_none());
    }

    #[test]
    fn request_serializes_to_expected_json() {
        let req = make_chat_request();
        let body = to_anthropic_request(&req);
        let json_val = serde_json::to_value(&body).unwrap();

        assert_eq!(json_val["model"], "claude-3-sonnet-20240229");
        assert_eq!(json_val["max_tokens"], 1024);
        assert_eq!(json_val["system"], "You are helpful.");
        assert_eq!(json_val["messages"][0]["role"], "user");
        assert_eq!(json_val["messages"][0]["content"], "Hi");
        // stream should not be present when None
        assert!(json_val.get("stream").is_none() || json_val["stream"].is_null());
    }

    #[test]
    fn response_conversion_concatenates_content() {
        let raw: AnthropicResponse = serde_json::from_value(json!({
            "id": "msg_abc",
            "type": "message",
            "model": "claude-3-sonnet-20240229",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " world"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }))
        .unwrap();

        let resp = from_anthropic_response(raw);

        assert_eq!(resp.id, "msg_abc");
        assert_eq!(resp.model, "claude-3-sonnet-20240229");
        assert_eq!(resp.choices[0].message.content, "Hello world");
        assert_eq!(resp.choices[0].message.role, Role::Assistant);
        assert_eq!(resp.choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 5);
        assert_eq!(resp.usage.total_tokens, 15);
    }

    #[test]
    fn stop_reason_mapping() {
        assert_eq!(map_stop_reason(Some("end_turn")), Some(FinishReason::Stop));
        assert_eq!(
            map_stop_reason(Some("max_tokens")),
            Some(FinishReason::Length)
        );
        assert_eq!(
            map_stop_reason(Some("tool_use")),
            Some(FinishReason::ToolCalls)
        );
        assert_eq!(
            map_stop_reason(Some("stop_sequence")),
            Some(FinishReason::Stop)
        );
        assert_eq!(
            map_stop_reason(Some("unknown")),
            Some(FinishReason::Other)
        );
        assert_eq!(map_stop_reason(None), None);
    }

    #[test]
    fn response_conversion_max_tokens_finish() {
        let raw: AnthropicResponse = serde_json::from_value(json!({
            "id": "msg_def",
            "type": "message",
            "model": "claude-3-haiku-20240307",
            "role": "assistant",
            "content": [{"type": "text", "text": "truncated"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 20, "output_tokens": 100}
        }))
        .unwrap();

        let resp = from_anthropic_response(raw);
        assert_eq!(resp.choices[0].finish_reason, Some(FinishReason::Length));
        assert_eq!(resp.usage.prompt_tokens, 20);
        assert_eq!(resp.usage.completion_tokens, 100);
    }

    #[test]
    fn stream_event_parsing_message_start() {
        let state = parking_lot::Mutex::new(StreamState::default());
        let mut buf = Vec::new();
        buf.extend_from_slice(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"model\":\"claude-3-sonnet-20240229\",\"usage\":{\"input_tokens\":25}}}\n\n",
        );

        let chunks = drain_anthropic_events(&mut buf, &state);
        assert_eq!(chunks.len(), 1);
        let chunk = chunks[0].as_ref().unwrap();
        assert_eq!(chunk.id, "msg_1");
        assert_eq!(chunk.choices[0].delta.role, Some(Role::Assistant));

        let st = state.lock();
        assert_eq!(st.input_tokens, 25);
    }

    #[test]
    fn stream_event_parsing_content_delta() {
        let state = parking_lot::Mutex::new(StreamState {
            id: "msg_1".into(),
            model: "claude-3-sonnet-20240229".into(),
            input_tokens: 10,
        });
        let mut buf = Vec::new();
        buf.extend_from_slice(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        );

        let chunks = drain_anthropic_events(&mut buf, &state);
        assert_eq!(chunks.len(), 1);
        let chunk = chunks[0].as_ref().unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn stream_event_parsing_message_delta_with_usage() {
        let state = parking_lot::Mutex::new(StreamState {
            id: "msg_1".into(),
            model: "claude-3-sonnet-20240229".into(),
            input_tokens: 10,
        });
        let mut buf = Vec::new();
        buf.extend_from_slice(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":15}}\n\n",
        );

        let chunks = drain_anthropic_events(&mut buf, &state);
        assert_eq!(chunks.len(), 1);
        let chunk = chunks[0].as_ref().unwrap();
        assert_eq!(chunk.choices[0].finish_reason, Some(FinishReason::Stop));
        let usage = chunk.usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 15);
        assert_eq!(usage.total_tokens, 25);
    }

    #[test]
    fn multiple_system_messages_concatenated() {
        let req = ChatRequest {
            model: "claude-3-sonnet-20240229".into(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "Rule 1.".into(),
                    name: None,
                },
                ChatMessage {
                    role: Role::System,
                    content: "Rule 2.".into(),
                    name: None,
                },
                ChatMessage {
                    role: Role::User,
                    content: "Go".into(),
                    name: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: false,
            extra: Default::default(),
        };
        let body = to_anthropic_request(&req);
        assert_eq!(body.system.as_deref(), Some("Rule 1.\nRule 2."));
        assert_eq!(body.messages.len(), 1);
    }
}
