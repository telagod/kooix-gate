//! ProviderRouter — 按 project_id + model 选择 Provider。
//!
//! 路由逻辑：
//! 1. 如果有 ModelAliasRepo，先做 alias → target_model 解析
//! 2. 从 ChannelGroupRepo 取 project 的默认分组（`projects.default_group_id`）
//! 3. 从 ChannelRepo 取分组内所有 healthy channel，按 strategy 选一个
//!    - priority（默认）：取 priority 数值最小的那条
//!    - 其余 strategy 在 C2 实现，当前退化为 priority
//! 4. 用 channel.provider_type 构造对应的 Provider（openai / anthropic / gemini）
//! 5. API key 来源策略（G1）：
//!    a. 优先从 channel_keys 表取 active key → 用 EnvelopeKms 解密
//!    b. 若 DB 无 key 或 repo 未配置 → 回退 env var
//! 6. 找不到 channel_group 或 channel → 返回 None，调用方 fallback 到 AppState.provider

use crate::Provider;
use crate::anthropic::AnthropicProvider;
use crate::error::{ProviderError, ProviderResult};
use crate::gemini::GeminiProvider;
use crate::openai::OpenAiProvider;
use gate_core::id::{ChannelId, ProjectId};
use gate_crypto::EnvelopeKms;
use gate_storage::{ChannelGroupRepo, ChannelKeyRepo, ChannelRepo, ModelAliasRepo};
use std::sync::Arc;

/// 路由命中结果：Provider + 它绑定的 channel_id（计费维度归属）+ 实际使用的 model。
#[derive(Clone)]
pub struct RoutedProvider {
    pub provider: Arc<dyn Provider>,
    pub channel_id: ChannelId,
    /// 经 alias 解析后的实际模型名。如果没有 alias 就是原始请求的 model。
    pub resolved_model: String,
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
}

impl ProviderRouter {
    pub fn new(channel_repo: Arc<dyn ChannelRepo>, group_repo: Arc<dyn ChannelGroupRepo>) -> Self {
        Self {
            channel_repo,
            group_repo,
            model_alias_repo: None,
            channel_key_repo: None,
            crypto: None,
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
        // G1: 优先从 DB 取 channel key → 解密；无则 fallback env
        let api_key = self.resolve_key_for_channel(selected.channel.channel_id, &selected.channel.code).await?;
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

    /// G1: 从 DB 取 channel key → 解密；无则 fallback env var。
    async fn resolve_key_for_channel(
        &self,
        channel_id: ChannelId,
        channel_code: &str,
    ) -> ProviderResult<String> {
        // 如果 repo 未配置，直接走 env
        let Some(repo) = &self.channel_key_repo else {
            return Ok(resolve_api_key_for_channel(channel_code));
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
                    return Ok(resolve_api_key_for_channel(channel_code));
                };
                // AAD = channel_key(channel_id) — 与 admin handler 加密时一致
                let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
                let plaintext = crypto
                    .open(&record.key_enc, &aad)
                    .await
                    .map_err(|e| {
                        ProviderError::Config(format!(
                            "decrypt channel key {}: {e}",
                            record.id
                        ))
                    })?;
                Ok(String::from_utf8(plaintext.to_vec()).map_err(|e| {
                    ProviderError::Config(format!("channel key is not valid UTF-8: {e}"))
                })?)
            }
            Err(gate_storage::DbError::NotFound) => {
                // DB 里没有 key，走 env
                tracing::debug!(
                    channel_id = %channel_id,
                    channel_code = channel_code,
                    "no channel key in DB, falling back to env"
                );
                Ok(resolve_api_key_for_channel(channel_code))
            }
            Err(e) => {
                // DB 查询出错，warn + fallback env
                tracing::warn!(
                    channel_id = %channel_id,
                    error = %e,
                    "channel key lookup failed, falling back to env"
                );
                Ok(resolve_api_key_for_channel(channel_code))
            }
        }
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

    // ====================================================================
    // G1: resolve_key_for_channel tests
    // ====================================================================

    use chrono::Utc;
    use gate_core::id::{ChannelGroupId, ChannelKeyId};
    use gate_storage::{
        ChannelKeyRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelKeyRepo,
        InMemoryChannelRepo,
    };
    use uuid::Uuid;

    fn make_channel(code: &str) -> (ChannelId, ChannelRecord) {
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

    /// 构造一个带 DB key 的 router 测试设施。
    async fn build_router_with_key(
        secret: &str,
    ) -> (ProviderRouter, ChannelId, ProjectId) {
        use gate_crypto::kms::{EnvKms, generate_master_key_b64};

        let (ch_id, ch_rec) = make_channel("test-ch");
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());

        // channel repo
        let ch_repo = Arc::new(InMemoryChannelRepo::new());
        ch_repo.seed_channel(ch_rec);
        ch_repo.seed_binding(group_id, ch_id, 1, 100);

        // group repo
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

        // crypto
        let kms = EnvKms::from_b64(&generate_master_key_b64(), "test").unwrap();
        let sealer = Arc::new(gate_crypto::EnvelopeKms::new(kms));

        // encrypt the key
        let aad = gate_crypto::aad::channel_key(*ch_id.as_uuid());
        let key_enc = sealer.seal(secret.as_bytes(), &aad).await.unwrap();

        // channel key repo
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

        // 路由命中后 provider 是用 DB key 构建的
        // 我们无法直接检查 provider 内部 key，但可以验证路由成功返回
        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some());
        let routed = result.unwrap();
        assert_eq!(routed.resolved_model, "gpt-4o");
    }

    #[tokio::test]
    async fn router_fallback_env_when_no_db_key() {
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let (ch_id, ch_rec) = make_channel("env-test-ch");

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

        // 空的 key repo — DB 里没有 key
        let ck_repo = Arc::new(InMemoryChannelKeyRepo::new());

        let router = ProviderRouter::new(ch_repo, grp_repo)
            .with_channel_key_repo(ck_repo);
        // 不挂 crypto（DB 没 key 就不需要）

        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should fallback to env var and still route");
    }

    #[tokio::test]
    async fn router_fallback_env_when_no_repo_configured() {
        let project_id = ProjectId::from(Uuid::now_v7());
        let group_id = ChannelGroupId::from(Uuid::now_v7());
        let (ch_id, ch_rec) = make_channel("no-repo-ch");

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

        // 不挂 channel_key_repo — 完全走 env
        let router = ProviderRouter::new(ch_repo, grp_repo);

        let result = router.route(project_id, "gpt-4o").await.unwrap();
        assert!(result.is_some(), "should use env var when no key repo");
    }

    #[tokio::test]
    async fn router_db_key_decrypt_roundtrip() {
        // 验证：加密 → 存 DB → router 解密 → 构建 Provider 时用的就是原始 key
        // 我们通过 resolve_key_for_channel 直接测试
        let secret = "sk-real-api-key-12345";
        let (router, ch_id, _project_id) = build_router_with_key(secret).await;

        let resolved = router
            .resolve_key_for_channel(ch_id, "test-ch")
            .await
            .unwrap();
        assert_eq!(resolved, secret);
    }
}
