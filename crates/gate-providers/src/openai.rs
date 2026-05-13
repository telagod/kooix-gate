//! OpenAI 兼容 provider —— 直接透传。
//!
//! 因为我们的统一类型本身就是 OpenAI 形状，几乎无需翻译。
//!
//! 配置：
//! - `base_url`：默认 `https://api.openai.com/v1`，可指向 OpenAI 兼容上游（vLLM / 通义千问 OpenAI mode 等）
//! - `api_key`：透传 Bearer
//! - `timeout`：默认 60s（流式场景请求建立超时，不限响应总长）

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::types::{ChatRequest, ChatResponse, ChatStreamChunk};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use std::time::Duration;

#[derive(Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600)) // 流式可能跑很久
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let parsed: ChatResponse = resp.json().await?;
        Ok(parsed)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        // 强制注入 stream_options.include_usage=true，
        // 这样上游会在最后一帧吐 usage（用于计费）。
        // 如果调用方已经传了 stream_options，仅补齐 include_usage 字段，不覆盖其他键。
        inject_include_usage(&mut req);
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;

        let byte_stream = resp.bytes_stream();
        let parsed = sse_to_chunks(byte_stream);
        Ok(parsed.boxed())
    }
}

/// 把 `stream_options.include_usage = true` 注入到 ChatRequest.extra。
///
/// 已存在 stream_options（任何类型）：
/// - 若它是 object，覆盖 `include_usage = true`，其它字段保留
/// - 若它不是 object（脏数据），整体替换为 `{include_usage: true}`
fn inject_include_usage(req: &mut ChatRequest) {
    use serde_json::{Value, json};
    let entry = req
        .extra
        .entry("stream_options".to_string())
        .or_insert_with(|| json!({}));
    match entry {
        Value::Object(map) => {
            map.insert("include_usage".to_string(), Value::Bool(true));
        }
        slot => {
            *slot = json!({ "include_usage": true });
        }
    }
}

/// 上游非 2xx 时映射出业务化错误（401/403/429 各自归位）。
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
    // 其他 4xx/5xx 等会在 .error_for_status 之后被处理
    Ok(())
}

/// SSE → ChatStreamChunk 流。
///
/// OpenAI SSE 格式：
/// ```text
/// data: {"id":..., "choices":[{"delta":{"content":"hi"}}]}\n\n
/// data: [DONE]\n\n
/// ```
fn sse_to_chunks<S>(byte_stream: S) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    let buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));

    byte_stream.flat_map(move |item| {
        let buf = buf.clone();
        let chunks = match item {
            Ok(bytes) => {
                let mut g = buf.lock();
                g.extend_from_slice(&bytes);
                drain_events(&mut g)
            }
            Err(e) => vec![Err(ProviderError::Network(e.to_string()))],
        };
        futures::stream::iter(chunks)
    })
}

/// 从缓冲区里读出完整的 SSE event（`\n\n` 分隔），返回解析结果。
fn drain_events(buf: &mut Vec<u8>) -> Vec<ProviderResult<ChatStreamChunk>> {
    let mut out = Vec::new();
    while let Some(idx) = find_double_newline(buf) {
        let event_bytes: Vec<u8> = buf.drain(..idx + 2).collect();
        // 去掉末尾的 \n\n
        let s = String::from_utf8_lossy(&event_bytes);
        for line in s.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return out; // 主动结束
            }
            match serde_json::from_str::<ChatStreamChunk>(data) {
                Ok(chunk) => out.push(Ok(chunk)),
                Err(e) => out.push(Err(ProviderError::Decode(format!("line {data:?}: {e}")))),
            }
        }
    }
    out
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, Role};
    use serde_json::{Value, json};

    fn make_stream_req() -> ChatRequest {
        ChatRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hi".into(),
                name: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: true,
            extra: Default::default(),
        }
    }

    #[test]
    fn inject_when_absent() {
        let mut req = make_stream_req();
        inject_include_usage(&mut req);
        let so = req.extra.get("stream_options").unwrap();
        assert_eq!(so["include_usage"], Value::Bool(true));
    }

    #[test]
    fn inject_preserves_other_stream_options_fields() {
        let mut req = make_stream_req();
        req.extra
            .insert("stream_options".into(), json!({"foo": "bar"}));
        inject_include_usage(&mut req);
        let so = req.extra.get("stream_options").unwrap();
        assert_eq!(so["include_usage"], Value::Bool(true));
        assert_eq!(so["foo"], Value::String("bar".into()));
    }

    #[test]
    fn inject_overrides_existing_include_usage_false() {
        let mut req = make_stream_req();
        req.extra
            .insert("stream_options".into(), json!({"include_usage": false}));
        inject_include_usage(&mut req);
        let so = req.extra.get("stream_options").unwrap();
        assert_eq!(so["include_usage"], Value::Bool(true));
    }

    #[test]
    fn inject_replaces_non_object_value() {
        let mut req = make_stream_req();
        req.extra.insert("stream_options".into(), json!("garbage"));
        inject_include_usage(&mut req);
        let so = req.extra.get("stream_options").unwrap();
        assert!(so.is_object());
        assert_eq!(so["include_usage"], Value::Bool(true));
    }

    #[test]
    fn drain_parses_usage_in_final_frame() {
        // 模拟带 usage 的最后一帧（OpenAI include_usage 行为）
        let mut buf = Vec::new();
        buf.extend_from_slice(b"data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n");
        let evs = drain_events(&mut buf);
        assert_eq!(evs.len(), 1);
        let chunk = evs.into_iter().next().unwrap().unwrap();
        let u = chunk.usage.expect("usage should be present");
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 5);
        assert_eq!(u.total_tokens, 15);
    }
}
