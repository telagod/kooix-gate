//! C1 路由集成测试：api_key → project → channel_group → channel → upstream (wiremock)
//!
//! 验证：
//! 1. ProviderRouter 按 priority 选中高优先级 channel
//! 2. 完整 handler 链：api_key → project → group → channel → upstream 返回 200

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::{ApiKeyId, ChannelGroupId, ChannelId, OrgId, ProjectId};
use gate_crypto::{EnvelopeKms, aad};
use gate_providers::ProviderRouter;
use gate_server::health_check::HealthChecker;
use gate_server::loader::InMemoryLoader;
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ChannelGroupRecord, ChannelKeyRepo, ChannelRecord, InMemoryChannelGroupRepo,
    InMemoryChannelKeyRepo, InMemoryChannelRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use wiremock::matchers::{body_json, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 构造测试用 ChannelRecord（healthy + active）。
fn make_channel(id: ChannelId, code: &str, base_url: &str) -> ChannelRecord {
    ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: format!("channel-{code}"),
        provider_type: "openai".to_string(),
        base_url: base_url.to_string(),
        supported_models: vec![],
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
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// 构造测试用 Plugin ChannelRecord。
fn make_plugin_channel(
    id: ChannelId,
    code: &str,
    base_url: &str,
    manifest: serde_json::Value,
) -> ChannelRecord {
    let mut ch = make_channel(id, code, base_url);
    ch.provider_type = "plugin".to_string();
    ch.model_mapping = manifest;
    ch
}

fn test_jwt() -> JwtIssuer {
    JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap()
}

fn repos_with_channels(
    ch_repo: Arc<InMemoryChannelRepo>,
    grp_repo: Arc<InMemoryChannelGroupRepo>,
    key_repo: Arc<InMemoryChannelKeyRepo>,
) -> Repos {
    Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects: Arc::new(gate_storage::InMemoryProjectRepo::new()),
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: key_repo,
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        sessions: Arc::new(gate_storage::InMemoryUserSessionRepo::new()),
        pg_pool: None,
    }
}

fn test_kms() -> Arc<EnvelopeKms> {
    Arc::new(EnvelopeKms::new(
        gate_crypto::EnvKms::from_b64(&gate_crypto::kms::generate_master_key_b64(), "test")
            .unwrap(),
    ))
}

async fn start_pg() -> (
    testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
    sqlx::PgPool,
) {
    use testcontainers::ImageExt;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.expect("connect");
    gate_storage::run_migrations(&pool).await.expect("migrate");
    (container, pool)
}

async fn seed_pg_plugin_channel(
    pool: &sqlx::PgPool,
    channel_id: ChannelId,
    code: &str,
    base_url: &str,
    manifest: serde_json::Value,
    status: &str,
    health: &str,
) {
    sqlx::query(
        "INSERT INTO channels \
         (id, code, name, provider_type, base_url, config_enc, supported_models, status, health, model_mapping) \
         VALUES ($1, $2, $3, 'plugin', $4, '\\x'::bytea, '{}'::text[], $5, $6, $7)",
    )
    .bind(channel_id.as_uuid())
    .bind(code)
    .bind(format!("channel-{code}"))
    .bind(base_url)
    .bind(status)
    .bind(health)
    .bind(manifest)
    .execute(pool)
    .await
    .unwrap();
}

/// ProviderRouter 按 priority 选中优先级最高（数字最小）的 channel。
#[tokio::test]
async fn provider_router_selects_highest_priority() {
    // SAFETY: test is single-threaded at this point
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let group_id = ChannelGroupId::new();
    let ch_high = ChannelId::new(); // priority=10
    let ch_low = ChannelId::new(); // priority=20

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());

    // 高优先级 channel（priority=10）
    ch_repo.seed_channel(make_channel(
        ch_high,
        "high-prio",
        "https://placeholder.example.com/v1",
    ));
    // 低优先级 channel（priority=20）
    ch_repo.seed_channel(make_channel(
        ch_low,
        "low-prio",
        "https://placeholder2.example.com/v1",
    ));

    // 先 seed 高优先级，再 seed 低优先级
    ch_repo.seed_binding(group_id, ch_high, 10, 1);
    ch_repo.seed_binding(group_id, ch_low, 20, 1);

    // project 绑定该 group
    let project_id = ProjectId::new();
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "test-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let router = ProviderRouter::new(ch_repo, grp_repo);
    let routed = router.route(project_id, "gpt-4o-mini").await.unwrap();
    // 应该能找到（高优先级 channel）
    assert!(routed.is_some(), "should get a provider");
    let routed = routed.unwrap();
    // Provider name 应该是 openai
    assert_eq!(routed.provider.name(), "openai");
    // 验证 channel_id 是高优先级那条
    assert_eq!(routed.channel_id, ch_high);
}

