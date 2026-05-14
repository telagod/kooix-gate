//! Ollama provider — local model serving, OpenAI-compatible API.

use crate::openai::OpenAiProvider;
use crate::{EmbeddingProvider, Provider};
use crate::error::ProviderResult;
use crate::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, EmbeddingRequest, EmbeddingResponse,
};
use async_trait::async_trait;
use futures::stream::BoxStream;

pub struct OllamaProvider {
    inner: OpenAiProvider,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> ProviderResult<Self> {
        Self::new_with_opts(base_url, crate::ProviderOpts::default())
    }

    pub fn new_with_opts(
        base_url: impl Into<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let url = base_url.into();
        let url = if url.is_empty() {
            "http://localhost:11434/v1".to_string()
        } else {
            url
        };
        Ok(Self {
            inner: OpenAiProvider::new_with_opts(url, "ollama", opts)?,
        })
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
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
impl EmbeddingProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        self.inner.embed(req).await
    }
}
