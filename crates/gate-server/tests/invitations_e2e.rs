//! Enterprise invitation flow e2e tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::{OrgId, ProjectId};
use gate_core::identity::{OrgRole, OrgStatus, Organization, PlatformRole, Project, ProjectStatus};
use gate_server::build_router;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::{AppState, Repos};
use gate_storage::{
    InMemoryAuditRepo, InMemoryInvitationRepo, InMemoryMembershipRepo, InMemoryOrgRepo,
    InMemoryProjectRepo, InMemoryUserRepo, InvitationRecord, MembershipRepo, UserRepo,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    router: axum::Router,
    jwt: JwtIssuer,
    admin_id: gate_core::id::UserId,
    org_owner_id: gate_core::id::UserId,
    no_access_id: gate_core::id::UserId,
    org_id: OrgId,
    project_id: ProjectId,
    users: Arc<InMemoryUserRepo>,
    memberships: Arc<InMemoryMembershipRepo>,
    invitations: Arc<InMemoryInvitationRepo>,
    audit: Arc<InMemoryAuditRepo>,
}

async fn fixture() -> Fixture {
    let users = Arc::new(InMemoryUserRepo::new());
    let memberships = Arc::new(InMemoryMembershipRepo::new());
    let orgs = Arc::new(InMemoryOrgRepo::new());
    let projects = Arc::new(InMemoryProjectRepo::new());
    let invitations = Arc::new(InMemoryInvitationRepo::new());
    let audit = Arc::new(InMemoryAuditRepo::new());

    let admin = users
        .create(
            "admin@example.com",
            Some(&gate_auth::password::hash("admin-password-123").unwrap()),
            Some("Root"),
            None,
        )
        .await
        .unwrap();
    let owner = users
        .create(
            "owner@example.com",
            Some(&gate_auth::password::hash("owner-password-123").unwrap()),
            Some("Owner"),
            None,
        )
        .await
        .unwrap();
    let outsider = users
        .create(
            "outsider@example.com",
            Some(&gate_auth::password::hash("outsider-password-123").unwrap()),
            Some("Outsider"),
            None,
        )
        .await
        .unwrap();

    memberships.seed_platform(admin.id, PlatformRole::SuperAdmin);
    let org_id = OrgId::new();
    let project_id = ProjectId::new();
    let now = chrono::Utc::now();
    orgs.seed(Organization {
        id: org_id,
        name: "Acme".into(),
        slug: "acme".into(),
        owner_user_id: owner.id,
        status: OrgStatus::Active,
        billing_email: None,
        created_at: now,
        updated_at: now,
    });
    projects.seed(Project {
        id: project_id,
        org_id,
        name: "Main".into(),
        slug: "main".into(),
        status: ProjectStatus::Active,
        default_group_id: None,
        created_at: now,
        updated_at: now,
    });
    memberships.seed_org(org_id, owner.id, OrgRole::Owner);

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_user(
        admin.id,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );
    loader.add_user(
        owner.id,
        UserRecord {
            orgs: HashMap::from([(org_id, OrgRole::Owner)]),
            projects: HashMap::new(),
            platform: None,
        },
    );
    loader.add_user(
        outsider.id,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
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
        orgs: orgs.clone(),
        projects: projects.clone(),
        memberships: memberships.clone(),
        invitations: invitations.clone(),
        audit: audit.clone(),
        ..Repos::in_memory()
    };
    let state =
        AppState::new(jwt.clone(), loader, repos).with_public_origin("https://console.example.com");
    let router = build_router(state);

    Fixture {
        router,
        jwt,
        admin_id: admin.id,
        org_owner_id: owner.id,
        no_access_id: outsider.id,
        org_id,
        project_id,
        users,
        memberships,
        invitations,
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
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method_name)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let req = builder
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

#[tokio::test]
async fn org_invitation_create_preview_accept_and_reuse_rejected() {
    let f = fixture().await;
    let token = access_token(&f, f.org_owner_id, false);
    let (status, created) = json_req(
        &f.router,
        "POST",
        &format!("/v1/admin/orgs/{}/invitations", f.org_id),
        Some(&token),
        json!({"email": "Invitee@Example.COM", "role": "member", "ttl_hours": 24}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={created}");
    assert_eq!(created["email"], "invitee@example.com");
    assert_eq!(created["status"], "pending");
    assert!(created["token"].as_str().unwrap().starts_with("kg_inv_"));
    assert!(
        created["accept_url"]
            .as_str()
            .unwrap()
            .starts_with("https://console.example.com/invite/accept?token=kg_inv_")
    );
    let invite_token = created["token"].as_str().unwrap().to_string();

    let (status, preview) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/preview",
        None,
        json!({"token": invite_token}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={preview}");
    assert_eq!(preview["scope_kind"], "org");
    assert_eq!(preview["status"], "pending");

    let invite_token = created["token"].as_str().unwrap().to_string();
    let (status, accepted) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/accept",
        None,
        json!({
            "token": invite_token,
            "email": "invitee@example.com",
            "display_name": "Invitee",
            "password": "invitee-password-123"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={accepted}");
    assert_eq!(accepted["role"], "member");

    let user = f.users.find_by_email("invitee@example.com").await.unwrap();
    let memberships = f.memberships.load_for_user(user.id).await.unwrap();
    assert_eq!(memberships.orgs.get(&f.org_id), Some(&OrgRole::Member));

    let invite_token = created["token"].as_str().unwrap().to_string();
    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/accept",
        None,
        json!({"token": invite_token, "email": "invitee@example.com"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");

    let actions = wait_audit_actions(&f.audit, 1).await;
    assert!(actions.contains(&"invitation.create".to_string()));
}

#[tokio::test]
async fn project_invitation_can_be_revoked_and_accept_then_fails() {
    let f = fixture().await;
    let token = access_token(&f, f.admin_id, true);
    let (status, created) = json_req(
        &f.router,
        "POST",
        &format!(
            "/v1/admin/orgs/{}/projects/{}/invitations",
            f.org_id, f.project_id
        ),
        Some(&token),
        json!({"email": "dev@example.com", "role": "developer", "ttl_hours": 24}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={created}");
    let id = created["id"].as_str().unwrap().to_string();
    let invite_token = created["token"].as_str().unwrap().to_string();

    let (status, list) = get_json(
        &f.router,
        &format!(
            "/v1/admin/orgs/{}/projects/{}/invitations?include_inactive=true",
            f.org_id, f.project_id
        ),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={list}");
    assert_eq!(list.as_array().unwrap().len(), 1);

    let (status, revoked) = empty_req(
        &f.router,
        "DELETE",
        &format!(
            "/v1/admin/orgs/{}/projects/{}/invitations/{id}",
            f.org_id, f.project_id
        ),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={revoked}");
    assert_eq!(revoked["status"], "revoked");

    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/accept",
        None,
        json!({
            "token": invite_token,
            "email": "dev@example.com",
            "password": "developer-password-123"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");

    let actions = wait_audit_actions(&f.audit, 2).await;
    assert!(actions.contains(&"invitation.create".to_string()));
    assert!(actions.contains(&"invitation.revoke".to_string()));
}

#[tokio::test]
async fn project_invitation_accept_adds_project_membership() {
    let f = fixture().await;
    let token = access_token(&f, f.org_owner_id, false);
    let (status, created) = json_req(
        &f.router,
        "POST",
        &format!(
            "/v1/admin/orgs/{}/projects/{}/invitations",
            f.org_id, f.project_id
        ),
        Some(&token),
        json!({"email": "project-dev@example.com", "role": "developer", "ttl_hours": 24}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={created}");

    let invite_token = created["token"].as_str().unwrap().to_string();
    let (status, accepted) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/accept",
        None,
        json!({
            "token": invite_token,
            "email": "project-dev@example.com",
            "password": "project-dev-password-123"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={accepted}");
    assert_eq!(accepted["scope_kind"], "project");

    let user = f
        .users
        .find_by_email("project-dev@example.com")
        .await
        .unwrap();
    let memberships = f.memberships.load_for_user(user.id).await.unwrap();
    assert_eq!(
        memberships.projects.get(&(f.org_id, f.project_id)),
        Some(&gate_core::identity::ProjectRole::Developer)
    );
}

#[tokio::test]
async fn invitation_permission_and_expiration_are_enforced() {
    let f = fixture().await;
    let outsider_token = access_token(&f, f.no_access_id, false);
    let (status, body) = json_req(
        &f.router,
        "POST",
        &format!("/v1/admin/orgs/{}/invitations", f.org_id),
        Some(&outsider_token),
        json!({"email": "blocked@example.com", "role": "member"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={body}");

    let admin_token = access_token(&f, f.admin_id, true);
    let (status, expired) = json_req(
        &f.router,
        "POST",
        &format!("/v1/admin/orgs/{}/invitations", f.org_id),
        Some(&admin_token),
        json!({"email": "late@example.com", "role": "member", "ttl_hours": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body={expired}");
    assert_eq!(expired["status"], "pending", "ttl is clamped to 1h");

    // Direct storage seed models a truly expired row and proves accept rejects it.
    let expired_token = "kg_inv_expiredexpiredexpiredexpiredexpiredexpired";
    f.memberships.seed_org(f.org_id, f.admin_id, OrgRole::Owner);
    f.users
        .create(
            "late@example.com",
            Some(&gate_auth::password::hash("late-password-123").unwrap()),
            None,
            None,
        )
        .await
        .unwrap();
    let expired_id = Uuid::now_v7();
    let invitation = InvitationRecord {
        id: expired_id,
        scope_kind: "org".into(),
        scope_id: *f.org_id.as_uuid(),
        email: "late@example.com".into(),
        role: "member".into(),
        token_hash: token_hash(expired_token),
        invited_by: f.admin_id,
        expires_at: chrono::Utc::now() - ChronoDuration::minutes(1),
        accepted_at: None,
        accepted_by: None,
        revoked_at: None,
        created_at: chrono::Utc::now() - ChronoDuration::hours(2),
    };
    f.invitations.seed(invitation);

    let (status, body) = json_req(
        &f.router,
        "POST",
        "/v1/invitations/accept",
        None,
        json!({"token": expired_token, "email": "late@example.com"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
}

fn token_hash(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

async fn wait_audit_actions(audit: &InMemoryAuditRepo, min_actions: usize) -> Vec<String> {
    for _ in 0..30 {
        let actions: Vec<_> = audit.all().into_iter().map(|r| r.action).collect();
        if actions.len() >= min_actions {
            return actions;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    audit.all().into_iter().map(|r| r.action).collect()
}
