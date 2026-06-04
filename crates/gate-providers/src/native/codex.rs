//! native:codex —— OpenAI Codex（ChatGPT backend Responses API）渠道。ADR-0005 第二个 native 重渠道。
//!
//! ## 为什么是 native（而非 manifest）
//!
//! codex 本体是 HTTP/SSE，**单看传输** manifest 几乎能表达；但它踩了 manifest 的两条边界：
//! - 请求体是 **Responses API** 私有形状（`input[]` items + `instructions` + `reasoning`），
//!   且 `store=false` / `stream=true` 强制、随 model 注入 `reasoning.effort`——这是过程逻辑。
//! - **claude → gpt-5.x-codex 的 model + reasoning-effort 联动映射**（一个输入决定两个上游字段）
//!   manifest 的 `model_mapping` 只能一对一字符串替换，表达不了。
//!
//! 移植自 foxnio `providerimpl/codex` + `internal/codex`。本 PoC 实现：chat 非流式 +
//! 收集式流式（先收完整条 SSE，再单次 emit）。
//!
//! 刻意省略（PoC 可省）：WebSocket 升级通道、token refresh（401 重刷）、tool calls 透传、
//! 多 model reasoning override、Originator/Session 精细推导、真·逐事件流式。

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

/// ChatGPT backend Codex endpoint。channel.base_url 未配置完整 URL 时兜底。
const DEFAULT_CODEX_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// channel 创建时若未改 plugin 占位 base_url，视为未配置。
const PLUGIN_PLACEHOLDER_BASE_URL: &str = "https://api.example.com/v1";
/// 对标 foxnio fallback：非官方 UA 时用此默认。
const CODEX_USER_AGENT: &str = "codex_cli_rs/0.101.0 (Linux; x86_64)";
const CODEX_VERSION: &str = "0.101.0";
const CODEX_ORIGINATOR: &str = "codex_cli_rs";

pub(super) fn registration() -> NativeProviderRegistration {
    NativeProviderRegistration {
        name: "codex",
        capabilities: ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            vision: true,
            ..ProviderCapabilities::none()
        },
        factory: Arc::new(|ctx: &NativeBuildContext<'_>| {
            Ok(Arc::new(CodexProvider::from_ctx(ctx)) as Arc<dyn Provider>)
        }),
    }
}

struct CodexProvider {
    /// Bearer access token（secret slot `primary`）。
    access_token: String,
    endpoint: String,
    timeout_ms: u64,
}

impl CodexProvider {
    fn from_ctx(ctx: &NativeBuildContext<'_>) -> Self {
        let base = ctx.channel.base_url.trim();
        let endpoint = if base.is_empty() || base == PLUGIN_PLACEHOLDER_BASE_URL {
            DEFAULT_CODEX_URL.to_string()
        } else if base.contains("/responses") {
            base.to_string()
        } else {
            // base 是 host（如 https://chatgpt.com/backend-api/codex）→ 拼 path
            format!("{}/responses", base.trim_end_matches('/'))
        };
        Self {
            access_token: ctx.primary_secret().to_string(),
            endpoint,
            timeout_ms: ctx.opts.timeout_ms,
        }
    }

    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mapped = map_model(&req.model);

        // system → instructions；其余 → input items。
        let instructions = req
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(ChatMessage::content_text)
            .collect::<Vec<_>>()
            .join("\n");

        let input: Vec<serde_json::Value> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let (role, part_type) = match m.role {
                    Role::Assistant => ("assistant", "output_text"),
                    _ => ("user", "input_text"),
                };
                serde_json::json!({
                    "type": "message",
                    "role": role,
                    "content": [{ "type": part_type, "text": m.content_text() }],
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": mapped.upstream,
            "input": input,
            "stream": true,
            "store": false,
            "parallel_tool_calls": true,
        });
        if !instructions.is_empty() {
            body["instructions"] = serde_json::Value::String(instructions);
        }
        if let Some(effort) = mapped.effort {
            body["reasoning"] = serde_json::json!({ "effort": effort, "summary": "auto" });
        }
        body
    }

    async fn execute(&self, req: &ChatRequest) -> ProviderResult<CodexCollect> {
        let client = crate::shared_http_client(&crate::ProviderOpts {
            timeout_ms: self.timeout_ms,
        })?;
        let body = self.build_body(req);
        let resp = client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("Accept-Encoding", "identity")
            .header("Origin", "https://chatgpt.com")
            .header("Referer", "https://chatgpt.com/")
            .header("User-Agent", CODEX_USER_AGENT)
            .header("Version", CODEX_VERSION)
            .header("Originator", CODEX_ORIGINATOR)
            .header("Session_id", Uuid::now_v7().to_string())
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("codex request failed: {e}")))?;

        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Network(format!("codex body read failed: {e}")))?;

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

        Ok(parse_sse(&String::from_utf8_lossy(&bytes)))
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &'static str {
        "native:codex"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        let model_in = req.model.clone();
        let collected = self.execute(&req).await?;
        Ok(ChatResponse {
            id: format!("codex-{}", Uuid::now_v7()),
            model: model_in,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::text(Role::Assistant, collected.text),
                finish_reason: Some(map_finish(collected.status.as_deref())),
            }],
            usage: Usage {
                prompt_tokens: collected.input_tokens,
                completion_tokens: collected.output_tokens,
                total_tokens: collected.input_tokens + collected.output_tokens,
                cached_tokens: collected.cached_tokens,
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
        // PoC：收集式流式 —— 先把整条 SSE 解析完，再单次 emit 一个 chunk。
        // 真·逐事件流式（跨 chunk 增量解析 Responses SSE）留作后续。
        let model_in = req.model.clone();
        let collected = self.execute(&req).await?;
        let chunk = ChatStreamChunk {
            id: format!("codex-{}", Uuid::now_v7()),
            model: model_in,
            choices: vec![ChatStreamChoice {
                index: 0,
                delta: ChatDelta {
                    role: Some(Role::Assistant),
                    content: Some(collected.text),
                    tool_calls: None,
                },
                finish_reason: Some(map_finish(collected.status.as_deref())),
            }],
            usage: Some(Usage {
                prompt_tokens: collected.input_tokens,
                completion_tokens: collected.output_tokens,
                total_tokens: collected.input_tokens + collected.output_tokens,
                cached_tokens: collected.cached_tokens,
                ..Usage::default()
            }),
        };
        Ok(Box::pin(stream::iter(vec![Ok(chunk)])))
    }
}