/// 完整链路：api_key → project → group → channel → wiremock upstream
#[tokio::test]
async fn full_chain_api_key_to_upstream() {
    // SAFETY: test is single-threaded at this point
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-c1",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "routed!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        })))
        .mount(&upstream)
        .await;

    // Setup IDs
    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let ch_id = ChannelId::new();

    // Channel repo + group repo
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());

    // channel 指向 wiremock
    ch_repo.seed_channel(make_channel(
        ch_id,
        "wm-channel",
        &format!("{}/v1", upstream.uri()),
    ));
    ch_repo.seed_binding(group_id, ch_id, 10, 1);

    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "wm-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());

    // JWT
    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    // API key（plaintext）
    let plaintext = "sk-kg-test-c1-routing-key-00000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects: Arc::new(gate_storage::InMemoryProjectRepo::new()),
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        sessions: Arc::new(gate_storage::InMemoryUserSessionRepo::new()),
        pg_pool: None,
    };

    let state = AppState::new(jwt, loader, repos).with_provider_router(provider_router);
    // 不挂 fallback provider，验证 router 独立工作
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "route me!"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "expected 200 from routed upstream"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "routed!");
}

/// 完整链路：model alias + channel model_mapping 应改写实际上游请求 model。
#[tokio::test]
async fn full_chain_rewrites_model_from_alias_and_channel_mapping() {
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "model": "native-mini"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-mapped",
            "model": "native-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "mapped!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        })))
        .mount(&upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let ch_id = ChannelId::new();

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let mut channel = make_channel(ch_id, "mapped-channel", &format!("{}/v1", upstream.uri()));
    channel.supported_models = vec!["gpt-4o-mini".to_string()];
    channel.model_mapping = json!({
        "gpt-4o-mini": "native-mini"
    });
    ch_repo.seed_channel(channel);
    ch_repo.seed_binding(group_id, ch_id, 10, 1);

    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "mapped-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let alias_repo = Arc::new(gate_storage::InMemoryModelAliasRepo::new());
    alias_repo.seed(gate_storage::ModelAliasRecord {
        id: uuid::Uuid::now_v7(),
        project_id: *project_id.as_uuid(),
        alias: "fast".to_string(),
        target_model: "gpt-4o-mini".to_string(),
        group_id: None,
        params_override: json!({}),
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });

    let provider_router =
        ProviderRouter::new(ch_repo.clone(), grp_repo.clone()).with_model_alias_repo(alias_repo);
    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let plaintext = "sk-kg-test-mapped-routing-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects: Arc::new(gate_storage::InMemoryProjectRepo::new()),
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        sessions: Arc::new(gate_storage::InMemoryUserSessionRepo::new()),
        pg_pool: None,
    };

    let state = AppState::new(jwt, loader, repos).with_provider_router(provider_router);
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "fast",
                "messages": [{"role": "user", "content": "rewrite me!"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "expected upstream mock to match rewritten model"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "mapped!");
}

