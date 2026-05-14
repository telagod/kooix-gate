//! OpenAI 兼容 provider —— 直接透传 + embedding。

use crate::{AudioProvider, EmbeddingProvider, ImageProvider, Provider};
use crate::error::{ProviderError, ProviderResult};
use crate::types::{
    AudioSpeechRequest, AudioTranscriptionResponse,
    ChatRequest, ChatResponse, ChatStreamChunk, EmbeddingRequest, EmbeddingResponse,
    ImageGenerationRequest, ImageGenerationResponse,
};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};

#[derive(Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
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
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
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
        let parsed: ChatResponse = resp.json().await?;
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
    Ok(())
}

pub(crate) fn sse_to_chunks<S>(
    byte_stream: S,
) -> impl futures::Stream<Item = ProviderResult<ChatStreamChunk>>
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
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

fn drain_events(buf: &mut Vec<u8>) -> Vec<ProviderResult<ChatStreamChunk>> {
    let mut out = Vec::new();
    while let Some(idx) = find_double_newline(buf) {
        let event_bytes: Vec<u8> = buf.drain(..idx + 2).collect();
        let s = String::from_utf8_lossy(&event_bytes);
        for line in s.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return out;
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
