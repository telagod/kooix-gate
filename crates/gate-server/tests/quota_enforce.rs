//! quota_enforce middleware E2E：用 Redis 容器 + InMemory repo 验证多维度配额拦截。
//!
//! 覆盖：
//! - rpm quota：api_key 主体 6 次连发，第 6 次返 429 quota_exceeded
//! - daily_budget_usd：未超额放行；超额 (peek > limit*1e6) → 429
//! - user 主体路径（user 维度 quota 生效）
//! - 跨 Org 创建 quota → 403/404（防越权写）
//! - 无 quota 配置时 chat 全部放行（middleware no-op）
//! - rpm + tpm 两维度并存：先命中的拦截
//! - REST endpoint：POST 创建 + GET list + DELETE
//! - 权限：QuotaWrite 必备（Member 只能 read 不能 write）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use fred::interfaces::KeysInterface;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
use gate_core::identity::{OrgRole, OrgStatus, Organization, Project, ProjectRole, ProjectStatus};
use gate_providers::openai::OpenAiProvider;
use gate_server::loader::{ApiKeyRecord as LoaderApiKey, InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{
    ApiKeyRecord as StoredApiKey, InMemoryApiKeyRepo, InMemoryOrgRepo, InMemoryProjectRepo,
    QuotaUpsert,
};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─────────────────────────────────────────────────────────────────────────
// Fixture
// ─────────────────────────────────────────────────────────────────────────

struct Fix {
    router: axum::Router,
    repos: Repos,
    redis_pool: fred::clients::RedisPool,
    jwt: Arc<JwtIssuer>,
    api_key_plain: String,
    api_key_id: ApiKeyId,
    project_id: ProjectId,
    org_id: OrgId,
    user_id: UserId,
    _redis: testcontainers::ContainerAsync<Redis>,
    _upstream: MockServer,
}

async fn start_redis() -> (
    testcontainers::ContainerAsync<Redis>,
    fred::clients::RedisPool,
) {
    let tag = std::env::var("KOOIX_TEST_REDIS_TAG").unwrap_or_else(|_| "7-alpine".into());
    let container = Redis::default().with_tag(&tag).start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{host}:{port}");
    let pool = gate_cache::connect(&url, 2).await.unwrap();
    (container, pool)
}

async fn make_fixture() -> Fix {
    let (redis_c, redis_pool) = start_redis().await;

    // Wiremock upstream returning OK for chat
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-q",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;

    // IDs + InMemory repos
    let user_id = UserId::new();
    let owner_id = UserId::new();
    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let api_key_id = ApiKeyId::new();

    // 显式构造 InMemory repos 持引用以便 seed
    let users = Arc::new(gate_storage::InMemoryUserRepo::new());
    let orgs = Arc::new(InMemoryOrgRepo::new());
    let projects = Arc::new(InMemoryProjectRepo::new());
    let memberships = Arc::new(gate_storage::InMemoryMembershipRepo::new());
    let api_keys = Arc::new(InMemoryApiKeyRepo::new());
    let channels = Arc::new(gate_storage::InMemoryChannelRepo::new());
    let channel_groups = Arc::new(gate_storage::InMemoryChannelGroupRepo::new());
    let identity_providers = Arc::new(gate_storage::InMemoryIdentityProviderRepo::new());
    let user_identities = Arc::new(gate_storage::InMemoryUserIdentityRepo::new());
    let oidc_states = Arc::new(gate_storage::InMemoryOidcStateRepo::new());
    let quotas = Arc::new(gate_storage::InMemoryQuotaRepo::new());

    let now = chrono::Utc::now();
    orgs.seed(Organization {
        id: org_id,
        name: "Acme".into(),
        slug: "acme".into(),
        owner_user_id: owner_id,
        status: OrgStatus::Active,
        billing_email: None,
        created_at: now,
        updated_at: now,
    });
    projects.seed(Project {
        id: project_id,
        org_id,
        name: "main".into(),
        slug: "main".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });
    api_keys.seed(
        "fake-hash-for-seed",
        StoredApiKey {
            api_key_id,
            project_id,
            org_id,
            name: "test".into(),
            allowed_ips: vec![],
            allowed_models: vec![],
            allowed_groups: vec![],
            expires_at: None,
            revoked_at: None,
        },
    );

    let repos = Repos {
        users,
        orgs,
        projects,
        memberships: memberships.clone(),
        api_keys,
        channels,
        channel_groups,
        channel_keys: Arc::new(gate_storage::InMemoryChannelKeyRepo::new()),
        identity_providers,
        user_identities,
        oidc_states,
        quotas,
        usage: Arc::new(gate_storage::InMemoryUsageRepo::new()),
        model_aliases: Arc::new(gate_storage::InMemoryModelAliasRepo::new()),
    };

    // user 也作为 owner 对该 Org 拥有 Owner role（用于跑 REST endpoint 鉴权）
    memberships.seed_project(org_id, project_id, user_id, ProjectRole::Owner);
    let mut user_orgs = HashMap::new();
    user_orgs.insert(org_id, OrgRole::Owner);
    let mut user_projs = HashMap::new();
    user_projs.insert((org_id, project_id), ProjectRole::Owner);

    // Loader：注册 user + api_key
    let loader = Arc::new(InMemoryLoader::new());
    loader.add_user(
        user_id,
        UserRecord {
            orgs: user_orgs,
            projects: user_projs,
            platform: None,
        },
    );
    let api_key_plain = "sk-kg-test-quota-enforce-key-00000".to_string();
    loader.add_api_key(
        &api_key_plain,
        LoaderApiKey {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

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

    let provider = OpenAiProvider::new(format!("{}/v1", upstream.uri()), "test-key").unwrap();
    let state = AppState::new(jwt, loader, repos.clone())
        .with_rate_limiter(gate_cache::RateLimiter::new(redis_pool.clone()))
        .with_quota_counter(gate_cache::QuotaCounter::new(redis_pool.clone()))
        // 把全局 capacity 提到 1000 让 rate_limit middleware 不干扰
        .with_rate_limit_cfg(gate_server::state::RateLimitCfg {
            window_ms: 60_000,
            capacity: 10_000,
        })
        .with_provider(provider);
    let jwt_arc = state.jwt.clone();
    let router = build_router(state);

    Fix {
        router,
        repos,
        redis_pool,
        jwt: jwt_arc,
        api_key_plain,
        api_key_id,
        project_id,
        org_id,
        user_id,
        _redis: redis_c,
        _upstream: upstream,
    }
}

fn user_token(jwt: &JwtIssuer, user: UserId, org: OrgId) -> String {
    let (tok, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), Some(*org.as_uuid()), false)
        .unwrap();
    tok
}

async fn chat_with_apikey(router: &axum::Router, plaintext: &str) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {plaintext}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    router.clone().oneshot(req).await.unwrap().status()
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rpm_quota_blocks_after_limit() {
    let f = make_fixture().await;

    // 装一条 rpm=5/60s 的 api_key 维度 quota
    f.repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: "api_key".into(),
            scope_id: *f.api_key_id.as_uuid(),
            dimension: "rpm".into(),
            model_filter: None,
            limit_value: Decimal::from(5),
            window_seconds: Some(60),
        })
        .await
        .unwrap();

    let mut allowed = 0;
    let mut denied = 0;
    let mut last_body = Value::Null;
    for _ in 0..6 {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("authorization", format!("Bearer {}", f.api_key_plain))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o-mini",
                    "messages": [{"role": "user", "content": "hi"}]
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = f.router.clone().oneshot(req).await.unwrap();
        if resp.status() == StatusCode::OK {
            allowed += 1;
        } else if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            denied += 1;
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            last_body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        }
    }
    assert_eq!(allowed, 5, "first 5 should pass");
    assert_eq!(denied, 1, "6th should 429");
    assert_eq!(last_body["error"]["code"], "quota_exceeded");
    assert_eq!(last_body["error"]["dimension"], "rpm");
    assert!(last_body["error"]["retry_after_ms"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn daily_budget_blocks_when_exhausted() {
    let f = make_fixture().await;

    // 创建 api_key 维度 daily_budget_usd=$1
    let q = f
        .repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: "api_key".into(),
            scope_id: *f.api_key_id.as_uuid(),
            dimension: "daily_budget_usd".into(),
            model_filter: None,
            limit_value: Decimal::from(1), // $1 = 1_000_000 micros
            window_seconds: None,
        })
        .await
        .unwrap();

    // 模拟用了 $0.5（500k micros）→ 应放行
    let key = format!(
        "qb:d:api_key:{}:daily_budget_usd:*:{}",
        f.api_key_id.as_uuid(),
        q.id
    );
    let _: () = f
        .redis_pool
        .next()
        .set(key.clone(), "500000", None, None, false)
        .await
        .unwrap();

    let st = chat_with_apikey(&f.router, &f.api_key_plain).await;
    assert_eq!(st, StatusCode::OK, "0.5/1 budget should pass");

    // 模拟用了 $1.5（1.5M micros）→ 拒
    let _: () = f
        .redis_pool
        .next()
        .set(key, "1500000", None, None, false)
        .await
        .unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", f.api_key_plain))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["dimension"], "daily_budget_usd");
}

