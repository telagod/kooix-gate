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
use crate::anthropic::AnthropicProvider;
use crate::azure::AzureProvider;
use crate::bedrock::BedrockProvider;
use crate::capabilities::ProviderCapability;
use crate::cohere::CohereProvider;
use crate::custom_provider::CustomHttpProvider;
use crate::deepseek::DeepSeekProvider;
use crate::error::{
    NormalizedProviderErrorKind, ProviderError, ProviderErrorMetadata, ProviderResult,
};
use crate::gemini::GeminiProvider;
use crate::mistral::MistralProvider;
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::plugin_manifest::{plugin_manifest, plugin_manifest_retry_config};
use crate::{ProviderCapabilities, provider_capabilities};
use gate_core::id::{ChannelGroupId, ChannelId, ChannelKeyId, ProjectId};
use gate_crypto::EnvelopeKms;
use gate_storage::{
    ChannelBinding, ChannelGroupRepo, ChannelKeyRepo, ChannelLatencyRepo, ChannelRepo,
    ModelAliasRepo,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 单个候选 channel 在一次路由决策中的快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteCandidateTrace {
    pub group_id: ChannelGroupId,
    pub group_name: String,
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub provider_type: String,
    pub priority: i32,
    pub weight: i32,
}

/// 候选 channel 被跳过的原因。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteSkipTrace {
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteMissReason {
    NoDefaultGroup,
    NoHealthyChannels,
    ModelUnsupported,
    MissingCapability,
    NoActiveSecret,
    RateLimited,
    FallbackExhausted,
}

impl RouteMissReason {
    fn as_str(self) -> &'static str {
        match self {
            RouteMissReason::NoDefaultGroup => "no_default_group",
            RouteMissReason::NoHealthyChannels => "no_healthy_channels",
            RouteMissReason::ModelUnsupported => "model_unsupported",
            RouteMissReason::MissingCapability => "missing_capability",
            RouteMissReason::NoActiveSecret => "no_active_secret",
            RouteMissReason::RateLimited => "rate_limited",
            RouteMissReason::FallbackExhausted => "fallback_exhausted",
        }
    }
}

#[derive(Debug, Clone)]
struct RouteMiss {
    reason: RouteMissReason,
    message: String,
    capability: Option<ProviderCapability>,
    selected_model: String,
    trace: RouteDecisionTrace,
}

impl RouteMiss {
    fn provider_error(self) -> ProviderError {
        let _ = (&self.capability, &self.selected_model, &self.trace);
        let metadata = ProviderErrorMetadata {
            kind: match self.reason {
                RouteMissReason::RateLimited => NormalizedProviderErrorKind::RateLimit,
                _ => NormalizedProviderErrorKind::ModelNotFound,
            },
            retryable: false,
            cooldown_ms: None,
            circuit_breaker_failures: None,
            retry_after_ms: None,
        };
        let status = match self.reason {
            RouteMissReason::RateLimited => Some(429),
            _ => Some(404),
        };
        let code = match self.reason {
            RouteMissReason::NoHealthyChannels => "no_healthy_channel",
            other => other.as_str(),
        };
        ProviderError::Mapped {
            status,
            code: Some(code.to_string()),
            message: self.message,
            metadata,
        }
    }
}

enum RouteAttempt<T> {
    Routed(T),
    Miss(Box<RouteMiss>),
}

fn route_not_found_message(kind: &str, model: &str, reason: RouteMissReason) -> String {
    format!(
        "no healthy {kind} channel found for model '{model}' ({})",
        reason.as_str()
    )
}

/// Provider 路由决策轨迹。
///
/// 当前 router 仍是 repo-backed lazy routing；`snapshot_version` 是热路径可观测钩子，
/// 后续切到 compiled `ProviderRuntimeSnapshot` 时无需改调用方契约。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteDecisionTrace {
    pub snapshot_version: u64,
    pub requested_model: String,
    /// alias 命中后的 target_model；未命中为 None。
    pub alias_target_model: Option<String>,
    /// alias 解析后的初始路由模型；未命中 alias 时等于 requested_model。
    pub initial_model: String,
    /// 实际选中的路由模型；可能来自 fallback model chain。
    pub selected_model: Option<String>,
    /// 经过 channel.model_mapping 后真正交给 provider 的模型名。
    pub provider_model: Option<String>,
    pub selected_group_id: Option<ChannelGroupId>,
    pub selected_group_name: Option<String>,
    pub selected_strategy: Option<String>,
    pub selected_channel_id: Option<ChannelId>,
    pub selected_channel_code: Option<String>,
    pub selected_provider_type: Option<String>,
    pub candidates: Vec<RouteCandidateTrace>,
    pub skipped: Vec<RouteSkipTrace>,
    pub fallbacks: Vec<String>,
}

/// Compiled provider runtime metadata snapshot.
///
/// The current router still resolves providers from repos lazily, but this
/// snapshot is already atomically replaceable and versioned. Control-plane code
/// can publish compiled channel/key metadata here before the route path fully
/// switches away from repo-backed reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeSnapshot {
    pub version: u64,
    pub compiled_at_unix_ms: u128,
    pub channels: Vec<ProviderRuntimeChannelSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeChannelSnapshot {
    pub channel_id: ChannelId,
    pub channel_code: String,
    pub provider_type: String,
    pub supported_models: Vec<String>,
    pub status: String,
    pub health: String,
}

impl ProviderRuntimeSnapshot {
    fn new(version: u64, channels: Vec<ProviderRuntimeChannelSnapshot>) -> Self {
        Self {
            version,
            compiled_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default(),
            channels,
        }
    }
}

impl RouteDecisionTrace {
    fn new(
        snapshot_version: u64,
        requested_model: &str,
        alias_target_model: Option<String>,
        initial_model: &str,
    ) -> Self {
        Self {
            snapshot_version,
            requested_model: requested_model.to_string(),
            alias_target_model,
            initial_model: initial_model.to_string(),
            selected_model: None,
            provider_model: None,
            selected_group_id: None,
            selected_group_name: None,
            selected_strategy: None,
            selected_channel_id: None,
            selected_channel_code: None,
            selected_provider_type: None,
            candidates: Vec::new(),
            skipped: Vec::new(),
            fallbacks: Vec::new(),
        }
    }

    fn record_candidates(
        &mut self,
        group: &gate_storage::ChannelGroupRecord,
        ordered: &[&ChannelBinding],
    ) {
        self.candidates
            .extend(ordered.iter().map(|candidate| RouteCandidateTrace {
                group_id: group.group_id,
                group_name: group.name.clone(),
                channel_id: candidate.channel.channel_id,
                channel_code: candidate.channel.code.clone(),
                provider_type: candidate.channel.provider_type.clone(),
                priority: candidate.priority,
                weight: candidate.weight,
            }));
    }

    fn record_skip(&mut self, candidate: &ChannelBinding, reason: &str) {
        self.skipped.push(RouteSkipTrace {
            channel_id: candidate.channel.channel_id,
            channel_code: candidate.channel.code.clone(),
            reason: reason.to_string(),
        });
    }

    fn record_selected(
        &mut self,
        group: &gate_storage::ChannelGroupRecord,
        candidate: &ChannelBinding,
        selected_model: &str,
        provider_model: &str,
    ) {
        self.selected_model = Some(selected_model.to_string());
        self.provider_model = Some(provider_model.to_string());
        self.selected_group_id = Some(group.group_id);
        self.selected_group_name = Some(group.name.clone());
        self.selected_strategy = Some(group.strategy.clone());
        self.selected_channel_id = Some(candidate.channel.channel_id);
        self.selected_channel_code = Some(candidate.channel.code.clone());
        self.selected_provider_type = Some(candidate.channel.provider_type.clone());
    }
}

