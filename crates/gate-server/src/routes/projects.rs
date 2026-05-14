//! /v1/orgs/:org_id/projects — Org 下 Project CRUD
//!
//! 本 handler 展示如何在业务代码里调 `require!` —— 所有需要鉴权的 endpoint
//! 必须遵循这个模式：抽取 Authed → 构造 Scope → require! → 查/写 → 返回。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{Json, Router, routing::get};
use gate_auth::{require, require_user};
use gate_core::id::OrgId;
use gate_core::id::ProjectId;
use gate_core::rbac::{Permission, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub status: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/orgs/:org_id/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/orgs/:org_id/projects/:project_id",
            get(get_project).put(update_project),
        )
}

async fn list_projects(
    State(app): State<AppState>,
    Path(org_id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ProjectSummary>>> {
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectRead, Scope::Org(&org));

    let projects = app.repos.projects.list_in_org(org).await?;
    Ok(Json(
        projects
            .into_iter()
            .map(|p| ProjectSummary {
                id: p.id.as_uuid().to_string(),
                name: p.name,
                slug: p.slug,
                status: format!("{:?}", p.status).to_lowercase(),
            })
            .collect(),
    ))
}

async fn create_project(
    State(app): State<AppState>,
    Path(org_id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateProjectRequest>,
) -> AppResult<Json<ProjectSummary>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectCreate, Scope::Org(&org));

    let name = req.name.trim();
    let slug = req.slug.trim();
    if name.is_empty() || slug.is_empty() {
        return Err(AppError::BadRequest("name and slug required".into()));
    }

    // 确保 org 存在（避免攻击者用任意 UUID）
    let _ = app.repos.orgs.find_by_id(org).await?;

    let p = app.repos.projects.create(org, name, slug).await?;
    Ok(Json(ProjectSummary {
        id: p.id.as_uuid().to_string(),
        name: p.name,
        slug: p.slug,
        status: format!("{:?}", p.status).to_lowercase(),
    }))
}

async fn get_project(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<ProjectSummary>> {
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectRead, Scope::Org(&org));

    let pid = ProjectId::from(project_id);
    let p = app.repos.projects.find_by_id(pid).await?;
    Ok(Json(ProjectSummary {
        id: p.id.as_uuid().to_string(),
        name: p.name,
        slug: p.slug,
        status: format!("{:?}", p.status).to_lowercase(),
    }))
}

async fn update_project(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateProjectRequest>,
) -> AppResult<Json<ProjectSummary>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectUpdate, Scope::Org(&org));

    if let Some(ref s) = req.status {
        let valid = ["active", "archived"];
        if !valid.contains(&s.as_str()) {
            return Err(AppError::BadRequest(format!("status must be one of: {valid:?}")));
        }
    }

    let pid = ProjectId::from(project_id);
    let p = app
        .repos
        .projects
        .update(pid, req.name.as_deref(), req.status.as_deref())
        .await?;
    Ok(Json(ProjectSummary {
        id: p.id.as_uuid().to_string(),
        name: p.name,
        slug: p.slug,
        status: format!("{:?}", p.status).to_lowercase(),
    }))
}
