//! /v1/admin/{audit-logs,orgs,users} — User / Org / Audit log 管理。
//!
//! 0.4.128：从 admin/mod.rs 物理拆出（11 handler + 多 helper + OrgView/UserView 类型，~520 行）。
//! 包含 audit log 列表、org CRUD、user CRUD + 会话管理。

#[allow(unused_imports)]
use super::shared::{
    audit_meta, channel_audit_snapshot, channel_capabilities, channel_inflight,
    group_audit_snapshot, is_plugin_provider, key_audit_snapshot, key_fingerprint,
    pricing_rule_audit_snapshot, record_to_summary, require_confirmation, user_audit_snapshot,
    validate_channel_key_alias,
};
use super::*;

// ============================================================================
// Audit Logs
// ============================================================================

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub org_id: Option<crate::flex_uuid::FlexUuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_audit_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_audit_sort_dir")]
    pub sort_dir: String,
}

pub(super) fn default_limit() -> i64 {
    50
}

fn default_audit_sort_by() -> String {
    "ts".into()
}

fn default_audit_sort_dir() -> String {
    "desc".into()
}

fn parse_audit_sort_by(value: &str) -> AuditSortBy {
    match value {
        "actor_kind" => AuditSortBy::ActorKind,
        "action" => AuditSortBy::Action,
        "resource_kind" => AuditSortBy::ResourceKind,
        "outcome" => AuditSortBy::Outcome,
        _ => AuditSortBy::Ts,
    }
}

fn parse_sort_dir(value: &str, default: SortDirection) -> SortDirection {
    if value.eq_ignore_ascii_case("asc") {
        SortDirection::Asc
    } else if value.eq_ignore_ascii_case("desc") {
        SortDirection::Desc
    } else {
        default
    }
}

#[derive(Serialize)]
pub struct AuditLogView {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub actor_ip: Option<String>,
    pub actor_user_agent: Option<String>,
    pub request_id: Option<String>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<String>,
    pub org_id: Option<String>,
    pub project_id: Option<String>,
    pub outcome: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub error_message: Option<String>,
}

pub(super) async fn list_audit_logs(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<AuditLogQuery>,
) -> AppResult<Json<Vec<AuditLogView>>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let sort_by = parse_audit_sort_by(&q.sort_by);
    let sort_dir = parse_sort_dir(&q.sort_dir, SortDirection::Desc);

    let records = if let Some(org_id) = q.org_id {
        app.repos
            .audit
            .list_by_org_sorted(Uuid::from(org_id), limit, offset, sort_by, sort_dir)
            .await?
    } else {
        // No org filter — platform admin sees all (via org_id=nil trick won't work;
        // for now require org_id)
        return Err(AppError::BadRequest("org_id query param required".into()));
    };

    Ok(Json(
        records
            .into_iter()
            .map(|r| AuditLogView {
                id: r.id.to_string(),
                ts: r.ts,
                actor_kind: r.actor_kind,
                actor_id: r.actor_id.map(|u| u.to_string()),
                actor_ip: r.actor_ip,
                actor_user_agent: r.actor_user_agent,
                request_id: r.request_id.map(|u| u.to_string()),
                action: r.action,
                resource_kind: r.resource_kind,
                resource_id: r.resource_id.map(|u| u.to_string()),
                org_id: r.org_id.map(|u| u.to_string()),
                project_id: r.project_id.map(|u| u.to_string()),
                outcome: r.outcome,
                before: r.before,
                after: r.after,
                error_message: r.error_message,
            })
            .collect(),
    ))
}

// ============================================================================
// Org Management (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct OrgView {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub owner_user_id: String,
    pub status: String,
    pub billing_email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Deserialize)]
pub struct UpdateOrgRequest {
    pub name: Option<String>,
    pub billing_email: Option<String>,
}

pub(super) async fn list_all_orgs(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<OrgView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let orgs = app.repos.orgs.list_all().await?;
    Ok(Json(orgs.into_iter().map(org_to_view).collect()))
}

pub(super) async fn create_org(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateOrgRequest>,
) -> AppResult<Json<OrgView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let name = req.name.trim();
    let slug = req.slug.trim();
    if name.is_empty() || slug.is_empty() {
        return Err(AppError::BadRequest("name and slug required".into()));
    }

    let owner_id = match ctx.subject().unwrap() {
        gate_auth::Subject::User { user_id, .. } => user_id,
        _ => return Err(AppError::Forbidden("only user subjects".into())),
    };

    let org = app.repos.orgs.create(name, slug, *owner_id).await?;

    app.repos
        .memberships
        .add_org_member(org.id, *owner_id, gate_core::identity::OrgRole::Owner)
        .await?;

    app.audit.emit(
        &ctx,
        "org.create",
        "org",
        Some(*org.id.as_uuid()),
        Some(serde_json::json!({"slug": &org.slug})),
    );

    Ok(Json(org_to_view(org)))
}

pub(super) async fn update_org(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateOrgRequest>,
) -> AppResult<Json<OrgView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org_id = gate_core::id::OrgId::from(id.0);
    let org = app
        .repos
        .orgs
        .update(org_id, req.name.as_deref(), req.billing_email.as_deref())
        .await?;

    app.audit.emit(&ctx, "org.update", "org", Some(*id), None);

    Ok(Json(org_to_view(org)))
}

fn org_to_view(o: gate_core::identity::Organization) -> OrgView {
    OrgView {
        id: o.id.to_string(),
        name: o.name,
        slug: o.slug,
        owner_user_id: o.owner_user_id.to_string(),
        status: format!("{:?}", o.status).to_lowercase(),
        billing_email: o.billing_email,
        created_at: o.created_at,
        updated_at: o.updated_at,
    }
}

