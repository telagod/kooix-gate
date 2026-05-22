//! router unit tests — 从 mod.rs 拆出（M1.3 T3.1 收尾）。
#![cfg(test)]

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

fn setup_fixtures(channels_spec: &[(&str, &str, Vec<String>, i32)]) -> (ProjectId, ProviderRouter) {
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
use gate_storage::{ChannelKeyRecord, ChannelKeyRepo, DbResult, InMemoryChannelKeyRepo};
use std::sync::atomic::AtomicUsize;

struct CountingChannelKeyRepo {
    inner: Arc<InMemoryChannelKeyRepo>,
    list_calls: AtomicUsize,
}

impl CountingChannelKeyRepo {
    fn new(inner: Arc<InMemoryChannelKeyRepo>) -> Self {
        Self {
            inner,
            list_calls: AtomicUsize::new(0),
        }
    }

    fn list_calls(&self) -> usize {
        self.list_calls.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl ChannelKeyRepo for CountingChannelKeyRepo {
    async fn find_active_for_channel(&self, channel_id: ChannelId) -> DbResult<ChannelKeyRecord> {
        self.inner.find_active_for_channel(channel_id).await
    }

    async fn list_by_channel(&self, channel_id: ChannelId) -> DbResult<Vec<ChannelKeyRecord>> {
        self.list_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.list_by_channel(channel_id).await
    }

    async fn create(
        &self,
        channel_id: ChannelId,
        key_enc: &[u8],
        key_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        self.inner
            .create(channel_id, key_enc, key_fingerprint, label)
            .await
    }

    async fn rotate(
        &self,
        channel_id: ChannelId,
        new_key_enc: &[u8],
        new_fingerprint: &str,
        label: Option<&str>,
    ) -> DbResult<ChannelKeyId> {
        self.inner
            .rotate(channel_id, new_key_enc, new_fingerprint, label)
            .await
    }

    async fn revoke(&self, key_id: ChannelKeyId) -> DbResult<()> {
        self.inner.revoke(key_id).await
    }

    async fn report_success(&self, key_id: ChannelKeyId) -> DbResult<()> {
        self.inner.report_success(key_id).await
    }

    async fn report_failure(
        &self,
        key_id: ChannelKeyId,
        error_code: Option<i32>,
        cooldown_secs: i64,
        circuit_breaker_failures: u32,
    ) -> DbResult<()> {
        self.inner
            .report_failure(key_id, error_code, cooldown_secs, circuit_breaker_failures)
            .await
    }
}

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

fn make_plugin_channel(
    code: &str,
    base_url: String,
    model_mapping: serde_json::Value,
) -> (ChannelId, ChannelRecord) {
    let id = ChannelId::from(Uuid::now_v7());
    let now = Utc::now();
    let rec = ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: code.to_string(),
        provider_type: "plugin".to_string(),
        base_url,
        supported_models: vec!["embed-model".to_string()],
        status: "active".to_string(),
        health: "healthy".to_string(),
        timeout_ms: 60000,
        max_retries: 0,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping,
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
async fn router_channel_key_cache_avoids_repeated_repo_loads() {
    use gate_crypto::kms::{EnvKms, generate_master_key_b64};

    let (ch_id, ch_rec) = make_channel_simple("cache-ch");
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
    let key_enc = sealer.seal(b"sk-cache-secret", &aad).await.unwrap();

    let inner = Arc::new(InMemoryChannelKeyRepo::new());
    inner.seed(ChannelKeyRecord {
        id: ChannelKeyId::from(Uuid::now_v7()),
        channel_id: ch_id,
        label: Some("primary".to_string()),
        key_enc,
        key_fingerprint: "fp-cache-secret".to_string(),
        weight: 1,
        health: "healthy".to_string(),
        consecutive_errors: 0,
        total_requests: 0,
        total_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    let counting_repo = Arc::new(CountingChannelKeyRepo::new(inner));

    let router = ProviderRouter::new(ch_repo, grp_repo)
        .with_channel_key_repo(counting_repo.clone())
        .with_crypto(sealer)
        .with_channel_key_cache_ttl(Duration::from_secs(60));

    for _ in 0..3 {
        let (resolved, key_id) = router
            .resolve_key_for_channel(ch_id, "cache-ch")
            .await
            .unwrap();
        assert_eq!(resolved, "sk-cache-secret");
        assert!(key_id.is_some());
    }

    assert_eq!(
        counting_repo.list_calls(),
        1,
        "subsequent resolves must hit the decrypted secret cache"
    );
}

#[tokio::test]
async fn router_channel_key_cache_ttl_zero_disables_cache() {
    use gate_crypto::kms::{EnvKms, generate_master_key_b64};

    let (ch_id, ch_rec) = make_channel_simple("no-cache-ch");
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
    let key_enc = sealer.seal(b"sk-no-cache-secret", &aad).await.unwrap();

    let inner = Arc::new(InMemoryChannelKeyRepo::new());
    inner.seed(ChannelKeyRecord {
        id: ChannelKeyId::from(Uuid::now_v7()),
        channel_id: ch_id,
        label: Some("primary".to_string()),
        key_enc,
        key_fingerprint: "fp-no-cache-secret".to_string(),
        weight: 1,
        health: "healthy".to_string(),
        consecutive_errors: 0,
        total_requests: 0,
        total_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    let counting_repo = Arc::new(CountingChannelKeyRepo::new(inner));

    let router = ProviderRouter::new(ch_repo, grp_repo)
        .with_channel_key_repo(counting_repo.clone())
        .with_crypto(sealer)
        .with_channel_key_cache_ttl(Duration::ZERO);

    for _ in 0..3 {
        let (resolved, key_id) = router
            .resolve_key_for_channel(ch_id, "no-cache-ch")
            .await
            .unwrap();
        assert_eq!(resolved, "sk-no-cache-secret");
        assert!(key_id.is_some());
    }

    assert_eq!(
        counting_repo.list_calls(),
        3,
        "TTL=0 must bypass the decrypted secret cache"
    );
}

#[tokio::test]
async fn router_channel_key_cache_invalidation_reloads_rotated_secret() {
    use gate_crypto::kms::{EnvKms, generate_master_key_b64};

    ensure_test_api_key();
    let (ch_id, ch_rec) = make_channel_simple("rotate-cache-ch");
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
    let old_key_enc = sealer.seal(b"sk-old-cache-secret", &aad).await.unwrap();
    let new_key_enc = sealer.seal(b"sk-new-cache-secret", &aad).await.unwrap();

    let inner = Arc::new(InMemoryChannelKeyRepo::new());
    inner.seed(ChannelKeyRecord {
        id: ChannelKeyId::from(Uuid::now_v7()),
        channel_id: ch_id,
        label: Some("primary".to_string()),
        key_enc: old_key_enc,
        key_fingerprint: "fp-old-cache-secret".to_string(),
        weight: 1,
        health: "healthy".to_string(),
        consecutive_errors: 0,
        total_requests: 0,
        total_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    let counting_repo = Arc::new(CountingChannelKeyRepo::new(inner.clone()));

    let router = ProviderRouter::new(ch_repo, grp_repo)
        .with_channel_key_repo(counting_repo.clone())
        .with_crypto(sealer)
        .with_channel_key_cache_ttl(Duration::from_secs(60));

    let (old_secret, old_key_id) = router
        .resolve_key_for_channel(ch_id, "rotate-cache-ch")
        .await
        .unwrap();
    assert_eq!(old_secret, "sk-old-cache-secret");
    let old_key_id = old_key_id.expect("old key id should come from DB");

    inner.revoke(old_key_id).await.unwrap();

    let (still_cached, _) = router
        .resolve_key_for_channel(ch_id, "rotate-cache-ch")
        .await
        .unwrap();
    assert_eq!(
        still_cached, "sk-old-cache-secret",
        "cache should hold a revoked value until explicit invalidation or TTL expiry"
    );

    router.invalidate_channel_key_cache(ch_id);
    let (fallback_secret, fallback_key_id) = router
        .resolve_key_for_channel(ch_id, "rotate-cache-ch")
        .await
        .unwrap();
    assert_eq!(fallback_secret, "test-key-for-unit-tests");
    assert!(
        fallback_key_id.is_none(),
        "revoked DB key should force env fallback after invalidation"
    );

    inner
        .rotate(ch_id, &new_key_enc, "fp-new-cache-secret", Some("primary"))
        .await
        .unwrap();
    router.invalidate_channel_key_cache(ch_id);
    let (rotated_secret, rotated_key_id) = router
        .resolve_key_for_channel(ch_id, "rotate-cache-ch")
        .await
        .unwrap();
    assert_eq!(rotated_secret, "sk-new-cache-secret");
    assert_ne!(rotated_key_id, Some(old_key_id));
    assert_eq!(
        counting_repo.list_calls(),
        3,
        "revoke and rotation invalidation should force fresh repo loads"
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

#[tokio::test]
async fn route_embedding_uses_plugin_runtime_and_secret_slots() {
    use gate_crypto::kms::{EnvKms, generate_master_key_b64};

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/embeddings"))
        .and(wiremock::matchers::header("x-embed-key", "db-embed-value"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{ "object": "embedding", "index": 0, "embedding": [0.1, 0.2] }],
                "model": "embed-model",
                "usage": { "prompt_tokens": 2, "total_tokens": 2 }
            })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let project_id = ProjectId::from(Uuid::now_v7());
    let group_id = ChannelGroupId::from(Uuid::now_v7());
    let (ch_id, ch_rec) = make_plugin_channel(
        "embed-plugin",
        server.uri(),
        serde_json::json!({
            "plugin": {
                "version": 1,
                "preset": { "provider": "openai_compatible" },
                "auth": {
                    "strategy": "api_key_header",
                    "header_name": "X-Embed-Key",
                    "secret_slot": "embed-key"
                }
            }
        }),
    );

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    ch_repo.seed_channel(ch_rec);
    ch_repo.seed_binding(group_id, ch_id, 1, 100);

    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    grp_repo.seed_group(ChannelGroupRecord {
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
    let key_enc = sealer.seal(b"db-embed-value", &aad).await.unwrap();
    let ck_repo = Arc::new(InMemoryChannelKeyRepo::new());
    ck_repo.seed(ChannelKeyRecord {
        id: ChannelKeyId::from(Uuid::now_v7()),
        channel_id: ch_id,
        label: Some("embed-key".to_string()),
        key_enc,
        key_fingerprint: "fp-embed-value".to_string(),
        weight: 1,
        health: "healthy".to_string(),
        consecutive_errors: 0,
        total_requests: 0,
        total_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    let router = ProviderRouter::new(ch_repo, grp_repo)
        .with_channel_key_repo(ck_repo)
        .with_crypto(sealer);

    let routed = router
        .route_embedding(project_id, "embed-model")
        .await
        .unwrap()
        .expect("plugin embedding channel should route");
    assert_eq!(routed.channel_id, ch_id);
    assert_eq!(routed.provider_type, "plugin");
    let response = routed
        .provider
        .embed(crate::types::EmbeddingRequest {
            model: routed.resolved_model.clone(),
            input: crate::types::EmbeddingInput::Single("hello".to_string()),
            encoding_format: None,
            dimensions: None,
        })
        .await
        .unwrap();
    assert_eq!(response.data[0].embedding, vec![0.1, 0.2]);
    assert_eq!(response.usage.total_tokens, 2);
}

#[tokio::test]
async fn route_embedding_skips_plugin_without_active_secret() {
    let project_id = ProjectId::from(Uuid::now_v7());
    let group_id = ChannelGroupId::from(Uuid::now_v7());
    let (plugin_id, plugin) = make_plugin_channel(
        "no-key-plugin",
        "https://api.example.com/v1".to_string(),
        serde_json::json!({
            "plugin": {
                "version": 1,
                "preset": { "provider": "openai_compatible" },
                "auth": {
                    "strategy": "api_key_header",
                    "header_name": "X-Embed-Key",
                    "secret_slot": "embed-key"
                }
            }
        }),
    );
    let mut fallback = make_channel_with_models("fallback-openai", "openai", vec![]);
    let fallback_id = fallback.channel_id;
    fallback.supported_models = vec!["embed-model".to_string()];

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    ch_repo.seed_channel(plugin);
    ch_repo.seed_channel(fallback);
    ch_repo.seed_binding(group_id, plugin_id, 1, 100);
    ch_repo.seed_binding(group_id, fallback_id, 2, 100);

    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    grp_repo.seed_group(ChannelGroupRecord {
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

    let router = ProviderRouter::new(ch_repo, grp_repo)
        .with_channel_key_repo(Arc::new(InMemoryChannelKeyRepo::new()));
    let routed = router
        .route_embedding(project_id, "embed-model")
        .await
        .unwrap()
        .expect("fallback compile-time embedding provider should route");
    assert_eq!(routed.channel_id, fallback_id);
    assert_eq!(routed.provider_type, "openai");
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

/// Helper: setup with custom strategy and per-channel canary bps.
fn setup_strategy_canary_fixtures(
    strategy: &str,
    channels_spec: &[(&str, i32, i32, Option<i32>)],
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
    for (code, priority, weight, canary_percent_bps) in channels_spec {
        let ch = make_channel_with_models(code, "openai", vec![]);
        let ch_id = ch.channel_id;
        channel_ids.push(ch_id);
        channel_repo.seed_channel(ch);
        channel_repo.seed_binding_with_canary(
            group_id,
            ch_id,
            *priority,
            *weight,
            *canary_percent_bps,
        );
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
    let (pid, router, ch_ids) = setup_strategy_fixtures("weighted_random", &[("ch-only", 1, 5)]);

    for _ in 0..100 {
        let routed = router.route(pid, "any").await.unwrap().unwrap();
        assert_eq!(routed.channel_id, ch_ids[0]);
    }
}

#[tokio::test]
async fn canary_binding_receives_configured_percentage() {
    let (pid, router, ch_ids) = setup_strategy_canary_fixtures(
        "priority",
        &[("ch-canary", 1, 1, Some(500)), ("ch-baseline", 2, 1, None)],
    );

    let iterations = 2_000;
    let mut canary_hits = 0u32;
    for _ in 0..iterations {
        let routed = router.route(pid, "any").await.unwrap().unwrap();
        if routed.channel_id == ch_ids[0] {
            canary_hits += 1;
        }
    }

    let ratio = canary_hits as f64 / iterations as f64;
    assert!(
        (0.045..=0.055).contains(&ratio),
        "expected deterministic ~5% canary traffic, got {ratio:.4} ({canary_hits}/{iterations})"
    );
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
async fn route_skips_draining_channel() {
    let (pid, _router, ch_ids) =
        setup_strategy_fixtures("priority", &[("ch-draining", 1, 1), ("ch-active", 2, 1)]);
    let group_id = ChannelGroupId::from(Uuid::now_v7());
    let channel_repo = Arc::new(InMemoryChannelRepo::new());
    let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = Utc::now();
    group_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "drain-skip".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    group_repo.seed_default(pid, group_id);

    let mut draining = make_channel_with_models("ch-draining", "openai", vec![]);
    draining.channel_id = ch_ids[0];
    draining.status = "draining".to_string();
    channel_repo.seed_channel(draining);
    channel_repo.seed_binding(group_id, ch_ids[0], 1, 1);

    let mut active = make_channel_with_models("ch-active", "openai", vec![]);
    active.channel_id = ch_ids[1];
    channel_repo.seed_channel(active);
    channel_repo.seed_binding(group_id, ch_ids[1], 2, 1);

    let router = ProviderRouter::new(channel_repo, group_repo);
    let routed = router.route(pid, "any").await.unwrap().unwrap();
    assert_eq!(
        routed.channel_id, ch_ids[1],
        "draining channel must not receive new routes"
    );
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
