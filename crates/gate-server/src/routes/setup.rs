//! POST /v1/setup — 首次初始化（无 auth，仅在无管理员时可用）
//!
//! 创建 super_admin + 默认 Org + 默认 Project，一步到位。
//! 安全：如果 platform_admins 表已有记录，直接 403 拒绝。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SetupRequest {
    pub email: String,
    pub password: String,
    #[serde(default = "default_org_name")]
    pub org_name: String,
    #[serde(default = "default_org_slug")]
    pub org_slug: String,
    #[serde(default = "default_project_name")]
    pub project_name: String,
    #[serde(default = "default_project_slug")]
    pub project_slug: String,
}

fn default_org_name() -> String { "default".into() }
fn default_org_slug() -> String { "default".into() }
fn default_project_name() -> String { "default".into() }
fn default_project_slug() -> String { "default".into() }

#[derive(Serialize)]
pub struct SetupResponse {
    pub user_id: String,
    pub email: String,
    pub org_id: String,
    pub org_name: String,
    pub project_id: String,
    pub project_name: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/setup", post(setup))
}

async fn setup(
    State(app): State<AppState>,
    Json(req): Json<SetupRequest>,
) -> AppResult<Json<SetupResponse>> {
    let already = app.repos.users.has_any_admin().await.unwrap_or(true);
    if already {
        return Err(AppError::Forbidden(
            "系统已初始化，不能重复执行 setup".into(),
        ));
    }

    if req.email.trim().is_empty() {
        return Err(AppError::BadRequest("email 必填".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest("密码至少 8 个字符".into()));
    }

    let phash = gate_auth::password::hash(&req.password)
        .map_err(|e| AppError::Internal(format!("密码哈希失败: {e}")))?;

    let user = app
        .repos
        .users
        .create(req.email.trim(), Some(&phash), None)
        .await?;

    let pool = app.repos.pool().ok_or_else(|| {
        AppError::Internal("setup 需要 PostgreSQL 连接".into())
    })?;

    sqlx::query("INSERT INTO platform_admins (user_id, role) VALUES ($1, 'super_admin')")
        .bind(user.id.as_uuid())
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(format!("授予管理员失败: {e}")))?;

    let org = app
        .repos
        .orgs
        .create(&req.org_name, &req.org_slug, user.id)
        .await?;

    app.repos
        .memberships
        .add_org_member(org.id, user.id, gate_core::identity::OrgRole::Owner)
        .await?;

    let project = app
        .repos
        .projects
        .create(org.id, &req.project_name, &req.project_slug)
        .await?;

    Ok(Json(SetupResponse {
        user_id: user.id.to_string(),
        email: user.email,
        org_id: org.id.to_string(),
        org_name: org.name,
        project_id: project.id.to_string(),
        project_name: project.name,
    }))
}
