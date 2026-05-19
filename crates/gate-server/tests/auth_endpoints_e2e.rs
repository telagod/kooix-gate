//! Auth 端点 PG 集成测试
//!
//! 覆盖：
//! - happy path login → 拿到 token → 用 access token 调 /v1/me 通
//! - 错密码 → 401 invalid_credentials
//! - 连续错密码 6 次 → 423 too_many_failures（第 ≥5 次起）
//! - refresh → 拿到新 access token
//! - 错 refresh token → 401 token_invalid

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::state::Repos;
use gate_server::{AppState, PgLoader, build_router};
use gate_storage::{PgUserRepo, UserRepo};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

// ──────────────────────────────────────────────
// Fixture
// ──────────────────────────────────────────────

struct Fixture {
    router: axum::Router,
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

    // Seed：一个带密码的用户
    let users = PgUserRepo::new(pool.clone());
    let pw_hash = gate_auth::password::hash("correct-password-123").unwrap();
    let alice = users
        .create("alice@example.com", Some(&pw_hash), Some("Alice"), None)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO platform_admins (user_id, role, granted_by) VALUES ($1, 'super_admin', $1)",
    )
    .bind(alice.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let suspended_hash = gate_auth::password::hash("suspended-password-123").unwrap();
    users
        .create(
            "suspended@example.com",
            Some(&suspended_hash),
            Some("Suspended"),
            Some("suspended"),
        )
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
    let router = build_router(state);

    Fixture {
        router,
        _container: container,
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

async fn request_json(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let body = if let Some(body) = body {
        req = req.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&body).unwrap())
    } else {
        Body::empty()
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

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request_json(router, "POST", uri, None, Some(body)).await
}

async fn post_authed(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_json(router, "POST", uri, Some(token), None).await
}

async fn get_authed(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_json(router, "GET", uri, Some(token), None).await
}

async fn delete_authed(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_json(router, "DELETE", uri, Some(token), None).await
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[tokio::test]
async fn login_happy_path_and_me() {
    let f = fixture().await;

    // 登录成功
    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let access_token = body["access_token"].as_str().unwrap().to_string();
    assert!(!access_token.is_empty());
    assert!(!body["refresh_token"].as_str().unwrap().is_empty());
    assert_eq!(body["user"]["email"], "alice@example.com");

    // 用 access token 调 /v1/me
    let (me_status, me_body) = get_authed(&f.router, "/v1/me", &access_token).await;
    assert_eq!(me_status, StatusCode::OK, "me body={me_body}");
    assert_eq!(me_body["subject"]["kind"], "user");
}

#[tokio::test]
async fn login_wrong_password_returns_401() {
    let f = fixture().await;

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "wrong-password-!!!"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "invalid_credentials");
}

#[tokio::test]
async fn login_suspended_user_returns_403() {
    let f = fixture().await;

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "suspended@example.com", "password": "suspended-password-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "account_suspended");
}

#[tokio::test]
async fn login_unknown_email_returns_401() {
    let f = fixture().await;

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "nobody@example.com", "password": "whatever"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "invalid_credentials");
}

#[tokio::test]
async fn login_too_many_failures_returns_423() {
    let f = fixture().await;

    // 5 次错密码，第 5 次起应返 429（too_many_failures 映射 HTTP 429）
    // 注意：bump_failed_login 返回次数，>= 5 时触发
    let mut last_status = StatusCode::UNAUTHORIZED;
    let mut last_code = String::new();
    for _ in 0..6 {
        let (status, body) = post_json(
            &f.router,
            "/v1/auth/login",
            json!({"email": "alice@example.com", "password": "wrong!"}),
        )
        .await;
        last_status = status;
        last_code = body["error"]["code"].as_str().unwrap_or("").to_string();
    }
    // 第 6 次（failed_logins >= 5）→ too_many_failures → HTTP 429
    assert_eq!(
        last_status,
        StatusCode::TOO_MANY_REQUESTS,
        "last code={last_code}"
    );
    assert_eq!(last_code, "too_many_failures");
}

