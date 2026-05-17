//! Admin user management e2e tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::identity::PlatformRole;
use gate_server::build_router;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::{AppState, Repos};
use gate_storage::{
    InMemoryApiKeyRepo, InMemoryAuditRepo, InMemoryMembershipRepo, InMemoryUserRepo, UserRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    router: axum::Router,
    jwt: JwtIssuer,
    admin_id: gate_core::id::UserId,
    users: Arc<InMemoryUserRepo>,
    loader: Arc<InMemoryLoader>,
    audit: Arc<InMemoryAuditRepo>,
}

async fn fixture() -> Fixture {
    let users = Arc::new(InMemoryUserRepo::new());
    let memberships = Arc::new(InMemoryMembershipRepo::new());
    let audit = Arc::new(InMemoryAuditRepo::new());

    let admin_hash = gate_auth::password::hash("admin-password-123").unwrap();
    let admin = users
        .create("admin@example.com", Some(&admin_hash), Some("Root"), None)
        .await
        .unwrap();
    memberships.seed_platform(admin.id, PlatformRole::SuperAdmin);

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_user(
        admin.id,
        UserRecord {
            orgs: Default::default(),
            projects: Default::default(),
            platform: Some(PlatformRole::SuperAdmin),
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

    let repos = Repos {
        users: users.clone(),
        memberships: memberships.clone(),
        api_keys: Arc::new(InMemoryApiKeyRepo::new()),
        audit: audit.clone(),
        ..Repos::in_memory()
    };
    let state = AppState::new(jwt.clone(), loader.clone(), repos);
    let router = build_router(state);

    Fixture {
        router,
        jwt,
        admin_id: admin.id,
        users,
        loader,
        audit,
    }
}

fn access_token(f: &Fixture, user_id: gate_core::id::UserId, is_platform_admin: bool) -> String {
    f.jwt
        .issue_access(*user_id.as_uuid(), Uuid::now_v7(), None, is_platform_admin)
        .unwrap()
        .0
}

async fn json_req(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn public_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn get_json(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn platform_admin_can_create_list_suspend_and_reset_password() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);

    let (status, created) = json_req(
        &f.router,
        "POST",
        "/v1/admin/users",
        &token,
        json!({
            "email": "New.User@Example.COM",
            "display_name": " New User ",
            "password": "initial-password-123",
            "status": "active"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={created}");
    assert_eq!(created["email"], "new.user@example.com");
    assert_eq!(created["display_name"], "New User");
    assert!(created["id"].as_str().unwrap().starts_with("usr_"));
    assert!(
        created.get("password").is_none(),
        "password must never be returned"
    );

    let user_id = created["id"].as_str().unwrap();
    let (status, users) = get_json(&f.router, "/v1/admin/users?limit=10&offset=0", &token).await;
    assert_eq!(status, StatusCode::OK, "body={users}");
    assert!(
        users
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["id"] == created["id"])
    );

    let (status, suspended) = json_req(
        &f.router,
        "PUT",
        &format!("/v1/admin/users/{user_id}/status"),
        &token,
        json!({"status": "suspended"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={suspended}");
    assert_eq!(suspended["status"], "suspended");

    let (status, reset) = json_req(
        &f.router,
        "PUT",
        &format!("/v1/admin/users/{user_id}/password"),
        &token,
        json!({"password": "new-password-456"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={reset}");
    assert_eq!(reset["id"], created["id"]);

    let audit_actions = wait_audit_actions(&f.audit).await;
    assert!(audit_actions.contains(&"user.create".to_string()));
    assert!(audit_actions.contains(&"user.update_status".to_string()));
    assert!(audit_actions.contains(&"user.reset_password".to_string()));
}

async fn wait_audit_actions(audit: &InMemoryAuditRepo) -> Vec<String> {
    for _ in 0..20 {
        let actions: Vec<_> = audit.all().into_iter().map(|r| r.action).collect();
        if actions.len() >= 3 {
            return actions;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    audit.all().into_iter().map(|r| r.action).collect()
}

#[tokio::test]
async fn user_management_requires_platform_admin() {
    let f = fixture().await;
    let user_hash = gate_auth::password::hash("member-password-123").unwrap();
    let user = f
        .users
        .create("member@example.com", Some(&user_hash), None, None)
        .await
        .unwrap();

    f.loader.add_user(
        user.id,
        UserRecord {
            orgs: Default::default(),
            projects: Default::default(),
            platform: None,
        },
    );

    let token = access_token(&f, user.id, false);
    let (status, body) = get_json(&f.router, "/v1/admin/users", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
}

#[tokio::test]
async fn cannot_suspend_current_admin_user() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);

    let (status, body) = json_req(
        &f.router,
        "PUT",
        &format!("/v1/admin/users/{}/status", f.admin_id),
        &token,
        json!({"status": "suspended"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn admin_user_validation_rejects_weak_password_and_bad_status() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);

    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/admin/users",
        &token,
        json!({"email": "bad@example.com", "password": "short", "status": "active"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert_eq!(body["error"]["code"], "password_too_weak");

    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/admin/users",
        &token,
        json!({"email": "bad@example.com", "password": "strong-password-123", "status": "deleted"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn refresh_rejects_user_after_status_changes_to_suspended() {
    let f = fixture().await;
    let pw_hash = gate_auth::password::hash("active-password-123").unwrap();
    let user = f
        .users
        .create("active@example.com", Some(&pw_hash), None, None)
        .await
        .unwrap();

    let (status, body) = public_json(
        &f.router,
        "/v1/auth/login",
        json!({"email": "active@example.com", "password": "active-password-123"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    let refresh_token = body["refresh_token"].as_str().unwrap().to_string();

    f.users.update_status(user.id, "suspended").await.unwrap();

    let (status, body) = public_json(
        &f.router,
        "/v1/auth/refresh",
        json!({"refresh_token": refresh_token}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
    assert_eq!(body["error"]["code"], "account_suspended");
}