#[tokio::test]
async fn plugin_manifest_channel_model_mapping_rewrites_deployment_path() {
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/deployments/native-mini/chat"))
        .and(body_partial_json(json!({
            "model": "native-mini",
            "metadata": { "tenant": "acme" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-plugin-mapped",
            "model": "native-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "plugin mapped!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        })))
        .mount(&upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let ch_id = ChannelId::new();

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let mut channel = make_plugin_channel(
        ch_id,
        "plugin-mapped-channel",
        &format!("{}/v1", upstream.uri()),
        json!({
            "plugin": {
                "version": 1,
                "auth": { "strategy": "none" },
                "request": {
                    "path": "/deployments/{{model}}/chat",
                    "body": {
                        "model": "{{model}}",
                        "prompt": "{{last_user_message}}",
                        "metadata": "{{metadata}}"
                    }
                }
            },
            "models": {
                "gpt-4o-mini": "native-mini"
            }
        }),
    );
    channel.supported_models = vec!["gpt-4o-mini".to_string()];
    ch_repo.seed_channel(channel);
    ch_repo.seed_binding(group_id, ch_id, 10, 1);

    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "plugin-mapped-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let alias_repo = Arc::new(gate_storage::InMemoryModelAliasRepo::new());
    alias_repo.seed(gate_storage::ModelAliasRecord {
        id: uuid::Uuid::now_v7(),
        project_id: *project_id.as_uuid(),
        alias: "fast".to_string(),
        target_model: "gpt-4o-mini".to_string(),
        group_id: None,
        params_override: json!({
            "metadata": { "tenant": "acme" }
        }),
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });

    let provider_router =
        ProviderRouter::new(ch_repo.clone(), grp_repo.clone()).with_model_alias_repo(alias_repo);
    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let plaintext = "sk-kg-test-plugin-mapped-routing-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );
    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects: Arc::new(gate_storage::InMemoryProjectRepo::new()),
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        sessions: Arc::new(gate_storage::InMemoryUserSessionRepo::new()),
        pg_pool: None,
    };
    let state = AppState::new(jwt, loader, repos).with_provider_router(provider_router);
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "fast",
                "messages": [{"role": "user", "content": "rewrite me!"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "expected plugin upstream mock to match rewritten deployment path"
    );
}

/// 完整链路：plugin channel 私有 SSE → /v1/chat/completions 归一化 SSE。
#[tokio::test]
async fn full_chain_plugin_channel_normalizes_private_sse() {
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"payload\":{\"rid\":\"plug-1\",\"model_name\":\"native\",\"speaker\":\"assistant\"}}\n\n",
        "data: {\"payload\":{\"token\":\"邪\"}}\n\n",
        "data: {\"payload\":{\"token\":\"修\"}}\n\n",
        "data: {\"payload\":{\"finish\":\"done\",\"usage\":{\"input\":3,\"output\":2}}}\n\n",
        "data: EOF\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let ch_id = ChannelId::new();

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    ch_repo.seed_channel(make_plugin_channel(
        ch_id,
        "plugin-wm",
        &upstream.uri(),
        json!({
            "plugin": {
                "request": { "chat_path": "/private/chat" },
                "stream": {
                    "openai_compatible": false,
                    "event_path": "payload",
                    "id_path": "rid",
                    "model_path": "model_name",
                    "role_path": "speaker",
                    "content_path": "token",
                    "finish_reason_path": "finish",
                    "done": ["EOF"],
                    "usage": {
                        "prompt_tokens_path": "usage.input",
                        "completion_tokens_path": "usage.output"
                    }
                }
            }
        }),
    ));
    ch_repo.seed_binding(group_id, ch_id, 10, 1);

    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "plugin-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone());
    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let plaintext = "sk-kg-test-plugin-routing-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects: Arc::new(gate_storage::InMemoryProjectRepo::new()),
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(InMemoryChannelKeyRepo::new()),
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: Arc::new(gate_storage::InMemoryAuditRepo::new()),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        sessions: Arc::new(gate_storage::InMemoryUserSessionRepo::new()),
        pg_pool: None,
    };

    let state = AppState::new(jwt, loader, repos).with_provider_router(provider_router);
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "odd-model",
                "stream": true,
                "messages": [{"role": "user", "content": "route plugin!"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("data: {"), "body={body}");
    assert!(body.contains("邪"), "body={body}");
    assert!(body.contains("修"), "body={body}");
    assert!(body.contains("\"total_tokens\":5"), "body={body}");
}

/// Plugin upstream error → normalized API error；连续失败进入 key cooldown，后续路由 fallback。
#[tokio::test]
async fn plugin_error_updates_key_health_and_falls_back_to_next_channel() {
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let bad_upstream = MockServer::start().await;
    let fallback_upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("retry-after", "1")
                .set_body_json(json!({
                    "vendor_error": {
                        "status": 429,
                        "code": "quota_busy",
                        "message": "slow down"
                    }
                })),
        )
        .mount(&bad_upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "fallback-ok",
            "model": "odd-model",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "fallback", "tool_calls": null },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&fallback_upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let bad_ch = ChannelId::new();
    let fallback_ch = ChannelId::new();

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let key_repo = Arc::new(InMemoryChannelKeyRepo::new());

    ch_repo.seed_channel(make_plugin_channel(
        bad_ch,
        "bad-plugin",
        &bad_upstream.uri(),
        json!({
            "plugin": {
                "request": {
                    "chat_path": "/private/chat",
                    "retry": { "max_retries": 0, "retryable_codes": ["quota_busy"], "cooldown_ms": 1000 }
                },
                "error": {
                    "status_path": "vendor_error.status",
                    "code_path": "vendor_error.code",
                    "message_path": "vendor_error.message",
                    "rate_limit_status": [429],
                    "retryable_codes": ["quota_busy"],
                    "cooldown_ms": 1000,
                    "circuit_breaker_failures": 2
                }
            }
        }),
    ));
    ch_repo.seed_channel(make_plugin_channel(
        fallback_ch,
        "fallback-plugin",
        &fallback_upstream.uri(),
        json!({
            "plugin": {
                "preset": { "provider": "openai_compatible" },
                "request": { "chat_path": "/private/chat" }
            }
        }),
    ));
    ch_repo.seed_binding(group_id, bad_ch, 1, 1);
    ch_repo.seed_binding(group_id, fallback_ch, 2, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "plugin-error-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let kms = test_kms();
    let bad_secret = kms
        .seal(b"bad-key", &aad::channel_key(*bad_ch.as_uuid()))
        .await
        .unwrap();
    let fallback_secret = kms
        .seal(b"fallback-key", &aad::channel_key(*fallback_ch.as_uuid()))
        .await
        .unwrap();
    key_repo
        .create(bad_ch, &bad_secret, "bad-fp", None)
        .await
        .unwrap();
    key_repo
        .create(fallback_ch, &fallback_secret, "fallback-fp", None)
        .await
        .unwrap();

    let provider_router = ProviderRouter::new(ch_repo.clone(), grp_repo.clone())
        .with_channel_key_repo(key_repo.clone())
        .with_crypto(kms.clone());
    let jwt = test_jwt();
    let plaintext = "sk-kg-test-plugin-error-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );
    let state = AppState::new(
        jwt,
        loader,
        repos_with_channels(ch_repo.clone(), grp_repo.clone(), key_repo.clone()),
    )
    .with_provider_router(provider_router)
    .with_crypto_arc(kms.clone());
    let router = build_router(state);

    for attempt in 1..=2 {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("authorization", format!("Bearer {plaintext}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "odd-model",
                    "messages": [{"role": "user", "content": "route plugin error!"}]
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["error"]["code"], "rate_limit_error",
            "attempt={attempt}"
        );
    }

    let keys = key_repo.list_by_channel(bad_ch).await.unwrap();
    assert_eq!(keys[0].health, "cooling_down");
    assert_eq!(keys[0].consecutive_errors, 2);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "odd-model",
                "messages": [{"role": "user", "content": "fallback now"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["id"], "fallback-ok");
    assert_eq!(body["choices"][0]["message"]["content"], "fallback");
}

#[tokio::test]
async fn health_checker_runs_manifest_plugin_probe_and_recovers_channel() {
    let (_pg, pool) = start_pg().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/healthz/tiny-health"))
        .and(body_json(json!({ "model": "tiny-health" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": ["tiny-health", "tiny-health"]
        })))
        .mount(&upstream)
        .await;

    let channel_id = ChannelId::new();
    seed_pg_plugin_channel(
        &pool,
        channel_id,
        "probe-plugin",
        &upstream.uri(),
        json!({
            "plugin": {
                "auth": { "strategy": "none" },
                "probe": {
                    "model": "tiny-health",
                    "path": "/healthz/{{model}}",
                    "body": { "model": "{{model}}" },
                    "success_status": [200],
                    "max_cost_micros": 0
                }
            }
        }),
        "disabled",
        "unhealthy",
    )
    .await;

    let state = AppState::new(
        test_jwt(),
        Arc::new(InMemoryLoader::new()),
        Repos::from_pg(pool.clone()),
    );
    let checker = HealthChecker::new(&state, std::time::Duration::from_millis(10)).unwrap();
    let shutdown = CancellationToken::new();
    let handle = checker.spawn_with_shutdown(shutdown.clone());
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    shutdown.cancel();
    handle.await.unwrap();

    let ch = state.repos.channels.find_by_id(channel_id).await.unwrap();
    assert_eq!(ch.status, "active");
    assert_eq!(ch.health, "healthy");
    assert_eq!(ch.supported_models, vec!["tiny-health".to_string()]);
}

#[tokio::test]
async fn health_checker_manifest_probe_failure_auto_disables_channel() {
    let (_pg, pool) = start_pg().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/healthz/tiny-health"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": "not ready"
        })))
        .mount(&upstream)
        .await;

    let channel_id = ChannelId::new();
    seed_pg_plugin_channel(
        &pool,
        channel_id,
        "probe-plugin-down",
        &upstream.uri(),
        json!({
            "plugin": {
                "auth": { "strategy": "none" },
                "probe": {
                    "model": "tiny-health",
                    "path": "/healthz/{{model}}",
                    "body": { "model": "{{model}}" },
                    "success_status": [200],
                    "max_cost_micros": 0
                }
            }
        }),
        "active",
        "healthy",
    )
    .await;

    let state = AppState::new(
        test_jwt(),
        Arc::new(InMemoryLoader::new()),
        Repos::from_pg(pool.clone()),
    );
    let checker = HealthChecker::new(&state, std::time::Duration::from_millis(10)).unwrap();
    let shutdown = CancellationToken::new();
    let handle = checker.spawn_with_shutdown(shutdown.clone());
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    shutdown.cancel();
    handle.await.unwrap();

    let ch = state.repos.channels.find_by_id(channel_id).await.unwrap();
    assert_eq!(ch.status, "disabled");
    assert_eq!(ch.health, "unhealthy");
    assert!(
        ch.last_error
            .as_deref()
            .unwrap_or_default()
            .contains("plugin_probe_http: 503")
    );
}