/// 路由命中结果：Provider + 它绑定的 channel_id（计费维度归属）+ 实际使用的 model。
#[derive(Clone)]
pub struct RoutedProvider {
    pub provider: Arc<dyn Provider>,
    pub channel_id: ChannelId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 从 channel 记录构造的 retry 配置。
    pub retry_config: crate::retry::RetryConfig,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
    /// params_override from model alias (empty object `{}` if no alias or no override).
    pub params_override: serde_json::Value,
    /// 命中 channel 的 provider_type（"anthropic", "bedrock", "gemini" 等）。
    /// 供调用方做参数适配（adapt_for_provider）。
    pub provider_type: String,
    /// 指向全局 ChannelMetrics，供调用方上报结果（auto-disable 机制）。
    pub metrics: Option<Arc<ChannelMetrics>>,
    /// 本次命中的路由决策轨迹，供审计、debug 与后续 snapshot 热更新验证。
    pub decision_trace: RouteDecisionTrace,
}

/// Embedding 路由命中结果：EmbeddingProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedEmbeddingProvider {
    pub provider: Arc<dyn EmbeddingProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}

/// Image 路由命中结果：ImageProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedImageProvider {
    pub provider: Arc<dyn ImageProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}

/// Audio 路由命中结果：AudioProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedAudioProvider {
    pub provider: Arc<dyn AudioProvider>,
    pub channel_id: ChannelId,
    pub group_id: ChannelGroupId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
    /// 本次路由命中的 channel key ID（来自 DB），用于熔断上报。env 回退时为 None。
    pub key_id: Option<ChannelKeyId>,
}

// ============================================================================
// ChannelMetrics — 滑动窗口成功率追踪（auto-disable 机制）
// ============================================================================

/// 轻量级内存滑动窗口，按 channel 追踪成功率和响应延迟。
///
/// 窗口满且成功率低于阈值时，`should_disable` 返回 true。
pub struct ChannelMetrics {
    windows: Mutex<HashMap<ChannelId, VecDeque<bool>>>,
    latencies: Mutex<HashMap<ChannelId, VecDeque<u64>>>, // ms
    window_size: usize,
    threshold: f64,
}

impl ChannelMetrics {
    pub fn new(window_size: usize, threshold: f64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            latencies: Mutex::new(HashMap::new()),
            window_size,
            threshold,
        }
    }

    /// 记录一次请求结果（true = 成功，false = 失败）。
    pub fn record(&self, channel_id: ChannelId, success: bool) {
        let mut windows = self.windows.lock();
        let window = windows.entry(channel_id).or_default();
        if window.len() >= self.window_size {
            window.pop_front();
        }
        window.push_back(success);
    }

    /// 窗口满且成功率低于阈值时返回 true，触发 auto-disable。
    pub fn should_disable(&self, channel_id: ChannelId) -> bool {
        let windows = self.windows.lock();
        let Some(window) = windows.get(&channel_id) else {
            return false;
        };
        if window.len() < self.window_size {
            return false;
        }
        let successes = window.iter().filter(|&&s| s).count();
        let rate = successes as f64 / window.len() as f64;
        rate < self.threshold
    }

    /// 记录响应延迟（毫秒）。
    pub fn record_latency(&self, channel_id: ChannelId, latency_ms: u64) {
        let mut latencies = self.latencies.lock();
        let window = latencies.entry(channel_id).or_default();
        if window.len() >= self.window_size {
            window.pop_front();
        }
        window.push_back(latency_ms);
    }

    /// 获取 channel 的平均延迟（ms）；无数据时返回 u64::MAX。
    pub fn avg_latency(&self, channel_id: ChannelId) -> u64 {
        let latencies = self.latencies.lock();
        let Some(window) = latencies.get(&channel_id) else {
            return u64::MAX;
        };
        if window.is_empty() {
            return u64::MAX;
        }
        let sum: u64 = window.iter().sum();
        sum / window.len() as u64
    }

    /// 清除 channel 的历史记录（re-enable 后调用）。
    pub fn clear(&self, channel_id: ChannelId) {
        let mut windows = self.windows.lock();
        windows.remove(&channel_id);
        let mut latencies = self.latencies.lock();
        latencies.remove(&channel_id);
    }
}

// ============================================================================
// ChannelRateCheck — per-channel RPM/TPM 限速 trait
// ============================================================================

/// Per-channel RPM/TPM 限速抽象。
///
/// 实现方式：
/// - [`InMemoryChannelRateLimiter`]：纯内存固定窗口（dev / 单实例）
/// - Redis 实现在 `gate-cache` crate（生产多实例 sliding window）
///
/// 路由器通过 `Arc<dyn ChannelRateCheck>` 持有，支持运行时注入。
#[async_trait::async_trait]
pub trait ChannelRateCheck: Send + Sync + 'static {
    /// 检查并消耗一次 RPM 额度。
    ///
    /// - `rpm_limit = None` → 无限制，返回 `true`
    /// - 通过 → `true`（计数 +1）
    /// - 超限 → `false`（不消耗）
    async fn check_rpm(&self, channel_id: ChannelId, rpm_limit: Option<i32>) -> bool;

    /// 检查 TPM 限额（pre-flight）。
    ///
    /// 不消耗额度，仅检查当前窗口内 token 总量是否 < limit。
    /// 实际 token 消耗由 `record_tokens` 在 response 后记录。
    ///
    /// - `tpm_limit = None` → 无限制，返回 `true`
    /// - 窗口内 token < limit → `true`
    /// - 超限 → `false`
    async fn check_tpm(&self, channel_id: ChannelId, tpm_limit: Option<i32>) -> bool;

    /// 记录实际 token 消耗（response 后调用）。
    ///
    /// 更新 TPM 滑动窗口计数。
    async fn record_tokens(&self, channel_id: ChannelId, tokens: u32);
}

// ============================================================================
// InMemoryChannelRateLimiter — 纯内存固定窗口实现
// ============================================================================

struct RateWindow {
    rpm_count: u32,
    tpm_count: u32,
    window_start: Instant,
}

/// 纯内存固定窗口限速器。每个 channel 一个 60s 窗口，过期自动重置。
///
/// 适用于 dev / 单实例部署。多实例部署应使用 Redis 实现。
pub struct InMemoryChannelRateLimiter {
    counters: Mutex<HashMap<ChannelId, RateWindow>>,
}

