//! gate-providers: 上游 LLM 适配层。
//!
//! 设计原则：
//! - [`Provider`] trait 定义统一接口（chat completions 流式 + 非流式）
//! - 每个上游一个模块（`openai`/`anthropic`/...），实现 trait
//! - 协议归一：对外 OpenAI 兼容；上游差异在 provider 内吸收
//! - 错误经 [`ProviderError`] 收口，给 server 层映射 4xx/5xx
//! - [`ProviderRouter`] 按 project_id + model 动态选路

pub mod error;
pub mod openai;
pub mod router;
pub mod types;

pub use error::{ProviderError, ProviderResult};
pub use router::ProviderRouter;
pub use types::{
    ChatChoice, ChatMessage, ChatRequest, ChatResponse, ChatStreamChunk, FinishReason, Role,
    Usage,
};

use async_trait::async_trait;
use futures::stream::BoxStream;

/// 上游 Provider 接口。
///
/// 实现需要：
/// - 把统一 [`ChatRequest`] 翻译成上游格式
/// - 把上游响应翻译回 [`ChatResponse`] / [`ChatStreamChunk`] 流
/// - 透传错误时尽量保留 status code（让 server 层做合适的 4xx/5xx 映射）
#[async_trait]
pub trait Provider: Send + Sync + 'static {
    /// 短名，比如 "openai" / "anthropic"。
    fn name(&self) -> &'static str;

    /// 非流式 chat。
    async fn chat(&self, req: ChatRequest) -> ProviderResult<ChatResponse>;

    /// 流式 chat — 返回一个 SSE chunk 流。
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<ChatStreamChunk>>>;
}
