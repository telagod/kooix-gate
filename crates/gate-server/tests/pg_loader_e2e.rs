//! PgLoader 端到端测试：真容器 + 真路由 + 真 JWT。
//!
//! 这是 InMemoryLoader 行为契约的"镜像测试"——只要这里绿，
//! 就说明换 PgLoader 后 auth_flow.rs 里覆盖的所有场景都成立。
//!
//! 覆盖：
//! - 真用户走 /v1/me
//! - 真 API key 走 /v1/me
//! - 撤销后的 API key → 403
//! - Org 隔离（user 跨 Org 查 projects → 403）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::*;
use gate_core::identity::{OrgRole, ProjectRole};
use gate_server::{build_router, AppState, PgLoader};
use gate_storage::{
    ApiKeyRepo, MembershipRepo, OrgRepo, PgApiKeyRepo, PgMembershipRepo, PgOrgRepo,
    PgProjectRepo, PgUserRepo, ProjectRepo, UserRepo,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    router: axum::Router,
    jwt: Arc<JwtIssuer>,
    org: OrgId,
    other_org: OrgId,
    user: UserId,
    api_key_plain: String,
    api_key_revoked: String,
    _container: testcontainers::ContainerAsync<Postgres>,
}

async fn fixture() -> Fixture {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.unwrap();
    gate_storage::run_migrations(&pool).await.unwrap();

    // 用 Repo 直接捏初始数据
    let users = PgUserRepo::new(pool.clone());
    let orgs = PgOrgRepo::new(pool.clone());
    let projects = PgProjectRepo::new(pool.clone());
    let memberships = PgMembershipRepo::new(pool.clone());
    let api_keys = PgApiKeyRepo::new(pool.clone());

    let user = users.create("dev@x.com", None, Some("Dev")).await.unwrap();
    let owner = users.create("owner@x.com", None, None).await.unwrap();
    let org = orgs.create("Acme", "acme", owner.id).await.unwrap();
    let other_org = orgs.create("Beta", "beta", owner.id).await.unwrap();
    let proj = projects.create(org.id, "main", "main").await.unwrap();

    memberships
        .add_org_member(org.id, user.id, OrgRole::Member)
        .await
        .unwrap();
    memberships
        .add_project_member(proj.id, user.id, ProjectRole::Developer)
        .await
        .unwrap();

    // 有效 + 撤销两把 key
    let k1 = gate_auth::api_key::generate();
    let h1 = gate_auth::api_key::hash(&k1.plaintext);
    api_keys
        .create(proj.id, "ci", &h1, &k1.prefix, &k1.last4, owner.id, &[])
        .await
        .unwrap();

    let k2 = gate_auth::api_key::generate();
    let h2 = gate_auth::api_key::hash(&k2.plaintext);
    let id2 = api_keys
        .create(proj.id, "old", &h2, &k2.prefix, &k2.last4, owner.id, &[])
        .await
        .unwrap();
    api_keys.revoke(id2, owner.id, Some("rotated")).await.unwrap();

    // 装路由
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
    let loader = Arc::new(PgLoader::new(pool));
    let state = AppState::new(jwt, loader);
    let jwt_arc = state.jwt.clone();
    let router = build_router(state);

    Fixture {
        router,
        jwt: jwt_arc,
        org: org.id,
        other_org: other_org.id,
        user: user.id,
        api_key_plain: k1.plaintext.to_string(),
        api_key_revoked: k2.plaintext.to_string(),
        _container: container,
    }
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    auth: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = auth {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let resp = router.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn me_works_with_pg_loader() {
    let f = fixture().await;
    let (tok, _) = f
        .jwt
        .issue_access(*f.user.as_uuid(), Uuid::now_v7(), Some(*f.org.as_uuid()), false)
        .unwrap();
    let (status, body) = call(&f.router, "GET", "/v1/me", Some(&tok)).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["subject"]["kind"], "user");
    assert_eq!(body["current_org"], f.org.to_string());
}

#[tokio::test]
async fn me_works_with_api_key_through_pg() {
    let f = fixture().await;
    let (status, body) = call(&f.router, "GET", "/v1/me", Some(&f.api_key_plain)).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["subject"]["kind"], "api_key");
}

#[tokio::test]
async fn revoked_api_key_rejected_by_pg_loader() {
    let f = fixture().await;
    let (status, body) = call(&f.router, "GET", "/v1/me", Some(&f.api_key_revoked)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "api_key_revoked");
}

#[tokio::test]
async fn cross_org_blocked_by_pg_loader() {
    let f = fixture().await;
    let (tok, _) = f
        .jwt
        .issue_access(*f.user.as_uuid(), Uuid::now_v7(), Some(*f.org.as_uuid()), false)
        .unwrap();
    let url = format!("/v1/orgs/{}/projects", f.other_org.as_uuid());
    let (status, _) = call(&f.router, "GET", &url, Some(&tok)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
