//! gate-providers: 上游 LLM 适配层。
//!
//! 设计原则：
//! - [`Provider`] trait 定义统一接口（chat completions 流式 + 非流式）
//! - [`EmbeddingProvider`] trait 定义 embedding 接口
//! - 每个上游一个模块（`openai`/`anthropic`/...），实现 trait
//! - 协议归一：对外 OpenAI 兼容；上游差异在 provider 内吸收
//! - 错误经 [`ProviderError`] 收口，给 server 层映射 4xx/5xx
//! - [`ProviderRouter`] 按 project_id + model 动态选路

pub mod adapt;
pub mod anthropic;
pub mod azure;
pub mod bedrock;
pub mod cohere;
pub mod deepseek;
pub mod error;
pub mod gemini;
pub mod mistral;
pub mod ollama;
pub mod openai;
pub mod retry;
pub mod router;
pub mod types;

pub use error::{ProviderError, ProviderResult};
pub use router::{ChannelMetrics, ChannelRateCheck, ChannelRateLimiter, InMemoryChannelRateLimiter, InflightTracker, ProviderRouter, RoutedEmbeddingProvider, RoutedProvider};
pub use types::{
    ChatChoice, ChatDelta, ChatMessage, ChatRequest, ChatResponse, ChatStreamChunk,
    ChatStreamChoice, ContentPart, ContentType, EmbeddingInput, EmbeddingRequest,
    EmbeddingResponse, EmbeddingUsage, FinishReason, FunctionCall, FunctionDef, ImageUrl,
    MessageContent, ModelInfo, ModelListResponse, Role, ToolCall, ToolCallDelta, ToolDef, Usage,
};

use async_trait::async_trait;
use futures::stream::BoxStream;

#[derive(Debug, Clone)]
pub struct ProviderOpts {
    pub timeout_ms: u64,
}

impl Default for ProviderOpts {
    fn default() -> Self {
        Self { timeout_ms: 600_000 } // 10 min default
    }
}

impl ProviderOpts {
    pub fn timeout_duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms)
    }

    pub fn connect_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(10)
    }
}

#[async_trait]
pub trait Provider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse>;
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>>;
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn embed(&self, req: EmbeddingRequest) -> ProviderResult<EmbeddingResponse>;
}
