//! native:kiro —— AWS Kiro（AmazonQ / CodeWhisperer）渠道 PoC。ADR-0005 首个 native 重渠道。
//!
//! ## 为什么必须是 native（manifest 表达不了）
//!
//! - 上游响应是 **AWS EventStream 二进制帧**（非 JSON、非标准 SSE），即使非流式请求
//!   也回二进制流：`[totalLen:4BE][headerLen:4BE][preludeCRC:4BE][headers][payload-JSON][msgCRC:4BE]`。
//! - 请求体是 `conversationState` 嵌套结构 + 一组私有 `X-Amz-*` / `x-amzn-kiro-*` header。
//! - model id 要从 `claude-sonnet-4-5` 规范化成上游的 `claude-sonnet-4.5`。
//!
//! 这些都是过程逻辑，声明式 manifest 无法描述 —— 正是 ADR-0005 native plane 的用武之地。
//!
//! ## PoC 范围
//!
//! 移植自 foxnio `providerimpl/kiro` + `service/request_executor`。本 PoC 实现：
//! chat 非流式 + 收集式流式（先收完再单次 emit）。
//!
//! 刻意省略（见 ADR-0005，标注 PoC 可省）：interference_guard 干扰重试、buffered
//! stream `input_tokens` 回填、no_cache 计费、tool name 大小写规范化、并发限流、
//! token refresh（PoC 直接用现成 access_token）、真·逐帧流式。

use super::{NativeBuildContext, NativeProviderRegistration};
use crate::Provider;
use crate::capabilities::ProviderCapabilities;
use crate::error::{ProviderError, ProviderResult};
use crate::types::{
    ChatChoice, ChatDelta, ChatMessage, ChatRequest, ChatResponse, ChatStreamChoice,
    ChatStreamChunk, FinishReason, Role, Usage,
};
use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use std::sync::Arc;
use uuid::Uuid;

/// AmazonQ 默认 endpoint（us-east-1）。channel.base_url 未配置完整 URL 时兜底。
const DEFAULT_AMAZONQ_URL: &str = "https://q.us-east-1.amazonaws.com/generateAssistantResponse";
const AMZ_TARGET: &str = "AmazonQDeveloperStreamingService.SendMessage";
/// channel 创建时若未改 plugin 占位 base_url，视为未配置。
const PLUGIN_PLACEHOLDER_BASE_URL: &str = "https://api.example.com/v1";

pub(super) fn registration() -> NativeProviderRegistration {
    NativeProviderRegistration {
        name: "kiro",
        capabilities: ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            vision: true,
            ..ProviderCapabilities::none()
        },
        factory: Arc::new(|ctx: &NativeBuildContext<'_>| {
            Ok(Arc::new(KiroProvider::from_ctx(ctx)) as Arc<dyn Provider>)
        }),
    }
}

struct KiroProvider {
    /// Bearer access token（secret slot `primary`）。
    access_token: String,
    /// Social 认证的 profileArn（secret slot `profile_arn`，可选）。
    profile_arn: Option<String>,
    /// 完整上游 URL。
    endpoint: String,
    timeout_ms: u64,
}

impl KiroProvider {
    fn from_ctx(ctx: &NativeBuildContext<'_>) -> Self {
        let profile_arn = match ctx.secret("profile_arn") {
            "" => None,
            p => Some(p.to_string()),
        };
        let base = ctx.channel.base_url.trim();
        let endpoint = if base.is_empty() || base == PLUGIN_PLACEHOLDER_BASE_URL {
            DEFAULT_AMAZONQ_URL.to_string()
        } else if base.contains("generateAssistantResponse") {
            base.to_string()
        } else {
            // base 是 host（如 https://q.eu-central-1.amazonaws.com）→ 拼 path
            format!("{}/generateAssistantResponse", base.trim_end_matches('/'))
        };
        Self {
            access_token: ctx.primary_secret().to_string(),
            profile_arn,
            endpoint,
            timeout_ms: ctx.opts.timeout_ms,
        }
    }

    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let model_id = canonical_model_id(&req.model);

        let system = req
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(ChatMessage::content_text)
            .collect::<Vec<_>>()
            .join("\n");

        let convo: Vec<&ChatMessage> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .collect();
        let last_user_text = convo.last().map(|m| m.content_text()).unwrap_or("");
        let content = if system.is_empty() {
            last_user_text.to_string()
        } else {
            format!("{system}\n\n{last_user_text}")
        };

