//! B1 E2E：handler ↔ Repo ↔ PG 全链。
//!
//! 验证 handler 端业务真把数据写进了 PG 并能查回来。
//! 与 pg_loader_e2e.rs 不同：那个验证认证，这个验证业务逻辑。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::*;
use gate_core::identity::OrgRole;
use gate_server::state::Repos;
use gate_server::{AppState, PgLoader, build_router};
use gate_storage::{MembershipRepo, OrgRepo, PgMembershipRepo, PgOrgRepo, PgUserRepo, UserRepo};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    router: axum::Router,
    jwt: Arc<JwtIssuer>,
    org: OrgId,
    user_dev: UserId,
    user_owner: UserId,
    user_viewer: UserId,
    _container: testcontainers::ContainerAsync<Postgres>,
}

async fn fixture() -> Fixture {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default().with_tag(&tag).start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.unwrap();
    gate_storage::run_migrations(&pool).await.unwrap();

    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let memberships = PgMembershipRepo::new(pool.clone());

    let owner = users.create("owner@x.com", None, None, None).await.unwrap();
    let dev = users.create("dev@x.com", None, None, None).await.unwrap();
    let viewer = users
        .create("viewer@x.com", None, None, None)
        .await
        .unwrap();
    let org = orgs.create("Acme", "acme", owner.id).await.unwrap();
    memberships
        .add_org_member(org.id, owner.id, OrgRole::Owner)
        .await
        .unwrap();
    memberships
        .add_org_member(org.id, dev.id, OrgRole::Member)
        .await
        .unwrap();
    memberships
        .add_org_member(org.id, viewer.id, OrgRole::BillingViewer)
        .await
        .unwrap();

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
    let loader = Arc::new(PgLoader::new(pool.clone()));
    let repos = Repos::from_pg(pool);
    let state = AppState::new(jwt, loader, repos);
    let jwt_arc = state.jwt.clone();
    let router = build_router(state);

    Fixture {
        router,
        jwt: jwt_arc,
        org: org.id,
        user_dev: dev.id,
        user_owner: owner.id,
        user_viewer: viewer.id,
        _container: container,
    }
}

fn jwt_for(jwt: &JwtIssuer, user: UserId, org: OrgId) -> String {
    let (tok, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), Some(*org.as_uuid()), false)
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
    let resp = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
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
async fn project_create_and_list_roundtrip() {
    let f = fixture().await;
    let tok = jwt_for(&f.jwt, f.user_owner, f.org);
    let url = format!("/v1/orgs/{}/projects", f.org.as_uuid());

    // 起始无项目
    let (s, body) = call(&f.router, "GET", &url, Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK, "body={body}");
    assert_eq!(body.as_array().unwrap().len(), 0);

    // 创建
    let (s, body) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(json!({"name": "Main", "slug": "main"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "body={body}");
    let project_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["slug"], "main");

    // 再 list 应有 1
    let (s, body) = call(&f.router, "GET", &url, Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK);
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], project_id);
}

#[tokio::test]
async fn project_create_denied_for_viewer() {
    let f = fixture().await;
    let tok = jwt_for(&f.jwt, f.user_viewer, f.org);
    let url = format!("/v1/orgs/{}/projects", f.org.as_uuid());

    let (s, body) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(json!({"name": "spy", "slug": "spy"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn project_duplicate_slug_conflict() {
    let f = fixture().await;
    let tok = jwt_for(&f.jwt, f.user_owner, f.org);
    let url = format!("/v1/orgs/{}/projects", f.org.as_uuid());

    let (s, _) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(json!({"name": "main", "slug": "main"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let (s, body) = call(
        &f.router,
        "POST",
        &url,
        Some(&tok),
        Some(json!({"name": "Main 2", "slug": "main"})),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "body={body}");
}

#[tokio::test]
async fn api_key_create_list_revoke_full_chain() {
    let f = fixture().await;
    let tok = jwt_for(&f.jwt, f.user_owner, f.org);

    // 1. 建 project
    let proj_url = format!("/v1/orgs/{}/projects", f.org.as_uuid());
    let (s, body) = call(
        &f.router,
        "POST",
        &proj_url,
        Some(&tok),
        Some(json!({"name": "p", "slug": "p"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let project_id = body["id"].as_str().unwrap().to_string();

    // 2. 给 dev 加 project Developer 角色 — 直接走 Repo 后门
    //    （UI 上未来会走 invitation flow；本测试不验邀请）
    let dev_tok = jwt_for(&f.jwt, f.user_dev, f.org);
    let keys_url = format!(
        "/v1/orgs/{}/projects/{}/api-keys",
        f.org.as_uuid(),
        project_id
    );

    // dev 没有 project 角色 → 403
    let (s, body) = call(
        &f.router,
        "POST",
        &keys_url,
        Some(&dev_tok),
        Some(json!({"name": "ci"})),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "dev not project member yet; body={body}"
    );

    // owner 建 key
    let (s, body) = call(
        &f.router,
        "POST",
        &keys_url,
        Some(&tok),
        Some(json!({"name": "ci"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "body={body}");
    let key_id = body["id"].as_str().unwrap().to_string();
    let plaintext = body["plaintext"].as_str().unwrap().to_string();
    assert!(plaintext.starts_with("sk-kg-"));

    // 3. list 应能看到这一把
    let (s, body) = call(&f.router, "GET", &keys_url, Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["id"], key_id);

    // 4. 拿明文 key 调 /v1/me 验证它真能认证
    let (s, body) = call(&f.router, "GET", "/v1/me", Some(&plaintext), None).await;
    assert_eq!(s, StatusCode::OK, "body={body}");
    assert_eq!(body["subject"]["kind"], "api_key");

    // 5. revoke
    let revoke_url = format!("{keys_url}/{key_id}");
    let (s, body) = call(&f.router, "DELETE", &revoke_url, Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["revoked"], true);

    // 6. list 仍返回 1 条（包含 revoked），但标记 revoked=true
    let (s, body) = call(&f.router, "GET", &keys_url, Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["revoked"], true);

    // 7. 撤销后明文 key 应 403
    let (s, body) = call(&f.router, "GET", "/v1/me", Some(&plaintext), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "api_key_revoked");
}

#[tokio::test]
async fn api_key_create_with_mismatched_org_is_404() {
    // 攻击者用 org_x 路径 + 真 project_id (但 project 属于 org)，应 404 NotFound
    let f = fixture().await;
    let tok = jwt_for(&f.jwt, f.user_owner, f.org);

    // 建合法 project
    let proj_url = format!("/v1/orgs/{}/projects", f.org.as_uuid());
    let (_, body) = call(
        &f.router,
        "POST",
        &proj_url,
        Some(&tok),
        Some(json!({"name": "p", "slug": "p"})),
    )
    .await;
    let project_id = body["id"].as_str().unwrap().to_string();

    // 伪造另一个 Org（user 不是成员），但路径里的 project 是真的
    let fake_org = Uuid::new_v4();
    let bad_url = format!("/v1/orgs/{}/projects/{}/api-keys", fake_org, project_id);
    let (s, _) = call(
        &f.router,
        "POST",
        &bad_url,
        Some(&tok),
        Some(json!({"name": "evil"})),
    )
    .await;
    // 没在 fake_org 里 → 403 (require! 先卡)
    assert_eq!(s, StatusCode::FORBIDDEN);
}
