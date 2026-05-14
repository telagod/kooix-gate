//! DeepSeek provider — OpenAI-compatible API.

use crate::openai::OpenAiProvider;
use crate::{EmbeddingProvider, Provider};
use crate::error::ProviderResult;
use crate::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, EmbeddingRequest, EmbeddingResponse,
};
use async_trait::async_trait;
use futures::stream::BoxStream;

pub struct DeepSeekProvider {
    inner: OpenAiProvider,
}

impl DeepSeekProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        let url = base_url.into();
        let url = if url.is_empty() {
            "https://api.deepseek.com/v1".to_string()
        } else {
            url
        };
        Ok(Self {
            inner: OpenAiProvider::new(url, api_key)?,
        })
    }
}

#[async_trait]
impl Provider for DeepSeekProvider {
    fn name(&self) -> &'static str {
        "deepseek"
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
impl EmbeddingProvider for DeepSeekProvider {
    fn name(&self) -> &'static str {
        "deepseek"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        self.inner.embed(req).await
    }
}