        let user_message = serde_json::json!({
            "content": content,
            "modelId": model_id,
            "origin": "AI_EDITOR",
            "userContext": {
                "client_id": "kooix-gate",
                "ide_category": "CLI",
                "ide_version": "0.5.0",
                "operating_system": "Linux",
                "product": "kooix-gate"
            }
        });

        let mut body = serde_json::json!({
            "conversationState": {
                "agentTaskType": "vibe",
                "chatTriggerType": "MANUAL",
                "conversationId": Uuid::now_v7().to_string(),
                "agentContinuationId": Uuid::now_v7().to_string(),
                "currentMessage": { "userInputMessage": user_message },
                "history": build_history(&convo, &model_id),
            }
        });
        if let Some(arn) = &self.profile_arn {
            body["profileArn"] = serde_json::Value::String(arn.clone());
        }
        body
    }

    async fn execute(&self, req: &ChatRequest) -> ProviderResult<EventStreamCollect> {
        let client = crate::shared_http_client(&crate::ProviderOpts {
            timeout_ms: self.timeout_ms,
        })?;
        let body = self.build_body(req);
        let resp = client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("X-Amz-Target", AMZ_TARGET)
            .header("x-amzn-kiro-agent-mode", "vibe")
            .header("x-amzn-codewhisperer-optout", "true")
            .header("amz-sdk-invocation-id", Uuid::now_v7().to_string())
            .header("amz-sdk-request", "attempt=1; max=3")
            .header(
                "User-Agent",
                "aws-sdk-js/1.0.27 ua/2.1 os/linux lang/js md/nodejs#22.20.0 \
                 api/codewhispererstreaming#1.0.27 m/E KiroIDE-0.5.0",
            )
            .header("x-amz-user-agent", "aws-sdk-js/1.0.27 KiroIDE-0.5.0")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("kiro request failed: {e}")))?;

        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Network(format!("kiro body read failed: {e}")))?;

        if !status.is_success() {
            let body = crate::redact_upstream_body(&String::from_utf8_lossy(&bytes));
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::Auth(body));
            }
            return Err(ProviderError::Upstream {
                status: status.as_u16(),
                body,
            });
        }

        Ok(parse_event_stream(&bytes))
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &'static str {
        "native:kiro"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        let model_in = req.model.clone();
        let collected = self.execute(&req).await?;
        Ok(ChatResponse {
            id: format!("kiro-{}", Uuid::now_v7()),
            model: model_in,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, collected.text),
                finish_reason: Some(map_stop_reason(collected.stop_reason.as_deref())),
            }],
            usage: Usage {
                prompt_tokens: collected.input_tokens,
                completion_tokens: collected.output_tokens,
                total_tokens: collected.input_tokens + collected.output_tokens,
                cached_tokens: collected.cache_read_tokens,
                ..Usage::default()
            },
            request_id: None,
            upstream_metadata: None,
        })
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        // PoC：收集式流式 —— 先把整条 EventStream 解析完，再单次 emit 一个 chunk。
        // 真·逐帧流式（跨 chunk 增量解析 AWS EventStream）留作后续。
        let model_in = req.model.clone();
        let collected = self.execute(&req).await?;
        let chunk = ChatStreamChunk {
            id: format!("kiro-{}", Uuid::now_v7()),
            model: model_in,
            choices: vec![ChatStreamChoice {
                index: 0,
                delta: ChatDelta {
                    role: Some(Role::Assistant),
                    content: Some(collected.text),
                    tool_calls: None,
                },
                finish_reason: Some(map_stop_reason(collected.stop_reason.as_deref())),
            }],
            usage: Some(Usage {
                prompt_tokens: collected.input_tokens,
                completion_tokens: collected.output_tokens,
                total_tokens: collected.input_tokens + collected.output_tokens,
                cached_tokens: collected.cache_read_tokens,
                ..Usage::default()
            }),
        };
        Ok(Box::pin(stream::iter(vec![Ok(chunk)])))
    }
}

/// 历史消息 → CW history 节点（不含最后一条 current message）。
fn build_history(convo: &[&ChatMessage], model_id: &str) -> Vec<serde_json::Value> {
    if convo.len() <= 1 {
        return Vec::new();
    }
    convo[..convo.len() - 1]
        .iter()
        .map(|m| match m.role {
            Role::Assistant => serde_json::json!({
                "assistantResponseMessage": { "content": m.content_text() }
            }),
            _ => serde_json::json!({
                "userInputMessage": {
                    "content": m.content_text(),
                    "modelId": model_id,
                    "origin": "AI_EDITOR"
                }
            }),
        })
        .collect()
}

