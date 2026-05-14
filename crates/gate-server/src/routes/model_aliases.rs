//! /v1/orgs/:org_id/projects/:project_id/model-aliases — Model Alias CRUD

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{Json, Router, routing::get};
use gate_auth::{require, require_user};
use gate_core::id::{OrgId, ProjectId};
use gate_core::rbac::{Permission, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct ModelAliasView {
    pub id: String,
    pub alias: String,
    pub target_model: String,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct UpsertAliasRequest {
    pub alias: String,
    pub target_model: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/orgs/:org_id/projects/:project_id/model-aliases",
        get(list_aliases).post(upsert_alias),
    ).route(
        "/orgs/:org_id/projects/:project_id/model-aliases/:alias",
        axum::routing::delete(delete_alias),
    )
}

async fn list_aliases(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ModelAliasView>>> {
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectRead, Scope::Org(&org));

    let pid = ProjectId::from(project_id);
    let records = app.repos.model_aliases.list_by_project(pid).await?;
    Ok(Json(records.into_iter().map(|r| ModelAliasView {
        id: r.id.to_string(),
        alias: r.alias,
        target_model: r.target_model,
        enabled: r.enabled,
        created_at: r.created_at.to_rfc3339(),
    }).collect()))
}

async fn upsert_alias(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
    Json(req): Json<UpsertAliasRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ModelAliasManage, Scope::Org(&org));

    if req.alias.trim().is_empty() || req.target_model.trim().is_empty() {
        return Err(AppError::BadRequest("alias and target_model required".into()));
    }

    let pid = ProjectId::from(project_id);
    app.repos.model_aliases.upsert(pid, req.alias.trim(), req.target_model.trim()).await?;

    app.audit.emit(&ctx, "model_alias.upsert", "model_alias", None,
        Some(serde_json::json!({"project_id": project_id.to_string(), "alias": req.alias})));

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn delete_alias(
    State(app): State<AppState>,
    Path((org_id, project_id, alias)): Path<(Uuid, Uuid, String)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ModelAliasManage, Scope::Org(&org));

    let pid = ProjectId::from(project_id);
    app.repos.model_aliases.delete(pid, &alias).await?;

    Ok(Json(serde_json::json!({"deleted": true})))
}
