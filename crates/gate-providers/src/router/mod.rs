//! ProviderRouter — 按 project_id + model 选择 Provider。
//!
//! 路由逻辑：
//! 1. 如果有 ModelAliasRepo，先做 alias → target_model 解析
//! 2. 从 ChannelGroupRepo 取 project 的默认分组（`projects.default_group_id`）
//! 3. 从 ChannelRepo 取分组内所有 healthy channel，按 strategy 选一个
//!    - priority（默认）：取 priority 数值最小的那条
//!    - weighted_random：按 weight 做加权随机
//!    - round_robin：循环轮转
//!    - least_conn：选 inflight 最少的 channel
//! 4. 用 channel.provider_type 构造对应的 Provider（openai / anthropic / gemini）
//! 5. Secret 来源策略（G1/P1）：
//!    a. plugin manifest 可用 `secret_slot` 引用 channel_keys.label
//!    b. 优先从 channel_keys 表取 active key(s) → 用 EnvelopeKms 解密
//!    c. 若 DB 无 key 或 repo 未配置 → 回退 env var
//! 6. 找不到 channel_group 或 channel → 返回 None，调用方 fallback 到 AppState.provider

use crate::AudioProvider;
use crate::EmbeddingProvider;
use crate::ImageProvider;
use crate::Provider;
// Compile-time providers (AnthropicProvider / AzureProvider / BedrockProvider /
// CohereProvider / CustomHttpProvider / DeepSeekProvider / GeminiProvider /
// MistralProvider / OllamaProvider / OpenAiProvider) 已 move 到 router::builder。
#[cfg(test)]
use crate::error::NormalizedProviderErrorKind;
use crate::error::{ProviderError, ProviderResult};
use crate::plugin_manifest::plugin_manifest_retry_config;
// `plugin_manifest` / `ProviderCapabilities` / `provider_capabilities` 已 move 到 router::helpers。
use gate_core::id::{ChannelId, ChannelKeyId, ProjectId};
use gate_crypto::EnvelopeKms;
use gate_storage::{
    ChannelBinding, ChannelGroupRepo, ChannelKeyRepo, ChannelLatencyRepo, ChannelRepo,
    ModelAliasRepo,
};
use parking_lot::RwLock;
// use serde::{Deserialize, Serialize}; (moved to trace)
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

mod builder;
mod helpers;
mod metrics;
mod routed;
mod selection;
mod trace;

use builder::{
    build_audio_provider, build_embedding_provider, build_embedding_provider_with_secrets,
    build_image_provider, build_provider, build_provider_with_secrets, resolve_api_key_for_channel,
};
use helpers::{
    channel_capabilities, env_secret_map, fallback_models, is_plugin_provider,
    normalize_secret_slot, resolve_model_mapping, supports_audio_runtime, supports_image_runtime,
};

use metrics::DEFAULT_CHANNEL_LATENCY_WINDOW_SECS;
pub use metrics::{
    ChannelMetrics, ChannelRateCheck, ChannelRateLimiter, InMemoryChannelRateLimiter,
    InflightTracker,
};
pub use routed::{
    RoutedAudioProvider, RoutedEmbeddingProvider, RoutedImageProvider, RoutedProvider,
};
use selection::order_channels_by_strategy;
pub use trace::{
    ProviderRuntimeChannelSnapshot, ProviderRuntimeSnapshot, RouteCandidateTrace,
    RouteDecisionTrace, RouteSkipTrace,
};
use trace::{RouteAttempt, RouteMiss, RouteMissReason, route_not_found_message};

#[derive(Clone)]
struct ResolvedChannelSecrets {
    primary: String,
    key_id: Option<ChannelKeyId>,
    secrets: HashMap<String, String>,
}

impl ResolvedChannelSecrets {
    fn into_parts(self) -> (String, Option<ChannelKeyId>, HashMap<String, String>) {
        (self.primary, self.key_id, self.secrets)
    }
}

struct CachedChannelSecrets {
    resolved: ResolvedChannelSecrets,
    expires_at: Instant,
}

const DEFAULT_CHANNEL_KEY_CACHE_TTL_SECS: u64 = 30;

