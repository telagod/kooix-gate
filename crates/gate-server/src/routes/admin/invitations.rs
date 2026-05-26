//! /v1/admin/{orgs,projects}/:id/invitations — invitation lifecycle.
//!
//! 0.4.124：从 admin/mod.rs 物理拆出（14 fn + 6 helper，270 行）。
//! 依赖 admin/mod.rs 顶层 InvitationView / OrgInviteRequest / ProjectInviteRequest
//! 类型与 audit / token hash helper。

use super::*;
#[allow(unused_imports)]
use super::channels::{require_confirmation, audit_meta, channel_audit_snapshot, key_audit_snapshot, group_audit_snapshot, pricing_rule_audit_snapshot, user_audit_snapshot, channel_capabilities, channel_inflight, is_plugin_provider, key_fingerprint, validate_channel_key_alias, record_to_summary};

pub(super) fn default_invitation_ttl_hours() -> i64 {
    168
}


pub(super) async fn list_org_invitations(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Query(q): Query<InvitationQuery>,
) -> AppResult<Json<Vec<InvitationView>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberInvite, Scope::Org(&org));
    let records = app
        .repos
        .invitations
        .list_scope("org", org_id.0, q.include_inactive)
        .await?;
    Ok(Json(records.into_iter().map(invitation_to_view).collect()))
}

pub(super) async fn create_org_invitation(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateInvitationRequest>,
) -> AppResult<Json<CreatedInvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberInvite, Scope::Org(&org));
    let _ = app.repos.orgs.find_by_id(org).await?;
    let role = parse_org_invite_role(&req.role)?;

    create_invitation(
        &app,
        &ctx,
        "org",
        org_id.0,
        normalize_email(&req.email)?,
        org_role_to_invite_str(role),
        req.ttl_hours,
    )
    .await
}

pub(super) async fn revoke_org_invitation(
    State(app): State<AppState>,
    Path((org_id, invitation_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<InvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberRemove, Scope::Org(&org));
    revoke_invitation(&app, &ctx, "org", org_id.0, invitation_id.0).await
}

pub(super) async fn list_project_invitations(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Query(q): Query<InvitationQuery>,
) -> AppResult<Json<Vec<InvitationView>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberInvite,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    let records = app
        .repos
        .invitations
        .list_scope("project", project_id.0, q.include_inactive)
        .await?;
    Ok(Json(records.into_iter().map(invitation_to_view).collect()))
}

pub(super) async fn create_project_invitation(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Json(req): Json<CreateInvitationRequest>,
) -> AppResult<Json<CreatedInvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberInvite,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    let role = parse_project_invite_role(&req.role)?;

    create_invitation(
        &app,
        &ctx,
        "project",
        project_id.0,
        normalize_email(&req.email)?,
        project_role_to_invite_str(role),
        req.ttl_hours,
    )
    .await
}

pub(super) async fn revoke_project_invitation(
    State(app): State<AppState>,
    Path((org_id, project_id, invitation_id)): Path<(FlexUuid, FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<InvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberRemove,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    revoke_invitation(&app, &ctx, "project", project_id.0, invitation_id.0).await
}

async fn create_invitation(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    scope_kind: &str,
    scope_id: Uuid,
    email: String,
    role: &str,
    ttl_hours: i64,
) -> AppResult<Json<CreatedInvitationView>> {
    let invited_by = ctx
        .user_id()
        .ok_or_else(|| AppError::Forbidden("only user subjects".into()))?;
    let ttl_hours = ttl_hours.clamp(1, 24 * 30);
    let token = generate_invitation_token();
    let rec = app
        .repos
        .invitations
        .create(InvitationCreate {
            id: Uuid::now_v7(),
            scope_kind: scope_kind.to_string(),
            scope_id,
            email: email.clone(),
            role: role.to_string(),
            token_hash: invitation_token_hash(&token),
            invited_by,
            expires_at: Utc::now() + ChronoDuration::hours(ttl_hours),
        })
        .await?;

    app.audit.emit(
        ctx,
        "invitation.create",
        "invitation",
        Some(rec.id),
        Some(serde_json::json!({
            "scope_kind": scope_kind,
            "scope_id": scope_id.to_string(),
            "email": email,
            "role": role,
            "expires_at": rec.expires_at
        })),
    );

    Ok(Json(CreatedInvitationView {
        invitation: invitation_to_view(rec),
        accept_url: invitation_accept_url(app, &token),
        token,
    }))
}

async fn revoke_invitation(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    scope_kind: &str,
    scope_id: Uuid,
    invitation_id: Uuid,
) -> AppResult<Json<InvitationView>> {
    let existing = app.repos.invitations.find_by_id(invitation_id).await?;
    if existing.scope_kind != scope_kind || existing.scope_id != scope_id {
        return Err(AppError::NotFound);
    }
    let revoked = app.repos.invitations.revoke(invitation_id).await?;
    app.audit.emit(
        ctx,
        "invitation.revoke",
        "invitation",
        Some(invitation_id),
        Some(serde_json::json!({
            "scope_kind": scope_kind,
            "scope_id": scope_id.to_string(),
            "email": revoked.email,
        })),
    );
    Ok(Json(invitation_to_view(revoked)))
}

async fn ensure_project_in_org(app: &AppState, org: OrgId, project: ProjectId) -> AppResult<()> {
    let p = app.repos.projects.find_by_id(project).await?;
    if p.org_id != org {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn invitation_to_view(rec: InvitationRecord) -> InvitationView {
    let status = rec.status_at(Utc::now()).to_string();
    InvitationView {
        id: rec.id.to_string(),
        scope_kind: rec.scope_kind,
        scope_id: rec.scope_id.to_string(),
        email: rec.email,
        role: rec.role,
        invited_by: rec.invited_by.to_string(),
        expires_at: rec.expires_at,
        accepted_at: rec.accepted_at,
        accepted_by: rec.accepted_by.map(|id| id.to_string()),
        revoked_at: rec.revoked_at,
        created_at: rec.created_at,
        status,
    }
}

fn generate_invitation_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("kg_inv_{}", B64.encode(buf))
}

fn invitation_accept_url(app: &AppState, token: &str) -> Option<String> {
    app.public_origin.as_ref().map(|origin| {
        format!(
            "{}/invite/accept?token={}",
            origin.trim_end_matches('/'),
            token
        )
    })
}

fn org_role_to_invite_str(role: gate_core::identity::OrgRole) -> &'static str {
    match role {
        gate_core::identity::OrgRole::Owner => "owner",
        gate_core::identity::OrgRole::Admin => "admin",
        gate_core::identity::OrgRole::BillingViewer => "billing_viewer",
        gate_core::identity::OrgRole::Member => "member",
    }
}

fn project_role_to_invite_str(role: gate_core::identity::ProjectRole) -> &'static str {
    match role {
        gate_core::identity::ProjectRole::Owner => "owner",
        gate_core::identity::ProjectRole::Admin => "admin",
        gate_core::identity::ProjectRole::Developer => "developer",
        gate_core::identity::ProjectRole::Viewer => "viewer",
    }
}