#[tokio::test]
async fn health_checker_standard_probe_records_latency_for_least_latency() {
    let (_pg, pool) = start_pg().await;
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "gpt-4o-mini" }]
        })))
        .mount(&upstream)
        .await;

    let channel_id = ChannelId::new();
    sqlx::query(
        "INSERT INTO channels \
         (id, code, name, provider_type, base_url, config_enc, supported_models, status, health, model_mapping) \
         VALUES ($1, 'probe-openai', 'probe-openai', 'openai', $2, '\\x'::bytea, '{}'::text[], 'active', 'healthy', '{}'::jsonb)",
    )
    .bind(channel_id.as_uuid())
    .bind(upstream.uri())
    .execute(&pool)
    .await
    .unwrap();

    let repos = Repos::from_pg(pool.clone());
    let router = Arc::new(
        ProviderRouter::new(
            Arc::new(gate_storage::PgChannelRepo::new(pool.clone())),
            Arc::new(gate_storage::PgChannelGroupRepo::new(pool.clone())),
        )
        .with_channel_latency_repo(repos.channel_latency.clone()),
    );
    let state = AppState::new(test_jwt(), Arc::new(InMemoryLoader::new()), repos)
        .with_provider_router_arc(router.clone());

    let checker = HealthChecker::new(&state, std::time::Duration::from_millis(10)).unwrap();
    let shutdown = CancellationToken::new();
    let handle = checker.spawn_with_shutdown(shutdown.clone());
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    shutdown.cancel();
    handle.await.unwrap();

    let ch = state.repos.channels.find_by_id(channel_id).await.unwrap();
    assert_eq!(ch.supported_models, vec!["gpt-4o-mini".to_string()]);
    let metrics = router.channel_metrics().unwrap();
    assert_ne!(metrics.avg_latency(channel_id), u64::MAX);
    let latency = state
        .repos
        .channel_latency
        .avg_latency_ms(&[channel_id], 300)
        .await
        .unwrap();
    assert!(
        latency.contains_key(&channel_id),
        "health probe should persist latency sample"
    );
}