#[tokio::test]
async fn refresh_returns_new_access_token() {
    let f = fixture().await;

    // 先登录拿 refresh token
    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let refresh_token = body["refresh_token"].as_str().unwrap().to_string();

    // 用 refresh token 换新 access token
    let (status, body) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": refresh_token}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let new_access = body["access_token"].as_str().unwrap();
    assert!(!new_access.is_empty());

    // 新 access token 可以调 /v1/me
    let (me_status, _) = get_authed(&f.router, "/v1/me", new_access).await;
    assert_eq!(me_status, StatusCode::OK);
}

#[tokio::test]
async fn refresh_rotates_token_and_rejects_replay() {
    let f = fixture().await;

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let first_refresh = body["refresh_token"].as_str().unwrap().to_string();

    let (status, refreshed) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": first_refresh}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={refreshed}");
    let second_refresh = refreshed["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(second_refresh, body["refresh_token"]);

    let (status, replay) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": body["refresh_token"]}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={replay}");
    assert_eq!(replay["error"]["code"], "token_invalid");

    let (status, third) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": second_refresh}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={third}");
    assert!(!third["access_token"].as_str().unwrap().is_empty());
    assert!(!third["refresh_token"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn logout_revokes_refresh_session() {
    let f = fixture().await;

    let (_, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    let access_token = body["access_token"].as_str().unwrap().to_string();
    let refresh_token = body["refresh_token"].as_str().unwrap().to_string();

    let (status, body) = post_authed(&f.router, "/v1/auth/logout", &access_token).await;
    assert_eq!(status, StatusCode::OK, "body={body}");

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": refresh_token}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "token_invalid");
}

#[tokio::test]
async fn refresh_with_invalid_token_returns_401() {
    let f = fixture().await;

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": "not.a.valid.jwt.token"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "token_invalid");
}

#[tokio::test]
async fn logout_requires_auth() {
    let f = fixture().await;

    // 无 token 调 logout → 401
    let req = Request::builder()
        .method("POST")
        .uri("/v1/auth/logout")
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_with_valid_token_returns_ok() {
    let f = fixture().await;

    // 先登录
    let (_, body) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    let access_token = body["access_token"].as_str().unwrap().to_string();

    // logout
    let req = Request::builder()
        .method("POST")
        .uri("/v1/auth/logout")
        .header("authorization", format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let b: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(b["ok"], true);
}

#[tokio::test]
async fn platform_admin_can_list_and_revoke_user_sessions() {
    let f = fixture().await;

    let (_, alice_login) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    let alice_user_id = alice_login["user"]["id"].as_str().unwrap().to_string();
    let alice_refresh = alice_login["refresh_token"].as_str().unwrap().to_string();

    let (_, admin_login) = post_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "alice@example.com", "password": "correct-password-123"}),
    )
    .await;
    let admin_access = admin_login["access_token"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &f.router,
        &format!("/v1/admin/users/{alice_user_id}/sessions"),
        &admin_access,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let sessions = body.as_array().unwrap();
    assert!(sessions.len() >= 2, "body={body}");
    let target_session = sessions
        .iter()
        .find(|s| s["current"] == false)
        .and_then(|s| s["id"].as_str())
        .unwrap()
        .to_string();

    let (status, body) = delete_authed(
        &f.router,
        &format!("/v1/admin/users/{alice_user_id}/sessions/{target_session}"),
        &admin_access,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["revoked"], 1);

    let (status, body) = post_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": alice_refresh}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={body}");
    assert_eq!(body["error"]["code"], "token_invalid");

    let (status, body) = delete_authed(
        &f.router,
        &format!("/v1/admin/users/{alice_user_id}/sessions"),
        &admin_access,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body["revoked"].as_u64().unwrap() >= 1);
}
