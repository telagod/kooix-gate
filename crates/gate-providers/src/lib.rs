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
// 0.4.65：4 个 fast-path provider 共享同一进程内的 reqwest pool。
// 0.4.105（followup §3.3）：把 eviction 从 clear-all 改为 LRU per-key eviction，
//   避免任何"第 9 个 timeout 桶"触发全清空 + 全 channel 重连雷暴。
//   策略：超限时删 last_used 最旧的一个 entry。
//
// 设计：
// - 维度只看 (connect_timeout_ms, total_timeout_ms)；其余 reqwest 选项走默认。
// - LRU 上限 16（从 8 调高，给 plugin manifest custom timeout 留余量）。
// - CustomHttpProvider 不走此 cache（需独立 dns_resolver + redirect=none）。
// ============================================================================

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
struct HttpClientKey {
    connect_ms: u64,
    timeout_ms: u64,
}

const SHARED_CLIENT_CACHE_LIMIT: usize = 16;

struct CachedClient {
    client: Arc<reqwest::Client>,
    last_used: std::time::Instant,
}

fn shared_client_cache() -> &'static Mutex<HashMap<HttpClientKey, CachedClient>> {
    static CACHE: OnceLock<Mutex<HashMap<HttpClientKey, CachedClient>>> = OnceLock::new();
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
    let now = std::time::Instant::now();
    let mut cache = shared_client_cache().lock();

    if let Some(entry) = cache.get_mut(&key) {
        entry.last_used = now;
        metrics::counter!("gate_providers_shared_client_hits_total").increment(1);
        return Ok(Arc::clone(&entry.client));
    }

    // LRU per-key eviction：超限时只删最久未用的一个，不全清空
    if cache.len() >= SHARED_CLIENT_CACHE_LIMIT {
        if let Some(victim_key) = cache
            .iter()
            .min_by_key(|(_, v)| v.last_used)
            .map(|(k, _)| *k)
        {
            cache.remove(&victim_key);
            metrics::counter!("gate_providers_shared_client_evictions_total").increment(1);
        }
    }

    let client = reqwest::Client::builder()
        .connect_timeout(opts.connect_timeout())
        .timeout(opts.timeout_duration())
        .build()
        .map_err(|e| ProviderError::Config(e.to_string()))?;
    let arc = Arc::new(client);
    cache.insert(
        key,
        CachedClient {
            client: Arc::clone(&arc),
            last_used: now,
        },
    );
    metrics::counter!("gate_providers_shared_client_misses_total").increment(1);
    metrics::gauge!("gate_providers_shared_client_size").set(cache.len() as f64);
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

    // 0.4.105：所有 cache 行为 test 合到一个 #[test]，避免 cargo test 并发跑时
    // 共享 OnceLock cache 互相干扰。每段开头 _reset_shared_http_clients()。
    #[test]
    fn shared_clients_full_behavior() {
        // ── 1. same opts → 同 Arc ──
        _reset_shared_http_clients();
        let opts = ProviderOpts::default();
        let a = shared_http_client(&opts).expect("client a");
        let b = shared_http_client(&opts).expect("client b");
        assert!(Arc::ptr_eq(&a, &b), "same opts must hand out identical Arc");

        // ── 2. different opts → 不同 Arc ──
        _reset_shared_http_clients();
        let a = shared_http_client(&ProviderOpts { timeout_ms: 30_000 }).expect("a");
        let b = shared_http_client(&ProviderOpts { timeout_ms: 60_000 }).expect("b");
        assert!(
            !Arc::ptr_eq(&a, &b),
            "different timeout buckets should not share the same client"
        );

        // ── 3. LRU eviction：超限只删 1 个，不清空全部 ──
        _reset_shared_http_clients();
        let mut clients = Vec::new();
        for i in 0..16 {
            let c = shared_http_client(&ProviderOpts {
                timeout_ms: 1000 + i as u64,
            })
            .expect("fill");
            clients.push(c);
        }
        // 访问 timeout=1001 让它成为 most-recently-used
        let bump = shared_http_client(&ProviderOpts { timeout_ms: 1001 }).expect("bump");
        assert!(Arc::ptr_eq(&bump, &clients[1]), "hit should reuse same Arc");

        // 加新 timeout 触发 evict（应该删 timeout=1000 即 clients[0]，最老）
        let _new = shared_http_client(&ProviderOpts { timeout_ms: 99_999 }).expect("overflow");

        // clients[0] 已 evict → 再访问得新 Arc
        let revisit = shared_http_client(&ProviderOpts { timeout_ms: 1000 }).expect("revisit");
        assert!(
            !Arc::ptr_eq(&revisit, &clients[0]),
            "oldest client should be evicted, new Arc on revisit"
        );

        // clients[5] (timeout=1005) 仍在 cache → 同 Arc（验证没清空全部）
        let still = shared_http_client(&ProviderOpts { timeout_ms: 1005 }).expect("middle");
        assert!(
            Arc::ptr_eq(&still, &clients[5]),
            "middle entries should NOT be evicted (no clear-all)"
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