/// 映射结果：上游 codex model + 可选 reasoning effort。
struct MappedModel {
    upstream: String,
    effort: Option<&'static str>,
}

/// 用户传入 model → 上游 codex model + reasoning effort。
///
/// 规则：已知 claude→gpt 映射优先（含 effort 联动）；原生 gpt-/codex- 直通无 effort；
/// 其余原样直通。剥 `-YYYYMMDD` 日期后缀后再匹配。
fn map_model(model: &str) -> MappedModel {
    let m = strip_date_suffix(model.trim());
    let (upstream, effort): (&str, Option<&str>) = match m {
        "claude-opus-4-7" | "claude-opus-4.7" => ("gpt-5.3-codex", Some("high")),
        "claude-opus-4-6" | "claude-opus-4.6" => ("gpt-5.3-codex", Some("high")),
        "claude-sonnet-4-6" | "claude-sonnet-4.6" => ("gpt-5.3-codex", Some("low")),
        "claude-opus-4-5" | "claude-opus-4.5" => ("gpt-5.2-codex", Some("high")),
        "claude-sonnet-4-5" | "claude-sonnet-4.5" => ("gpt-5.1-codex", Some("low")),
        "claude-3-7-sonnet" | "claude-3.7-sonnet" => ("gpt-5.1-codex", Some("low")),
        "claude-sonnet-4" => ("gpt-5.1-codex", Some("low")),
        "claude-haiku-4-5" | "claude-haiku-4.5" => ("gpt-5.1-codex-mini", None),
        // 原生 codex / gpt 模型直通，不注入 effort（让上游用默认）。
        other if other.starts_with("gpt-") || other.starts_with("codex-") => (other, None),
        other => (other, None),
    };
    MappedModel {
        upstream: upstream.to_string(),
        effort,
    }
}

fn strip_date_suffix(model: &str) -> &str {
    if let Some((head, tail)) = model.rsplit_once('-')
        && tail.len() == 8
        && tail.chars().all(|c| c.is_ascii_digit())
    {
        return head;
    }
    model
}

fn map_finish(status: Option<&str>) -> FinishReason {
    match status {
        Some("completed") | None => FinishReason::Stop,
        Some("incomplete") | Some("max_output_tokens") | Some("length") => FinishReason::Length,
        Some("failed") => FinishReason::Other,
        _ => FinishReason::Stop,
    }
}

// ── Responses API SSE 解析 ──────────────────────────────
//
// 帧格式：`event: <type>\ndata: <json>\n\n`，末尾 `data: [DONE]`。
// PoC 取舍：不依赖 `event:` 行，直接扫 `data:` 的 JSON，按其内 `type` 字段分发：
//   - `response.output_text.delta` → 累加文本（delta 兼容 string / {text}）
//   - `response.completed` / `response.done` → 取 response.usage + status
// reasoning / tool / item 事件 PoC 忽略。

#[derive(Default, Debug, PartialEq)]
struct CodexCollect {
    text: String,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    status: Option<String>,
}

fn parse_sse(raw: &str) -> CodexCollect {
    let mut out = CodexCollect::default();
    for line in raw.lines() {
        let line = line.trim_start();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
            collect_event(&v, &mut out);
        }
    }
    out
}

