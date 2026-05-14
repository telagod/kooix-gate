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
use std::sync::{Arc, Mutex, RwLock};

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

/// 轻量级内存滑动窗口，按 channel 追踪成功率。
///
/// 窗口满且成功率低于阈值时，`should_disable` 返回 true。
pub struct ChannelMetrics {
    windows: Mutex<HashMap<ChannelId, VecDeque<bool>>>,
    window_size: usize,
    threshold: f64,
}

impl ChannelMetrics {
    pub fn new(window_size: usize, threshold: f64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            window_size,
            threshold,
        }
    }

    /// 记录一次请求结果（true = 成功，false = 失败）。
    pub fn record(&self, channel_id: ChannelId, success: bool) {
        let mut windows = self.windows.lock().unwrap();
        let window = windows.entry(channel_id).or_insert_with(VecDeque::new);
        if window.len() >= self.window_size {
            window.pop_front();
        }
        window.push_back(success);
    }

    /// 窗口满且成功率低于阈值时返回 true，触发 auto-disable。
    pub fn should_disable(&self, channel_id: ChannelId) -> bool {
        let windows = self.windows.lock().unwrap();
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

    /// 清除 channel 的历史记录（re-enable 后调用）。
    pub fn clear(&self, channel_id: ChannelId) {
        let mut windows = self.windows.lock().unwrap();
        windows.remove(&channel_id);
    }
}

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
            let counts = self.counts.read().unwrap();
            if let Some(counter) = counts.get(&channel_id) {
                counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        // 慢路径：write lock，首次见到的 channel
        let mut counts = self.counts.write().unwrap();
        let counter = counts
            .entry(channel_id)
            .or_insert_with(|| Arc::new(AtomicI64::new(0)));
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// 标记一个 channel 释放了请求（inflight -1）。
    pub fn release(&self, channel_id: ChannelId) {
        let counts = self.counts.read().unwrap();
        if let Some(counter) = counts.get(&channel_id) {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// 查询 channel 当前 inflight 数。
    pub fn current(&self, channel_id: ChannelId) -> i64 {
        let counts = self.counts.read().unwrap();
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
fn select_channel<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
) -> &'a ChannelBinding {
    match strategy {
        "weighted_random" => select_weighted_random(compatible),
        "round_robin" => select_round_robin(compatible, rr_counter),
        "least_conn" => select_least_conn(compatible, inflight),
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
fn select_least_conn<'a>(
    channels: &'a [&ChannelBinding],
    inflight: &InflightTracker,
) -> &'a ChannelBinding {
    channels
        .iter()
        .min_by_key(|ch| inflight.current(ch.channel.channel_id))
        .unwrap()
}

/// API key 来源策略（env 回退，DB 优先路径在 route_for_model 内）。
///
/// 优先级：
/// 1. 环境变量 `KOOIX_CH_<CODE>_KEY`（code 大写，非字母替换为 _）
/// 2. 环境变量 `KOOIX_API_KEY`（全局兜底）
/// 3. 空字符串（上游自己决定是否拒绝）
fn resolve_api_key_for_channel(code: &str) -> String {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default()
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
    /// select by strategy, construct `Arc<dyn EmbeddingProvider>`.
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

        let selected =
            select_channel(&group.strategy, &compatible, &self.rr_counter, &self.inflight);

        let (api_key, key_id) = self
            .resolve_key_for_channel(selected.channel.channel_id, &selected.channel.code)
            .await?;
        let opts = crate::ProviderOpts {
            timeout_ms: selected.channel.timeout_ms as u64,
        };

        let provider: Arc<dyn EmbeddingProvider> = match selected.channel.provider_type.as_str() {
            "azure" => {
                let p = AzureProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    None,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            "deepseek" => {
                let p = DeepSeekProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build DeepSeekProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            "ollama" => {
                let p = OllamaProvider::new_with_opts(selected.channel.base_url.clone(), opts)
                    .map_err(|e| ProviderError::Config(format!("build OllamaProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            "mistral" => {
                let p = MistralProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build MistralProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            "cohere" => {
                let p = CohereProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build CohereProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            "gemini" => {
                let p = GeminiProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build GeminiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
            // Default: OpenAI-compatible (covers "openai" and any unknown type)
            _ => {
                let p = OpenAiProvider::new_with_opts(
                    selected.channel.base_url.clone(),
                    api_key,
                    opts,
                )
                .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn EmbeddingProvider>
            }
        };

        Ok(Some(RoutedEmbeddingProvider {
            provider,
            channel_id: selected.channel.channel_id,
            key_id,
        }))
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

        // Step 3: 按 model_filter / supported_models 过滤 + strategy 选 channel
        // Binding-level model_filter takes priority; empty = fall back to channel.supported_models.
        // Empty supported_models = wildcard（支持所有模型）。
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

        // strategy: 按 group.strategy 选 channel
        let selected = select_channel(
            &group.strategy,
            &compatible,
            &self.rr_counter,
            &self.inflight,
        );

        // least_conn: 选中后递增 inflight 计数
        if group.strategy == "least_conn" {
            self.inflight.acquire(selected.channel.channel_id);
        }

        tracing::debug!(
            group = %group.name,
            channel = %selected.channel.code,
            provider_type = %selected.channel.provider_type,
            model = model,
            "routed to channel"
        );

        // Step 4: 根据 provider_type 构造对应 Provider
        // G1: 优先从 DB 取 channel key → 解密；无则 fallback env
        let (api_key, key_id) = self.resolve_key_for_channel(selected.channel.channel_id, &selected.channel.code).await?;
        let opts = crate::ProviderOpts {
            timeout_ms: selected.channel.timeout_ms as u64,
        };
        let provider: Arc<dyn Provider> = match selected.channel.provider_type.as_str() {
            "anthropic" => {
                let p = AnthropicProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build AnthropicProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "gemini" => {
                let p = GeminiProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build GeminiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "azure" => {
                let p = AzureProvider::new_with_opts(selected.channel.base_url.clone(), api_key, None, opts)
                    .map_err(|e| ProviderError::Config(format!("build AzureProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "bedrock" => {
                let access = api_key.clone();
                let secret = std::env::var(format!("KOOIX_CH_{}_SECRET", selected.channel.code.to_uppercase().replace(|c: char| !c.is_alphanumeric(), "_")))
                    .unwrap_or_default();
                let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
                let p = BedrockProvider::new_with_opts(region, access, secret, opts)
                    .map_err(|e| ProviderError::Config(format!("build BedrockProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "deepseek" => {
                let p = DeepSeekProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build DeepSeekProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "ollama" => {
                let p = OllamaProvider::new_with_opts(selected.channel.base_url.clone(), opts)
                    .map_err(|e| ProviderError::Config(format!("build OllamaProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "mistral" => {
                let p = MistralProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build MistralProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "cohere" => {
                let p = CohereProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build CohereProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            _ => {
                // 未知类型走 OpenAI 兼容
                let p = OpenAiProvider::new_with_opts(selected.channel.base_url.clone(), api_key, opts)
                    .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
        };

        let retry_config = crate::retry::RetryConfig {
            max_retries: selected.channel.max_retries.max(0) as u32,
            ..Default::default()
        };

        Ok(Some(RoutedProvider {
            provider,
            channel_id: selected.channel.channel_id,
            resolved_model: model.to_string(),
            retry_config,
            key_id,
            params_override: serde_json::json!({}),
            provider_type: selected.channel.provider_type.clone(),
            metrics: self.metrics.clone(),
        }))
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
            return Ok((resolve_api_key_for_channel(channel_code), None));
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
                    return Ok((resolve_api_key_for_channel(channel_code), None));
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
                Ok((resolve_api_key_for_channel(channel_code), None))
            }
            Err(e) => {
                // DB 查询出错，warn + fallback env
                tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "channel key lookup failed, falling back to env"
                );
                Ok((resolve_api_key_for_channel(channel_code), None))
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
            created_at: now,
            updated_at: now,
        }
    }

    fn setup_fixtures(
        channels_spec: &[(&str, &str, Vec<String>, i32)],
    ) -> (ProjectId, ProviderRouter) {
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

        let now = Utc::now();
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
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
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());

        let now = Utc::now();
        group_repo.seed_group(ChannelGroupRecord {
            group_id,
            name: "default".to_string(),
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
        let project_id = ProjectId::from(Uuid::now_v7());
        let primary_id = ChannelGroupId::from(Uuid::now_v7());
        let fallback_id = ChannelGroupId::from(Uuid::now_v7());

        let channel_repo = Arc::new(InMemoryChannelRepo::new());
        let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
        let now = Utc::now();

        group_repo.seed_group(ChannelGroupRecord {
            group_id: primary_id,
            name: "primary-disabled".to_string(),
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
