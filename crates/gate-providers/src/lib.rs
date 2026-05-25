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
pub mod capabilities;
pub mod custom_provider;
pub mod error;
pub mod openai;
pub mod plugin_manifest;
pub(crate) mod plugin_preset;
pub mod retry;
pub mod router;
pub(crate) mod sigv4;
pub mod sse;
pub mod types;

pub use capabilities::{
    ProviderCapabilities, ProviderCapability, plugin_preset_base_url_suggestion,
    plugin_preset_capabilities, provider_base_url_suggestion, provider_capabilities,
};
pub use custom_provider::{CustomHttpProvider, replay_plugin_sse};
pub use error::{ProviderError, ProviderResult, redact_upstream_body};
pub use plugin_manifest::{
    CapabilitiesManifest, ChannelPluginMapping, PluginManifest, PluginPermissionsManifest,
    ProbeManifest, plugin_manifest, plugin_manifest_schema_json, validate_plugin_manifest,
};
pub use router::{
    ChannelMetrics, ChannelRateCheck, ChannelRateLimiter, InMemoryChannelRateLimiter,
    InflightTracker, ProviderRouter, ProviderRuntimeChannelSnapshot, ProviderRuntimeSnapshot,
    RouteCandidateTrace, RouteDecisionTrace, RouteSkipTrace, RoutedAudioProvider,
    RoutedEmbeddingProvider, RoutedImageProvider, RoutedProvider,
};
pub use types::{
    AudioSpeechRequest, AudioTranscriptionResponse, ChatChoice, ChatDelta, ChatMessage,
    ChatRequest, ChatResponse, ChatStreamChoice, ChatStreamChunk, ContentPart, ContentType,
    EmbeddingInput, EmbeddingRequest, EmbeddingResponse, EmbeddingUsage, FinishReason,
    FunctionCall, FunctionDef, ImageData, ImageGenerationRequest, ImageGenerationResponse,
    ImageUrl, MessageContent, ModelInfo, ModelListResponse, Role, ToolCall, ToolCallDelta, ToolDef,
    Usage,
};

use async_trait::async_trait;
use futures::stream::BoxStream;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone)]
pub struct ProviderOpts {
    pub timeout_ms: u64,
}

impl Default for ProviderOpts {
    fn default() -> Self {
        Self {
            timeout_ms: 600_000,
        } // 10 min default
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

// ============================================================================
// SharedHttpClient — 按 (connect_timeout, total_timeout) 维度缓存的 reqwest::Client。
//
// 0.4.65：4 个 fast-path provider（OpenAI / Anthropic / Azure / Bedrock）共享同一
// 进程内的 reqwest pool，避免每次 build provider 都新建一个独立连接池——这在
// channel 数量多且 base_url 重叠（多个 channel 指向同一上游）时会导致严重的
// TCP/TLS 握手成本浪费、HTTP2 multiplexing 失效。
//
// 设计：
// - 维度只看 (connect_timeout_ms, total_timeout_ms)；其余 reqwest 选项（rustls /
//   keepalive / http2 prior knowledge / pool idle）走 reqwest 默认即可。
// - LRU 上限 8（远小于实际 channel 数；同 timeout 共用一个 client 已能覆盖 99%
//   场景）。超出时直接清空重建——保守做法，保证不会内存泄漏。
// - CustomHttpProvider 仍走独立 builder 链：它需要每 channel 一份 dns_resolver
//   sandbox + redirect=none + manifest 自带 timeout override，无法共享。
//
// 验收：crates/gate-providers/tests 内 reqwest::Client::builder() 命中数 >0
// 仅出现在测试 fixture / custom_provider；4 个 fast-path provider 改走 helper。
// ============================================================================

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
struct HttpClientKey {
    connect_ms: u64,
    timeout_ms: u64,
}

const SHARED_CLIENT_CACHE_LIMIT: usize = 8;

fn shared_client_cache() -> &'static Mutex<HashMap<HttpClientKey, Arc<reqwest::Client>>> {
    static CACHE: OnceLock<Mutex<HashMap<HttpClientKey, Arc<reqwest::Client>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取一个按 ProviderOpts 维度共享的 reqwest::Client。
///
/// 同一进程内、相同 timeout 配置的 caller 拿到同一个 Arc<Client>，从而复用
/// connection pool / TLS session / HTTP2 stream。
pub fn shared_http_client(opts: &ProviderOpts) -> ProviderResult<Arc<reqwest::Client>> {
    let key = HttpClientKey {
        connect_ms: opts.connect_timeout().as_millis() as u64,
        timeout_ms: opts.timeout_ms,
    };
    let mut cache = shared_client_cache().lock();
    if let Some(c) = cache.get(&key) {
        return Ok(Arc::clone(c));
    }
    if cache.len() >= SHARED_CLIENT_CACHE_LIMIT {
        cache.clear();
    }
    let client = reqwest::Client::builder()
        .connect_timeout(opts.connect_timeout())
        .timeout(opts.timeout_duration())
        .build()
        .map_err(|e| ProviderError::Config(e.to_string()))?;
    let arc = Arc::new(client);
    cache.insert(key, Arc::clone(&arc));
    Ok(arc)
}

/// 仅测试用：清空共享 client 缓存，避免跨测试 case 污染连接池状态。
#[doc(hidden)]
pub fn _reset_shared_http_clients() {
    shared_client_cache().lock().clear();
}

#[cfg(test)]
mod shared_client_tests {
    use super::*;

    #[test]
    fn shared_clients_with_same_opts_are_identical_arc() {
        _reset_shared_http_clients();
        let opts = ProviderOpts::default();
        let a = shared_http_client(&opts).expect("client a");
        let b = shared_http_client(&opts).expect("client b");
        assert!(Arc::ptr_eq(&a, &b), "same opts must hand out identical Arc");
    }

    #[test]
    fn shared_clients_with_different_opts_are_distinct() {
        _reset_shared_http_clients();
        let a = shared_http_client(&ProviderOpts { timeout_ms: 30_000 }).expect("a");
        let b = shared_http_client(&ProviderOpts { timeout_ms: 60_000 }).expect("b");
        assert!(
            !Arc::ptr_eq(&a, &b),
            "different timeout buckets should not share the same client"
        );
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

#[async_trait]
pub trait ImageProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn generate_image(
        &self,
        req: ImageGenerationRequest,
    ) -> ProviderResult<ImageGenerationResponse>;
}

#[async_trait]
pub trait AudioProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn speech(&self, req: AudioSpeechRequest) -> ProviderResult<bytes::Bytes>;
    async fn transcription(
        &self,
        audio: bytes::Bytes,
        filename: String,
        model: String,
        language: Option<String>,
    ) -> ProviderResult<AudioTranscriptionResponse>;
}