fn collect_event(v: &serde_json::Value, out: &mut CodexCollect) {
    let event_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match event_type {
        "response.output_text.delta" => {
            if let Some(d) = v.get("delta") {
                if let Some(s) = d.as_str() {
                    out.text.push_str(s);
                } else if let Some(s) = d.get("text").and_then(|x| x.as_str()) {
                    out.text.push_str(s);
                }
            }
        }
        "response.completed" | "response.done" => {
            // usage / status 通常嵌在 response 对象下；兼容顶层。
            let resp = v.get("response").unwrap_or(v);
            if let Some(st) = resp.get("status").and_then(|x| x.as_str()) {
                out.status = Some(st.to_string());
            }
            let usage = resp.get("usage").or_else(|| v.get("usage"));
            if let Some(u) = usage {
                if let Some(n) = u.get("input_tokens").and_then(serde_json::Value::as_u64) {
                    out.input_tokens = n as u32;
                }
                if let Some(n) = u.get("output_tokens").and_then(serde_json::Value::as_u64) {
                    out.output_tokens = n as u32;
                }
                let cached = u
                    .pointer("/input_tokens_details/cached_tokens")
                    .or_else(|| u.get("cached_tokens"))
                    .and_then(serde_json::Value::as_u64);
                if let Some(n) = cached {
                    out.cached_tokens = n as u32;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_mapping_claude_to_codex_with_effort() {
        let m = map_model("claude-opus-4-7");
        assert_eq!(m.upstream, "gpt-5.3-codex");
        assert_eq!(m.effort, Some("high"));

        let m = map_model("claude-sonnet-4-5");
        assert_eq!(m.upstream, "gpt-5.1-codex");
        assert_eq!(m.effort, Some("low"));

        let m = map_model("claude-haiku-4-5");
        assert_eq!(m.upstream, "gpt-5.1-codex-mini");
        assert_eq!(m.effort, None);
    }

    #[test]
    fn model_mapping_native_passthrough_and_date_strip() {
        // 原生 codex 模型直通，无 effort
        let m = map_model("gpt-5.3-codex");
        assert_eq!(m.upstream, "gpt-5.3-codex");
        assert_eq!(m.effort, None);
        // 日期后缀剥离后命中映射
        let m = map_model("claude-opus-4-7-20260101");
        assert_eq!(m.upstream, "gpt-5.3-codex");
        assert_eq!(m.effort, Some("high"));
        // 未知原样直通
        assert_eq!(map_model("some-unknown").upstream, "some-unknown");
    }

    #[test]
    fn build_body_shapes_responses_request() {
        let ctx_channel = make_channel();
        let ctx = NativeBuildContext {
            channel: &ctx_channel,
            secrets: std::collections::HashMap::from([(
                "primary".to_string(),
                "tok-codex".to_string(),
            )]),
            opts: crate::ProviderOpts::default(),
        };
        let provider = CodexProvider::from_ctx(&ctx);
        assert_eq!(provider.endpoint, DEFAULT_CODEX_URL);
        assert_eq!(provider.access_token, "tok-codex");

        let req = ChatRequest {
            model: "claude-opus-4-7".to_string(),
            messages: vec![
                ChatMessage::text(Role::System, "be terse"),
                ChatMessage::text(Role::User, "hi"),
                ChatMessage::text(Role::Assistant, "yo"),
                ChatMessage::text(Role::User, "more"),
            ],
            ..Default::default()
        };
        let body = provider.build_body(&req);
        assert_eq!(body["model"], "gpt-5.3-codex");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "be terse");
        assert_eq!(body["reasoning"]["effort"], "high");
        // system 不进 input；其余 3 条进
        let input = body["input"].as_array().expect("input array");
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[2]["content"][0]["text"], "more");
    }

    #[test]
    fn sse_parses_text_and_usage() {
        let raw = "\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\
\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\", world\"}\n\
\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":5,\"input_tokens_details\":{\"cached_tokens\":4}}}}\n\
\n\
data: [DONE]\n";
        let out = parse_sse(raw);
        assert_eq!(out.text, "Hello, world");
        assert_eq!(out.input_tokens, 12);
        assert_eq!(out.output_tokens, 5);
        assert_eq!(out.cached_tokens, 4);
        assert_eq!(out.status.as_deref(), Some("completed"));
        assert_eq!(map_finish(out.status.as_deref()), FinishReason::Stop);
    }

    #[test]
    fn sse_delta_object_form_and_toplevel_usage() {
        // delta 兼容 {text} 对象形态；usage 兼容顶层 + cached_tokens 扁平。
        let raw = "\
data: {\"type\":\"response.output_text.delta\",\"delta\":{\"type\":\"text\",\"text\":\"ok\"}}\n\
data: {\"type\":\"response.done\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"cached_tokens\":2},\"status\":\"completed\"}\n";
        // 注意：response.done 顶层无 response 包裹，status 在顶层
        let out = parse_sse(raw);
        assert_eq!(out.text, "ok");
        assert_eq!(out.input_tokens, 3);
        assert_eq!(out.output_tokens, 1);
        assert_eq!(out.cached_tokens, 2);
    }

    #[test]
    fn sse_ignores_malformed_and_reasoning_events() {
        let raw = "\
data: not-json\n\
data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"thinking...\"}\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"real\"}\n";
        let out = parse_sse(raw);
        assert_eq!(out.text, "real");
    }

    fn make_channel() -> gate_storage::ChannelRecord {
        let now = chrono::Utc::now();
        gate_storage::ChannelRecord {
            channel_id: gate_core::id::ChannelId::new(),
            code: "codex-test".into(),
            name: "codex-test".into(),
            provider_type: "native:codex".into(),
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