/// 用户传入 model → 上游 canonical modelId。
///
/// 规则：已知映射优先；否则通用 —— 剥 `-YYYYMMDD` 日期后缀，末尾 `-N-M` 版本段转 `N.M`。
fn canonical_model_id(model: &str) -> String {
    let stripped = strip_date_suffix(model.trim());
    match stripped {
        "claude-sonnet-4-5" | "claude-sonnet-4.5" => return "claude-sonnet-4.5".to_string(),
        "claude-opus-4-6" | "claude-opus-4.6" => return "claude-opus-4.6".to_string(),
        "claude-haiku-4-5" | "claude-haiku-4.5" => return "claude-haiku-4.5".to_string(),
        "claude-sonnet-4-6" | "claude-sonnet-4.6" => return "claude-sonnet-4.6".to_string(),
        "claude-opus-4-7" | "claude-opus-4.7" => return "claude-opus-4.7".to_string(),
        "claude-sonnet-4" => return "claude-sonnet-4".to_string(),
        _ => {}
    }
    generic_dot_version(stripped)
}

fn strip_date_suffix(model: &str) -> &str {
    // 形如 claude-x-4-5-20260101 → 剥末尾 8 位日期
    if let Some((head, tail)) = model.rsplit_once('-')
        && tail.len() == 8
        && tail.chars().all(|c| c.is_ascii_digit())
    {
        return head;
    }
    model
}

fn generic_dot_version(model: &str) -> String {
    let parts: Vec<&str> = model.split('-').collect();
    let n = parts.len();
    if n >= 2
        && parts[n - 1].chars().all(|c| c.is_ascii_digit())
        && parts[n - 2].chars().all(|c| c.is_ascii_digit())
        && !parts[n - 1].is_empty()
        && !parts[n - 2].is_empty()
    {
        let head = parts[..n - 2].join("-");
        return format!("{head}-{}.{}", parts[n - 2], parts[n - 1]);
    }
    model.to_string()
}

fn map_stop_reason(sr: Option<&str>) -> FinishReason {
    match sr {
        Some("tool_use") => FinishReason::ToolCalls,
        Some("max_tokens") | Some("length") => FinishReason::Length,
        Some("end_turn") | Some("stop") | None => FinishReason::Stop,
        _ => FinishReason::Other,
    }
}

// ── AWS EventStream 二进制帧解析 ──────────────────────────────
//
// 帧布局：
//   [total_len:4BE][header_len:4BE][prelude_crc:4BE][headers:header_len][payload][msg_crc:4BE]
//   payload_len = total_len - 12 - header_len - 4
//
// PoC 取舍：跳过 CRC 校验 + headers 解析（不靠 `:event-type` 区分，直接扫 payload
// JSON 字段：`content` 累加文本、`tokenUsage` 取用量、`stopReason` 取结束原因）。

#[derive(Default, Debug, PartialEq)]
struct EventStreamCollect {
    text: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    stop_reason: Option<String>,
}

fn parse_event_stream(buf: &[u8]) -> EventStreamCollect {
    let mut out = EventStreamCollect::default();
    let mut pos = 0usize;
    while pos + 16 <= buf.len() {
        let total_len =
            u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
        if total_len < 16 || pos + total_len > buf.len() {
            break;
        }
        let header_len =
            u32::from_be_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]) as usize;
        let payload_start = pos + 12 + header_len;
        let payload_end = pos + total_len - 4;
        if payload_start <= payload_end && payload_end <= buf.len() {
            let payload = &buf[payload_start..payload_end];
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
                collect_payload(&v, &mut out);
            }
        }
        pos += total_len;
    }
    out
}

