//! Lightweight perf / observability smoke.
//!
//! Keeps CI cheap: in-memory auth/repos + wiremock upstream, asserts hot routes stay
//! responsive and `/metrics` exposes the business SLO signals.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
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

fn make_channel(id: ChannelId, code: &str, base_url: &str) -> ChannelRecord {
    let now = Utc::now();
    ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: code.to_string(),
        provider_type: "openai".to_string(),
        base_url: base_url.to_string(),
        supported_models: vec!["gpt-4o-mini".to_string()],
        status: "active".to_string(),
        health: "healthy".to_string(),
        timeout_ms: 60_000,
        max_retries: 1,
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
        &format!("{}/v1", upstream.uri()),
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
        api_keys: Arc::new(gate_storage::InMemoryApiKeyRepo::new()),
        channels: channels.clone(),
        channel_groups: groups.clone(),
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
    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(ProviderRouter::new(channels, groups));
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
    ] {
        assert!(
            metrics.contains(needle),
            "missing metric {needle}; metrics={metrics}"
        );
    }
}