fn default_channel_key_cache_ttl() -> Duration {
    std::env::var("KOOIX_CHANNEL_KEY_CACHE_TTL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_CHANNEL_KEY_CACHE_TTL_SECS))
}

/// 多 Provider 路由器。
///
/// 持有 Repo 引用（Arc），路由元数据仍惰性查询；channel key 明文只做短 TTL 缓存，
/// 管理面 create / rotate / revoke 会显式失效，外部 DB 变更最迟 TTL 后生效。
pub struct ProviderRouter {
    channel_repo: Arc<dyn ChannelRepo>,
    group_repo: Arc<dyn ChannelGroupRepo>,
    model_alias_repo: Option<Arc<dyn ModelAliasRepo>>,
    /// G1: channel_keys 表读取（加密 key 存储）。
    channel_key_repo: Option<Arc<dyn ChannelKeyRepo>>,
    /// G1: 解密 channel key 的 envelope KMS。
    crypto: Option<Arc<EnvelopeKms>>,
    /// P2.2: channel_keys 解密结果短 TTL 缓存，避免热路径重复 KMS unwrap。
    channel_secret_cache: RwLock<HashMap<ChannelId, CachedChannelSecrets>>,
    channel_secret_cache_ttl: Duration,
    /// round_robin 策略的全局计数器。
    rr_counter: AtomicU64,
    /// Deterministic canary gate counter. Avoids flaky RNG and keeps each bps
    /// target bounded over long windows.
    canary_counter: AtomicU64,
    /// least_conn 策略的 inflight 计数器。
    inflight: Arc<InflightTracker>,
    /// 滑动窗口成功率追踪（auto-disable 机制）。
    metrics: Option<Arc<ChannelMetrics>>,
    /// 持久化 latency samples；least_latency 先查它，再 fail-open 回退内存 metrics。
    channel_latency_repo: Option<Arc<dyn ChannelLatencyRepo>>,
    latency_window_secs: i64,
    /// per-channel RPM/TPM 限速器。
    rate_limiter: Arc<dyn ChannelRateCheck>,
    /// Repo-backed runtime 的单调版本钩子；control plane 热更新 snapshot 后可递增。
    snapshot_version: AtomicU64,
    /// Atomically replaceable compiled snapshot metadata for hot-path observability.
    runtime_snapshot: RwLock<Arc<ProviderRuntimeSnapshot>>,
    /// 0.4.57: ADR-0003 v0 WASM Plugin runtime host（全局共享）。
    /// 若 channel manifest.security.wasm 配置则由 builder 自动 mount + 调用 with_wasm_host。
    wasm_host: Option<Arc<dyn gate_wasm::WasmHost>>,
    /// 0.4.143: WASM blob store —— auto-mount 装配链的 backend。
    wasm_blob_store: Option<Arc<dyn gate_wasm::WasmBlobStore>>,
}

impl ProviderRouter {
    pub fn new(channel_repo: Arc<dyn ChannelRepo>, group_repo: Arc<dyn ChannelGroupRepo>) -> Self {
        Self {
            channel_repo,
            group_repo,
            model_alias_repo: None,
            channel_key_repo: None,
            crypto: None,
            channel_secret_cache: RwLock::new(HashMap::new()),
            channel_secret_cache_ttl: default_channel_key_cache_ttl(),
            rr_counter: AtomicU64::new(0),
            canary_counter: AtomicU64::new(0),
            inflight: Arc::new(InflightTracker::new()),
            metrics: Some(Arc::new(ChannelMetrics::new(10, 0.8))),
            channel_latency_repo: None,
            latency_window_secs: DEFAULT_CHANNEL_LATENCY_WINDOW_SECS,
            rate_limiter: Arc::new(InMemoryChannelRateLimiter::new()),
            snapshot_version: AtomicU64::new(1),
            runtime_snapshot: RwLock::new(Arc::new(ProviderRuntimeSnapshot::new(1, Vec::new()))),
            wasm_host: None,
            wasm_blob_store: None,
        }
    }

    /// 0.4.57: 挂载 ADR-0003 v0 WASM Plugin runtime host。
    /// 配置后 builder 创建 CustomHttpProvider 时按 channel manifest.security.wasm
    /// 自动 with_wasm_host(host, channel_id)。
    pub fn with_wasm_host(mut self, host: Arc<dyn gate_wasm::WasmHost>) -> Self {
        self.wasm_host = Some(host);
        self
    }

    /// 0.4.57: 当前注入的 wasm host（getter，用于 builder 调用）。
    pub fn wasm_host(&self) -> Option<Arc<dyn gate_wasm::WasmHost>> {
        self.wasm_host.clone()
    }

    /// 0.4.143（按 product-gaps G-002 step 2/2）：挂载 WASM blob store。
    /// 配置后 ProviderRouter 在 reload 阶段会迭代 channel.manifest.security.wasm
    /// 字段，按 sha256 fetch 字节 → host.load_module → 与 with_wasm_host 联动。
    ///
    /// 当前仅暴露 setter；自动装配链（reload 时迭代 + fetch + load_module）的
    /// 真实运转在 v0.5.x 实装（需要 manifest schema 加 module_sha256 字段 +
    /// 调用方决定何时 reload + 失败回滚 strategy）。
    pub fn with_wasm_blob_store(
        mut self,
        store: Arc<dyn gate_wasm::WasmBlobStore>,
    ) -> Self {
        self.wasm_blob_store = Some(store);
        self
    }

    /// 0.4.143: 当前注入的 wasm blob store（getter）。
    pub fn wasm_blob_store(&self) -> Option<Arc<dyn gate_wasm::WasmBlobStore>> {
        self.wasm_blob_store.clone()
    }

    /// 挂载 ModelAliasRepo，启用 alias 解析。
    pub fn with_model_alias_repo(mut self, repo: Arc<dyn ModelAliasRepo>) -> Self {
        self.model_alias_repo = Some(repo);
        self
    }

    /// 挂载 ChannelKeyRepo，启用 DB 密钥读取。
    pub fn with_channel_key_repo(mut self, repo: Arc<dyn ChannelKeyRepo>) -> Self {
        self.channel_key_repo = Some(repo);
        self
    }

    /// 挂载 EnvelopeKms，用于解密 DB 中的 channel key。
    pub fn with_crypto(mut self, kms: Arc<EnvelopeKms>) -> Self {
        self.crypto = Some(kms);
        self
    }

    /// 设置 channel key 解密缓存 TTL。
    ///
    /// `Duration::ZERO` 表示禁用缓存；生产默认由
    /// `KOOIX_CHANNEL_KEY_CACHE_TTL_SECS` 控制，未设置为 30s。
    pub fn with_channel_key_cache_ttl(mut self, ttl: Duration) -> Self {
        self.channel_secret_cache_ttl = ttl;
        self
    }

    /// 显式失效某个 channel 的解密密钥缓存。
    ///
    /// create / rotate / revoke 路径调用该方法，保证控制面变更立即进入数据面。
    pub fn invalidate_channel_key_cache(&self, channel_id: ChannelId) {
        self.channel_secret_cache.write().remove(&channel_id);
        self.bump_snapshot_version();
    }

    /// 清空全部 channel key 解密缓存（运维或测试场景）。
    pub fn clear_channel_key_cache(&self) {
        self.channel_secret_cache.write().clear();
        self.bump_snapshot_version();
    }

    fn cached_channel_secrets(&self, channel_id: ChannelId) -> Option<ResolvedChannelSecrets> {
        if self.channel_secret_cache_ttl.is_zero() {
            return None;
        }

        let now = Instant::now();
        {
            let cache = self.channel_secret_cache.read();
            match cache.get(&channel_id) {
                Some(entry) if entry.expires_at > now => return Some(entry.resolved.clone()),
                Some(_) => {}
                None => return None,
            }
        }

        let mut cache = self.channel_secret_cache.write();
        if cache
            .get(&channel_id)
            .is_some_and(|entry| entry.expires_at <= now)
        {
            cache.remove(&channel_id);
        }
        None
    }

    fn store_channel_secrets(&self, channel_id: ChannelId, resolved: ResolvedChannelSecrets) {
        if self.channel_secret_cache_ttl.is_zero() {
            return;
        }

        self.channel_secret_cache.write().insert(
            channel_id,
            CachedChannelSecrets {
                resolved,
                expires_at: Instant::now() + self.channel_secret_cache_ttl,
            },
        );
    }

    /// 替换默认 ChannelMetrics（自定义 window_size / threshold）。
    pub fn with_metrics(mut self, metrics: Arc<ChannelMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// 挂载持久化 latency repo，启用 least_latency 多实例滑窗。
    pub fn with_channel_latency_repo(mut self, repo: Arc<dyn ChannelLatencyRepo>) -> Self {
        self.channel_latency_repo = Some(repo);
        self
    }

    /// 配置 least_latency 持久化滑窗大小（秒）。
    pub fn with_latency_window_secs(mut self, secs: i64) -> Self {
        self.latency_window_secs = secs.max(1);
        self
    }

    /// 释放 channel 的 inflight 计数（请求结束后调用）。
    ///
    /// 对非 least_conn 策略也是安全的（no-op if channel wasn't tracked）。
    pub fn release_channel(&self, channel_id: ChannelId) {
        self.inflight.release(channel_id);
    }

    /// 获取 inflight tracker 的引用（供调用方在 async 场景中持有）。
    pub fn inflight_tracker(&self) -> Arc<InflightTracker> {
        self.inflight.clone()
    }

    /// 清除 channel 的 metrics 滑动窗口（re-enable 后由 health_check 调用）。
    pub fn clear_channel_metrics(&self, channel_id: ChannelId) {
        if let Some(m) = &self.metrics {
            m.clear(channel_id);
        }
    }

    /// 暴露只读 metrics 句柄给 health checker，用于把 probe 成功率/延迟喂给 least_latency。
    pub fn channel_metrics(&self) -> Option<Arc<ChannelMetrics>> {
        self.metrics.clone()
    }

    /// 记录一次 channel latency observation；DB 故障 fail-open，只告警不阻断数据面。
    pub async fn record_channel_latency(
        &self,
        channel_id: ChannelId,
        latency_ms: u64,
        success: bool,
        source: &str,
    ) {
        let Some(repo) = &self.channel_latency_repo else {
            return;
        };
        if let Err(e) = repo
            .record_sample(channel_id, latency_ms, success, source)
            .await
        {
            tracing::warn!(
                channel_id = %channel_id.as_uuid(),
                source = source,
                error = %e,
                "channel latency sample write failed; falling back to in-memory metrics"
            );
        }
    }

    async fn persistent_latencies_for_strategy(
        &self,
        strategy: &str,
        compatible: &[&ChannelBinding],
    ) -> Option<HashMap<ChannelId, u64>> {
        if strategy != "least_latency" || compatible.len() <= 1 {
            return None;
        }
        let repo = self.channel_latency_repo.as_ref()?;
        let ids: Vec<ChannelId> = compatible
            .iter()
            .map(|candidate| candidate.channel.channel_id)
            .collect();
        match repo.avg_latency_ms(&ids, self.latency_window_secs).await {
            Ok(latencies) if !latencies.is_empty() => Some(latencies),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    window_secs = self.latency_window_secs,
                    "channel latency window query failed; falling back to in-memory metrics"
                );
                None
            }
        }
    }

    fn canary_gate_allows(&self, percent_bps: i32) -> bool {
        let percent_bps = percent_bps.clamp(0, 10_000) as u64;
        if percent_bps == 0 {
            return false;
        }
        if percent_bps >= 10_000 {
            return true;
        }
        let seq = self.canary_counter.fetch_add(1, Ordering::Relaxed);
        // 7919 is coprime with 10000, so a full 10k window hits each bucket once.
        (seq.wrapping_mul(7_919) % 10_000) < percent_bps
    }

    fn apply_canary_gate<'a>(
        &self,
        compatible: &[&'a ChannelBinding],
        trace: Option<&mut RouteDecisionTrace>,
    ) -> Vec<&'a ChannelBinding> {
        let mut allowed = Vec::with_capacity(compatible.len());
        let mut trace = trace;
        for candidate in compatible {
            if let Some(percent_bps) = candidate.canary_percent_bps
                && !self.canary_gate_allows(percent_bps)
            {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.record_skip(candidate, "canary_not_selected");
                }
                continue;
            }
            allowed.push(*candidate);
        }
        allowed
    }

    /// 获取 rate_limiter 的引用（供调用方上报 token 消耗）。
    pub fn rate_limiter(&self) -> Arc<dyn ChannelRateCheck> {
        self.rate_limiter.clone()
    }

    /// 替换 rate limiter 实现（注入 Redis 后端）。
    pub fn with_rate_limiter(mut self, rl: Arc<dyn ChannelRateCheck>) -> Self {
        self.rate_limiter = rl;
        self
    }

    /// 当前 provider runtime snapshot 版本。
    pub fn snapshot_version(&self) -> u64 {
        self.snapshot_version.load(Ordering::Relaxed)
    }

    /// Return the currently published compiled runtime snapshot metadata.
    pub fn runtime_snapshot(&self) -> Arc<ProviderRuntimeSnapshot> {
        self.runtime_snapshot.read().clone()
    }

    /// Publish a freshly compiled runtime snapshot and advance the observable version.
    pub fn replace_runtime_snapshot(&self, channels: Vec<ProviderRuntimeChannelSnapshot>) -> u64 {
        let version = self.snapshot_version.fetch_add(1, Ordering::Relaxed) + 1;
        *self.runtime_snapshot.write() = Arc::new(ProviderRuntimeSnapshot::new(version, channels));
        version
    }

    /// 手动推进 snapshot 版本，供 control plane 配置变更后标记热路径可观测版本。
    pub fn bump_snapshot_version(&self) -> u64 {
        let channels = self.runtime_snapshot.read().channels.clone();
        self.replace_runtime_snapshot(channels)
    }

    /// 根据 project_id + model 选 Provider。
    ///
    /// - `requested_model`：先做 alias 解析，再用于路由
    /// - 返回 `None` 表示找不到可用渠道，调用方 fallback 到全局 provider
    /// - 返回 `Some(RoutedProvider)` 时 channel_id 为计费/审计追溯依据
    pub async fn route(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> ProviderResult<Option<RoutedProvider>> {
        self.route_with_request(project_id, requested_model, None)
            .await
    }

    /// 严格路由：找不到可用 channel 时返回 normalized provider error，而不是 None。
    pub async fn route_chat_required(
        &self,
        project_id: ProjectId,
        req: &crate::types::ChatRequest,
    ) -> ProviderResult<RoutedProvider> {
        match self
            .route_with_request_outcome(project_id, &req.model, Some(req))
            .await?
        {
            Ok(routed) => Ok(routed),
            Err(miss) => Err(miss.provider_error()),
        }
    }

    /// 根据完整 chat request 选 Provider，并按 channel capability 做细粒度过滤。
    pub async fn route_chat(
        &self,
        project_id: ProjectId,
        req: &crate::types::ChatRequest,
    ) -> ProviderResult<Option<RoutedProvider>> {
        self.route_with_request(project_id, &req.model, Some(req))
            .await
    }

    async fn route_with_request(
        &self,
        project_id: ProjectId,
        requested_model: &str,
        req: Option<&crate::types::ChatRequest>,
    ) -> ProviderResult<Option<RoutedProvider>> {
        Ok(self
            .route_with_request_outcome(project_id, requested_model, req)
            .await?
            .ok())
    }

    async fn route_with_request_outcome(
        &self,
        project_id: ProjectId,
        requested_model: &str,
        req: Option<&crate::types::ChatRequest>,
    ) -> ProviderResult<Result<RoutedProvider, Box<RouteMiss>>> {
        // Step 0: alias 解析
        let alias_result = self.resolve_alias(project_id, requested_model).await?;
        let (model, params_override) = match &alias_result {
            Some((target, po)) => (target.as_str(), po.clone()),
            None => (requested_model, serde_json::json!({})),
        };
        let mut trace = RouteDecisionTrace::new(
            self.snapshot_version(),
            requested_model,
            alias_result.as_ref().map(|(target, _)| target.clone()),
            model,
        );

        // Step 1: 尝试主模型路由
        let mut miss = match self
            .route_for_model(project_id, model, req, &mut trace)
            .await?
        {
            RouteAttempt::Routed(mut routed) => {
                routed.params_override = params_override;
                return Ok(Ok(routed));
            }
            RouteAttempt::Miss(miss) => Some(miss),
        };

        // Step 2: 主模型路由失败，尝试 fallback 链
        for fallback in fallback_models(model) {
            let mut miss_before_fallback = miss;
            tracing::info!(
                project_id = %project_id,
                original_model = model,
                fallback_model = fallback,
                "primary model route failed, trying fallback"
            );
            trace.fallbacks.push(fallback.to_string());
            match self
                .route_for_model(project_id, fallback, req, &mut trace)
                .await?
            {
                RouteAttempt::Routed(mut routed) => {
                    routed.params_override = params_override;
                    return Ok(Ok(routed));
                }
                RouteAttempt::Miss(next_miss) => {
                    if !matches!(
                        next_miss.reason,
                        RouteMissReason::NoDefaultGroup | RouteMissReason::NoHealthyChannels
                    ) {
                        miss_before_fallback = Some(next_miss);
                    }
                }
            }
            miss = miss_before_fallback;
        }

        Ok(Err(miss.unwrap_or_else(|| {
            Box::new(RouteMiss {
                reason: RouteMissReason::FallbackExhausted,
                message: route_not_found_message("chat", model, RouteMissReason::FallbackExhausted),
                capability: None,
                selected_model: model.to_string(),
                trace,
            })
        })))
    }

    /// 按 project_id + model 选 EmbeddingProvider。
    ///
    /// 路由逻辑与 `route()` 相同，但构造 `Arc<dyn EmbeddingProvider>` 而非 `Arc<dyn Provider>`。
    /// 不支持 embedding 的 provider type（anthropic、bedrock）会被过滤掉。
    /// 返回 `None` 时调用方应 fallback 或返回 400。
    pub async fn route_embedding(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedEmbeddingProvider>> {
        let alias_result = self.resolve_alias(project_id, model).await?;
        let resolved = match &alias_result {
            Some((target, _)) => target.as_str(),
            None => model,
        };

        if let Some(routed) = self.route_embedding_for_model(project_id, resolved).await? {
            return Ok(Some(routed));
        }

        for fallback in fallback_models(resolved) {
            if let Some(routed) = self.route_embedding_for_model(project_id, fallback).await? {
                return Ok(Some(routed));
            }
        }

        Ok(None)
    }

    /// 为指定 model 做 embedding 路由（group → fallback chain → channel → EmbeddingProvider）。
    async fn route_embedding_for_model(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedEmbeddingProvider>> {
        let initial_group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => return Ok(None),
            Err(e) => {
                return Err(ProviderError::Config(format!(
                    "channel_group lookup failed: {e}"
                )));
            }
        };

        let mut current_group = initial_group;
        let mut depth = 0u8;
        const MAX_FALLBACK_DEPTH: u8 = 5;

        loop {
            if depth > MAX_FALLBACK_DEPTH {
                tracing::warn!(
                    model = model,
                    depth = depth,
                    "embedding fallback chain exceeded max depth"
                );
                return Ok(None);
            }

            if current_group.enabled
                && let Some(routed) = self
                    .try_route_embedding_in_group(&current_group, model)
                    .await?
            {
                return Ok(Some(routed));
            }

            match current_group.fallback_group_id {
                Some(fallback_id) => match self.group_repo.find_by_id(fallback_id).await {
                    Ok(g) => {
                        current_group = g;
                        depth += 1;
                    }
                    Err(_) => return Ok(None),
                },
                None => return Ok(None),
            }
        }
    }

    /// Steps 2-4 for embedding: list channels, filter (model + embedding-capable provider),
    /// select by strategy with rate limit fallback, construct `Arc<dyn EmbeddingProvider>`.
    async fn try_route_embedding_in_group(
        &self,
        group: &gate_storage::ChannelGroupRecord,
        model: &str,
    ) -> ProviderResult<Option<RoutedEmbeddingProvider>> {
        let bindings = self
            .channel_repo
            .list_healthy_in_group(group.group_id)
            .await
            .map_err(|e| ProviderError::Config(format!("channel list failed: {e}")))?;

        if bindings.is_empty() {
            return Ok(None);
        }

        // Filter: model compatible AND provider advertises embedding capability.
        // Runtime HTTP plugin channels are eligible here; candidate loop below
        // still requires an active secret before constructing the provider.
        let compatible: Vec<_> = bindings
            .iter()
            .filter(|b| {
                let model_ok = if !b.model_filter.is_empty() {
                    b.model_filter.iter().any(|m| m == model)
                } else {
                    b.channel.supported_models.is_empty()
                        || b.channel.supported_models.iter().any(|m| m == model)
                };
                let provider_ok = channel_capabilities(&b.channel).embeddings;
                model_ok && provider_ok
            })
            .collect();

        if compatible.is_empty() {
            return Ok(None);
        }

        let compatible = self.apply_canary_gate(&compatible, None);
        if compatible.is_empty() {
            return Ok(None);
        }

        let persistent_latencies = self
            .persistent_latencies_for_strategy(&group.strategy, &compatible)
            .await;
        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
            persistent_latencies.as_ref(),
        );

        for candidate in &ordered {
            if is_plugin_provider(&candidate.channel.provider_type)
                && !self
                    .has_available_plugin_secret(
                        candidate.channel.channel_id,
                        &candidate.channel.code,
                    )
                    .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    "plugin embedding channel has no active secret, trying next channel"
                );
                continue;
            }

            // RPM 检查
            if !self
                .rate_limiter
                .check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached for embedding, trying next"
                );
                continue;
            }

            // TPM 检查
            if !self
                .rate_limiter
                .check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    tpm_limit = ?candidate.channel.tpm_limit,
                    "channel TPM limit reached for embedding, trying next"
                );
                continue;
            }

            let (api_key, key_id, secrets) = self
                .resolve_secrets_for_channel(candidate.channel.channel_id, &candidate.channel.code)
                .await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };

            let provider: Arc<dyn EmbeddingProvider> =
                if is_plugin_provider(&candidate.channel.provider_type) {
                    build_embedding_provider_with_secrets(
                        &candidate.channel,
                        api_key,
                        secrets,
                        opts,
                    )?
                } else {
                    build_embedding_provider(&candidate.channel, api_key, opts)?
                };

            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            return Ok(Some(RoutedEmbeddingProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                group_id: group.group_id,
                provider_type: candidate.channel.provider_type.clone(),
                resolved_model: resolve_model_mapping(&candidate.channel.model_mapping, model),
                key_id,
            }));
        }

        Ok(None)
    }

    /// 按 project_id + model 选 ImageProvider。
    ///
    /// 当前仅 compile-time OpenAI-compatible image provider 支持图片生成；plugin image
    /// runtime adapter 还未实现，所以 plugin channel 即使声明 image 也会被过滤。
    pub async fn route_image(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedImageProvider>> {
        let alias_result = self.resolve_alias(project_id, model).await?;
        let resolved = match &alias_result {
            Some((target, _)) => target.as_str(),
            None => model,
        };

        if let Some(routed) = self.route_image_for_model(project_id, resolved).await? {
            return Ok(Some(routed));
        }

        for fallback in fallback_models(resolved) {
            if let Some(routed) = self.route_image_for_model(project_id, fallback).await? {
                return Ok(Some(routed));
            }
        }

        Ok(None)
    }

    /// 为指定 model 做 image 路由（group → fallback chain → channel → ImageProvider）。
    async fn route_image_for_model(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedImageProvider>> {
        let initial_group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => return Ok(None),
            Err(e) => {
                return Err(ProviderError::Config(format!(
                    "channel_group lookup failed: {e}"
                )));
            }
        };

        let mut current_group = initial_group;
        let mut depth = 0u8;
        const MAX_FALLBACK_DEPTH: u8 = 5;

        loop {
            if depth > MAX_FALLBACK_DEPTH {
                tracing::warn!(
                    model = model,
                    depth = depth,
                    "image fallback chain exceeded max depth"
                );
                return Ok(None);
            }

            if current_group.enabled
                && let Some(routed) = self.try_route_image_in_group(&current_group, model).await?
            {
                return Ok(Some(routed));
            }

            match current_group.fallback_group_id {
                Some(fallback_id) => match self.group_repo.find_by_id(fallback_id).await {
                    Ok(g) => {
                        current_group = g;
                        depth += 1;
                    }
                    Err(_) => return Ok(None),
                },
                None => return Ok(None),
            }
        }
    }

    /// Steps 2-4 for image: list channels, filter (model + image-capable provider),
    /// select by strategy with rate limit fallback, construct `Arc<dyn ImageProvider>`.
    async fn try_route_image_in_group(
        &self,
        group: &gate_storage::ChannelGroupRecord,
        model: &str,
    ) -> ProviderResult<Option<RoutedImageProvider>> {
        let bindings = self
            .channel_repo
            .list_healthy_in_group(group.group_id)
            .await
            .map_err(|e| ProviderError::Config(format!("channel list failed: {e}")))?;

        if bindings.is_empty() {
            return Ok(None);
        }

        let compatible: Vec<_> = bindings
            .iter()
            .filter(|b| {
                let model_ok = if !b.model_filter.is_empty() {
                    b.model_filter.iter().any(|m| m == model)
                } else {
                    b.channel.supported_models.is_empty()
                        || b.channel.supported_models.iter().any(|m| m == model)
                };
                let provider_ok = channel_capabilities(&b.channel).image
                    && supports_image_runtime(&b.channel.provider_type);
                model_ok && provider_ok
            })
            .collect();

        if compatible.is_empty() {
            return Ok(None);
        }

        let compatible = self.apply_canary_gate(&compatible, None);
        if compatible.is_empty() {
            return Ok(None);
        }

        let persistent_latencies = self
            .persistent_latencies_for_strategy(&group.strategy, &compatible)
            .await;
        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
            persistent_latencies.as_ref(),
        );

        for candidate in &ordered {
            if !self
                .rate_limiter
                .check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached for image generation, trying next"
                );
                continue;
            }

            if !self
                .rate_limiter
                .check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    tpm_limit = ?candidate.channel.tpm_limit,
                    "channel TPM limit reached for image generation, trying next"
                );
                continue;
            }

            let (api_key, key_id) = self
                .resolve_key_for_channel(candidate.channel.channel_id, &candidate.channel.code)
                .await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };

            let provider: Arc<dyn ImageProvider> =
                build_image_provider(&candidate.channel, api_key, opts)?;

            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            return Ok(Some(RoutedImageProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                group_id: group.group_id,
                provider_type: candidate.channel.provider_type.clone(),
                resolved_model: resolve_model_mapping(&candidate.channel.model_mapping, model),
                key_id,
            }));
        }

        Ok(None)
    }

    /// 按 project_id + model 选 AudioProvider。
    ///
    /// 当前仅 compile-time OpenAI-compatible audio provider 支持 TTS/STT；
    /// plugin audio runtime adapter 还未实现，所以 plugin channel 即使声明 audio 也会被过滤。
    pub async fn route_audio(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedAudioProvider>> {
        let alias_result = self.resolve_alias(project_id, model).await?;
        let resolved = match &alias_result {
            Some((target, _)) => target.as_str(),
            None => model,
        };

        if let Some(routed) = self.route_audio_for_model(project_id, resolved).await? {
            return Ok(Some(routed));
        }

        for fallback in fallback_models(resolved) {
            if let Some(routed) = self.route_audio_for_model(project_id, fallback).await? {
                return Ok(Some(routed));
            }
        }

        Ok(None)
    }

    /// 为指定 model 做 audio 路由（group → fallback chain → channel → AudioProvider）。
    async fn route_audio_for_model(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedAudioProvider>> {
        let initial_group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => return Ok(None),
            Err(e) => {
                return Err(ProviderError::Config(format!(
                    "channel_group lookup failed: {e}"
                )));
            }
        };

        let mut current_group = initial_group;
        let mut depth = 0u8;
        const MAX_FALLBACK_DEPTH: u8 = 5;

        loop {
            if depth > MAX_FALLBACK_DEPTH {
                tracing::warn!(
                    model = model,
                    depth = depth,
                    "audio fallback chain exceeded max depth"
                );
                return Ok(None);
            }

            if current_group.enabled
                && let Some(routed) = self.try_route_audio_in_group(&current_group, model).await?
            {
                return Ok(Some(routed));
            }

            match current_group.fallback_group_id {
                Some(fallback_id) => match self.group_repo.find_by_id(fallback_id).await {
                    Ok(g) => {
                        current_group = g;
                        depth += 1;
                    }
                    Err(_) => return Ok(None),
                },
                None => return Ok(None),
            }
        }
    }

    /// Steps 2-4 for audio: list channels, filter (model + audio-capable provider),
    /// select by strategy with rate limit fallback, construct `Arc<dyn AudioProvider>`.
    async fn try_route_audio_in_group(
        &self,
        group: &gate_storage::ChannelGroupRecord,
        model: &str,
    ) -> ProviderResult<Option<RoutedAudioProvider>> {
        let bindings = self
            .channel_repo
            .list_healthy_in_group(group.group_id)
            .await
            .map_err(|e| ProviderError::Config(format!("channel list failed: {e}")))?;

        if bindings.is_empty() {
            return Ok(None);
        }

        let compatible: Vec<_> = bindings
            .iter()
            .filter(|b| {
                let model_ok = if !b.model_filter.is_empty() {
                    b.model_filter.iter().any(|m| m == model)
                } else {
                    b.channel.supported_models.is_empty()
                        || b.channel.supported_models.iter().any(|m| m == model)
                };
                let provider_ok = channel_capabilities(&b.channel).audio
                    && supports_audio_runtime(&b.channel.provider_type);
                model_ok && provider_ok
            })
            .collect();

        if compatible.is_empty() {
            return Ok(None);
        }

        let compatible = self.apply_canary_gate(&compatible, None);
        if compatible.is_empty() {
            return Ok(None);
        }

        let persistent_latencies = self
            .persistent_latencies_for_strategy(&group.strategy, &compatible)
            .await;
        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
            persistent_latencies.as_ref(),
        );

        for candidate in &ordered {
            if !self
                .rate_limiter
                .check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached for audio, trying next"
                );
                continue;
            }

            if !self
                .rate_limiter
                .check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    tpm_limit = ?candidate.channel.tpm_limit,
                    "channel TPM limit reached for audio, trying next"
                );
                continue;
            }

            let (api_key, key_id) = self
                .resolve_key_for_channel(candidate.channel.channel_id, &candidate.channel.code)
                .await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };

            let provider: Arc<dyn AudioProvider> =
                build_audio_provider(&candidate.channel, api_key, opts)?;

            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            return Ok(Some(RoutedAudioProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                group_id: group.group_id,
                provider_type: candidate.channel.provider_type.clone(),
                resolved_model: resolve_model_mapping(&candidate.channel.model_mapping, model),
                key_id,
            }));
        }

        Ok(None)
    }

    /// alias 解析：查 ModelAliasRepo，返回 Some((target, params_override)) 或 None（无 alias）。
    async fn resolve_alias(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> ProviderResult<Option<(String, serde_json::Value)>> {
        let Some(repo) = &self.model_alias_repo else {
            return Ok(None);
        };
        match repo.resolve(project_id, requested_model).await {
            Ok(Some(resolved)) => {
                tracing::debug!(
                    project_id = %project_id,
                    alias = requested_model,
                    target = &resolved.target_model,
                    "model alias resolved"
                );
                Ok(Some((resolved.target_model, resolved.params_override)))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                tracing::warn!(
                    project_id = %project_id,
                    model = requested_model,
                    error = %e,
                    "model alias lookup failed, using original model"
                );
                Ok(None)
            }
        }
    }

    /// 为指定模型做实际路由（查 group → fallback chain → channel → 构造 provider）。
    async fn route_for_model(
        &self,
        project_id: ProjectId,
        model: &str,
        req: Option<&crate::types::ChatRequest>,
        trace: &mut RouteDecisionTrace,
    ) -> ProviderResult<RouteAttempt<RoutedProvider>> {
        // Step 1: 找 project 的默认 channel_group
        let initial_group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => {
                tracing::debug!(
                    project_id = %project_id,
                    model = model,
                    "no default channel_group for project, falling back"
                );
                return Ok(RouteAttempt::Miss(Box::new(RouteMiss {
                    reason: RouteMissReason::NoDefaultGroup,
                    message: route_not_found_message(
                        "chat",
                        model,
                        RouteMissReason::NoDefaultGroup,
                    ),
                    capability: None,
                    selected_model: model.to_string(),
                    trace: trace.clone(),
                })));
            }
            Err(e) => {
                return Err(ProviderError::Config(format!(
                    "channel_group lookup failed: {e}"
                )));
            }
        };

        // Walk fallback chain (max depth 5 to prevent cycles)
        let mut current_group = initial_group;
        let mut depth = 0u8;
        const MAX_FALLBACK_DEPTH: u8 = 5;
        let mut last_miss = None;

        loop {
            if depth > MAX_FALLBACK_DEPTH {
                tracing::warn!(
                    project_id = %project_id,
                    model = model,
                    depth = depth,
                    "fallback chain exceeded max depth"
                );
                return Ok(RouteAttempt::Miss(last_miss.unwrap_or_else(|| {
                    Box::new(RouteMiss {
                        reason: RouteMissReason::FallbackExhausted,
                        message: route_not_found_message(
                            "chat",
                            model,
                            RouteMissReason::FallbackExhausted,
                        ),
                        capability: None,
                        selected_model: model.to_string(),
                        trace: trace.clone(),
                    })
                })));
            }

            if !current_group.enabled {
                tracing::debug!(
                    group_id = %current_group.group_id,
                    "channel_group is disabled, trying fallback"
                );
            } else {
                // Try to find a channel in this group
                match self
                    .try_route_in_group(&current_group, model, req, trace)
                    .await?
                {
                    RouteAttempt::Routed(routed) => return Ok(RouteAttempt::Routed(routed)),
                    RouteAttempt::Miss(miss) => last_miss = Some(miss),
                }
            }

            // No channel found — try fallback group
            match current_group.fallback_group_id {
                Some(fallback_id) => {
                    tracing::info!(
                        group = %current_group.name,
                        fallback_group_id = %fallback_id,
                        model = model,
                        "no compatible channel in group, falling through to fallback group"
                    );
                    match self.group_repo.find_by_id(fallback_id).await {
                        Ok(g) => {
                            current_group = g;
                            depth += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                fallback_group_id = %fallback_id,
                                error = %e,
                                "fallback group lookup failed"
                            );
                            return Ok(RouteAttempt::Miss(last_miss.unwrap_or_else(|| {
                                Box::new(RouteMiss {
                                    reason: RouteMissReason::FallbackExhausted,
                                    message: route_not_found_message(
                                        "chat",
                                        model,
                                        RouteMissReason::FallbackExhausted,
                                    ),
                                    capability: None,
                                    selected_model: model.to_string(),
                                    trace: trace.clone(),
                                })
                            })));
                        }
                    }
                }
                None => {
                    return Ok(RouteAttempt::Miss(last_miss.unwrap_or_else(|| {
                        Box::new(RouteMiss {
                            reason: RouteMissReason::FallbackExhausted,
                            message: route_not_found_message(
                                "chat",
                                model,
                                RouteMissReason::FallbackExhausted,
                            ),
                            capability: None,
                            selected_model: model.to_string(),
                            trace: trace.clone(),
                        })
                    })));
                }
            }
        }
    }

    /// Steps 2-4: list healthy channels in group, filter, select, construct provider.
    ///
    /// Rate limiting integration:
    /// - After strategy selection, check RPM + TPM limits
    /// - If rate-limited, try the next channel in priority order
    /// - Only fail (return None) if ALL compatible channels are rate-limited
    async fn try_route_in_group(
        &self,
        group: &gate_storage::ChannelGroupRecord,
        model: &str,
        req: Option<&crate::types::ChatRequest>,
        trace: &mut RouteDecisionTrace,
    ) -> ProviderResult<RouteAttempt<RoutedProvider>> {
        // Step 2: 取 group 内 healthy channels
        let bindings = self
            .channel_repo
            .list_healthy_in_group(group.group_id)
            .await
            .map_err(|e| ProviderError::Config(format!("channel list failed: {e}")))?;

        if bindings.is_empty() {
            tracing::warn!(
                group_id = %group.group_id,
                model = model,
                "no healthy channels in group"
            );
            return Ok(RouteAttempt::Miss(Box::new(RouteMiss {
                reason: RouteMissReason::NoHealthyChannels,
                message: route_not_found_message("chat", model, RouteMissReason::NoHealthyChannels),
                capability: None,
                selected_model: model.to_string(),
                trace: trace.clone(),
            })));
        }

        // Step 3: 按 model_filter / supported_models 过滤
        let compatible: Vec<_> = bindings
            .iter()
            .filter(|b| {
                if !b.model_filter.is_empty() {
                    b.model_filter.iter().any(|m| m == model)
                } else {
                    b.channel.supported_models.is_empty()
                        || b.channel.supported_models.iter().any(|m| m == model)
                }
            })
            .collect();

        if compatible.is_empty() {
            tracing::warn!(
                group_id = %group.group_id,
                model = model,
                "no channels support model in group"
            );
            return Ok(RouteAttempt::Miss(Box::new(RouteMiss {
                reason: RouteMissReason::ModelUnsupported,
                message: route_not_found_message("chat", model, RouteMissReason::ModelUnsupported),
                capability: None,
                selected_model: model.to_string(),
                trace: trace.clone(),
            })));
        }

        let compatible = self.apply_canary_gate(&compatible, Some(trace));
        if compatible.is_empty() {
            tracing::warn!(
                group_id = %group.group_id,
                model = model,
                "no canary-gated channel selected in group"
            );
            return Ok(RouteAttempt::Miss(Box::new(RouteMiss {
                reason: RouteMissReason::ModelUnsupported,
                message: route_not_found_message("chat", model, RouteMissReason::ModelUnsupported),
                capability: None,
                selected_model: model.to_string(),
                trace: trace.clone(),
            })));
        }

        // Strategy ordering: sort channels into preference order based on strategy
        // For priority/round_robin/weighted_random, select_channel picks one.
        // To support "try next on rate limit", we iterate candidates in order.
        let persistent_latencies = self
            .persistent_latencies_for_strategy(&group.strategy, &compatible)
            .await;
        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
            persistent_latencies.as_ref(),
        );
        trace.record_candidates(group, &ordered);
        let mut last_miss = RouteMiss {
            reason: RouteMissReason::FallbackExhausted,
            message: route_not_found_message("chat", model, RouteMissReason::FallbackExhausted),
            capability: None,
            selected_model: model.to_string(),
            trace: trace.clone(),
        };

        // Try each channel in order until one passes rate limits
        for candidate in &ordered {
            if let Some(req) = req {
                let caps = channel_capabilities(&candidate.channel);
                let missing = caps.missing_for_chat_request(req);
                if !missing.is_empty() {
                    let reason = format!(
                        "missing_capability:{}",
                        missing
                            .iter()
                            .map(|cap| cap.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    tracing::info!(
                        channel = %candidate.channel.code,
                        missing = %reason,
                        "channel capability matrix rejected chat route candidate"
                    );
                    trace.record_skip(candidate, &reason);
                    last_miss = RouteMiss {
                        reason: RouteMissReason::MissingCapability,
                        message: route_not_found_message(
                            "chat",
                            model,
                            RouteMissReason::MissingCapability,
                        ),
                        capability: missing.first().copied(),
                        selected_model: model.to_string(),
                        trace: trace.clone(),
                    };
                    continue;
                }
            }

            if is_plugin_provider(&candidate.channel.provider_type)
                && !self
                    .has_available_plugin_secret(
                        candidate.channel.channel_id,
                        &candidate.channel.code,
                    )
                    .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    "plugin channel has no active secret, trying next channel"
                );
                trace.record_skip(candidate, "no_active_secret");
                last_miss = RouteMiss {
                    reason: RouteMissReason::NoActiveSecret,
                    message: route_not_found_message(
                        "chat",
                        model,
                        RouteMissReason::NoActiveSecret,
                    ),
                    capability: None,
                    selected_model: model.to_string(),
                    trace: trace.clone(),
                };
                continue;
            }

            // RPM 检查
            if !self
                .rate_limiter
                .check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached, trying next channel"
                );
                trace.record_skip(candidate, "rpm_limit");
                last_miss = RouteMiss {
                    reason: RouteMissReason::RateLimited,
                    message: route_not_found_message("chat", model, RouteMissReason::RateLimited),
                    capability: None,
                    selected_model: model.to_string(),
                    trace: trace.clone(),
                };
                continue;
            }

            // TPM 检查（pre-flight）
            if !self
                .rate_limiter
                .check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit)
                .await
            {
                tracing::info!(
                    channel = %candidate.channel.code,
                    tpm_limit = ?candidate.channel.tpm_limit,
                    "channel TPM limit reached, trying next channel"
                );
                trace.record_skip(candidate, "tpm_limit");
                last_miss = RouteMiss {
                    reason: RouteMissReason::RateLimited,
                    message: route_not_found_message("chat", model, RouteMissReason::RateLimited),
                    capability: None,
                    selected_model: model.to_string(),
                    trace: trace.clone(),
                };
                continue;
            }

            tracing::debug!(
                group = %group.name,
                channel = %candidate.channel.code,
                provider_type = %candidate.channel.provider_type,
                model = model,
                "routed to channel"
            );

            // Step 4: 根据 provider_type 构造对应 Provider
            let (api_key, key_id, secrets) = self
                .resolve_secrets_for_channel(candidate.channel.channel_id, &candidate.channel.code)
                .await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };
            let provider: Arc<dyn Provider> =
                if is_plugin_provider(&candidate.channel.provider_type) {
                    build_provider_with_secrets(&candidate.channel, secrets, opts)?
                } else {
                    build_provider(&candidate.channel, api_key, opts)?
                };

            let retry_config = crate::retry::RetryConfig {
                max_retries: candidate.channel.max_retries.max(0) as u32,
                ..Default::default()
            };
            let retry_config = if is_plugin_provider(&candidate.channel.provider_type) {
                plugin_manifest_retry_config(
                    &candidate.channel.model_mapping,
                    &candidate.channel.base_url,
                )
                .unwrap_or(retry_config)
            } else {
                retry_config
            };

            // model_mapping: 如果 channel 配置了映射，翻译模型名
            let resolved_model = resolve_model_mapping(&candidate.channel.model_mapping, model);
            trace.record_selected(group, candidate, model, &resolved_model);

            // least_conn: provider/key 构造成功后再递增 inflight，避免构造失败泄露计数。
            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            return Ok(RouteAttempt::Routed(RoutedProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                resolved_model,
                retry_config,
                key_id,
                params_override: serde_json::json!({}),
                provider_type: candidate.channel.provider_type.clone(),
                metrics: self.metrics.clone(),
                decision_trace: trace.clone(),
            }));
        }

        // All compatible channels are rate-limited
        tracing::warn!(
            group_id = %group.group_id,
            model = model,
            compatible_count = compatible.len(),
            "all compatible channels are rate-limited"
        );
        Ok(RouteAttempt::Miss(Box::new(last_miss)))
    }

    /// G1: 从 DB 取 primary channel key → 解密；无则 fallback env var。
    /// 返回 (plaintext_key, Option<key_id>)——key_id 仅在从 DB 取到时有值。
    async fn resolve_key_for_channel(
        &self,
        channel_id: ChannelId,
        channel_code: &str,
    ) -> ProviderResult<(String, Option<ChannelKeyId>)> {
        let (primary, key_id, _) = self
            .resolve_secrets_for_channel(channel_id, channel_code)
            .await?;
        Ok((primary, key_id))
    }

    async fn has_available_plugin_secret(&self, channel_id: ChannelId, channel_code: &str) -> bool {
        if self.cached_channel_secrets(channel_id).is_some() {
            return true;
        }
        if let Some(repo) = &self.channel_key_repo {
            return match repo.find_active_for_channel(channel_id).await {
                Ok(_) => true,
                Err(gate_storage::DbError::NotFound) => false,
                Err(e) => {
                    tracing::warn!(
                        channel_id = %channel_id,
                        error = %e,
                        "channel key availability check failed"
                    );
                    false
                }
            };
        }
        resolve_api_key_for_channel(channel_code)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    }

    /// P1: plugin secret slots 从 channel_keys.label 解密；primary 仍保持旧行为。
    async fn resolve_secrets_for_channel(
        &self,
        channel_id: ChannelId,
        channel_code: &str,
    ) -> ProviderResult<(String, Option<ChannelKeyId>, HashMap<String, String>)> {
        if let Some(cached) = self.cached_channel_secrets(channel_id) {
            return Ok(cached.into_parts());
        }

        // 如果 repo 未配置，直接走 env
        let Some(repo) = &self.channel_key_repo else {
            let primary = resolve_api_key_for_channel(channel_code)?;
            return Ok((primary.clone(), None, env_secret_map(channel_code, primary)));
        };

        let Some(crypto) = &self.crypto else {
            match repo.find_active_for_channel(channel_id).await {
                Ok(_) => tracing::warn!(
                    channel_id = %channel_id,
                    "channel key found in DB but crypto not configured, falling back to env"
                ),
                Err(e) if !matches!(e, gate_storage::DbError::NotFound) => tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "channel key lookup failed and crypto not configured, falling back to env"
                ),
                Err(_) => {}
            }
            let primary = resolve_api_key_for_channel(channel_code)?;
            return Ok((primary.clone(), None, env_secret_map(channel_code, primary)));
        };

        let records = match repo.list_by_channel(channel_id).await {
            Ok(records) => records,
            Err(gate_storage::DbError::NotFound) => Vec::new(),
            Err(e) => {
                tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "channel key lookup failed, falling back to env"
                );
                let primary = resolve_api_key_for_channel(channel_code)?;
                return Ok((primary.clone(), None, env_secret_map(channel_code, primary)));
            }
        };

        let mut active: Vec<_> = records
            .into_iter()
            .filter(|record| record.health == "healthy")
            .filter(|record| {
                record
                    .cooldown_until
                    .is_none_or(|until| until < chrono::Utc::now())
            })
            .collect();
        active.sort_by_key(|record| (-record.weight, record.created_at));

        if active.is_empty() {
            tracing::debug!(
                channel_id = %channel_id,
                channel_code = channel_code,
                "no active channel key in DB, falling back to env"
            );
            let primary = resolve_api_key_for_channel(channel_code)?;
            return Ok((primary.clone(), None, env_secret_map(channel_code, primary)));
        }

        let mut selected: Option<(i32, chrono::DateTime<chrono::Utc>, ChannelKeyId, String)> = None;
        let mut best_active: Option<(ChannelKeyId, String)> = None;
        let mut secrets = HashMap::new();
        for record in active {
            let secret = decrypt_channel_key(crypto, channel_id, &record).await?;
            let slot = record
                .label
                .as_deref()
                .map(normalize_secret_slot)
                .unwrap_or_else(|| "primary".to_string());
            best_active.get_or_insert_with(|| (record.id, secret.clone()));
            secrets.entry(slot.clone()).or_insert(secret.clone());
            if slot == "primary" {
                match selected {
                    Some((weight, created_at, _, _))
                        if (record.weight, std::cmp::Reverse(record.created_at))
                            <= (weight, std::cmp::Reverse(created_at)) => {}
                    _ => selected = Some((record.weight, record.created_at, record.id, secret)),
                }
            }
        }

        let (primary, key_id) = if let Some((_, _, key_id, primary)) = selected {
            (primary, Some(key_id))
        } else if let Some((key_id, primary)) = best_active {
            (primary, Some(key_id))
        } else {
            let primary = resolve_api_key_for_channel(channel_code)?;
            (primary, None)
        };
        secrets
            .entry("primary".to_string())
            .or_insert_with(|| primary.clone());
        let resolved = ResolvedChannelSecrets {
            primary,
            key_id,
            secrets,
        };
        self.store_channel_secrets(channel_id, resolved.clone());
        Ok(resolved.into_parts())
    }
}

async fn decrypt_channel_key(
    crypto: &EnvelopeKms,
    channel_id: ChannelId,
    record: &gate_storage::ChannelKeyRecord,
) -> ProviderResult<String> {
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let plaintext = crypto
        .open(&record.key_enc, &aad)
        .await
        .map_err(|e| ProviderError::Config(format!("decrypt channel key {}: {e}", record.id)))?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|e| ProviderError::Config(format!("channel key is not valid UTF-8: {e}")))
}

#[cfg(test)]
mod tests;
