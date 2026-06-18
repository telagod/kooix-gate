//! Lightweight perf / observability smoke.
//!
//! Keeps CI cheap: in-memory auth/repos + wiremock upstream, asserts hot routes stay
//! responsive and `/metrics` exposes the business SLO signals.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_billing::{InMemoryOutboxRepo, InMemoryPricingRepo, OutboxRepo, PricingRepo, UsageEvent};
use gate_core::id::{ApiKeyId, ChannelGroupId, ChannelId, OrgId, ProjectId, UserId};
use gate_core::identity::{PlatformRole, Project, ProjectStatus};
use gate_providers::ProviderRouter;
use gate_server::loader::{ApiKeyRecord, InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelKeyRepo,
    InMemoryChannelRepo, InMemoryProjectRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API_KEY: &str = "sk-kg-perf-smoke-key-000000000000";
const MAX_ROUTE_LATENCY: Duration = Duration::from_secs(2);

struct Harness {
    router: axum::Router,
    api_key: &'static str,
    admin_token: String,
}

fn make_channel(
    id: ChannelId,
    code: &str,
    provider_type: &str,
    base_url: &str,
    supported_models: Vec<String>,
    model_mapping: Value,
) -> ChannelRecord {
    let now = Utc::now();
    ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: code.to_string(),
        provider_type: provider_type.to_string(),
        base_url: base_url.to_string(),
        supported_models,
        status: "active".to_string(),
        health: "healthy".to_string(),
        timeout_ms: 60_000,
        max_retries: 1,
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
    }
}

async fn harness() -> Harness {
    gate_server::metrics::install_recorder();
    unsafe {
        std::env::set_var("KOOIX_API_KEY", "test-upstream-key");
    }

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-perf-smoke",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5 }
        })))
        .mount(&upstream)
        .await;

    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let admin_user = UserId::new();
    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let now = Utc::now();

    let channels = Arc::new(InMemoryChannelRepo::new());
    let groups = Arc::new(InMemoryChannelGroupRepo::new());
    channels.seed_channel(make_channel(
        channel_id,
        "perf-wm",
        "plugin",
        &format!("{}/v1", upstream.uri()),
        vec!["gpt-4o-mini".to_string()],
        serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
    ));
    channels.seed_binding(group_id, channel_id, 10, 1);
    groups.seed_group(ChannelGroupRecord {
        group_id,
        name: "perf".to_string(),
        description: String::new(),
        strategy: "priority".to_string(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
        use_health_score: false,
        health_weights: None,
    });
    groups.seed_default(project_id, group_id);

    let projects = Arc::new(InMemoryProjectRepo::new());
    projects.seed(Project {
        id: project_id,
        org_id,
        name: "perf".into(),
        slug: "perf".into(),
        status: ProjectStatus::Active,
        default_group_id: Some(group_id),
        created_at: now,
        updated_at: now,
    });

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        API_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );
    loader.add_user(
        admin_user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );

    let repos = Repos {
        users: Arc::new(gate_storage::InMemoryUserRepo::new()),
        orgs: Arc::new(gate_storage::InMemoryOrgRepo::new()),
        projects,
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: channels.clone(),
        channel_groups: groups.clone(),
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
    let pricing = Arc::new(InMemoryPricingRepo::new());
    pricing.seed_global("gpt-4o-mini", 0.15, 0.60);
    let outbox = Arc::new(InMemoryOutboxRepo::new());

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(ProviderRouter::new(channels, groups))
        .with_outbox(outbox as Arc<dyn OutboxRepo>)
        .with_pricing(pricing as Arc<dyn PricingRepo>);
    let jwt = state.jwt.clone();
    let router = build_router(state);
    let admin_token = jwt
        .issue_access(*admin_user.as_uuid(), Uuid::now_v7(), None, true)
        .unwrap()
        .0;

    Harness {
        router,
        api_key: API_KEY,
        admin_token,
    }
}

