//! Azure OpenAI provider — deployment-based URL routing.

use crate::error::{ProviderError, ProviderResult};
use crate::openai::{check_status, sse_to_chunks};
use crate::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, EmbeddingRequest, EmbeddingResponse,
};
use crate::{EmbeddingProvider, Provider};
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};

#[derive(Clone)]
pub struct AzureProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    api_version: String,
}

impl AzureProvider {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        api_version: Option<String>,
    ) -> ProviderResult<Self> {
        Self::new_with_opts(
            endpoint,
            api_key,
            api_version,
            crate::ProviderOpts::default(),
        )
    }

    pub fn new_with_opts(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        api_version: Option<String>,
        opts: crate::ProviderOpts,
    ) -> ProviderResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(opts.connect_timeout())
            .timeout(opts.timeout_duration())
            .build()
            .map_err(|e| ProviderError::Config(e.to_string()))?;
        Ok(Self {
            client,
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            api_version: api_version.unwrap_or_else(|| "2024-08-01-preview".to_string()),
        })
    }

    fn chat_url(&self, model: &str) -> String {
        format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint, model, self.api_version
        )
    }

    fn embeddings_url(&self, model: &str) -> String {
        format!(
            "{}/openai/deployments/{}/embeddings?api-version={}",
            self.endpoint, model, self.api_version
        )
    }
}

#[async_trait]
impl Provider for AzureProvider {
    fn name(&self) -> &'static str {
        "azure"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let url = self.chat_url(&req.model);
        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }

    async fn chat_stream(
        &self,
        mut req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>> {
        req.stream = true;
        let url = self.chat_url(&req.model);
        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        Ok(sse_to_chunks(resp.bytes_stream()).boxed())
    }
}

#[async_trait]
impl EmbeddingProvider for AzureProvider {
    fn name(&self) -> &'static str {
        "azure"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        let url = self.embeddings_url(&req.model);
        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        Ok(resp.json().await?)
    }
}
