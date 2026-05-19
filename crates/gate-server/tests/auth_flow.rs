//! 集成测试：在内存里跑完整 Axum 路由 + Auth 抽取器 + require!
//!
//! 覆盖：
//! - 健康检查无 auth
//! - 缺凭证 → 401
//! - 无效 JWT → 401
//! - 有 JWT 但权限不足 → 403
//! - 跨 Org 重放 (用 A Org 的 JWT 调 B Org 路径) → 403
//! - 角色足够 → 200
//! - API key 拒绝管理类端点 (require_user!)
//! - API key revoked → 403
//! - require_user! 在 admin 路由生效

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::*;
use gate_core::identity::{
    OrgRole, OrgStatus, Organization, PlatformRole, Project, ProjectRole, ProjectStatus,
};
use gate_providers::ProviderRouter;
use gate_server::loader::{ApiKeyRecord, InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ApiKeyRecord as RepoApiKey, ChannelGroupRecord, ChannelRecord, InMemoryApiKeyRepo,
    InMemoryChannelGroupRepo, InMemoryChannelRepo, InMemoryMembershipRepo, InMemoryOrgRepo,
    InMemoryProjectRepo, InMemoryUserRepo,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ----------------------------------------------------------------------------
// Test scaffolding
// ----------------------------------------------------------------------------

struct Fixture {
    router: axum::Router,
    jwt: Arc<JwtIssuer>,
    org_a: OrgId,
    org_b: OrgId,
    proj_a: ProjectId,
    user_dev: UserId,
    user_orgowner: UserId,
    user_super: UserId,
    user_other: UserId,
    api_key_plain: String,
    api_key_revoked: String,
}

fn fixture() -> Fixture {
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

    let loader = Arc::new(InMemoryLoader::new());

    let org_a = OrgId::new();
    let org_b = OrgId::new();
    let proj_a = ProjectId::new();

    // user_dev: Org A 的 Developer，Project A 的 Developer
    let user_dev = UserId::new();
    {
        let mut orgs = HashMap::new();
        orgs.insert(org_a, OrgRole::Member);
        let mut projs = HashMap::new();
        projs.insert((org_a, proj_a), ProjectRole::Developer);
        loader.add_user(
            user_dev,
            UserRecord {
                orgs,
                projects: projs,
                platform: None,
            },
        );
    }

    // user_orgowner: Org A 的 Owner
    let user_orgowner = UserId::new();
    {
        let mut orgs = HashMap::new();
        orgs.insert(org_a, OrgRole::Owner);
        loader.add_user(
            user_orgowner,
            UserRecord {
                orgs,
                projects: HashMap::new(),
                platform: None,
            },
        );
    }

    // user_super: 平台 SuperAdmin
    let user_super = UserId::new();
    loader.add_user(
        user_super,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );

    // user_other: Org B 的 Owner（验证跨 Org 隔离）
    let user_other = UserId::new();
    {
        let mut orgs = HashMap::new();
        orgs.insert(org_b, OrgRole::Owner);
        loader.add_user(
            user_other,
            UserRecord {
                orgs,
                projects: HashMap::new(),
                platform: None,
            },
        );
    }

    // 一把有效 API key + 一把已撤销
    let key = gate_auth::api_key::generate();
    let api_key_plain = key.plaintext.to_string();
    loader.add_api_key(
        &api_key_plain,
        ApiKeyRecord {
            api_key_id: ApiKeyId::new(),
            project_id: proj_a,
            org_id: org_a,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let key_r = gate_auth::api_key::generate();
    let api_key_revoked = key_r.plaintext.to_string();
    loader.add_api_key(
        &api_key_revoked,
        ApiKeyRecord {
            api_key_id: ApiKeyId::new(),
            project_id: proj_a,
            org_id: org_a,
            revoked: true,
            allowed_ips: vec![],
        },
    );

    let state = AppState::new(
        jwt,
        loader,
        build_repos(
            org_a,
            org_b,
            proj_a,
            user_dev,
            &api_key_plain,
            &api_key_revoked,
        ),
    );
    let jwt_arc = state.jwt.clone();
    let router = build_router(state);

    Fixture {
        router,
        jwt: jwt_arc,
        org_a,
        org_b,
        proj_a,
        user_dev,
        user_orgowner,
        user_super,
        user_other,
        api_key_plain,
        api_key_revoked,
    }
}

fn build_repos(
    org_a: OrgId,
    org_b: OrgId,
    proj_a: ProjectId,
    user_dev: UserId,
    api_key_plain: &str,
    api_key_revoked: &str,
) -> Repos {
    let now = Utc::now();
    let users = Arc::new(InMemoryUserRepo::new());
    seed_active_user(&users, user_dev);
    let orgs = Arc::new(InMemoryOrgRepo::new());
    let projects = Arc::new(InMemoryProjectRepo::new());
    let memberships = Arc::new(InMemoryMembershipRepo::new());
    let api_keys = Arc::new(InMemoryApiKeyRepo::new());

    orgs.seed(Organization {
        id: org_a,
        name: "Acme".into(),
        slug: "acme".into(),
        owner_user_id: user_dev,
        status: OrgStatus::Active,
        billing_email: None,
        created_at: now,
        updated_at: now,
    });
    orgs.seed(Organization {
        id: org_b,
        name: "Beta".into(),
        slug: "beta".into(),
        owner_user_id: user_dev,
        status: OrgStatus::Active,
        billing_email: None,
        created_at: now,
        updated_at: now,
    });
    projects.seed(Project {
        id: proj_a,
        org_id: org_a,
        name: "main".into(),
        slug: "main".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });

    // 两把 API key 也塞进 Repo（供 list / revoke 走真 Repo）
    api_keys.seed(
        &gate_auth::api_key::hash(api_key_plain),
        RepoApiKey {
            api_key_id: ApiKeyId::new(),
            project_id: proj_a,
            org_id: org_a,
            name: "active".into(),
            allowed_ips: vec![],
            allowed_models: vec![],
            allowed_groups: vec![],
            expires_at: None,
            revoked_at: None,
        },
    );
    api_keys.seed(
        &gate_auth::api_key::hash(api_key_revoked),
        RepoApiKey {
            api_key_id: ApiKeyId::new(),
            project_id: proj_a,
            org_id: org_a,
            name: "revoked".into(),
            allowed_ips: vec![],
            allowed_models: vec![],
            allowed_groups: vec![],
            expires_at: None,
            revoked_at: Some(now),
        },
    );

    Repos {
        users,
        orgs,
        projects,
        memberships,
        api_keys,
        channels: Arc::new(gate_storage::InMemoryChannelRepo::new()),
        channel_groups: Arc::new(gate_storage::InMemoryChannelGroupRepo::new()),
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(gate_storage::InMemoryChannelKeyRepo::new()),
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
    }
}

fn seed_active_user(users: &InMemoryUserRepo, user_id: UserId) {
    let now = Utc::now();
    users.seed(
        gate_core::identity::User {
            id: user_id,
            email: format!("{}@example.com", user_id.as_uuid()),
            display_name: None,
            status: gate_core::identity::UserStatus::Active,
            mfa_enabled: false,
            email_verified_at: None,
            last_login_at: None,
            created_at: now,
            updated_at: now,
        },
        None,
    );
}

fn jwt_for(jwt: &JwtIssuer, user: UserId, org: Option<OrgId>, is_super: bool) -> String {
    let (tok, _) = jwt
        .issue_access(
            *user.as_uuid(),
            Uuid::now_v7(),
            org.map(|o| *o.as_uuid()),
            is_super,
        )
        .unwrap();
    tok
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let req = req.body(body).unwrap();
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

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[tokio::test]
async fn health_open() {
    let f = fixture();
    let (status, body) = call(&f.router, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn me_requires_auth() {
    let f = fixture();
    let (status, body) = call(&f.router, "GET", "/v1/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "missing_credentials");
}

#[tokio::test]
async fn invalid_jwt_rejected() {
    let f = fixture();
    let (status, body) = call(&f.router, "GET", "/v1/me", Some("not.a.jwt"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "token_invalid");
}

#[tokio::test]
async fn me_returns_user_summary() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_dev, Some(f.org_a), false);
    let (status, body) = call(&f.router, "GET", "/v1/me", Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subject"]["kind"], "user");
    assert_eq!(body["current_org"], f.org_a.to_string());
    assert_eq!(body["is_platform_admin"], false);
}

#[tokio::test]
async fn list_projects_allowed_for_org_member() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_dev, Some(f.org_a), false);
    let url = format!("/v1/orgs/{}/projects", f.org_a.as_uuid());
    let (status, _) = call(&f.router, "GET", &url, Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn cross_org_access_denied() {
    // user_dev 只属于 Org A，调 Org B 的路径必须 403
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_dev, Some(f.org_a), false);
    let url = format!("/v1/orgs/{}/projects", f.org_b.as_uuid());
    let (status, body) = call(&f.router, "GET", &url, Some(&tok), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn create_apikey_requires_user_subject() {
    let f = fixture();
    let url = format!(
        "/v1/orgs/{}/projects/{}/api-keys",
        f.org_a.as_uuid(),
        f.proj_a.as_uuid()
    );

    // 用 API key 调（应被 require_user! 拒绝）
    let (status, body) = call(
        &f.router,
        "POST",
        &url,
        Some(&f.api_key_plain),
        Some(serde_json::json!({"name": "test"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn create_apikey_succeeds_for_developer() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_dev, Some(f.org_a), false);
    let url = format!(
        "/v1/orgs/{}/projects/{}/api-keys",
        f.org_a.as_uuid(),
        f.proj_a.as_uuid()
    );
    let (status, body) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(serde_json::json!({"name": "ci-key"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["plaintext"].as_str().unwrap().starts_with("sk-kg-"));
}

#[tokio::test]
async fn create_apikey_denied_when_not_member() {
    let f = fixture();
    // user_other 不在 Org A
    let tok = jwt_for(&f.jwt, f.user_other, Some(f.org_b), false);
    let url = format!(
        "/v1/orgs/{}/projects/{}/api-keys",
        f.org_a.as_uuid(),
        f.proj_a.as_uuid()
    );
    let (status, _) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(serde_json::json!({"name": "evil"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn revoked_api_key_rejected() {
    let f = fixture();
    let (status, body) = call(&f.router, "GET", "/v1/me", Some(&f.api_key_revoked), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "api_key_revoked");
}

#[tokio::test]
async fn invalid_api_key_rejected() {
    let f = fixture();
    let (status, body) = call(
        &f.router,
        "GET",
        "/v1/me",
        Some("sk-kg-this-key-doesnt-exist-at-all-XX"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "api_key_invalid");
}

#[tokio::test]
async fn admin_channels_requires_platform() {
    let f = fixture();

    // 普通 Org owner 不能访问
    let tok = jwt_for(&f.jwt, f.user_orgowner, Some(f.org_a), false);
    let (status, _) = call(&f.router, "GET", "/v1/admin/channels", Some(&tok), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // SuperAdmin 通过
    let tok = jwt_for(&f.jwt, f.user_super, None, true);
    let (status, body) = call(&f.router, "GET", "/v1/admin/channels", Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_array());
}

#[tokio::test]
async fn admin_can_create_plugin_channel_with_provider_preset_manifest() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);
    let body = serde_json::json!({
        "code": "plugin-openai-preset",
        "provider_type": "plugin",
        "base_url": "https://api.openai.com/v1",
        "supported_models": ["gpt-4o-mini"],
        "model_mapping": {
            "plugin": { "preset": { "provider": "openai_compatible" } }
        }
    });

    let (status, body) = call(
        &f.router,
        "POST",
        "/v1/admin/channels",
        Some(&tok),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["provider_type"], "plugin");
    assert_eq!(
        body["model_mapping"]["plugin"]["preset"]["provider"],
        "openai_compatible"
    );
    assert_eq!(body["capabilities"]["chat"], true);
    assert_eq!(body["capabilities"]["streaming"], true);
    assert_eq!(body["capabilities"]["embeddings"], true);
}

#[tokio::test]
async fn admin_exposes_plugin_manifest_schema() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);
    let (status, body) = call(
        &f.router,
        "GET",
        "/v1/admin/plugin-manifest/schema",
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["$defs"]["PluginManifest"]["properties"]["version"]["type"],
        "integer"
    );
    assert!(body["$defs"]["PluginManifest"]["properties"]["auth"].is_object());
}

#[tokio::test]
async fn admin_rejects_invalid_plugin_manifest_with_pointer() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);
    let body = serde_json::json!({
        "code": "plugin-bad",
        "provider_type": "plugin",
        "base_url": "https://api.openai.com/v1",
        "supported_models": ["gpt-4o-mini"],
        "model_mapping": {
            "plugin": {
                "version": 1,
                "security": { "max_request_bytes": "too-large" }
            }
        }
    });

    let (status, body) = call(
        &f.router,
        "POST",
        "/v1/admin/channels",
        Some(&tok),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("/plugin/security/max_request_bytes"),
        "body={body}"
    );
}

#[tokio::test]
async fn admin_group_detail_exposes_fallback_chain_and_validates_cycles() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);

    let (status, primary) = call(
        &f.router,
        "POST",
        "/v1/admin/groups",
        Some(&tok),
        Some(serde_json::json!({
            "name": "Primary",
            "strategy": "priority",
            "description": "primary traffic"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "primary={primary}");
    assert_eq!(primary["description"], "primary traffic");
    let primary_id = primary["id"].as_str().unwrap().to_string();

    let (status, fallback) = call(
        &f.router,
        "POST",
        "/v1/admin/groups",
        Some(&tok),
        Some(serde_json::json!({
            "name": "Fallback",
            "strategy": "least_latency",
            "fallback_group_id": primary_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "fallback={fallback}");
    let fallback_id = fallback["id"].as_str().unwrap().to_string();

    let (status, body) = call(
        &f.router,
        "PUT",
        &format!("/v1/admin/groups/{primary_id}"),
        Some(&tok),
        Some(serde_json::json!({ "fallback_group_id": fallback_id })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert_eq!(body["error"]["code"], "bad_request");

    let (status, detail) = call(
        &f.router,
        "GET",
        &format!("/v1/admin/groups/{fallback_id}/detail"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "detail={detail}");
    assert_eq!(detail["group"]["id"], fallback_id);
    assert_eq!(detail["fallback_chain"].as_array().unwrap().len(), 2);
    assert_eq!(detail["fallback_chain"][0]["id"], fallback_id);
    assert_eq!(detail["fallback_chain"][0]["is_fallback"], false);
    assert_eq!(detail["fallback_chain"][1]["id"], primary_id);
    assert_eq!(detail["fallback_chain"][1]["is_fallback"], true);
    assert_eq!(detail["fallback_stats"]["window_hours"], 24);
    assert_eq!(detail["fallback_stats"]["has_cycle"], false);
    assert!(detail["projects_using"].is_array());
    assert!(detail["project_ids"].is_array());
}

#[tokio::test]
async fn admin_group_binding_canary_validates_and_returns_stats_shape() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);

    let (status, group) = call(
        &f.router,
        "POST",
        "/v1/admin/groups",
        Some(&tok),
        Some(serde_json::json!({
            "name": "Canary",
            "strategy": "priority"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "group={group}");
    let group_id = group["id"].as_str().unwrap().to_string();

    let (status, channel) = call(
        &f.router,
        "POST",
        "/v1/admin/channels",
        Some(&tok),
        Some(serde_json::json!({
            "code": "canary-control",
            "provider_type": "openai",
            "base_url": "https://api.openai.com/v1",
            "supported_models": ["gpt-canary"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "channel={channel}");
    let channel_uuid = channel["id"].as_str().unwrap().split_once('_').unwrap().1;

    let (status, body) = call(
        &f.router,
        "POST",
        &format!("/v1/admin/groups/{group_id}/bindings"),
        Some(&tok),
        Some(serde_json::json!({
            "channel_id": channel_uuid,
            "priority": 1,
            "weight": 1,
            "canary_percent_bps": 50
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("canary_percent_bps"),
        "body={body}"
    );

    let (status, body) = call(
        &f.router,
        "POST",
        &format!("/v1/admin/groups/{group_id}/bindings"),
        Some(&tok),
        Some(serde_json::json!({
            "channel_id": channel_uuid,
            "priority": 1,
            "weight": 1,
            "canary_percent_bps": 500
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let (status, detail) = call(
        &f.router,
        "GET",
        &format!("/v1/admin/groups/{group_id}/detail"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "detail={detail}");
    assert_eq!(detail["bindings"][0]["canary_percent_bps"], 500);
    assert_eq!(detail["canary_stats"][0]["is_canary"], true);
    assert_eq!(detail["canary_stats"][0]["requests"], 0);

    let (status, body) = call(
        &f.router,
        "PUT",
        &format!("/v1/admin/groups/{group_id}/bindings/{channel_uuid}"),
        Some(&tok),
        Some(serde_json::json!({ "canary_percent_bps": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let (status, bindings) = call(
        &f.router,
        "GET",
        &format!("/v1/admin/groups/{group_id}/bindings"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bindings={bindings}");
    assert!(
        bindings[0]["canary_percent_bps"].is_null(),
        "bindings={bindings}"
    );
}

#[tokio::test]
async fn admin_channel_draining_stops_new_requests_and_waits_for_inflight() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);

    let channel_repo = Arc::new(InMemoryChannelRepo::new());
    let group_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let project_id = f.proj_a;
    let group_id = ChannelGroupId::new();
    let draining_id = ChannelId::new();
    let active_id = ChannelId::new();
    let now = Utc::now();

    group_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "drain-test".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    group_repo.seed_default(project_id, group_id);

    let make_channel = |id: ChannelId, code: &str| ChannelRecord {
        channel_id: id,
        code: code.to_string(),
        name: code.to_string(),
        provider_type: "openai".to_string(),
        base_url: "http://localhost:9999".to_string(),
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
        created_at: now,
        updated_at: now,
    };
    channel_repo.seed_channel(make_channel(draining_id, "draining-candidate"));
    channel_repo.seed_channel(make_channel(active_id, "active-survivor"));
    channel_repo.seed_binding(group_id, draining_id, 1, 1);
    channel_repo.seed_binding(group_id, active_id, 2, 1);

    let provider_router = Arc::new(ProviderRouter::new(
        channel_repo.clone(),
        group_repo.clone(),
    ));
    provider_router.inflight_tracker().acquire(draining_id);

    let mut repos = build_repos(
        f.org_a,
        f.org_b,
        f.proj_a,
        f.user_dev,
        &f.api_key_plain,
        &f.api_key_revoked,
    );
    repos.channels = channel_repo;
    repos.channel_groups = group_repo;

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_user(
        f.user_super,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );
    let state = AppState::new((*f.jwt).clone(), loader, repos)
        .with_provider_router_arc(provider_router.clone());
    let router = build_router(state);

    let (status, body) = call(
        &router,
        "POST",
        &format!("/v1/admin/channels/{}/drain", draining_id.as_uuid()),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["channel"]["status"], "draining");
    assert_eq!(body["inflight"], 1);
    assert_eq!(body["safe_to_disable"], false);

    let routed = provider_router
        .route(project_id, "any")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        routed.channel_id, active_id,
        "new route must skip draining channel"
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!(
            "/v1/admin/channels/{}/disable-when-idle",
            draining_id.as_uuid()
        ),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("inflight")
    );

    provider_router.release_channel(draining_id);
    let (status, body) = call(
        &router,
        "POST",
        &format!(
            "/v1/admin/channels/{}/disable-when-idle",
            draining_id.as_uuid()
        ),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["channel"]["status"], "disabled");
    assert_eq!(body["inflight"], 0);
    assert_eq!(body["safe_to_disable"], true);
}

#[tokio::test]
async fn admin_channel_drain_rejects_api_key_subject() {
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_super, None, true);
    let (status, body) = call(
        &f.router,
        "POST",
        "/v1/admin/channels",
        Some(&tok),
        Some(serde_json::json!({
            "code": "drain-api-key-subject",
            "provider_type": "openai",
            "base_url": "https://api.openai.com/v1"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let channel_id = body["id"].as_str().unwrap();

    let (status, body) = call(
        &f.router,
        "POST",
        &format!("/v1/admin/channels/{channel_id}/drain"),
        Some(&f.api_key_plain),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn admin_rejects_api_key_subject() {
    let f = fixture();
    let (status, body) = call(
        &f.router,
        "GET",
        "/v1/admin/channels",
        Some(&f.api_key_plain),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn header_org_switch_denied_if_not_member() {
    // user_dev 属于 Org A；通过 X-Kooix-Org 切到 Org B 应被拒
    let f = fixture();
    let tok = jwt_for(&f.jwt, f.user_dev, Some(f.org_a), false);
    let req = Request::builder()
        .method("GET")
        .uri("/v1/me")
        .header("authorization", format!("Bearer {tok}"))
        .header("x-kooix-org", f.org_b.as_uuid().to_string())
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// 集成测试：创建 API key 后 audit_logs 表应有一条 `api_key.create` 记录。
#[tokio::test]
async fn create_apikey_emits_audit_record() {
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

    let org_a = OrgId::new();
    let proj_a = ProjectId::new();
    let user_dev = UserId::new();

    let loader = Arc::new(InMemoryLoader::new());
    {
        let mut orgs = HashMap::new();
        orgs.insert(org_a, OrgRole::Member);
        let mut projs = HashMap::new();
        projs.insert((org_a, proj_a), ProjectRole::Developer);
        loader.add_user(
            user_dev,
            UserRecord {
                orgs,
                projects: projs,
                platform: None,
            },
        );
    }

    let now = Utc::now();
    let projects = Arc::new(InMemoryProjectRepo::new());
    projects.seed(Project {
        id: proj_a,
        org_id: org_a,
        name: "main".into(),
        slug: "main".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });

    let audit_repo = Arc::new(gate_storage::InMemoryAuditRepo::new());

    let repos = Repos {
        users: Arc::new(InMemoryUserRepo::new()),
        orgs: Arc::new(InMemoryOrgRepo::new()),
        projects,
        memberships: Arc::new(InMemoryMembershipRepo::new()),
        api_keys: Arc::new(InMemoryApiKeyRepo::new()),
        channels: Arc::new(gate_storage::InMemoryChannelRepo::new()),
        channel_groups: Arc::new(gate_storage::InMemoryChannelGroupRepo::new()),
        channel_latency: Arc::new(gate_storage::InMemoryChannelLatencyRepo::new()),
        channel_keys: Arc::new(gate_storage::InMemoryChannelKeyRepo::new()),
        identity_providers: Arc::new(gate_storage::InMemoryIdentityProviderRepo::new()),
        user_identities: Arc::new(gate_storage::InMemoryUserIdentityRepo::new()),
        oidc_states: Arc::new(gate_storage::InMemoryOidcStateRepo::new()),
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        quotas: Arc::new(gate_storage::InMemoryQuotaRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
        audit: audit_repo.clone(),
        billing: Arc::new(gate_storage::InMemoryBillingRepo::new()),
        request_logs: Arc::new(gate_storage::InMemoryRequestLogRepo::new()),
        inflight: Arc::new(gate_storage::InMemoryInFlightRepo::new()),
        pg_pool: None,
    };

    let state = AppState::new(jwt.clone(), loader, repos);
    let router = build_router(state);

    let tok = jwt_for(&jwt, user_dev, Some(org_a), false);
    let url = format!(
        "/v1/orgs/{}/projects/{}/api-keys",
        org_a.as_uuid(),
        proj_a.as_uuid()
    );
    let (status, body) = call(
        &router,
        "POST",
        &url,
        Some(&tok),
        Some(serde_json::json!({"name": "audit-test-key"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["plaintext"].as_str().unwrap().starts_with("sk-kg-"));

    // Audit is spawned — give it a moment to land
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let logs = audit_repo.all();
    assert_eq!(logs.len(), 1, "expected exactly 1 audit record");
    assert_eq!(logs[0].action, "api_key.create");
    assert_eq!(logs[0].resource_kind, "api_key");
    assert_eq!(logs[0].actor_kind, "user");
    assert_eq!(logs[0].actor_id, Some(*user_dev.as_uuid()));
    assert_eq!(logs[0].org_id, Some(*org_a.as_uuid()));
    assert_eq!(logs[0].outcome, "success");
}