#[tokio::test]
async fn route_chat_skips_channel_missing_requested_capability() {
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-key");
    }
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-capability",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "fallback selected" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let plugin_id = ChannelId::new();
    let fallback_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let key_repo = Arc::new(InMemoryChannelKeyRepo::new());

    ch_repo.seed_channel(make_plugin_channel(
        plugin_id,
        "text-only-plugin",
        "https://placeholder.invalid",
        json!({
            "plugin": {
                "version": 1,
                "capabilities": { "chat": true, "streaming": true },
                "auth": { "strategy": "none" }
            }
        }),
    ));
    ch_repo.seed_channel(make_channel(
        fallback_id,
        "vision-openai",
        &format!("{}/v1", upstream.uri()),
    ));
    ch_repo.seed_binding(group_id, plugin_id, 1, 1);
    ch_repo.seed_binding(group_id, fallback_id, 2, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "capability-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let plaintext = "sk-kg-test-capability-routing-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let repos = repos_with_channels(ch_repo.clone(), grp_repo.clone(), key_repo);
    let state = AppState::new(test_jwt(), loader, repos)
        .with_provider_router(ProviderRouter::new(ch_repo, grp_repo));
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "describe" },
                        { "type": "image_url", "image_url": { "url": "data:image/png;base64,AA==" } }
                    ]
                }]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "fallback selected"
    );
}

#[tokio::test]
async fn route_chat_no_healthy_channel_returns_normalized_error() {
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    let api_key_id = ApiKeyId::new();
    let group_id = ChannelGroupId::new();
    let disabled_id = ChannelId::new();

    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let key_repo = Arc::new(InMemoryChannelKeyRepo::new());

    let mut disabled = make_channel(disabled_id, "dead-openai", "https://placeholder.invalid/v1");
    disabled.status = "disabled".to_string();
    disabled.health = "unhealthy".to_string();
    ch_repo.seed_channel(disabled);
    ch_repo.seed_binding(group_id, disabled_id, 1, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "dead-group".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    grp_repo.seed_default(project_id, group_id);

    let plaintext = "sk-kg-test-no-healthy-channel-key-000000";
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        plaintext,
        gate_server::loader::ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let repos = repos_with_channels(ch_repo.clone(), grp_repo.clone(), key_repo);
    let state = AppState::new(test_jwt(), loader, repos)
        .with_provider_router(ProviderRouter::new(ch_repo, grp_repo));
    let router = build_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "x"}]
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "no_healthy_channel");
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert_eq!(body["error"]["upstream_code"], "no_healthy_channel");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no healthy chat channel found")
    );
}
