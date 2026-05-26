//! /v1/admin/orgs/:id/members — Org membership CRUD
//!
//! 0.4.123：从 admin/mod.rs 物理拆出（原 inline `mod org_members`，0.4.109）。
//! 依赖 admin/mod.rs 顶层 MemberView / AddMemberRequest 类型。

use super::*;

pub(super) async fn list_org_members(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<MemberView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org = gate_core::id::OrgId::from(org_id.0);
    let members = app.repos.memberships.list_org_members(org).await?;
    Ok(Json(
        members
            .into_iter()
            .map(|m| MemberView {
                user_id: m.user_id.to_string(),
                email: m.email,
                display_name: m.display_name,
                role: m.role,
                joined_at: m.joined_at,
            })
            .collect(),
    ))
}

pub(super) async fn add_org_member(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<AddMemberRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let valid_roles = ["owner", "admin", "billing_viewer", "member"];
    if !valid_roles.contains(&req.role.as_str()) {
        return Err(AppError::BadRequest(format!(
            "role must be one of: {valid_roles:?}"
        )));
    }

    let user = app
        .repos
        .users
        .find_by_email(&req.email)
        .await
        .map_err(|_| AppError::BadRequest(format!("user '{}' not found", req.email)))?;

    let role = match req.role.as_str() {
        "owner" => gate_core::identity::OrgRole::Owner,
        "admin" => gate_core::identity::OrgRole::Admin,
        "billing_viewer" => gate_core::identity::OrgRole::BillingViewer,
        _ => gate_core::identity::OrgRole::Member,
    };

    let org = gate_core::id::OrgId::from(org_id.0);
    app.repos
        .memberships
        .add_org_member(org, user.id, role)
        .await?;

    app.audit.emit(
        &ctx,
        "membership.add",
        "membership",
        None,
        Some(serde_json::json!({"org_id": org_id.to_string(), "email": req.email})),
    );

    Ok(Json(serde_json::json!({"ok": true})))
}

pub(super) async fn remove_org_member_handler(
    State(app): State<AppState>,
    Path((org_id, user_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org = gate_core::id::OrgId::from(org_id.0);
    let uid = gate_core::id::UserId::from(user_id.0);
    app.repos.memberships.remove_org_member(org, uid).await?;

    app.audit.emit(
        &ctx,
        "membership.remove",
        "membership",
        Some(*user_id),
        None,
    );

    Ok(Json(serde_json::json!({"removed": true})))
}
