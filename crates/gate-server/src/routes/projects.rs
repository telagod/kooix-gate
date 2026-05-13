//! /v1/orgs/:org_id/projects — 列出某 Org 下的项目（需 ProjectRead）
//!
//! 本 handler 展示如何在业务代码里调 `require!` —— 所有需要鉴权的 endpoint
//! 必须遵循这个模式：抽取 Authed → 构造 Scope → require! → 查数据返回。

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::Path;
use axum::{routing::get, Json, Router};
use gate_auth::require;
use gate_core::id::OrgId;
use gate_core::rbac::{Permission, Scope};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/orgs/:org_id/projects", get(list_projects))
}

async fn list_projects(
    Path(org_id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ProjectSummary>>> {
    let org = OrgId::from(org_id);
    require!(ctx, Permission::ProjectRead, Scope::Org(&org));

    // TODO: 查数据库 — gate-storage Repo 就绪后替换
    Ok(Json(vec![]))
}