// ============================================================================
// User Management (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub status: String,
    pub mfa_enabled: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct UsersQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Serialize)]
pub struct UserSessionView {
    pub id: String,
    pub user_id: String,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}

#[derive(Serialize)]
pub struct RevokeSessionsResponse {
    pub revoked: u64,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub password: String,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateStatusRequest {
    pub status: String,
}

#[derive(Deserialize)]
pub struct ResetUserPasswordRequest {
    pub password: String,
}

fn normalize_user_email(email: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || email.len() > 320 || !email.contains('@') {
        return Err(AppError::BadRequest("valid email is required".into()));
    }
    Ok(email)
}

fn normalize_display_name(display_name: Option<String>) -> Option<String> {
    display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn validate_admin_user_status(status: &str) -> AppResult<()> {
    let valid = ["active", "suspended", "pending_verification"];
    if !valid.contains(&status) {
        return Err(AppError::BadRequest(format!(
            "status must be one of: {valid:?}"
        )));
    }
    Ok(())
}

fn current_user_id(ctx: &gate_auth::AuthContext) -> AppResult<UserId> {
    ctx.user_id()
        .ok_or_else(|| AppError::Forbidden("user subject required".into()))
}

pub(super) async fn list_users(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<UsersQuery>,
) -> AppResult<Json<Vec<UserView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let users = app.repos.users.list_all(limit, offset).await?;
    Ok(Json(users.into_iter().map(user_to_view).collect()))
}

pub(super) async fn create_user(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let email = normalize_user_email(&req.email)?;
    let display_name = normalize_display_name(req.display_name);
    let status = req.status.unwrap_or_else(|| "active".into());
    validate_admin_user_status(&status)?;

    let password_hash = gate_auth::password::hash(&req.password)?;
    let user = app
        .repos
        .users
        .create(
            &email,
            Some(&password_hash),
            display_name.as_deref(),
            Some(&status),
        )
        .await?;

    app.audit.emit(
        &ctx,
        "user.create",
        "user",
        Some(*user.id.as_uuid()),
        Some(serde_json::json!({"email": &user.email, "status": status})),
    );

    Ok(Json(user_to_view(user)))
}

pub(super) async fn update_user_status(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpdateStatusRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    validate_admin_user_status(&req.status)?;

    let user_id = UserId::from(id.0);
    if user_id == current_user_id(&ctx)? && req.status != "active" {
        return Err(AppError::BadRequest(
            "cannot suspend or deactivate the current admin user".into(),
        ));
    }

    let before = app.repos.users.find_by_id(user_id).await?;
    if before.status != gate_core::identity::UserStatus::Suspended && req.status == "suspended" {
        require_confirmation(&headers, format!("suspend:{}", before.email))?;
    }
    let user = app.repos.users.update_status(user_id, &req.status).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "user.update_status",
        resource_kind: "user",
        resource_id: Some(*id),
        before: Some(user_audit_snapshot(&before)),
        after: Some(user_audit_snapshot(&user)),
    });

    Ok(Json(user_to_view(user)))
}

pub(super) async fn reset_user_password(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<ResetUserPasswordRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let password_hash = gate_auth::password::hash(&req.password)?;
    let user_id = UserId::from(id.0);
    let user = app
        .repos
        .users
        .reset_password(user_id, &password_hash)
        .await?;

    app.audit.emit(
        &ctx,
        "user.reset_password",
        "user",
        Some(*id),
        Some(serde_json::json!({"email": &user.email})),
    );

    Ok(Json(user_to_view(user)))
}

pub(super) async fn list_user_sessions(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<UserSessionView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    app.repos.users.find_by_id(user_id).await?;
    let current_session = ctx.session_id();
    let sessions = app.repos.sessions.list_active_for_user(user_id).await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|session| UserSessionView {
                id: session.id.to_string(),
                user_id: session.user_id.to_string(),
                user_agent: session.user_agent,
                ip: session.ip.map(|ip| ip.to_string()),
                created_at: session.created_at,
                last_used_at: session.last_used_at,
                expires_at: session.expires_at,
                current: current_session == Some(session.id),
            })
            .collect(),
    ))
}

pub(super) async fn revoke_user_session(
    State(app): State<AppState>,
    Path((id, session_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<RevokeSessionsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    app.repos
        .sessions
        .revoke_for_user(user_id, session_id.0)
        .await?;

    app.audit.emit(
        &ctx,
        "user_session.revoke",
        "user_session",
        Some(session_id.0),
        Some(serde_json::json!({"user_id": user_id.to_string()})),
    );

    Ok(Json(RevokeSessionsResponse { revoked: 1 }))
}

pub(super) async fn revoke_user_sessions(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<RevokeSessionsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    let revoked = app.repos.sessions.revoke_user_sessions(user_id).await?;

    app.audit.emit(
        &ctx,
        "user_session.revoke_all",
        "user",
        Some(*user_id.as_uuid()),
        Some(serde_json::json!({"revoked": revoked})),
    );

    Ok(Json(RevokeSessionsResponse { revoked }))
}

fn user_to_view(u: gate_core::identity::User) -> UserView {
    UserView {
        id: u.id.to_string(),
        email: u.email,
        display_name: u.display_name,
        status: format!("{:?}", u.status).to_lowercase(),
        mfa_enabled: u.mfa_enabled,
        last_login_at: u.last_login_at,
        created_at: u.created_at,
    }
}