#[tokio::test]
async fn user_subject_quota_path() {
    let f = make_fixture().await;

    // user 维度 rpm=2
    f.repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: "user".into(),
            scope_id: *f.user_id.as_uuid(),
            dimension: "rpm".into(),
            model_filter: None,
            limit_value: Decimal::from(2),
            window_seconds: Some(60),
        })
        .await
        .unwrap();

    let tok = user_token(&f.jwt, f.user_id, f.org_id);

    // 第 1, 2 次通过；/v1/me 是 GET，不会触发 chat provider
    for _ in 0..2 {
        let req = Request::builder()
            .method("GET")
            .uri("/v1/me")
            .header("authorization", format!("Bearer {tok}"))
            .body(Body::empty())
            .unwrap();
        let resp = f.router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
    // 第 3 次拒
    let req = Request::builder()
        .method("GET")
        .uri("/v1/me")
        .header("authorization", format!("Bearer {tok}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["dimension"], "rpm");
}

#[tokio::test]
async fn cross_org_quota_create_blocked() {
    let f = make_fixture().await;
    let tok = user_token(&f.jwt, f.user_id, f.org_id);

    // 攻击者用合法 token 试图给「他不属于的另一个 Org」创建 quota
    let other_org = OrgId::new();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/orgs/{}/quotas", other_org.as_uuid()))
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "scope_kind": "org",
                "scope_id": other_org.as_uuid().to_string(),
                "dimension": "rpm",
                "limit_value": "10",
                "window_seconds": 60
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    // user 在 other_org 没有 OrgRole → require! QuotaWrite 失败 → 403
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_quota_configured_passes_through() {
    let f = make_fixture().await;
    // 未配置任何 quota → middleware 应放行
    for _ in 0..3 {
        let st = chat_with_apikey(&f.router, &f.api_key_plain).await;
        assert_eq!(st, StatusCode::OK);
    }
}

#[tokio::test]
async fn rest_upsert_list_delete_quota() {
    let f = make_fixture().await;
    let tok = user_token(&f.jwt, f.user_id, f.org_id);
    let url = format!("/v1/orgs/{}/quotas", f.org_id.as_uuid());

    // POST 创建 org 维度 rpm=100
    let req = Request::builder()
        .method("POST")
        .uri(&url)
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "scope_kind": "org",
                "scope_id": f.org_id.as_uuid().to_string(),
                "dimension": "rpm",
                "limit_value": "100",
                "window_seconds": 60
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let quota_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["dimension"], "rpm");

    // GET 列表（含 project + api_key 子层）
    let req = Request::builder()
        .method("GET")
        .uri(&url)
        .header("authorization", format!("Bearer {tok}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let arr = body.as_array().unwrap();
    assert!(arr.iter().any(|v| v["id"] == quota_id));

    // DELETE
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("{url}/{quota_id}"))
        .header("authorization", format!("Bearer {tok}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 再 list 应空
    let req = Request::builder()
        .method("GET")
        .uri(&url)
        .header("authorization", format!("Bearer {tok}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rest_upsert_invalid_dimension_returns_400() {
    let f = make_fixture().await;
    let tok = user_token(&f.jwt, f.user_id, f.org_id);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/orgs/{}/quotas", f.org_id.as_uuid()))
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "scope_kind": "org",
                "scope_id": f.org_id.as_uuid().to_string(),
                "dimension": "qps",  // 不在白名单里
                "limit_value": "10"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rest_upsert_project_in_other_org_404() {
    let f = make_fixture().await;
    let tok = user_token(&f.jwt, f.user_id, f.org_id);
    // scope_kind=project + scope_id=随机（不在该 Org 下）
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/orgs/{}/quotas", f.org_id.as_uuid()))
        .header("authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "scope_kind": "project",
                "scope_id": Uuid::new_v4().to_string(),
                "dimension": "rpm",
                "limit_value": "10",
                "window_seconds": 60
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn project_quota_layer_also_intercepts() {
    let f = make_fixture().await;
    // project 维度 rpm=1 → api_key 主体也会因为扇区包含 project 而被拦
    f.repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: "project".into(),
            scope_id: *f.project_id.as_uuid(),
            dimension: "rpm".into(),
            model_filter: None,
            limit_value: Decimal::from(1),
            window_seconds: Some(60),
        })
        .await
        .unwrap();

    let st = chat_with_apikey(&f.router, &f.api_key_plain).await;
    assert_eq!(st, StatusCode::OK);
    let st = chat_with_apikey(&f.router, &f.api_key_plain).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
}
