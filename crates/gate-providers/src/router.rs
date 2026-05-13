//! ProviderRouter — 按 project_id + model 选择 Provider。
//!
//! 路由逻辑：
//! 1. 如果有 ModelAliasRepo，先做 alias → target_model 解析
//! 2. 从 ChannelGroupRepo 取 project 的默认分组（`projects.default_group_id`）
//! 3. 从 ChannelRepo 取分组内所有 healthy channel，按 strategy 选一个
//!    - priority（默认）：取 priority 数值最小的那条
//!    - 其余 strategy 在 C2 实现，当前退化为 priority
//! 4. 用 channel.provider_type 构造对应的 Provider（openai / anthropic / gemini）
//! 5. 找不到 channel_group 或 channel → 返回 None，调用方 fallback 到 AppState.provider
//!
//! **channel_keys 表暂不读取**（C1 阶段 Provider 用 env 占位）。

use crate::Provider;
use crate::anthropic::AnthropicProvider;
use crate::error::{ProviderError, ProviderResult};
use crate::gemini::GeminiProvider;
use crate::openai::OpenAiProvider;
use gate_core::id::{ChannelId, ProjectId};
use gate_storage::{ChannelGroupRepo, ChannelRepo, ModelAliasRepo};
use std::sync::Arc;

/// 路由命中结果：Provider + 它绑定的 channel_id（计费维度归属）+ 实际使用的 model。
#[derive(Clone)]
pub struct RoutedProvider {
    pub provider: Arc<dyn Provider>,
    pub channel_id: ChannelId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
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
}

impl ProviderRouter {
    pub fn new(channel_repo: Arc<dyn ChannelRepo>, group_repo: Arc<dyn ChannelGroupRepo>) -> Self {
        Self {
            channel_repo,
            group_repo,
            model_alias_repo: None,
        }
    }

    /// 挂载 ModelAliasRepo，启用 alias 解析。
    pub fn with_model_alias_repo(mut self, repo: Arc<dyn ModelAliasRepo>) -> Self {
        self.model_alias_repo = Some(repo);
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
        let canonical_model = self.resolve_alias(project_id, requested_model).await?;
        let model = canonical_model.as_deref().unwrap_or(requested_model);

        // Step 1: 尝试主模型路由
        if let Some(routed) = self.route_for_model(project_id, model).await? {
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
            if let Some(routed) = self.route_for_model(project_id, fallback).await? {
                return Ok(Some(routed));
            }
        }

        Ok(None)
    }

    /// alias 解析：查 ModelAliasRepo，返回 Some(target) 或 None（无 alias）。
    async fn resolve_alias(
        &self,
        project_id: ProjectId,
        requested_model: &str,
    ) -> ProviderResult<Option<String>> {
        let Some(repo) = &self.model_alias_repo else {
            return Ok(None);
        };
        match repo.resolve(project_id, requested_model).await {
            Ok(target) => {
                if let Some(ref t) = target {
                    tracing::debug!(
                        project_id = %project_id,
                        alias = requested_model,
                        target = t,
                        "model alias resolved"
                    );
                }
                Ok(target)
            }
            Err(e) => {
                // alias 解析失败不阻断请求，退化为原始模型
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

    /// 为指定模型做实际路由（查 group → channel → 构造 provider）。
    async fn route_for_model(
        &self,
        project_id: ProjectId,
        model: &str,
    ) -> ProviderResult<Option<RoutedProvider>> {
        // Step 1: 找 project 的默认 channel_group
        let group = match self.group_repo.find_default_for_project(project_id).await {
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
                model = model,
                "no healthy channels in group"
            );
            return Ok(None);
        }

        // Step 3: 按 strategy 选 channel
        // strategy: priority → 取第一条（已 ORDER BY priority ASC）
        let selected = &bindings[0];

        tracing::debug!(
            project_id = %project_id,
            group = %group.name,
            channel = %selected.channel.code,
            provider_type = %selected.channel.provider_type,
            model = model,
            "routed to channel"
        );

        // Step 4: 根据 provider_type 构造对应 Provider
        let api_key = resolve_api_key_for_channel(&selected.channel.code);
        let provider: Arc<dyn Provider> = match selected.channel.provider_type.as_str() {
            "anthropic" => {
                let p = AnthropicProvider::new(selected.channel.base_url.clone(), api_key)
                    .map_err(|e| ProviderError::Config(format!("build AnthropicProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            "gemini" => {
                let p = GeminiProvider::new(selected.channel.base_url.clone(), api_key)
                    .map_err(|e| ProviderError::Config(format!("build GeminiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
            _ => {
                // "openai" + 其他未知类型都走 OpenAI 兼容
                let p = OpenAiProvider::new(selected.channel.base_url.clone(), api_key)
                    .map_err(|e| ProviderError::Config(format!("build OpenAiProvider: {e}")))?;
                Arc::new(p) as Arc<dyn Provider>
            }
        };

        Ok(Some(RoutedProvider {
            provider,
            channel_id: selected.channel.channel_id,
            resolved_model: model.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
