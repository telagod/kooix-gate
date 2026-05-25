//! OpenAI 兼容 provider —— 直接透传 + embedding。

use crate::error::{ProviderError, ProviderResult};
use crate::sse;
use crate::types::{
    AudioSpeechRequest, AudioTranscriptionResponse, ChatRequest, ChatResponse, ChatStreamChunk,
    EmbeddingRequest, EmbeddingResponse, ImageGenerationRequest, ImageGenerationResponse,
};
use crate::{AudioProvider, EmbeddingProvider, ImageProvider, Provider};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use std::sync::Arc;

#[derive(Clone)]
pub struct OpenAiProvider {
    client: Arc<reqwest::Client>,
    base_url: String,
    api_key: String,
}

impl OpenAiProvider {
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

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn embeddings_url(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }

    fn images_url(&self) -> String {
        format!("{}/images/generations", self.base_url)
    }

    fn audio_speech_url(&self) -> String {
        format!("{}/audio/speech", self.base_url)
    }

    fn audio_transcriptions_url(&self) -> String {
        format!("{}/audio/transcriptions", self.base_url)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
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
        // 0.4.68: 先解为 Value，捞 prompt_tokens_details.cached_tokens 与
        // completion_tokens_details.reasoning_tokens 提到 usage 顶层，再反序成
        // ChatResponse。o1 / o3 / o4-mini-reasoning 等模型必有这两组 details。
        let raw: serde_json::Value = resp.json().await?;
        let raw = lift_openai_usage_details(raw);
        let parsed: ChatResponse = serde_json::from_value(raw).map_err(ProviderError::from)?;
        Ok(parsed)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
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

#[async_trait]
impl EmbeddingProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        let resp = self
            .client
            .post(self.embeddings_url())
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }
}

#[async_trait]
impl ImageProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn generate_image(
        &self,
        req: ImageGenerationRequest,
    ) -> ProviderResult<ImageGenerationResponse> {
        let resp = self
            .client
            .post(self.images_url())
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }
}

#[async_trait]
impl AudioProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn speech(&self, req: AudioSpeechRequest) -> ProviderResult<bytes::Bytes> {
        let resp = self
            .client
            .post(self.audio_speech_url())
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.bytes().await?)
    }

    async fn transcription(
        &self,
        audio: bytes::Bytes,
        filename: String,
        model: String,
        language: Option<String>,
    ) -> ProviderResult<AudioTranscriptionResponse> {
        let file_part = reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name(filename)
            .mime_str("application/octet-stream")
            .map_err(|e| ProviderError::Config(e.to_string()))?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", model);

        if let Some(lang) = language {
            form = form.text("language", lang);
        }

        let resp = self
            .client
            .post(self.audio_transcriptions_url())
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }
}

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

pub(crate) fn check_status(resp: &reqwest::Response) -> ProviderResult<()> {
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
    if code == 404 {
        return Err(ProviderError::ModelNotFound(format!(
            "upstream returned {code}"
        )));
    }
    Ok(())
}

pub(crate) fn sse_to_chunks<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    sse::sse_to_json_values(byte_stream).map(|item| {
        item.and_then(|value| {
            // 0.4.68: 与非流路径一致地提升 usage.prompt_tokens_details /
            // completion_tokens_details 到顶层。
            let value = lift_openai_usage_details(value);
            serde_json::from_value::<ChatStreamChunk>(value).map_err(ProviderError::from)
        })
    })
}

/// 0.4.68: 把 OpenAI 嵌套 usage details 提升到顶级 usage 字段。
///
/// OpenAI 在 o1/o3/o4-mini-reasoning 等模型上返回：
/// ```json
/// "usage": {
///   "prompt_tokens": 100,
///   "completion_tokens": 200,
///   "prompt_tokens_details": {"cached_tokens": 80},
///   "completion_tokens_details": {"reasoning_tokens": 50}
/// }
/// ```
/// 而 `Usage` 结构体期望 `cached_tokens` / `reasoning_tokens` 在 usage 顶级。
/// 此 helper 把嵌套字段拷贝出来；保留原 details 在 raw 里以供审计。
pub(crate) fn lift_openai_usage_details(mut v: serde_json::Value) -> serde_json::Value {
    let Some(usage) = v.get_mut("usage").and_then(|u| u.as_object_mut()) else {
        return v;
    };

    // 把整个 usage 备份到 raw 之前操作
    let original = serde_json::Value::Object(usage.clone());

    if let Some(prompt_details) = usage.get("prompt_tokens_details").and_then(|d| d.as_object()) {
        if let Some(cached) = prompt_details.get("cached_tokens").and_then(|x| x.as_u64()) {
            usage
                .entry("cached_tokens")
                .or_insert(serde_json::json!(cached as u32));
        }
    }
    if let Some(comp_details) = usage
        .get("completion_tokens_details")
        .and_then(|d| d.as_object())
    {
        if let Some(reasoning) = comp_details.get("reasoning_tokens").and_then(|x| x.as_u64()) {
            usage
                .entry("reasoning_tokens")
                .or_insert(serde_json::json!(reasoning as u32));
        }
    }
    // 留原始 details 在 raw 用于审计 / debugging
    usage
        .entry("raw")
        .or_insert(original);
    v
}

#[cfg(test)]
mod openai_lift_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lift_cached_and_reasoning_tokens_from_details() {
        let raw = json!({
            "id": "chatcmpl-x",
            "model": "o3-mini",
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 200,
                "total_tokens": 300,
                "prompt_tokens_details": {"cached_tokens": 80, "audio_tokens": 0},
                "completion_tokens_details": {"reasoning_tokens": 50, "accepted_prediction_tokens": 0}
            }
        });
        let lifted = lift_openai_usage_details(raw);
        let usage = lifted.get("usage").unwrap();
        assert_eq!(usage["cached_tokens"], 80);
        assert_eq!(usage["reasoning_tokens"], 50);
        // 原始 details 仍可在 raw 里找回
        assert!(usage.get("raw").is_some());
        assert_eq!(
            usage["raw"]["prompt_tokens_details"]["cached_tokens"],
            80
        );
    }

    #[test]
    fn lift_no_details_is_noop() {
        let raw = json!({
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        let lifted = lift_openai_usage_details(raw.clone());
        let usage = lifted.get("usage").unwrap();
        assert_eq!(usage["prompt_tokens"], 10);
        // 没有 cached/reasoning 字段被加进来
        assert!(usage.get("cached_tokens").is_none());
        assert!(usage.get("reasoning_tokens").is_none());
    }

    #[test]
    fn lift_does_not_overwrite_explicit_top_level() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "cached_tokens": 999,  // 上游已经在顶级写了
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        });
        let lifted = lift_openai_usage_details(raw);
        // 顶级显式值优先，不被 details 覆盖
        assert_eq!(lifted["usage"]["cached_tokens"], 999);
    }
}
