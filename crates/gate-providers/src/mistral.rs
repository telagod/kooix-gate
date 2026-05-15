//! Mistral AI provider — OpenAI-compatible API.

use crate::error::ProviderResult;
use crate::openai::OpenAiProvider;
use crate::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, EmbeddingRequest, EmbeddingResponse,
};
use crate::{EmbeddingProvider, Provider};
use async_trait::async_trait;
use futures::stream::BoxStream;

pub struct MistralProvider {
    inner: OpenAiProvider,
}

impl MistralProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        Self::new_with_opts(base_url, api_key, crate::ProviderOpts::default())
    }

    pub fn new_with_opts(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let url = base_url.into();
        let url = if url.is_empty() {
            "https://api.mistral.ai/v1".to_string()
        } else {
            url
        };
        Ok(Self {
            inner: OpenAiProvider::new_with_opts(url, api_key, opts)?,
        })
    }
}

#[async_trait]
impl Provider for MistralProvider {
    fn name(&self) -> &'static str {
        "mistral"
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
impl EmbeddingProvider for MistralProvider {
    fn name(&self) -> &'static str {
        "mistral"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        self.inner.embed(req).await
    }
}
