//! Gemini 适配器 — 使用 Google 的 OpenAI 兼容端点。
//!
//! Google 提供了 `v1beta/openai/chat/completions` 端点，请求/响应格式
//! 与 OpenAI 完全一致，因此本模块是 [`OpenAiProvider`] 的薄封装，
//! 仅改变 URL 前缀和默认 base_url。
//!
//! 配置：
//! - `base_url`：默认 `https://generativelanguage.googleapis.com`
//! - `api_key`：通过 `Authorization: Bearer` header 传递（同 OpenAI）

use crate::Provider;
use crate::error::ProviderResult;
use crate::openai::OpenAiProvider;
use crate::types::{ChatRequest, ChatResponse, ChatStreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

/// Gemini OpenAI-compat 端点的 URL 后缀。
///
/// Google 的兼容端点格式：`{base_url}/v1beta/openai/chat/completions`
/// 而 [`OpenAiProvider`] 期望 base_url 直接拼 `/chat/completions`。
/// 所以我们传入 `{base_url}/v1beta/openai` 作为 inner 的 base_url。
pub struct GeminiProvider {
    inner: OpenAiProvider,
}

impl GeminiProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> ProviderResult<Self> {
        let base = base_url.into();
        let base = base.trim_end_matches('/');
        // OpenAiProvider 会在 base_url 后拼 /chat/completions
        let compat_url = format!("{base}/v1beta/openai");
        let inner = OpenAiProvider::new(compat_url, api_key)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_name_is_gemini() {
        let p = GeminiProvider::new("https://generativelanguage.googleapis.com", "test-key")
            .unwrap();
        assert_eq!(p.name(), "gemini");
    }

    #[test]
    fn default_url_construction() {
        // 验证不会 panic
        let _p = GeminiProvider::new("https://generativelanguage.googleapis.com", "k").unwrap();
    }

    #[test]
    fn trailing_slash_stripped() {
        let _p =
            GeminiProvider::new("https://generativelanguage.googleapis.com/", "k").unwrap();
    }
}