fn collect_payload(v: &serde_json::Value, out: &mut EventStreamCollect) {
    if let Some(c) = v.get("content").and_then(|x| x.as_str()) {
        out.text.push_str(c);
    }
    if let Some(sr) = v.get("stopReason").and_then(|x| x.as_str()) {
        out.stop_reason = Some(sr.to_string());
    }
    let token_usage = v
        .get("tokenUsage")
        .or_else(|| v.pointer("/messageMetadataEvent/tokenUsage"));
    if let Some(tu) = token_usage {
        if let Some(n) = tu.get("inputTokens").and_then(serde_json::Value::as_u64) {
            out.input_tokens = n as u32;
        }
        if let Some(n) = tu.get("outputTokens").and_then(serde_json::Value::as_u64) {
            out.output_tokens = n as u32;
        }
        if let Some(n) = tu
            .get("cacheReadInputTokens")
            .and_then(serde_json::Value::as_u64)
        {
            out.cache_read_tokens = n as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个 PoC 简化帧（header_len=0，CRC 置零，parser 不校验）。
    fn frame(payload: &[u8]) -> Vec<u8> {
        let total_len = (12 + payload.len() + 4) as u32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&total_len.to_be_bytes()); // total_len
        buf.extend_from_slice(&0u32.to_be_bytes()); // header_len = 0
        buf.extend_from_slice(&0u32.to_be_bytes()); // prelude_crc (ignored)
        buf.extend_from_slice(payload); // payload
        buf.extend_from_slice(&0u32.to_be_bytes()); // msg_crc (ignored)
        buf
    }

    #[test]
    fn model_id_canonicalization() {
        assert_eq!(canonical_model_id("claude-sonnet-4-5"), "claude-sonnet-4.5");
        assert_eq!(canonical_model_id("claude-sonnet-4.5"), "claude-sonnet-4.5");
        assert_eq!(canonical_model_id("claude-opus-4-6"), "claude-opus-4.6");
        // 通用规则：未知但形如 -N-M
        assert_eq!(canonical_model_id("claude-foo-3-7"), "claude-foo-3.7");
        // 日期后缀剥离
        assert_eq!(
            canonical_model_id("claude-sonnet-4-5-20260101"),
            "claude-sonnet-4.5"
        );
        // 无版本段原样
        assert_eq!(canonical_model_id("gpt-4o"), "gpt-4o");
    }

    #[test]
    fn event_stream_parses_text_and_usage() {
        let mut stream = Vec::new();
        stream.extend(frame(br#"{"content":"Hello"}"#));
        stream.extend(frame(br#"{"content":", world"}"#));
        stream.extend(frame(
            br#"{"tokenUsage":{"inputTokens":42,"outputTokens":7,"cacheReadInputTokens":3},"stopReason":"end_turn"}"#,
        ));

        let out = parse_event_stream(&stream);
        assert_eq!(out.text, "Hello, world");
        assert_eq!(out.input_tokens, 42);
        assert_eq!(out.output_tokens, 7);
        assert_eq!(out.cache_read_tokens, 3);
        assert_eq!(out.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn event_stream_handles_nested_metadata_event() {
        let stream = frame(
            br#"{"messageMetadataEvent":{"tokenUsage":{"inputTokens":10,"outputTokens":20}}}"#,
        );
        let out = parse_event_stream(&stream);
        assert_eq!(out.input_tokens, 10);
        assert_eq!(out.output_tokens, 20);
    }

    #[test]
    fn event_stream_ignores_truncated_trailing_frame() {
        let mut stream = frame(br#"{"content":"ok"}"#);
        // 追加一个声称很长但实际截断的帧 → parser 应安全 break
        stream.extend_from_slice(&1000u32.to_be_bytes());
        stream.extend_from_slice(&[0u8; 4]);
        let out = parse_event_stream(&stream);
        assert_eq!(out.text, "ok");
    }

    #[test]
    fn build_body_shapes_conversation_state() {
        let ctx_channel = make_channel();
        let ctx = NativeBuildContext {
            channel: &ctx_channel,
            secrets: std::collections::HashMap::from([(
                "primary".to_string(),
                "tok-abc".to_string(),
            )]),
            opts: crate::ProviderOpts::default(),
        };
        let provider = KiroProvider::from_ctx(&ctx);
        assert_eq!(provider.endpoint, DEFAULT_AMAZONQ_URL);
        assert_eq!(provider.access_token, "tok-abc");

        let req = ChatRequest {
            model: "claude-sonnet-4-5".to_string(),
            messages: vec![
                ChatMessage::text(Role::System, "be terse"),
                ChatMessage::text(Role::User, "hi"),
            ],
            ..Default::default()
        };
        let body = provider.build_body(&req);
        let um = &body["conversationState"]["currentMessage"]["userInputMessage"];
        assert_eq!(um["modelId"], "claude-sonnet-4.5");
        assert_eq!(um["origin"], "AI_EDITOR");
        assert_eq!(um["content"], "be terse\n\nhi");
        assert!(body["conversationState"]["conversationId"].is_string());
    }

    fn make_channel() -> gate_storage::ChannelRecord {
        let now = chrono::Utc::now();
        gate_storage::ChannelRecord {
            channel_id: gate_core::id::ChannelId::new(),
            code: "kiro-test".into(),
            name: "kiro-test".into(),
            provider_type: "native:kiro".into(),
            base_url: String::new(),
            supported_models: vec![],
            status: "active".into(),
            health: "healthy".into(),
            timeout_ms: 60_000,
            max_retries: 2,
            rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::json!({}),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}
