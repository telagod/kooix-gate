//! ProviderRouter — 按 project_id + model 选择 Provider。
//!
//! 路由逻辑：
//! 1. 从 ChannelGroupRepo 取 project 的默认分组（`projects.default_group_id`）
//! 2. 从 ChannelRepo 取分组内所有 healthy channel，按 strategy 选一个
//!    - priority（默认）：取 priority 数值最小的那条
//!    - 其余 strategy 在 C2 实现，当前退化为 priority
//! 3. 用 channel.base_url + env KOOIX_API_KEY（或 channel code 对应的 env 变量）构造 OpenAiProvider
//! 4. 找不到 channel_group 或 channel → 返回 None，调用方 fallback 到 AppState.provider
//!
//! **channel_keys 表暂不读取**（C1 阶段 Provider 用 env 占位）。

use crate::Provider;
use crate::error::{ProviderError, ProviderResult};
use crate::openai::OpenAiProvider;
use gate_core::id::{ChannelId, ProjectId};
use gate_storage::{ChannelGroupRepo, ChannelRepo};
use std::sync::Arc;

/// 路由命中结果：Provider + 它绑定的 channel_id（计费维度归属）。
#[derive(Clone)]
pub struct RoutedProvider {
    pub provider: Arc<dyn Provider>,
    pub channel_id: ChannelId,
}

/// API key 来源策略（C1 阶段只看 env）。
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

/// 多 Provider 路由器。
///
/// 持有 Repo 引用（Arc），每次请求惰性查询——无缓存（C1 阶段简单版）。
pub struct ProviderRouter {
    channel_repo: Arc<dyn ChannelRepo>,
    group_repo: Arc<dyn ChannelGroupRepo>,
}

impl ProviderRouter {
    pub fn new(channel_repo: Arc<dyn ChannelRepo>, group_repo: Arc<dyn ChannelGroupRepo>) -> Self {
        Self {
            channel_repo,
            group_repo,
        }
    }

    /// 根据 project_id + model 选 Provider。
    ///
    /// - `requested_model`：目前仅用于日志，C1 不做 model_filter 匹配
    /// - 返回 `None` 表示找不到可用渠道，调用方 fallback 到全局 provider
    /// - 返回 `Some(RoutedProvider)` 时 channel_id 为计费/审计追溯依据
    pub async fn route(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> ProviderResult<Option<RoutedProvider>> {
        // Step 1: 找 project 的默认 channel_group
        let group = match self.group_repo.find_default_for_project(project_id).await {
            Ok(g) => g,
            Err(gate_storage::DbError::NotFound) => {
                tracing::debug!(
                    project_id = %project_id,
                    model = requested_model,
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

        if !group.enabled {
            tracing::debug!(
                group_id = %group.group_id,
                "channel_group is disabled, falling back"
            );
            return Ok(None);
        }

        // Step 2: 取 group 内 healthy channels
        let bindings = self
            .channel_repo
            .list_healthy_in_group(group.group_id)
            .await
            .map_err(|e| ProviderError::Config(format!("channel list failed: {e}")))?;

        if bindings.is_empty() {
            tracing::warn!(
                group_id = %group.group_id,
                model = requested_model,
                "no healthy channels in group"
            );
            return Ok(None);
        }

        // Step 3: 按 strategy 选 channel
        // strategy: priority → 取第一条（已 ORDER BY priority ASC）
        // 其余 strategy C1 退化为 priority
        let selected = &bindings[0];

        tracing::debug!(
            project_id = %project_id,
            group = %group.name,
            channel = %selected.channel.code,
            model = requested_model,
            "routed to channel"
        );

        // Step 4: 构造 Provider
        let api_key = resolve_api_key_for_channel(&selected.channel.code);
        let provider = OpenAiProvider::new(selected.channel.base_url.clone(), api_key)
            .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;

        Ok(Some(RoutedProvider {
            provider: Arc::new(provider) as Arc<dyn Provider>,
            channel_id: selected.channel.channel_id,
        }))
    }
}