fn models_harness() -> Harness {
    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();
    let channel_id = ChannelId::new();
    let plugin_channel_id = ChannelId::new();
    let disabled_channel_id = ChannelId::new();
    let now = Utc::now();

    let channels = Arc::new(InMemoryChannelRepo::new());
    channels.seed_channel(make_channel(
        channel_id,
        "openai-cap",
        "plugin",
        "https://openai.example/v1",
        vec!["gpt-4o-mini".to_string(), "shared-model".to_string()],
        serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
    ));
    channels.seed_channel(make_channel(
        plugin_channel_id,
        "plugin-cap",
        "plugin",
        "https://plugin.example/v1",
        vec!["shared-model".to_string(), "text-only-model".to_string()],
        json!({
            "plugin": {
                "version": 1,
                "capabilities": {
                    "chat": true,
                    "streaming": true,
                    "tools": false,
                    "embeddings": false,
                    "image": false,
                    "audio": false,
                    "vision": false,
                    "json_mode": false,
                    "batch": false
                },
                "auth": { "strategy": "none" }
            }
        }),
    ));
    let mut disabled = make_channel(
        disabled_channel_id,
        "disabled-cap",
        "plugin",
        "https://disabled.example/v1",
        vec!["disabled-model".to_string()],
        serde_json::json!({"plugin":{"version":1,"capabilities":{"chat":true,"streaming":true,"embeddings":true},"auth":{"strategy":"none"},"preset":{"provider":"openai"}}}),
    );
    disabled.status = "disabled".to_string();
    disabled.health = "unhealthy".to_string();
    channels.seed_channel(disabled);

    let groups = Arc::new(InMemoryChannelGroupRepo::new());
    let projects = Arc::new(InMemoryProjectRepo::new());
    projects.seed(Project {
        id: project_id,
        org_id,
        name: "models".into(),
        slug: "models".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        API_KEY,
        ApiKeyRecord {
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
        projects,
        memberships: Arc::new(gate_storage::InMemoryMembershipRepo::new()),
        invitations: Arc::new(gate_storage::InMemoryInvitationRepo::new()),
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: channels.clone(),
        channel_groups: groups.clone(),
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
    let state = AppState::new(jwt, loader, repos);
    let router = build_router(state);
    Harness {
        router,
        api_key: API_KEY,
        admin_token: String::new(),
    }
}

async fn timed(router: &axum::Router, req: Request<Body>) -> (StatusCode, Vec<u8>, Duration) {
    let started = Instant::now();
    let resp = router.clone().oneshot(req).await.unwrap();
    let elapsed = started.elapsed();
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes, elapsed)
}

#[tokio::test]
#[ignore = "ADR-0004: plugin channels need runtime snapshot population; coverage in channel_plugin_e2e"]
async fn models_endpoint_aggregates_healthy_channel_capabilities() {
    let h = models_harness();

    let (status, body, _) = timed(
        &h.router,
        Request::builder()
            .method("GET")
            .uri("/v1/models")
            .header("authorization", format!("Bearer {}", h.api_key))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    let body: Value = serde_json::from_slice(&body).unwrap();
    let data = body["data"].as_array().unwrap();

    assert!(
        data.iter().all(|m| m["id"] != "disabled-model"),
        "disabled/unhealthy channel leaked into /v1/models: {body}"
    );

    let shared = data
        .iter()
        .find(|m| m["id"] == "shared-model")
        .expect("shared-model present");
    assert_eq!(shared["object"], "model");
    assert_eq!(shared["capabilities"]["chat"], true);
    assert_eq!(shared["capabilities"]["streaming"], true);
    assert_eq!(shared["capabilities"]["tools"], true);
    assert_eq!(shared["capabilities"]["embeddings"], true);
    assert_eq!(shared["capabilities"]["image"], true);
    assert_eq!(shared["capabilities"]["audio"], true);
    assert_eq!(shared["capabilities"]["vision"], true);
    assert_eq!(shared["capabilities"]["json_mode"], true);

    let text_only = data
        .iter()
        .find(|m| m["id"] == "text-only-model")
        .expect("plugin-only model present");
    assert_eq!(text_only["owned_by"], "plugin");
    assert_eq!(text_only["capabilities"]["chat"], true);
    assert_eq!(text_only["capabilities"]["streaming"], true);
    assert_eq!(text_only["capabilities"]["vision"], false);
    assert_eq!(text_only["capabilities"]["embeddings"], false);
}

#[tokio::test]
async fn gateway_controlplane_and_metrics_smoke() {
    let h = harness().await;

    let (status, body, elapsed) = timed(
        &h.router,
        Request::builder()
            .method("GET")
            .uri("/v1/models")
            .header("authorization", format!("Bearer {}", h.api_key))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        elapsed < MAX_ROUTE_LATENCY,
        "/v1/models too slow: {elapsed:?}"
    );

    let (status, body, elapsed) = timed(
        &h.router,
        Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("authorization", format!("Bearer {}", h.api_key))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o-mini",
                    "messages": [{"role": "user", "content": "smoke"}]
                }))
                .unwrap(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    let chat: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(chat["choices"][0]["message"]["content"], "ok");
    assert!(
        elapsed < MAX_ROUTE_LATENCY,
        "/v1/chat/completions too slow: {elapsed:?}"
    );

    let (status, body, elapsed) = timed(
        &h.router,
        Request::builder()
            .method("GET")
            .uri("/v1/admin/dashboard-stats")
            .header("authorization", format!("Bearer {}", h.admin_token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        elapsed < MAX_ROUTE_LATENCY,
        "/v1/admin/dashboard-stats too slow: {elapsed:?}"
    );

    gate_server::metrics::reset_runtime_snapshots_for_tests();
    gate_server::metrics::record_quota_deny("daily_budget_usd", "api_key", "enforce");
    gate_server::metrics::record_upstream_error_with_context(
        "authentication_error",
        "plugin",
        "ch_perf_smoke",
        "gpt-4o-mini",
    );
    gate_server::metrics::record_billing_settle_lag_seconds(0.25);
    gate_server::metrics::record_billing_outbox_lag_seconds(0.5);
    gate_server::metrics::record_usage_rollup_lag_seconds(0.25);

    let (status, body, elapsed) = timed(
        &h.router,
        Request::builder()
            .method("GET")
            .uri("/v1/admin/incidents?hours=24")
            .header("authorization", format!("Bearer {}", h.admin_token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        elapsed < MAX_ROUTE_LATENCY,
        "/v1/admin/incidents too slow: {elapsed:?}"
    );
    let incidents: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(incidents["hours"], 24);
    assert!(incidents["recent_errors"].is_array());
    assert!(incidents["top_failing_channels"].is_array());
    assert_eq!(
        incidents["quota_denies_top"][0]["dimension"],
        "daily_budget_usd"
    );
    assert_eq!(
        incidents["upstream_errors_runtime_top"][0]["kind"],
        "authentication_error"
    );
    assert!(incidents["upstream_error_classes"]["auth_401"].is_number());

    let (status, body, _) = timed(
        &h.router,
        Request::builder()
            .method("GET")
            .uri("/metrics")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let metrics = String::from_utf8(body).unwrap();
    for needle in [
        "gateway_stage_duration_seconds",
        "provider_route_decisions_total",
        "gate_tokens_total",
        "gate_requests_total",
        "gateway_requests_total",
        "gateway_request_duration_seconds",
        "gateway_upstream_errors_total",
        "quota_denies_total",
        "billing_outbox_enqueued_total",
        "billing_outbox_lag_seconds",
        "billing_settle_lag_seconds",
    ] {
        assert!(
            metrics.contains(needle),
            "missing metric {needle}; metrics={metrics}"
        );
    }
}

#[tokio::test]
async fn outbox_metrics_track_pending_lag_without_pg() {
    gate_server::metrics::install_recorder();
    let outbox = InMemoryOutboxRepo::new();
    let event = UsageEvent {
        request_id: Uuid::now_v7(),
        idempotency_key: Some("perf-outbox-metric".to_string()),
        api_key_id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        org_id: Uuid::now_v7(),
        channel_id: None,
        group_id: None,
        model: "gpt-4o-mini".to_string(),
        prompt_tokens: 1,
        completion_tokens: 1,
        cached_tokens: 0,
        reasoning_tokens: 0,
        image_units: 0,
        audio_seconds: 0.0,
        raw_usage: None,
        cost_micros: 1,
        occurred_at: Utc::now() - ChronoDuration::seconds(2),
        status: 200,
    };

    outbox.enqueue(&event).await.unwrap();
    let batch = outbox.fetch_batch(10).await.unwrap();
    assert_eq!(batch.len(), 1);

    let metrics = gate_server::metrics::render_for_tests().expect("metrics installed");
    for needle in [
        "billing_outbox_enqueued_total",
        "billing_outbox_lag_seconds",
    ] {
        assert!(
            metrics.contains(needle),
            "missing metric {needle}; metrics={metrics}"
        );
    }
}
