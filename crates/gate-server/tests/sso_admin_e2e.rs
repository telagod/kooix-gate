//! Admin SSO provider management e2e tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::identity::PlatformRole;
use gate_crypto::{EnvKms, EnvelopeKms, Sealer, kms::generate_master_key_b64};
use gate_server::build_router;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::{AppState, Repos};
use gate_storage::{InMemoryAuditRepo, InMemoryMembershipRepo, InMemoryUserRepo, UserRepo};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixture {
    router: axum::Router,
    jwt: JwtIssuer,
    admin_id: gate_core::id::UserId,
    user_id: gate_core::id::UserId,
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

    let user_hash = gate_auth::password::hash("user-password-123").unwrap();
    let user = users
        .create("user@example.com", Some(&user_hash), Some("User"), None)
        .await
        .unwrap();

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_user(
        admin.id,
        UserRecord {
            orgs: Default::default(),
            projects: Default::default(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );
    loader.add_user(
        user.id,
        UserRecord {
            orgs: Default::default(),
            projects: Default::default(),
            platform: None,
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
        audit: audit.clone(),
        ..Repos::in_memory()
    };
    let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
    let sealer: EnvelopeKms = Sealer::new(kms);
    let state = AppState::new(jwt.clone(), loader, repos).with_crypto(sealer);
    let router = build_router(state);

    Fixture {
        router,
        jwt,
        admin_id: admin.id,
        user_id: user.id,
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
    method_name: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method_name)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    response_json(router.clone().oneshot(req).await.unwrap()).await
}

async fn empty_req(
    router: &axum::Router,
    method_name: &str,
    uri: &str,
    token: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method_name)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    response_json(router.clone().oneshot(req).await.unwrap()).await
}

async fn get_json(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    response_json(router.clone().oneshot(req).await.unwrap()).await
}

async fn public_get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    response_json(router.clone().oneshot(req).await.unwrap()).await
}

async fn response_json(resp: axum::response::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn wait_audit_actions(audit: &InMemoryAuditRepo, min_actions: usize) -> Vec<String> {
    for _ in 0..20 {
        let actions: Vec<_> = audit.all().into_iter().map(|r| r.action).collect();
        if actions.len() >= min_actions {
            return actions;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    audit.all().into_iter().map(|r| r.action).collect()
}

#[tokio::test]
async fn platform_admin_can_create_list_update_delete_identity_provider_without_secret_echo() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);

    let (status, created) = json_req(
        &f.router,
        "POST",
        "/v1/admin/identity-providers",
        &token,
        json!({
            "name": "Google Workspace",
            "slug": "google-workspace",
            "issuer": "https://accounts.example.com",
            "client_id": "client-abc",
            "client_secret": "test-client-secret-value",
            "scopes": ["openid email", "profile"],
            "email_domain_allowlist": ["Example.COM", "corp.example.com"],
            "auto_create_users": true,
            "auto_join_org_role": "member",
            "enabled": true,
            "redirect_policy": {
                "allow_relative": true,
                "allowed_origins": ["https://console.example.com/app"]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={created}");
    assert_eq!(created["slug"], "google-workspace");
    assert_eq!(created["email_domain_allowlist"][0], "corp.example.com");
    assert_eq!(created["email_domain_allowlist"][1], "example.com");
    assert_eq!(
        created["redirect_policy"]["allowed_origins"][0],
        "https://console.example.com"
    );
    assert!(created.get("client_secret").is_none());
    assert!(created.get("client_secret_enc").is_none());
    let id = created["id"].as_str().unwrap().to_string();

    let (status, providers) = get_json(&f.router, "/v1/admin/identity-providers", &token).await;
    assert_eq!(status, StatusCode::OK, "body={providers}");
    assert_eq!(providers.as_array().unwrap().len(), 1);

    let (status, public_providers) = public_get_json(&f.router, "/v1/auth/sso/providers").await;
    assert_eq!(status, StatusCode::OK, "body={public_providers}");
    assert_eq!(public_providers[0]["slug"], "google-workspace");

    let (status, updated) = json_req(
        &f.router,
        "PUT",
        &format!("/v1/admin/identity-providers/{id}"),
        &token,
        json!({
            "name": "Workspace",
            "enabled": false,
            "redirect_policy": {
                "allow_relative": false,
                "allowed_origins": ["https://admin.example.com"]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={updated}");
    assert_eq!(updated["name"], "Workspace");
    assert_eq!(updated["enabled"], false);
    assert_eq!(updated["redirect_policy"]["allow_relative"], false);

    let (status, body) = empty_req(
        &f.router,
        "DELETE",
        &format!("/v1/admin/identity-providers/{id}"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["deleted"], true);

    let actions = wait_audit_actions(&f.audit, 3).await;
    assert!(actions.contains(&"identity_provider.create".to_string()));
    assert!(actions.contains(&"identity_provider.update".to_string()));
    assert!(actions.contains(&"identity_provider.delete".to_string()));
}

#[tokio::test]
async fn identity_provider_management_requires_platform_admin() {
    let f = fixture().await;
    let token = access_token(&f, f.user_id, false);
    let (status, body) = get_json(&f.router, "/v1/admin/identity-providers", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");
}

#[tokio::test]
async fn oidc_discovery_validates_metadata() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);
    let oidc = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": oidc.uri(),
            "authorization_endpoint": format!("{}/authorize", oidc.uri()),
            "token_endpoint": format!("{}/token", oidc.uri()),
            "jwks_uri": format!("{}/jwks", oidc.uri()),
            "scopes_supported": ["openid", "email", "profile"]
        })))
        .mount(&oidc)
        .await;

    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/admin/identity-providers/discover",
        &token,
        json!({"issuer": oidc.uri()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert_eq!(body["issuer"], oidc.uri());
    assert!(body["scopes_supported"].as_array().unwrap().len() >= 3);
}
