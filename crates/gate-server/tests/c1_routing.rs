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
use gate_providers::ProviderRouter;
use gate_server::loader::InMemoryLoader;
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelKeyRepo,
    InMemoryChannelRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::matchers::{body_partial_json, method, path};
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
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
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
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
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
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: ch_repo,
        channel_groups: grp_repo,
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
