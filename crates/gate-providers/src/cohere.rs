//! Cohere provider — native API for chat + embeddings.

use crate::openai::{check_status, sse_to_chunks};
use crate::{EmbeddingProvider, Provider};
use crate::error::{ProviderError, ProviderResult};
use crate::types::*;
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use serde::{Deserialize, Serialize};

pub struct CohereProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl CohereProvider {
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
        let url = base_url.into();
        let url = if url.is_empty() {
            "https://api.cohere.com/v2".to_string()
        } else {
            url
        };
        Ok(Self {
            client,
            base_url: url.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        })
    }
}

#[async_trait]
impl Provider for CohereProvider {
    fn name(&self) -> &'static str {
        "cohere"
    }

    async fn chat(&self, mut req: ChatRequest) -> ProviderResult<ChatResponse> {
        req.stream = false;
        let resp = self
            .client
            .post(format!("{}/chat", self.base_url))
            .bearer_auth(&self.api_key)
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
        let resp = self
            .client
            .post(format!("{}/chat", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;
        check_status(&resp)?;
        Ok(sse_to_chunks(resp.bytes_stream()).boxed())
    }
}

#[derive(Serialize)]
struct CohereEmbedRequest {
    model: String,
    texts: Vec<String>,
    input_type: String,
    embedding_types: Vec<String>,
}

#[derive(Deserialize)]
struct CohereEmbedResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    float: Option<Vec<Vec<f32>>>,
}

#[async_trait]
impl EmbeddingProvider for CohereProvider {
    fn name(&self) -> &'static str {
        "cohere"
    }

    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse> {
        let texts = match &req.input {
            EmbeddingInput::Single(s) => vec![s.clone()],
            EmbeddingInput::Multiple(v) => v.clone(),
        };
        let body = CohereEmbedRequest {
            model: req.model.clone(),
            texts: texts.clone(),
            input_type: "search_document".to_string(),
            embedding_types: vec!["float".to_string()],
        };
        let resp = self
            .client
            .post(format!("{}/embed", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        check_status(&resp)?;
        let resp = resp.error_for_status().map_err(ProviderError::from)?;
        let parsed: CohereEmbedResponse = resp.json().await?;

        let data = parsed
            .embeddings
            .float
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, emb)| EmbeddingData {
                object: "embedding".to_string(),
                index: i as u32,
                embedding: emb,
            })
            .collect();

        let prompt_tokens = texts.iter().map(|t| t.len() as u32 / 4).sum::<u32>();
        Ok(EmbeddingResponse {
            object: "list".to_string(),
            data,
            model: req.model,
            usage: EmbeddingUsage {
                prompt_tokens,
                total_tokens: prompt_tokens,
            },
        })
    }
}
