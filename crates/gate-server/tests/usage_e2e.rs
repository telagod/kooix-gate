//! E2 集成测试：/v1/usage 聚合 + /v1/orgs/:org/channels 只读视图
//!
//! 用 InMemory repos 跑：
//! - happy path：seed 几条 usage_records → GET /v1/usage?range=7d 得到正确聚合
//! - 跨 Org：普通 user 调别 Org → 403
//! - SuperAdmin 跨 Org：?org_id=other 通过
//! - SuperAdmin 不指定 org → 跨 Org 全量
//! - 错误 range → 400
//! - channels：member user 列出 channels（admin 视图）
//! - channels：API key 主体 → 403（require_user）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::{ChannelId, OrgId, UserId};
use gate_core::identity::{OrgRole, PlatformRole};
use gate_server::loader::{ApiKeyRecord, InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ChannelRecord, InMemoryApiKeyRepo, InMemoryChannelGroupRepo, InMemoryChannelRepo,
    InMemoryIdentityProviderRepo, InMemoryMembershipRepo, InMemoryOidcStateRepo, InMemoryOrgRepo,
    InMemoryProjectRepo, InMemoryUsageRepo, InMemoryUserIdentityRepo, InMemoryUserRepo,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ──────────────────────────────────────────────
// Fixture
// ──────────────────────────────────────────────

struct Fixture {
    router: axum::Router,
    user_token: String,
    super_token: String,
    api_key_token: String,
    org: OrgId,
    other_org: OrgId,
}

fn make_channel(id: ChannelId, code: &str) -> ChannelRecord {
    ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: format!("channel-{code}"),
        provider_type: "openai".to_string(),
        base_url: "https://example.com/v1".to_string(),
        supported_models: vec![],
        status: "active".to_string(),
        health: "healthy".to_string(),
        timeout_ms: 60_000,
        max_retries: 2,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn build_fixture() -> Fixture {
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

    let user = UserId::new();
    let super_user = UserId::new();
    let org = OrgId::new();
    let other_org = OrgId::new();

    let loader = Arc::new(InMemoryLoader::new());

    // 普通 user：org 内 Owner（含 UsageRead + OrgRead）
    let mut orgs_map = HashMap::new();
    orgs_map.insert(org, OrgRole::Owner);
    loader.add_user(
        user,
        UserRecord {
            orgs: orgs_map,
            projects: HashMap::new(),
            platform: None,
        },
    );

    // SuperAdmin：跨 Org 视野
    loader.add_user(
        super_user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );

    // ApiKey 主体（用来验 require_user! 拒绝）
    let api_key_plain = "sk-kg-test-e2-usage-key-aaaaaaaaaa";
    loader.add_api_key(
        api_key_plain,
        ApiKeyRecord {
            api_key_id: gate_core::id::ApiKeyId::new(),
            project_id: gate_core::id::ProjectId::new(),
            org_id: org,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    // 用量数据：org 三天，other_org 一天
    let usage = Arc::new(InMemoryUsageRepo::new());
    let now = Utc::now();
    usage.seed_usage(
        org,
        now - ChronoDuration::days(1),
        "gpt-4o-mini",
        0.10,
        100,
        50,
    );
    usage.seed_usage(
        org,
        now - ChronoDuration::days(1),
        "gpt-4o-mini",
        0.20,
        200,
        100,
    );
    usage.seed_usage(org, now - ChronoDuration::days(2), "gpt-4o", 0.50, 300, 150);
    usage.seed_usage(
        other_org,
        now - ChronoDuration::days(1),
        "gpt-4o",
        1.00,
        1_000,
        500,
    );

    // Channels：seed 两条 active + 一条 disabled，admin 视图全列
    let channels_repo = Arc::new(InMemoryChannelRepo::new());
    let ch_a = ChannelId::new();
    let ch_b = ChannelId::new();
    channels_repo.seed_channel(make_channel(ch_a, "openai-prod"));
    channels_repo.seed_channel(make_channel(ch_b, "azure-backup"));

    let repos = Repos {
        users: Arc::new(InMemoryUserRepo::new()),
        orgs: Arc::new(InMemoryOrgRepo::new()),
        projects: Arc::new(InMemoryProjectRepo::new()),
        memberships: Arc::new(InMemoryMembershipRepo::new()),
        api_keys: Arc::new(InMemoryApiKeyRepo::new()),
        channels: channels_repo,
        channel_groups: Arc::new(InMemoryChannelGroupRepo::new()),
        identity_providers: Arc::new(InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(InMemoryOidcStateRepo::new()),
        usage,
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
    };

    let state = AppState::new(jwt, loader, repos);
    let jwt_issuer = state.jwt.clone();
    let router = build_router(state);

    let (user_token, _) = jwt_issuer
        .issue_access(*user.as_uuid(), Uuid::now_v7(), Some(*org.as_uuid()), false)
        .unwrap();
    let (super_token, _) = jwt_issuer
        .issue_access(*super_user.as_uuid(), Uuid::now_v7(), None, false)
        .unwrap();

    Fixture {
        router,
        user_token,
        super_token,
        api_key_token: api_key_plain.to_string(),
        org,
        other_org,
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

async fn get_authed(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

// ──────────────────────────────────────────────
// /v1/usage Tests
// ──────────────────────────────────────────────

#[tokio::test]
async fn usage_returns_aggregated_series_for_current_org() {
    let f = build_fixture();
    let (status, body) = get_authed(&f.router, "/v1/usage?range=7d", &f.user_token).await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    assert_eq!(body["range"], "7d");
    assert_eq!(body["group_by"], "day");

    // 当前 org 总计：0.10 + 0.20 + 0.50 = 0.80
    let cost = body["total_cost_usd"].as_f64().unwrap();
    assert!((cost - 0.80).abs() < 1e-9, "cost={cost}");
    let tokens_in = body["total_tokens_in"].as_i64().unwrap();
    assert_eq!(tokens_in, 100 + 200 + 300);
    let tokens_out = body["total_tokens_out"].as_i64().unwrap();
    assert_eq!(tokens_out, 50 + 100 + 150);

    let series = body["series"].as_array().unwrap();
    // 两个 day bucket（day-1 和 day-2）
    assert_eq!(series.len(), 2);
}

#[tokio::test]
async fn usage_group_by_model_returns_distinct_buckets() {
    let f = build_fixture();
    let (status, body) = get_authed(
        &f.router,
        "/v1/usage?range=7d&group_by=model",
        &f.user_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["group_by"], "model");
    let series = body["series"].as_array().unwrap();
    let keys: Vec<&str> = series.iter().map(|p| p["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"gpt-4o-mini"));
    assert!(keys.contains(&"gpt-4o"));
    // gpt-4o cost (0.50) > gpt-4o-mini cost (0.30) → gpt-4o 排第一
    assert_eq!(keys[0], "gpt-4o");
}

#[tokio::test]
async fn usage_invalid_range_returns_400() {
    let f = build_fixture();
    let (status, body) = get_authed(&f.router, "/v1/usage?range=999d", &f.user_token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn usage_cross_org_query_forbidden_for_member() {
    let f = build_fixture();
    let url = format!("/v1/usage?range=7d&org_id={}", f.other_org.as_uuid());
    let (status, body) = get_authed(&f.router, &url, &f.user_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn usage_super_admin_can_query_other_org() {
    let f = build_fixture();
    let url = format!("/v1/usage?range=7d&org_id={}", f.other_org.as_uuid());
    let (status, body) = get_authed(&f.router, &url, &f.super_token).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let cost = body["total_cost_usd"].as_f64().unwrap();
    // other_org 只一条 1.00 USD
    assert!((cost - 1.00).abs() < 1e-9, "cost={cost}");
}

#[tokio::test]
async fn usage_super_admin_without_org_filter_aggregates_all() {
    let f = build_fixture();
    let (status, body) = get_authed(&f.router, "/v1/usage?range=7d", &f.super_token).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let cost = body["total_cost_usd"].as_f64().unwrap();
    // 全部：0.80 + 1.00 = 1.80
    assert!((cost - 1.80).abs() < 1e-9, "cost={cost}");
}

#[tokio::test]
async fn usage_apikey_subject_forbidden() {
    let f = build_fixture();
    let (status, body) = get_authed(&f.router, "/v1/usage?range=7d", &f.api_key_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
}

#[tokio::test]
async fn usage_unauthenticated_returns_401() {
    let f = build_fixture();
    let req = Request::builder()
        .method("GET")
        .uri("/v1/usage?range=7d")
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ──────────────────────────────────────────────
// /v1/orgs/:org/channels Tests
// ──────────────────────────────────────────────

#[tokio::test]
async fn channels_lists_all_admin_view_for_member() {
    let f = build_fixture();
    let url = format!("/v1/orgs/{}/channels", f.org.as_uuid());
    let (status, body) = get_authed(&f.router, &url, &f.user_token).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let codes: Vec<&str> = arr.iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"openai-prod"));
    assert!(codes.contains(&"azure-backup"));
    // 字段全在
    let first = &arr[0];
    assert!(first["id"].is_string());
    assert!(first["provider_type"].is_string());
    assert!(first["status"].is_string());
    assert!(first["health"].is_string());
}

#[tokio::test]
async fn channels_api_key_subject_forbidden() {
    let f = build_fixture();
    let url = format!("/v1/orgs/{}/channels", f.org.as_uuid());
    let (status, body) = get_authed(&f.router, &url, &f.api_key_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
}

#[tokio::test]
async fn channels_for_other_org_forbidden() {
    let f = build_fixture();
    // user 只在 f.org 里，f.other_org 越权
    let url = format!("/v1/orgs/{}/channels", f.other_org.as_uuid());
    let (status, body) = get_authed(&f.router, &url, &f.user_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
}
