//! Gemini 适配器 — Google OpenAI-compat endpoint.

use crate::error::ProviderResult;
use crate::openai::OpenAiProvider;
use crate::types::*;
use crate::{EmbeddingProvider, Provider};
use async_trait::async_trait;
use futures::stream::BoxStream;

pub struct GeminiProvider {
    inner: OpenAiProvider,
}

impl GeminiProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        Self::new_with_opts(base_url, api_key, crate::ProviderOpts::default())
    }

    pub fn new_with_opts(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let base = base_url.into();
        let base = base.trim_end_matches('/');
        let compat_url = format!("{base}/v1beta/openai");
        let inner = OpenAiProvider::new_with_opts(compat_url, api_key, opts)?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse> {
        self.inner.chat(req).await
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        self.inner.chat_stream(req).await
    }
}

#[async_trait]
impl EmbeddingProvider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        self.inner.embed(req).await
    }
}
