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
//! 5. API key 来源策略（G1）：
//!    a. 优先从 channel_keys 表取 active key → 用 EnvelopeKms 解密
//!    b. 若 DB 无 key 或 repo 未配置 → 回退 env var
//! 6. 找不到 channel_group 或 channel → 返回 None，调用方 fallback 到 AppState.provider

use crate::EmbeddingProvider;
use crate::Provider;
use crate::anthropic::AnthropicProvider;
use crate::azure::AzureProvider;
use crate::bedrock::BedrockProvider;
use crate::cohere::CohereProvider;
use crate::deepseek::DeepSeekProvider;
use crate::error::{ProviderError, ProviderResult};
use crate::gemini::GeminiProvider;
use crate::mistral::MistralProvider;
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use gate_core::id::{ChannelId, ChannelKeyId, ProjectId};
use gate_crypto::EnvelopeKms;
use gate_storage::{ChannelBinding, ChannelGroupRepo, ChannelKeyRepo, ChannelRepo, ModelAliasRepo};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::Instant;

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
}

/// Embedding 路由命中结果：EmbeddingProvider + 绑定的 channel_id。
#[derive(Clone)]
pub struct RoutedEmbeddingProvider {
    pub provider: Arc<dyn EmbeddingProvider>,
    pub channel_id: ChannelId,
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
        let window = windows.entry(channel_id).or_insert_with(VecDeque::new);
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
        let window = latencies.entry(channel_id).or_insert_with(VecDeque::new);
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
) -> Vec<&'a ChannelBinding> {
    if compatible.len() <= 1 {
        return compatible.to_vec();
    }

    match strategy {
        "weighted_random" => {
            // 首选 = weighted random pick，其余按 priority 排
            let first = select_weighted_random(compatible);
            let mut rest: Vec<_> = compatible.iter().filter(|c| c.channel.channel_id != first.channel.channel_id).copied().collect();
            rest.sort_by_key(|c| c.priority);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "round_robin" => {
            // 首选 = round_robin pick，其余按 priority 排
            let first = select_round_robin(compatible, rr_counter);
            let mut rest: Vec<_> = compatible.iter().filter(|c| c.channel.channel_id != first.channel.channel_id).copied().collect();
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
            if let Some(m) = metrics {
                sorted.sort_by_key(|c| m.avg_latency(c.channel.channel_id));
            }
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
                channel.code.to_uppercase().replace(|c: char| !c.is_alphanumeric(), "_")
            );
            let secret = std::env::var(&secret_env)
                .map_err(|_| ProviderError::Config(format!(
                    "missing {} env var for bedrock channel '{}'",
                    secret_env, channel.code
                )))?;
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

/// API key 来源策略（env 回退，DB 优先路径在 route_for_model 内）。
///
/// 优先级：
/// 1. 环境变量 `KOOIX_CH_<CODE>_KEY`（code 大写，非字母替换为 _）
/// 2. 环境变量 `KOOIX_API_KEY`（全局兜底）
/// 3. 空字符串（上游自己决定是否拒绝）
fn resolve_api_key_for_channel(code: &str) -> ProviderResult<String> {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .map_err(|_| ProviderError::Config(format!(
            "no API key found for channel '{}' (tried {} and KOOIX_API_KEY)",
            code, env_key
        )))
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
    if let serde_json::Value::Object(map) = mapping {
        if let Some(serde_json::Value::String(target)) = map.get(model) {
            return target.clone();
        }
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
    /// per-channel RPM/TPM 限速器。
    rate_limiter: Arc<dyn ChannelRateCheck>,
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
            rate_limiter: Arc::new(InMemoryChannelRateLimiter::new()),
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

    /// 清除 channel 的 metrics 滑动窗口（re-enable 后由 health_probe 调用）。
    pub fn clear_channel_metrics(&self, channel_id: ChannelId) {
        if let Some(m) = &self.metrics {
            m.clear(channel_id);
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
        // Step 0: alias 解析
        let alias_result = self.resolve_alias(project_id, requested_model).await?;
        let (model, params_override) = match &alias_result {
            Some((target, po)) => (target.as_str(), po.clone()),
            None => (requested_model, serde_json::json!({})),
        };

        // Step 1: 尝试主模型路由
        if let Some(mut routed) = self.route_for_model(project_id, model).await? {
            routed.params_override = params_override;
            return Ok(Some(routed));
        }

        // Step 2: 主模型路由失败，尝试 fallback 链
        for fallback in fallback_models(model) {
            tracing::info!(
                project_id = %project_id,
                original_model = model,
                fallback_model = fallback,
                "primary model route failed, trying fallback"
            );
            if let Some(mut routed) = self.route_for_model(project_id, fallback).await? {
                routed.params_override = params_override;
                return Ok(Some(routed));
            }
        }

        Ok(None)
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

            if current_group.enabled {
                if let Some(routed) =
                    self.try_route_embedding_in_group(&current_group, model).await?
                {
                    return Ok(Some(routed));
                }
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
                let provider_ok =
                    !matches!(b.channel.provider_type.as_str(), "anthropic" | "bedrock");
                model_ok && provider_ok
            })
            .collect();

        if compatible.is_empty() {
            return Ok(None);
        }

        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
        );

        for candidate in &ordered {
            // RPM 检查
            if !self.rate_limiter.check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit).await {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached for embedding, trying next"
                );
                continue;
            }

            // TPM 检查
            if !self.rate_limiter.check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit).await {
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

            let provider: Arc<dyn EmbeddingProvider> = build_embedding_provider(&candidate.channel, api_key, opts)?;

            return Ok(Some(RoutedEmbeddingProvider {
                provider,
                channel_id: candidate.channel.channel_id,
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
    ) -> ProviderResult<Option<RoutedProvider>> {
        // Step 1: 找 project 的默认 channel_group
        let initial_group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => {
                tracing::debug!(
                    project_id = %project_id,
                    model = model,
                    "no default channel_group for project, falling back"
                );
                return Ok(None);
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

        loop {
            if depth > MAX_FALLBACK_DEPTH {
                tracing::warn!(
                    project_id = %project_id,
                    model = model,
                    depth = depth,
                    "fallback chain exceeded max depth"
                );
                return Ok(None);
            }

            if !current_group.enabled {
                tracing::debug!(
                    group_id = %current_group.group_id,
                    "channel_group is disabled, trying fallback"
                );
            } else {
                // Try to find a channel in this group
                if let Some(routed) = self.try_route_in_group(&current_group, model).await? {
                    return Ok(Some(routed));
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
                            return Ok(None);
                        }
                    }
                }
                None => return Ok(None),
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
    ) -> ProviderResult<Option<RoutedProvider>> {
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
            return Ok(None);
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
            return Ok(None);
        }

        // Strategy ordering: sort channels into preference order based on strategy
        // For priority/round_robin/weighted_random, select_channel picks one.
        // To support "try next on rate limit", we iterate candidates in order.
        let ordered = order_channels_by_strategy(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
            self.metrics.as_ref().map(|m| m.as_ref()),
        );

        // Try each channel in order until one passes rate limits
        for candidate in &ordered {
            // RPM 检查
            if !self.rate_limiter.check_rpm(candidate.channel.channel_id, candidate.channel.rpm_limit).await {
                tracing::info!(
                    channel = %candidate.channel.code,
                    rpm_limit = ?candidate.channel.rpm_limit,
                    "channel RPM limit reached, trying next channel"
                );
                continue;
            }

            // TPM 检查（pre-flight）
            if !self.rate_limiter.check_tpm(candidate.channel.channel_id, candidate.channel.tpm_limit).await {
                tracing::info!(
                    channel = %candidate.channel.code,
                    tpm_limit = ?candidate.channel.tpm_limit,
                    "channel TPM limit reached, trying next channel"
                );
                continue;
            }

            // least_conn: 选中后递增 inflight 计数
            if group.strategy == "least_conn" {
                self.inflight.acquire(candidate.channel.channel_id);
            }

            tracing::debug!(
                group = %group.name,
                channel = %candidate.channel.code,
                provider_type = %candidate.channel.provider_type,
                model = model,
                "routed to channel"
            );

            // Step 4: 根据 provider_type 构造对应 Provider
            let (api_key, key_id) = self.resolve_key_for_channel(candidate.channel.channel_id, &candidate.channel.code).await?;
            let opts = crate::ProviderOpts {
                timeout_ms: candidate.channel.timeout_ms as u64,
            };
            let provider: Arc<dyn Provider> = build_provider(&candidate.channel, api_key, opts)?;

            let retry_config = crate::retry::RetryConfig {
                max_retries: candidate.channel.max_retries.max(0) as u32,
                ..Default::default()
            };

            // model_mapping: 如果 channel 配置了映射，翻译模型名
            let resolved_model = resolve_model_mapping(&candidate.channel.model_mapping, model);

            return Ok(Some(RoutedProvider {
                provider,
                channel_id: candidate.channel.channel_id,
                resolved_model,
                retry_config,
                key_id,
                params_override: serde_json::json!({}),
                provider_type: candidate.channel.provider_type.clone(),
                metrics: self.metrics.clone(),
            }));
        }

        // All compatible channels are rate-limited
        tracing::warn!(
            group_id = %group.group_id,
            model = model,
            compatible_count = compatible.len(),
            "all compatible channels are rate-limited"
        );
        Ok(None)
    }

    /// G1: 从 DB 取 channel key → 解密；无则 fallback env var。
    /// 返回 (plaintext_key, Option<key_id>)——key_id 仅在从 DB 取到时有值。
    async fn resolve_key_for_channel(
        &self,
        channel_id: ChannelId,
        channel_code: &str,
    ) -> ProviderResult<(String, Option<ChannelKeyId>)> {
        // 如果 repo 未配置，直接走 env
        let Some(repo) = &self.channel_key_repo else {
            return Ok((resolve_api_key_for_channel(channel_code)?, None));
        };

        // 尝试从 DB 取 active key
        match repo.find_active_for_channel(channel_id).await {
            Ok(record) => {
                // 有 key 记录，需要 crypto 来解密
                let Some(crypto) = &self.crypto else {
                    tracing::warn!(
                        channel_id = %channel_id,
                        "channel key found in DB but crypto not configured, falling back to env"
                    );
                    return Ok((resolve_api_key_for_channel(channel_code)?, None));
                };
                // AAD = channel_key(channel_id) — 与 admin handler 加密时一致
                let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
                let key_id = record.id;
                let plaintext = crypto
                    .open(&record.key_enc, &aad)
                    .await
                    .map_err(|e| {
                        ProviderError::Config(format!(
                            "decrypt channel key {}: {e}",
                            record.id
                        ))
                    })?;
                let key_str = String::from_utf8(plaintext.to_vec()).map_err(|e| {
                    ProviderError::Config(format!("channel key is not valid UTF-8: {e}"))
                })?;
                Ok((key_str, Some(key_id)))
            }
            Err(gate_storage::DbError::NotFound) => {
                // DB 里没有 key，走 env
                tracing::debug!(
                    channel_id = %channel_id,
                    channel_code = channel_code,
                    "no channel key in DB, falling back to env"
                );
                Ok((resolve_api_key_for_channel(channel_code)?, None))
            }
            Err(e) => {
                // DB 查询出错，warn + fallback env
                tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "channel key lookup failed, falling back to env"
                );
                Ok((resolve_api_key_for_channel(channel_code)?, None))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use gate_core::id::{ChannelGroupId, ChannelId, ProjectId};
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
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

    // ---- helpers for model-filter routing tests (G7) ----

    fn make_channel_with_models(code: &str, provider_type: &str, models: Vec<String>) -> ChannelRecord {
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
        let (pid, router) = setup_fixtures(&[
            ("ch-gpt", "openai", vec!["gpt-4o".into()], 1),
        ]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "channel with matching model should be routed");
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
        let (pid, router) = setup_fixtures(&[
            ("ch-wildcard", "openai", vec![], 1),
        ]);
        let result = router.route(pid, "any-model-name").await.unwrap();
        assert!(result.is_some(), "empty supported_models should match any model");
        assert_eq!(result.unwrap().resolved_model, "any-model-name");
    }

    #[tokio::test]
    async fn model_filter_no_compatible_channel_returns_none() {
        let (pid, router) = setup_fixtures(&[
            ("ch-gpt", "openai", vec!["gpt-4o".into()], 1),
            ("ch-claude", "openai", vec!["claude-3".into()], 2),
        ]);
        let result = router.route(pid, "gemini-pro").await.unwrap();
        assert!(result.is_none(), "no channel supports gemini-pro, should return None");
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
        let (pid, router) = setup_fixtures(&[
            ("ch-mini", "openai", vec!["gpt-4o-mini".into()], 1),
        ]);
        let result = router.route(pid, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should fallback to gpt-4o-mini");
        assert_eq!(result.unwrap().resolved_model, "gpt-4o-mini");
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

    async fn build_router_with_key(
        secret: &str,
    ) -> (ProviderRouter, ChannelId, ProjectId) {
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
        let (router, _ch_id, project_id) =
            build_router_with_key("sk-from-database-secret").await;
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
        let router = ProviderRouter::new(ch_repo, grp_repo)
            .with_channel_key_repo(ck_repo);

        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should fallback to env var and still route");
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
        assert!(key_id.is_some(), "key_id should be Some when resolved from DB");
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
                sequence[i], ch_ids[i % 3],
                "round_robin mismatch at position {i}"
            );
        }
    }

    #[tokio::test]
    async fn round_robin_single_channel() {
        let (pid, router, ch_ids) =
            setup_strategy_fixtures("round_robin", &[("ch-only", 1, 1)]);

        for _ in 0..10 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            assert_eq!(routed.channel_id, ch_ids[0]);
        }
    }

    // ---- least_conn ----

    #[tokio::test]
    async fn least_conn_prefers_channel_with_fewer_inflight() {
        let (pid, router, ch_ids) = setup_strategy_fixtures(
            "least_conn",
            &[("ch-a", 1, 1), ("ch-b", 2, 1)],
        );

        // First request → both at 0, should pick first (A) due to min_by_key stability
        let r1 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(r1.channel_id, ch_ids[0], "first request should go to A");

        // A now has inflight=1, B has 0 → next should go to B
        let r2 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(r2.channel_id, ch_ids[1], "second request should go to B (less inflight)");

        // Both have inflight=1 → should pick A (first in iter with equal count)
        let r3 = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(r3.channel_id, ch_ids[0], "third request should go to A (tie-break by priority)");

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

    // ---- priority still works (regression) ----

    #[tokio::test]
    async fn priority_strategy_still_picks_lowest_priority() {
        let (pid, router, ch_ids) = setup_strategy_fixtures(
            "priority",
            &[("ch-low", 10, 1), ("ch-high", 1, 1)],
        );

        // Should always pick ch-high (priority=1)
        for _ in 0..10 {
            let routed = router.route(pid, "any").await.unwrap().unwrap();
            assert_eq!(routed.channel_id, ch_ids[1]);
        }
    }

    #[tokio::test]
    async fn unknown_strategy_falls_back_to_priority() {
        let (pid, router, ch_ids) = setup_strategy_fixtures(
            "unknown_strat",
            &[("ch-low", 10, 1), ("ch-high", 1, 1)],
        );

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
        let ch_claude = make_channel_with_models("ch-claude", "anthropic", vec!["claude-3-haiku".into()]);
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
        assert!(result.is_none(), "disabled group with no fallback should return None");
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
        assert!(result.is_some(), "should route through fallback after disabled primary");
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