impl Default for InMemoryChannelRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryChannelRateLimiter {
    pub fn new() -> Self {
        Self {
            counters: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl ChannelRateCheck for InMemoryChannelRateLimiter {
    async fn check_rpm(&self, channel_id: ChannelId, rpm_limit: Option<i32>) -> bool {
        let Some(limit) = rpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u32;
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        if window.rpm_count >= limit {
            return false;
        }
        window.rpm_count += 1;
        true
    }

    async fn check_tpm(&self, channel_id: ChannelId, tpm_limit: Option<i32>) -> bool {
        let Some(limit) = tpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u32;
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        window.tpm_count < limit
    }

    async fn record_tokens(&self, channel_id: ChannelId, tokens: u32) {
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        window.tpm_count = window.tpm_count.saturating_add(tokens);
    }
}

/// Backward-compat alias.
pub type ChannelRateLimiter = InMemoryChannelRateLimiter;

const DEFAULT_CHANNEL_LATENCY_WINDOW_SECS: i64 = 300;

// ============================================================================
// InflightTracker — least_conn 策略用的 inflight 计数器
// ============================================================================

/// 轻量级 inflight 请求计数器。
///
/// warmup 之后热路径只做 `RwLock::read` + `AtomicI64` CAS，无分配。
pub struct InflightTracker {
    counts: RwLock<HashMap<ChannelId, Arc<AtomicI64>>>,
}

impl Default for InflightTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl InflightTracker {
    pub fn new() -> Self {
        Self {
            counts: RwLock::new(HashMap::new()),
        }
    }

    /// 标记一个 channel 获取了请求（inflight +1）。
    pub fn acquire(&self, channel_id: ChannelId) {
        // 快路径：read lock
        {
            let counts = self.counts.read();
            if let Some(counter) = counts.get(&channel_id) {
                counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        // 慢路径：write lock，首次见到的 channel
        let mut counts = self.counts.write();
        let counter = counts
            .entry(channel_id)
            .or_insert_with(|| Arc::new(AtomicI64::new(0)));
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// 标记一个 channel 释放了请求（inflight -1）。
    pub fn release(&self, channel_id: ChannelId) {
        let counts = self.counts.read();
        if let Some(counter) = counts.get(&channel_id) {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// 查询 channel 当前 inflight 数。
    pub fn current(&self, channel_id: ChannelId) -> i64 {
        let counts = self.counts.read();
        counts
            .get(&channel_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }
}

// ============================================================================
// Strategy selection functions
// ============================================================================

/// 根据策略名选择 channel。
///
/// 保留用于未来可能需要「单选」的场景（如 health probe 等）。
#[allow(dead_code)]
fn select_channel<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
) -> &'a ChannelBinding {
    match strategy {
        "weighted_random" => select_weighted_random(compatible),
        "round_robin" => select_round_robin(compatible, rr_counter),
        "least_conn" => select_least_conn(compatible, inflight),
        "least_latency" => select_least_latency(compatible, metrics),
        // "priority" + 未知 strategy → 取第一条（已按 priority ASC 排序）
        _ => compatible[0],
    }
}

/// 按 weight 做加权随机选择。
fn select_weighted_random<'a>(channels: &'a [&ChannelBinding]) -> &'a ChannelBinding {
    use rand::Rng;
    let total_weight: u32 = channels.iter().map(|c| c.weight.max(1) as u32).sum();
    let mut rng = rand::thread_rng();
    let mut roll = rng.gen_range(0..total_weight);
    for ch in channels {
        let w = ch.weight.max(1) as u32;
        if roll < w {
            return ch;
        }
        roll -= w;
    }
    // 安全兜底（浮点/溢出不可能，但 defensive）
    channels.last().unwrap()
}

/// 循环轮转：用 AtomicU64 取模。
fn select_round_robin<'a>(
    channels: &'a [&ChannelBinding],
    counter: &AtomicU64,
) -> &'a ChannelBinding {
    let idx = counter.fetch_add(1, Ordering::Relaxed) as usize % channels.len();
    channels[idx]
}

/// 选 inflight 最少的 channel；同分时取第一个（即 priority 最高的）。
#[allow(dead_code)]
fn select_least_conn<'a>(
    channels: &'a [&ChannelBinding],
    inflight: &InflightTracker,
) -> &'a ChannelBinding {
    channels
        .iter()
        .min_by_key(|ch| inflight.current(ch.channel.channel_id))
        .unwrap()
}

/// 选平均延迟最低的 channel；无延迟数据时 fallback 到第一条。
#[allow(dead_code)]
fn select_least_latency<'a>(
    channels: &'a [&ChannelBinding],
    metrics: Option<&ChannelMetrics>,
) -> &'a ChannelBinding {
    let Some(m) = metrics else {
        return channels[0];
    };
    channels
        .iter()
        .min_by_key(|ch| m.avg_latency(ch.channel.channel_id))
        .unwrap()
}

/// 按策略返回所有 compatible channels 的有序列表（首选在前）。
///
/// 与 `select_channel` 不同，这里返回全部候选而非单个。
/// 用于 rate limit fallback：首选被限速时依次尝试后续。
fn order_channels_by_strategy<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
    persistent_latencies: Option<&HashMap<ChannelId, u64>>,
) -> Vec<&'a ChannelBinding> {
    if compatible.len() <= 1 {
        return compatible.to_vec();
    }

    match strategy {
        "weighted_random" => {
            // 首选 = weighted random pick，其余按 priority 排
            let first = select_weighted_random(compatible);
            let mut rest: Vec<_> = compatible
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            rest.sort_by_key(|c| c.priority);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "round_robin" => {
            // 首选 = round_robin pick，其余按 priority 排
            let first = select_round_robin(compatible, rr_counter);
            let mut rest: Vec<_> = compatible
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            rest.sort_by_key(|c| c.priority);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "least_conn" => {
            // 按 inflight 升序排
            let mut sorted: Vec<_> = compatible.to_vec();
            sorted.sort_by_key(|c| inflight.current(c.channel.channel_id));
            sorted
        }
        "least_latency" => {
            // 按延迟升序排
            let mut sorted: Vec<_> = compatible.to_vec();
            sorted.sort_by_key(|c| {
                persistent_latencies
                    .and_then(|latencies| latencies.get(&c.channel.channel_id).copied())
                    .or_else(|| metrics.map(|m| m.avg_latency(c.channel.channel_id)))
                    .unwrap_or(u64::MAX)
            });
            sorted
        }
        // "priority" + 未知 → 已按 priority ASC 排序的原始顺序
        _ => compatible.to_vec(),
    }
}

/// 按 provider_type 构造 Provider 实例。
fn build_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn Provider>> {
    build_provider_with_secrets(
        channel,
        HashMap::from([("primary".to_string(), api_key)]),
        opts,
    )
}

/// 按 provider_type 构造 Provider 实例，并把 manifest secret slots 传入 runtime plugin。
fn build_provider_with_secrets(
    channel: &gate_storage::ChannelRecord,
    secrets: HashMap<String, String>,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn Provider>> {
    let api_key = secrets.get("primary").cloned().unwrap_or_default();
    match channel.provider_type.as_str() {
        "anthropic" => {
            let p = AnthropicProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build AnthropicProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "gemini" => {
            let p = GeminiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build GeminiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "azure" => {
            let p = AzureProvider::new_with_opts(channel.base_url.clone(), api_key, None, opts)
                .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "bedrock" => {
            let access = api_key;
            let secret_env = format!(
                "KOOIX_CH_{}_SECRET",
                channel
                    .code
                    .to_uppercase()
                    .replace(|c: char| !c.is_alphanumeric(), "_")
            );
            let secret = std::env::var(&secret_env).map_err(|_| {
                ProviderError::Config(format!(
                    "missing {} env var for bedrock channel '{}'",
                    secret_env, channel.code
                ))
            })?;
            let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            let p = BedrockProvider::new_with_opts(region, access, secret, opts)
                .map_err(|e| ProviderError::Config(format!("build BedrockProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "deepseek" => {
            let p = DeepSeekProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build DeepSeekProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "ollama" => {
            let p = OllamaProvider::new_with_opts(channel.base_url.clone(), opts)
                .map_err(|e| ProviderError::Config(format!("build OllamaProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "mistral" => {
            let p = MistralProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build MistralProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "cohere" => {
            let p = CohereProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build CohereProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        "plugin" | "custom" | "http" | "http_plugin" => {
            let p = CustomHttpProvider::new_with_secret_slots(
                channel.base_url.clone(),
                secrets,
                channel.model_mapping.clone(),
                opts,
            )
            .map_err(|e| ProviderError::Config(format!("build CustomHttpProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
        _ => {
            // 未知类型走 OpenAI 兼容
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn Provider>)
        }
    }
}

/// 按 provider_type 构造 EmbeddingProvider 实例。
fn build_embedding_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn EmbeddingProvider>> {
    match channel.provider_type.as_str() {
        "azure" => {
            let p = AzureProvider::new_with_opts(channel.base_url.clone(), api_key, None, opts)
                .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "deepseek" => {
            let p = DeepSeekProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build DeepSeekProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "ollama" => {
            let p = OllamaProvider::new_with_opts(channel.base_url.clone(), opts)
                .map_err(|e| ProviderError::Config(format!("build OllamaProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "mistral" => {
            let p = MistralProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build MistralProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "cohere" => {
            let p = CohereProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build CohereProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        "gemini" => {
            let p = GeminiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build GeminiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>)
        }
    }
}

/// 按 provider_type 构造 ImageProvider 实例。
fn build_image_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn ImageProvider>> {
    match channel.provider_type.as_str() {
        "openai" | "openai_compatible" => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn ImageProvider>)
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn ImageProvider>)
        }
    }
}

/// 按 provider_type 构造 AudioProvider 实例。
fn build_audio_provider(
    channel: &gate_storage::ChannelRecord,
    api_key: String,
    opts: crate::ProviderOpts,
) -> ProviderResult<Arc<dyn AudioProvider>> {
    match channel.provider_type.as_str() {
        "openai" | "openai_compatible" => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn AudioProvider>)
        }
        _ => {
            let p = OpenAiProvider::new_with_opts(channel.base_url.clone(), api_key, opts)
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
            Ok(Arc::new(p) as Arc<dyn AudioProvider>)
        }
    }
}

/// API key 来源策略（env 回退，DB 优先路径在 route_for_model 内）。
///
/// 优先级：
/// 1. 环境变量 `KOOIX_CH_<CODE>_KEY`（code 大写，非字母替换为 _）
/// 2. 环境变量 `KOOIX_API_KEY`（全局兜底）
/// 3. 空字符串（plugin auth 可完全依赖 named secret slots；上游自己决定是否拒绝）
fn resolve_api_key_for_channel(code: &str) -> ProviderResult<String> {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    Ok(std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default())
}

/// 静态 fallback 链：model → 可尝试的替代模型列表。
///
/// 仅在主路由返回 None 时按顺序尝试。
fn fallback_models(model: &str) -> &'static [&'static str] {
    match model {
        "gpt-4o" => &["gpt-4o-mini"],
        "claude-3-opus" => &["claude-3-sonnet", "claude-3-haiku"],
        "claude-3-sonnet" => &["claude-3-haiku"],
        "gemini-1.5-pro" => &["gemini-1.5-flash"],
        _ => &[],
    }
}

fn resolve_model_mapping(mapping: &serde_json::Value, model: &str) -> String {
    let mapping = mapping
        .as_object()
        .and_then(|map| {
            if map.contains_key("plugin") {
                map.get("models")
                    .or_else(|| map.get("model_aliases"))
                    .or_else(|| map.get("deployments"))
            } else {
                Some(mapping)
            }
        })
        .unwrap_or(mapping);
    if let serde_json::Value::Object(map) = mapping
        && let Some(serde_json::Value::String(target)) = map.get(model)
    {
        return target.clone();
    }
    model.to_string()
}

/// 多 Provider 路由器。
///
/// 持有 Repo 引用（Arc），每次请求惰性查询——无缓存（C1 阶段简单版）。
pub struct ProviderRouter {
    channel_repo: Arc<dyn ChannelRepo>,
    group_repo: Arc<dyn ChannelGroupRepo>,
    model_alias_repo: Option<Arc<dyn ModelAliasRepo>>,
    /// G1: channel_keys 表读取（加密 key 存储）。
    channel_key_repo: Option<Arc<dyn ChannelKeyRepo>>,
    /// G1: 解密 channel key 的 envelope KMS。
    crypto: Option<Arc<EnvelopeKms>>,
    /// round_robin 策略的全局计数器。
    rr_counter: AtomicU64,
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
}

impl ProviderRouter {
    pub fn new(channel_repo: Arc<dyn ChannelRepo>, group_repo: Arc<dyn ChannelGroupRepo>) -> Self {
        Self {
            channel_repo,
            group_repo,
            model_alias_repo: None,
            channel_key_repo: None,
            crypto: None,
            rr_counter: AtomicU64::new(0),
            inflight: Arc::new(InflightTracker::new()),
            metrics: Some(Arc::new(ChannelMetrics::new(10, 0.8))),
            channel_latency_repo: None,
            latency_window_secs: DEFAULT_CHANNEL_LATENCY_WINDOW_SECS,
            rate_limiter: Arc::new(InMemoryChannelRateLimiter::new()),
            snapshot_version: AtomicU64::new(1),
            runtime_snapshot: RwLock::new(Arc::new(ProviderRuntimeSnapshot::new(1, Vec::new()))),
        }
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

        // Filter: model compatible AND provider supports EmbeddingProvider
        let compatible: Vec<_> = bindings
            .iter()
            .filter(|b| {
                let model_ok = if !b.model_filter.is_empty() {
                    b.model_filter.iter().any(|m| m == model)
                } else {
                    b.channel.supported_models.is_empty()
                        || b.channel.supported_models.iter().any(|m| m == model)
                };
                let provider_ok = channel_capabilities(&b.channel).embeddings
                    && !is_plugin_provider(&b.channel.provider_type);
                model_ok && provider_ok
            })
            .collect();

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

            let (api_key, key_id) = self
                .resolve_key_for_channel(candidate.channel.channel_id, &candidate.channel.code)
                .await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };

            let provider: Arc<dyn EmbeddingProvider> =
                build_embedding_provider(&candidate.channel, api_key, opts)?;

            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            return Ok(Some(RoutedEmbeddingProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                group_id: group.group_id,
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
        Ok((primary, key_id, secrets))
    }
}

fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

fn supports_image_runtime(provider_type: &str) -> bool {
    matches!(provider_type, "openai" | "openai_compatible")
}

fn supports_audio_runtime(provider_type: &str) -> bool {
    matches!(provider_type, "openai" | "openai_compatible")
}

fn channel_capabilities(channel: &gate_storage::ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&channel.provider_type) {
        return plugin_manifest(channel.model_mapping.clone(), &channel.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| provider_capabilities(&channel.provider_type));
    }
    provider_capabilities(&channel.provider_type)
}

fn env_secret_map(channel_code: &str, primary: String) -> HashMap<String, String> {
    let mut secrets = CustomHttpProvider::env_secret_slots(channel_code);
    secrets.insert("primary".to_string(), primary);
    secrets
}

fn normalize_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
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
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ChatRequest, Role};
    use chrono::Utc;
    use gate_core::id::{ChannelGroupId, ChannelId, ProjectId};
    use gate_storage::{
        ChannelGroupRecord, ChannelLatencyRepo, ChannelRecord, InMemoryChannelGroupRepo,
        InMemoryChannelLatencyRepo, InMemoryChannelRepo,
    };
    use uuid::Uuid;

    fn ensure_test_api_key() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| unsafe {
            std::env::set_var("KOOIX_API_KEY", "test-key-for-unit-tests");
        });
    }

    #[test]
    fn fallback_chain_gpt4o() {
        assert_eq!(fallback_models("gpt-4o"), &["gpt-4o-mini"]);
    }

    #[test]
    fn fallback_chain_claude_opus() {
        assert_eq!(
            fallback_models("claude-3-opus"),
            &["claude-3-sonnet", "claude-3-haiku"]
        );
    }

    #[test]
    fn fallback_chain_gemini() {
        assert_eq!(fallback_models("gemini-1.5-pro"), &["gemini-1.5-flash"]);
    }

    #[test]
    fn fallback_chain_unknown_model() {
        assert!(fallback_models("unknown-model-xyz").is_empty());
    }

    #[test]
    fn fallback_chain_claude_sonnet() {
        assert_eq!(fallback_models("claude-3-sonnet"), &["claude-3-haiku"]);
    }

    #[test]
    fn plugin_model_mapping_preserves_manifest_and_maps_deployment_model() {
        let mapping = serde_json::json!({
            "plugin": {
                "version": 1,
                "preset": { "provider": "azure_openai" }
            },
            "models": {
                "gpt-4o-mini": "native-mini-deployment"
            }
        });

        assert_eq!(
            resolve_model_mapping(&mapping, "gpt-4o-mini"),
            "native-mini-deployment"
        );
        assert_eq!(
            resolve_model_mapping(&mapping, "gpt-4o"),
            "gpt-4o",
            "plugin manifest must not be mistaken for legacy flat model mapping"
        );
    }

    // ---- helpers for model-filter routing tests (G7) ----

    fn make_channel_with_models(
        code: &str,
        provider_type: &str,
        models: Vec<String>,
    ) -> ChannelRecord {
        let now = Utc::now();
        ChannelRecord {
            channel_id: ChannelId::from(Uuid::now_v7()),
            code: code.to_string(),
            name: code.to_string(),
            provider_type: provider_type.to_string(),
            base_url: "http://localhost:9999".to_string(),
            supported_models: models,
            status: "active".to_string(),
            health: "healthy".to_string(),
            timeout_ms: 60000,
            max_retries: 2,
            rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::Value::Object(Default::default()),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn setup_fixtures(
        channels_spec: &[(&str, &str, Vec<String>, i32)],
    ) -> (ProjectId, ProviderRouter) {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

        let now = Utc::now();
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        group_repo.seed_default(project_id, group_id);

        for (code, provider_type, models, priority) in channels_spec {
            let ch = make_channel_with_models(code, provider_type, models.clone());
            let ch_id = ch.channel_id;
            channel_repo.seed_channel(ch);
            channel_repo.seed_binding(group_id, ch_id, *priority, 1);
        }

        let router = ProviderRouter::new(channel_repo, group_repo);
        (project_id, router)
    }

    #[tokio::test]
    async fn model_filter_matching_channel_selected() {
        let (pid, router) = setup_fixtures(&[("ch-gpt", "openai", vec!["gpt-4o".into()], 1)]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        assert!(
            result.is_some(),
            "channel with matching model should be routed"
        );
        assert_eq!(result.unwrap().resolved_model, "gpt-4o");
    }

    #[tokio::test]
    async fn model_filter_non_matching_channel_skipped() {
        let (pid, router) = setup_fixtures(&[
            ("ch-gpt", "openai", vec!["gpt-4o".into()], 1),
            ("ch-claude", "openai", vec!["claude-3".into()], 2),
        ]);
        let result = router.route(pid, "claude-3").await.unwrap();
        assert!(result.is_some(), "channel B should match claude-3");
        let routed = result.unwrap();
        assert_eq!(routed.resolved_model, "claude-3");
    }

    #[tokio::test]
    async fn model_filter_empty_supported_models_is_wildcard() {
        let (pid, router) = setup_fixtures(&[("ch-wildcard", "openai", vec![], 1)]);
        let result = router.route(pid, "any-model-name").await.unwrap();
        assert!(
            result.is_some(),
            "empty supported_models should match any model"
        );
        assert_eq!(result.unwrap().resolved_model, "any-model-name");
    }

    #[tokio::test]
    async fn model_filter_no_compatible_channel_returns_none() {
        let (pid, router) = setup_fixtures(&[
            ("ch-gpt", "openai", vec!["gpt-4o".into()], 1),
            ("ch-claude", "openai", vec!["claude-3".into()], 2),
        ]);
        let result = router.route(pid, "gemini-pro").await.unwrap();
        assert!(
            result.is_none(),
            "no channel supports gemini-pro, should return None"
        );
    }

    #[tokio::test]
    async fn route_chat_required_normalizes_model_not_found() {
        let (pid, router) = setup_fixtures(&[
            ("ch-gpt", "openai", vec!["gpt-4o".into()], 1),
            ("ch-claude", "openai", vec!["claude-3".into()], 2),
        ]);
        let err = match router
            .route_chat_required(
                pid,
                &ChatRequest {
                    model: "gemini-pro".to_string(),
                    messages: vec![ChatMessage::text(Role::User, "hi")],
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => panic!("expected route miss"),
            Err(err) => err,
        };

        match err {
            ProviderError::Mapped {
                status,
                code,
                message,
                metadata,
            } => {
                assert_eq!(status, Some(404));
                assert_eq!(code.as_deref(), Some("model_unsupported"));
                assert_eq!(metadata.kind, NormalizedProviderErrorKind::ModelNotFound);
                assert!(message.contains("no healthy chat channel found"));
            }
            other => panic!("expected mapped route miss, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_filter_priority_respected_among_compatible() {
        let (pid, router) = setup_fixtures(&[
            ("ch-low-prio", "openai", vec!["gpt-4o".into()], 10),
            ("ch-high-prio", "openai", vec!["gpt-4o".into()], 1),
        ]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        let routed = result.expect("should route");
        assert_eq!(routed.resolved_model, "gpt-4o");
    }

    #[tokio::test]
    async fn model_filter_wildcard_lower_priority_than_specific() {
        let (pid, router) = setup_fixtures(&[
            ("ch-specific", "openai", vec!["gpt-4o".into()], 1),
            ("ch-wildcard", "openai", vec![], 2),
        ]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn model_filter_fallback_model_also_filtered() {
        let (pid, router) = setup_fixtures(&[("ch-mini", "openai", vec!["gpt-4o-mini".into()], 1)]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should fallback to gpt-4o-mini");
        let routed = result.unwrap();
        assert_eq!(routed.resolved_model, "gpt-4o-mini");
        assert_eq!(
            routed.decision_trace.fallbacks,
            vec!["gpt-4o-mini".to_string()]
        );
        assert_eq!(
            routed.decision_trace.selected_model.as_deref(),
            Some("gpt-4o-mini")
        );
    }

    #[tokio::test]
    async fn route_decision_trace_records_candidates_and_selection() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("priority", &[("ch-low", 10, 1), ("ch-high", 1, 1)]);

        let routed = router.route(pid, "gpt-4o").await.unwrap().unwrap();
        let trace = routed.decision_trace;

        assert_eq!(trace.snapshot_version, 1);
        assert_eq!(trace.requested_model, "gpt-4o");
        assert_eq!(trace.initial_model, "gpt-4o");
        assert_eq!(trace.selected_model.as_deref(), Some("gpt-4o"));
        assert_eq!(trace.provider_model.as_deref(), Some("gpt-4o"));
        assert_eq!(trace.selected_strategy.as_deref(), Some("priority"));
        assert_eq!(trace.selected_channel_id, Some(ch_ids[1]));
        assert_eq!(trace.selected_channel_code.as_deref(), Some("ch-high"));
        assert_eq!(trace.candidates.len(), 2);
        assert_eq!(trace.candidates[0].channel_id, ch_ids[1]);
    }

    #[tokio::test]
    async fn route_decision_trace_records_snapshot_version_and_alias() {
        use gate_storage::{InMemoryModelAliasRepo, ModelAliasRecord};

        let (pid, router) = setup_fixtures(&[("ch-mini", "openai", vec!["gpt-4o-mini".into()], 1)]);
        let alias_repo = Arc::new(InMemoryModelAliasRepo::new());
        alias_repo.seed(ModelAliasRecord {
            id: Uuid::now_v7(),
            project_id: *pid.as_uuid(),
            alias: "fast".to_string(),
            target_model: "gpt-4o-mini".to_string(),
            group_id: None,
            params_override: serde_json::json!({ "temperature": 0.2 }),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        let router = router.with_model_alias_repo(alias_repo);
        assert_eq!(router.bump_snapshot_version(), 2);

        let routed = router.route(pid, "fast").await.unwrap().unwrap();
        let trace = routed.decision_trace;

        assert_eq!(trace.snapshot_version, 2);
        assert_eq!(trace.requested_model, "fast");
        assert_eq!(trace.alias_target_model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(trace.initial_model, "gpt-4o-mini");
        assert_eq!(trace.selected_model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(
            routed.params_override,
            serde_json::json!({ "temperature": 0.2 })
        );
    }

    #[tokio::test]
    async fn route_chat_records_capability_skip_reason() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let text_only = make_channel_with_models("text-only", "plugin", vec![]);
        let selected = make_channel_with_models("vision-openai", "openai", vec![]);
        let text_only_id = text_only.channel_id;
        let selected_id = selected.channel_id;

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "capabilities".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        group_repo.seed_default(project_id, group_id);
        channel_repo.seed_channel(ChannelRecord {
            provider_type: "plugin".to_string(),
            model_mapping: serde_json::json!({
                "plugin": {
                    "version": 1,
                    "capabilities": { "chat": true, "streaming": true },
                    "auth": { "strategy": "none" }
                }
            }),
            ..text_only
        });
        channel_repo.seed_channel(selected);
        channel_repo.seed_binding(group_id, text_only_id, 1, 1);
        channel_repo.seed_binding(group_id, selected_id, 2, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let routed = router
            .route_chat(
                project_id,
                &ChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: vec![ChatMessage {
                        role: Role::User,
                        content: Some(crate::types::MessageContent::Parts(vec![
                            crate::types::ContentPart::ImageUrl {
                                r#type: crate::types::ContentType::ImageUrl,
                                image_url: crate::types::ImageUrl {
                                    url: "data:image/png;base64,AA==".to_string(),
                                    detail: None,
                                },
                            },
                        ])),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }],
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .expect("vision-capable fallback should route");

        assert_eq!(routed.channel_id, selected_id);
        assert!(
            routed.decision_trace.skipped.iter().any(|skip| {
                skip.channel_id == text_only_id && skip.reason == "missing_capability:vision"
            }),
            "trace should explain capability rejection: {:?}",
            routed.decision_trace.skipped
        );
    }

    #[tokio::test]
    async fn route_chat_required_normalizes_no_healthy_channel() {
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let mut dead = make_channel_with_models("dead-openai", "openai", vec![]);
        dead.status = "disabled".to_string();
        dead.health = "unhealthy".to_string();
        let dead_id = dead.channel_id;

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "dead-group".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        group_repo.seed_default(project_id, group_id);
        channel_repo.seed_channel(dead);
        channel_repo.seed_binding(group_id, dead_id, 1, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let err = match router
            .route_chat_required(
                project_id,
                &ChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: vec![ChatMessage::text(Role::User, "hi")],
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => panic!("expected route miss"),
            Err(err) => err,
        };

        match err {
            ProviderError::Mapped {
                status,
                code,
                metadata,
                ..
            } => {
                assert_eq!(status, Some(404));
                assert_eq!(code.as_deref(), Some("no_healthy_channel"));
                assert_eq!(metadata.kind, NormalizedProviderErrorKind::ModelNotFound);
            }
            other => panic!("expected mapped no healthy route miss, got {other:?}"),
        }
    }

    #[test]
    fn provider_runtime_snapshot_is_replaceable_and_versioned() {
        let (_pid, router, ch_ids) = setup_strategy_fixtures("priority", &[("ch-one", 1, 1)]);

        assert_eq!(router.runtime_snapshot().version, 1);
        let version = router.replace_runtime_snapshot(vec![ProviderRuntimeChannelSnapshot {
            channel_id: ch_ids[0],
            channel_code: "ch-one".to_string(),
            provider_type: "openai".to_string(),
            supported_models: vec!["gpt-4o-mini".to_string()],
            status: "active".to_string(),
            health: "healthy".to_string(),
        }]);

        let snapshot = router.runtime_snapshot();
        assert_eq!(version, 2);
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.channels.len(), 1);
        assert_eq!(snapshot.channels[0].channel_code, "ch-one");
    }

    // ---- G1: channel key resolution tests ----

    use gate_core::id::ChannelKeyId;
    use gate_storage::{ChannelKeyRecord, InMemoryChannelKeyRepo};

    fn make_channel_simple(code: &str) -> (ChannelId, ChannelRecord) {
        let id = ChannelId::from(Uuid::now_v7());
        let now = Utc::now();
        let rec = ChannelRecord {
            channel_id: id,
            code: code.to_string(),
            name: code.to_string(),
            provider_type: "openai".to_string(),
            base_url: "https://api.example.com".to_string(),
            supported_models: vec!["gpt-4o".to_string()],
            status: "active".to_string(),
            health: "healthy".to_string(),
            timeout_ms: 60000,
            max_retries: 2,
            rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::Value::Object(Default::default()),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        };
        (id, rec)
    }

    async fn build_router_with_key(secret: &str) -> (ProviderRouter, ChannelId, ProjectId) {
        use gate_crypto::kms::{EnvKms, generate_master_key_b64};

        let (ch_id, ch_rec) = make_channel_simple("test-ch");
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let ch_repo = Arc::new(InMemoryChannelRepo::new());
        ch_repo.seed_channel(ch_rec);
        ch_repo.seed_binding(group_id, ch_id, 1, 100);

        let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
        grp_repo.seed_group(gate_storage::ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        grp_repo.seed_default(project_id, group_id);

        let kms = EnvKms::from_b64(&generate_master_key_b64(), "test").unwrap();
        let sealer = Arc::new(gate_crypto::EnvelopeKms::new(kms));

        let aad = gate_crypto::aad::channel_key(*ch_id.as_uuid());
        let key_enc = sealer.seal(secret.as_bytes(), &aad).await.unwrap();

        let ck_repo = Arc::new(InMemoryChannelKeyRepo::new());
        let now = Utc::now();
        ck_repo.seed(ChannelKeyRecord {
            id: ChannelKeyId::from(Uuid::now_v7()),
            channel_id: ch_id,
            label: Some("test-key".to_string()),
            key_enc: key_enc.clone(),
            key_fingerprint: "fp-test".to_string(),
            weight: 1,
            health: "healthy".to_string(),
            consecutive_errors: 0,
            total_requests: 0,
            total_errors: 0,
            last_error_code: None,
            last_error_at: None,
            cooldown_until: None,
            created_at: now,
            updated_at: now,
        });

        let router = ProviderRouter::new(ch_repo, grp_repo)
            .with_channel_key_repo(ck_repo)
            .with_crypto(sealer);

        (router, ch_id, project_id)
    }

    #[tokio::test]
    async fn router_prefers_db_key_over_env() {
        let (router, _ch_id, project_id) = build_router_with_key("sk-from-database-secret").await;
        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some());
        let routed = result.unwrap();
        assert_eq!(routed.resolved_model, "gpt-4o");
    }

    #[tokio::test]
    async fn router_fallback_env_when_no_db_key() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let (ch_id, ch_rec) = make_channel_simple("env-test-ch");

        let ch_repo = Arc::new(InMemoryChannelRepo::new());
        ch_repo.seed_channel(ch_rec);
        ch_repo.seed_binding(group_id, ch_id, 1, 100);

        let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
        grp_repo.seed_group(gate_storage::ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        grp_repo.seed_default(project_id, group_id);

        let ck_repo = Arc::new(InMemoryChannelKeyRepo::new());
        let router = ProviderRouter::new(ch_repo, grp_repo).with_channel_key_repo(ck_repo);

        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(
            result.is_some(),
            "should fallback to env var and still route"
        );
    }

    #[tokio::test]
    async fn router_fallback_env_when_no_repo_configured() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let (ch_id, ch_rec) = make_channel_simple("no-repo-ch");

        let ch_repo = Arc::new(InMemoryChannelRepo::new());
        ch_repo.seed_channel(ch_rec);
        ch_repo.seed_binding(group_id, ch_id, 1, 100);

        let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
        grp_repo.seed_group(gate_storage::ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        grp_repo.seed_default(project_id, group_id);

        let router = ProviderRouter::new(ch_repo, grp_repo);

        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should use env var when no key repo");
    }

    #[tokio::test]
    async fn router_db_key_decrypt_roundtrip() {
        let secret = "sk-real-api-key-12345";
        let (router, ch_id, _project_id) = build_router_with_key(secret).await;

        let (resolved, key_id) = router
            .resolve_key_for_channel(ch_id, "test-ch")
            .await
            .unwrap();
        assert_eq!(resolved, secret);
        assert!(
            key_id.is_some(),
            "key_id should be Some when resolved from DB"
        );
    }

    #[tokio::test]
    async fn router_secret_slots_use_channel_key_labels() {
        use gate_crypto::kms::{EnvKms, generate_master_key_b64};

        let ch_id = ChannelId::from(Uuid::now_v7());
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let now = Utc::now();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (request_tx, request_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = request_tx.send(raw);
            let body = serde_json::json!({
                "id": "chatcmpl-slot",
                "model": "odd-model",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "ok" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let ch_repo = Arc::new(InMemoryChannelRepo::new());
        ch_repo.seed_channel(ChannelRecord {
            channel_id: ch_id,
            code: "slot-plugin".to_string(),
            name: "slot-plugin".to_string(),
            provider_type: "plugin".to_string(),
            base_url,
            supported_models: vec!["odd-model".to_string()],
            status: "active".to_string(),
            health: "healthy".to_string(),
            timeout_ms: 60000,
            max_retries: 0,
            rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::json!({
                "plugin": {
                    "version": 1,
                    "auth": {
                        "strategy": "api_key_header",
                        "header_name": "X-Alt-Key",
                        "secret_slot": "alt-key"
                    }
                }
            }),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        });
        ch_repo.seed_binding(group_id, ch_id, 1, 100);

        let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
        grp_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        grp_repo.seed_default(project_id, group_id);

        let kms = EnvKms::from_b64(&generate_master_key_b64(), "test").unwrap();
        let sealer = Arc::new(gate_crypto::EnvelopeKms::new(kms));
        let aad = gate_crypto::aad::channel_key(*ch_id.as_uuid());
        let primary_enc = sealer.seal(b"sk-primary-slot", &aad).await.unwrap();
        let alt_enc = sealer.seal(b"sk-alt-slot", &aad).await.unwrap();

        let ck_repo = Arc::new(InMemoryChannelKeyRepo::new());
        ck_repo.seed(ChannelKeyRecord {
            id: ChannelKeyId::from(Uuid::now_v7()),
            channel_id: ch_id,
            label: Some("primary".to_string()),
            key_enc: primary_enc,
            key_fingerprint: "fp-primary-slot".to_string(),
            weight: 10,
            health: "healthy".to_string(),
            consecutive_errors: 0,
            total_requests: 0,
            total_errors: 0,
            last_error_code: None,
            last_error_at: None,
            cooldown_until: None,
            created_at: now,
            updated_at: now,
        });
        ck_repo.seed(ChannelKeyRecord {
            id: ChannelKeyId::from(Uuid::now_v7()),
            channel_id: ch_id,
            label: Some("alt-key".to_string()),
            key_enc: alt_enc,
            key_fingerprint: "fp-alt-slot".to_string(),
            weight: 1,
            health: "healthy".to_string(),
            consecutive_errors: 0,
            total_requests: 0,
            total_errors: 0,
            last_error_code: None,
            last_error_at: None,
            cooldown_until: None,
            created_at: now,
            updated_at: now,
        });

        let router = ProviderRouter::new(ch_repo, grp_repo)
            .with_channel_key_repo(ck_repo)
            .with_crypto(sealer);
        let (_, key_id, secrets) = router
            .resolve_secrets_for_channel(ch_id, "slot-plugin")
            .await
            .unwrap();
        assert!(key_id.is_some());
        assert_eq!(
            secrets.get("primary").map(String::as_str),
            Some("sk-primary-slot")
        );
        assert_eq!(
            secrets.get("alt-key").map(String::as_str),
            Some("sk-alt-slot")
        );

        let routed = router
            .route(project_id, "odd-model")
            .await
            .unwrap()
            .unwrap();
        let response = routed
            .provider
            .chat(ChatRequest {
                model: "odd-model".to_string(),
                messages: vec![ChatMessage::text(Role::User, "slot check")],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.choices[0].message.content_text(), "ok");

        let raw_request = request_rx.await.unwrap();
        assert!(
            raw_request.contains("x-alt-key: sk-alt-slot"),
            "plugin auth must use the manifest secret_slot, request={raw_request}"
        );
        assert!(
            !raw_request.contains("sk-primary-slot"),
            "primary secret must not be injected for alt-key auth, request={raw_request}"
        );
    }

    // ============================================================================
    // G8: routing strategy tests
    // ============================================================================

    /// Helper: setup with custom strategy and per-channel weights.
    fn setup_strategy_fixtures(
        strategy: &str,
        channels_spec: &[(&str, i32, i32)], // (code, priority, weight)
    ) -> (ProjectId, ProviderRouter, Vec<ChannelId>) {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

        let now = Utc::now();
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
            description: String::new(),
            strategy: strategy.to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        group_repo.seed_default(project_id, group_id);

        let mut channel_ids = Vec::new();
        for (code, priority, weight) in channels_spec {
            let ch = make_channel_with_models(code, "openai", vec![]);
            let ch_id = ch.channel_id;
            channel_ids.push(ch_id);
            channel_repo.seed_channel(ch);
            channel_repo.seed_binding(group_id, ch_id, *priority, *weight);
        }

        let router = ProviderRouter::new(channel_repo, group_repo);
        (project_id, router, channel_ids)
    }

    // ---- weighted_random ----

    #[tokio::test]
    async fn weighted_random_distribution_roughly_matches_weights() {
        // channel A weight=9, channel B weight=1
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("weighted_random", &[("ch-a", 1, 9), ("ch-b", 2, 1)]);

        let mut counts = [0u32; 2];
        let iterations = 2000;
        for _ in 0..iterations {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            if routed.channel_id == ch_ids[0] {
                counts[0] += 1;
            } else {
                counts[1] += 1;
            }
        }

        // Expected: ~90% for A, ~10% for B. Allow wide tolerance (±15%).
        let ratio_a = counts[0] as f64 / iterations as f64;
        assert!(
            ratio_a > 0.70 && ratio_a < 0.98,
            "expected ~90% for weight-9 channel, got {ratio_a:.2} ({}/{})",
            counts[0],
            iterations
        );
    }

    #[tokio::test]
    async fn weighted_random_single_channel_always_selected() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("weighted_random", &[("ch-only", 1, 5)]);

        for _ in 0..100 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            assert_eq!(routed.channel_id, ch_ids[0]);
        }
    }

    // ---- round_robin ----

    #[tokio::test]
    async fn round_robin_cycles_through_channels() {
        let (pid, router, ch_ids) = setup_strategy_fixtures(
            "round_robin",
            &[("ch-a", 1, 1), ("ch-b", 2, 1), ("ch-c", 3, 1)],
        );

        let mut sequence = Vec::new();
        for _ in 0..9 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            sequence.push(routed.channel_id);
        }

        // Should cycle: A, B, C, A, B, C, A, B, C
        for i in 0..9 {
            assert_eq!(
                sequence[i],
                ch_ids[i % 3],
                "round_robin mismatch at position {i}"
            );
        }
    }

    #[tokio::test]
    async fn round_robin_single_channel() {
        let (pid, router, ch_ids) = setup_strategy_fixtures("round_robin", &[("ch-only", 1, 1)]);

        for _ in 0..10 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            assert_eq!(routed.channel_id, ch_ids[0]);
        }
    }

    // ---- least_conn ----

    #[tokio::test]
    async fn least_conn_prefers_channel_with_fewer_inflight() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("least_conn", &[("ch-a", 1, 1), ("ch-b", 2, 1)]);

        // First request → both at 0, should pick first (A) due to min_by_key stability
        let r1 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(r1.channel_id, ch_ids[0], "first request should go to A");

        // A now has inflight=1, B has 0 → next should go to B
        let r2 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(
            r2.channel_id, ch_ids[1],
            "second request should go to B (less inflight)"
        );

        // Both have inflight=1 → should pick A (first in iter with equal count)
        let r3 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(
            r3.channel_id, ch_ids[0],
            "third request should go to A (tie-break by priority)"
        );

        // Release A twice → A has 0, B has 1
        router.release_channel(ch_ids[0]);
        router.release_channel(ch_ids[0]);

        let r4 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(r4.channel_id, ch_ids[0], "after release, A preferred again");
    }

    #[tokio::test]
    async fn least_conn_release_channel_decrements() {
        let tracker = InflightTracker::new();
        let ch = ChannelId::from(Uuid::now_v7());

        assert_eq!(tracker.current(ch), 0);
        tracker.acquire(ch);
        assert_eq!(tracker.current(ch), 1);
        tracker.acquire(ch);
        assert_eq!(tracker.current(ch), 2);
        tracker.release(ch);
        assert_eq!(tracker.current(ch), 1);
        tracker.release(ch);
        assert_eq!(tracker.current(ch), 0);
    }

    #[tokio::test]
    async fn least_latency_prefers_persistent_sliding_window() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("least_latency", &[("ch-a", 1, 1), ("ch-b", 2, 1)]);
        let latency_repo = Arc::new(InMemoryChannelLatencyRepo::new());
        latency_repo
            .record_sample(ch_ids[0], 200, true, "request")
            .await
            .unwrap();
        latency_repo
            .record_sample(ch_ids[1], 50, true, "health_probe")
            .await
            .unwrap();
        let router = router.with_channel_latency_repo(latency_repo);

        let routed = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(
            routed.channel_id, ch_ids[1],
            "persistent sliding window should beat priority for least_latency"
        );
    }

    // ---- priority still works (regression) ----

    #[tokio::test]
    async fn priority_strategy_still_picks_lowest_priority() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("priority", &[("ch-low", 10, 1), ("ch-high", 1, 1)]);

        // Should always pick ch-high (priority=1)
        for _ in 0..10 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            assert_eq!(routed.channel_id, ch_ids[1]);
        }
    }

    #[tokio::test]
    async fn unknown_strategy_falls_back_to_priority() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("unknown_strat", &[("ch-low", 10, 1), ("ch-high", 1, 1)]);

        let routed = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(routed.channel_id, ch_ids[1]);
    }

    // ============================================================================
    // Group fallback chain tests
    // ============================================================================

    /// Build a router with two groups: primary has no channels for model X,
    /// fallback has a channel. Expect routing to succeed via fallback group.
    #[tokio::test]
    async fn group_fallback_chain_routes_to_fallback_group() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let primary_group_id = ChannelGroupId::from(Uuid::now_v7());
        let fallback_group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        let now = Utc::now();

        // Primary group has only a gpt-4o channel
        group_repo.seed_group(ChannelGroupRecord {
            group_id: primary_group_id,
            name: "primary".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: Some(fallback_group_id),
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        group_repo.seed_default(project_id, primary_group_id);

        let ch_gpt = make_channel_with_models("ch-gpt", "openai", vec!["gpt-4o".into()]);
        let ch_gpt_id = ch_gpt.channel_id;
        channel_repo.seed_channel(ch_gpt);
        channel_repo.seed_binding(primary_group_id, ch_gpt_id, 1, 1);

        // Fallback group has a claude channel
        group_repo.seed_group(ChannelGroupRecord {
            group_id: fallback_group_id,
            name: "fallback".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        let ch_claude =
            make_channel_with_models("ch-claude", "anthropic", vec!["claude-3-haiku".into()]);
        let ch_claude_id = ch_claude.channel_id;
        channel_repo.seed_channel(ch_claude);
        channel_repo.seed_binding(fallback_group_id, ch_claude_id, 1, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let result = router.route(project_id, "claude-3-haiku").await.unwrap();
        assert!(result.is_some(), "should route via fallback group");
        let routed = result.unwrap();
        assert_eq!(routed.channel_id, ch_claude_id);
        assert_eq!(routed.resolved_model, "claude-3-haiku");
    }

    /// Disabled primary group with no fallback → None.
    #[tokio::test]
    async fn disabled_group_no_fallback_returns_none() {
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        let now = Utc::now();

        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "disabled".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: false, // disabled!
            created_at: now,
            updated_at: now,
        });
        group_repo.seed_default(project_id, group_id);

        let ch = make_channel_with_models("ch-any", "openai", vec![]);
        let ch_id = ch.channel_id;
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding(group_id, ch_id, 1, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(
            result.is_none(),
            "disabled group with no fallback should return None"
        );
    }

    /// Disabled primary group → fallback to enabled group with a channel.
    #[tokio::test]
    async fn disabled_group_falls_through_to_fallback() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let primary_id = ChannelGroupId::from(Uuid::now_v7());
        let fallback_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        let now = Utc::now();

        group_repo.seed_group(ChannelGroupRecord {
            group_id: primary_id,
            name: "primary-disabled".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: Some(fallback_id),
            enabled: false,
            created_at: now,
            updated_at: now,
        });
        group_repo.seed_default(project_id, primary_id);

        group_repo.seed_group(ChannelGroupRecord {
            group_id: fallback_id,
            name: "fallback-enabled".to_string(),
            description: String::new(),
            strategy: "priority".to_string(),
            fallback_group_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        });
        let ch = make_channel_with_models("ch-fb", "openai", vec![]);
        let ch_id = ch.channel_id;
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding(fallback_id, ch_id, 1, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(
            result.is_some(),
            "should route through fallback after disabled primary"
        );
        assert_eq!(result.unwrap().channel_id, ch_id);
    }

    /// Three-level chain: A→B→C, only C has a matching channel.
    #[tokio::test]
    async fn group_fallback_three_levels_deep() {
        ensure_test_api_key();
        let project_id = ProjectId::from(Uuid::now_v7());
        let id_a = ChannelGroupId::from(Uuid::now_v7());
        let id_b = ChannelGroupId::from(Uuid::now_v7());
        let id_c = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        let now = Utc::now();

        for (id, fallback, name) in [
            (id_a, Some(id_b), "group-a"),
            (id_b, Some(id_c), "group-b"),
            (id_c, None, "group-c"),
        ] {
            group_repo.seed_group(ChannelGroupRecord {
                group_id: id,
                name: name.to_string(),
                description: String::new(),
                strategy: "priority".to_string(),
                fallback_group_id: fallback,
                enabled: true,
                created_at: now,
                updated_at: now,
            });
        }
        group_repo.seed_default(project_id, id_a);

        // Only group C has a channel
        let ch = make_channel_with_models("ch-c", "openai", vec!["target-model".into()]);
        let ch_id = ch.channel_id;
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding(id_c, ch_id, 1, 1);

        let router = ProviderRouter::new(channel_repo, group_repo);
        let result = router.route(project_id, "target-model").await.unwrap();
        assert!(result.is_some(), "should route through 3-level chain");
        assert_eq!(result.unwrap().channel_id, ch_id);
    }
}
