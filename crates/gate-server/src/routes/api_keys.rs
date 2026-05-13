//! /v1/orgs/:org_id/projects/:project_id/api-keys
//!
//! POST 创建 - 需 ApiKeyCreate 权限 + 必须是 User subject（拒绝 API key 自我复制）
//! GET  列出 - 需 ApiKeyRead
//! DELETE :id 撤销 - 需 ApiKeyRevoke

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{
    routing::{delete, post},
    Json, Router,
};
use gate_auth::{require, require_user};
use gate_core::id::{ApiKeyId, OrgId, ProjectId};
use gate_core::rbac::{Permission, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub name: String,
    /// 明文仅在创建时返回一次
    pub plaintext: String,
    pub prefix: String,
    pub last4: String,
}

#[derive(Serialize)]
pub struct ApiKeySummary {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub last4: String,
    pub allowed_models: Vec<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/orgs/:org_id/projects/:project_id/api-keys",
            post(create).get(list),
        )
        .route(
            "/orgs/:org_id/projects/:project_id/api-keys/:key_id",
            delete(revoke),
        )
}

async fn create(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
    Json(req): Json<CreateApiKeyRequest>,
) -> AppResult<Json<CreateApiKeyResponse>> {
    // 拒绝 API key 创建 API key（安全：凭证不自我繁殖）
    require_user!(ctx);

    let org = OrgId::from(org_id);
    let project = ProjectId::from(project_id);
    require!(
        ctx,
        Permission::ApiKeyCreate,
        Scope::Project {
            org: &org,
            project: &project
        }
    );

    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }

    // 路径越权防护：project 必须属于路径里的 org
    let p = app.repos.projects.find_by_id(project).await?;
    if p.org_id != org {
        return Err(AppError::NotFound);
    }

    let user_id = ctx
        .user_id()
        .ok_or_else(|| AppError::BadRequest("user subject required".into()))?;

    let generated = gate_auth::api_key::generate();

    let id = app
        .repos
        .api_keys
        .create(
            project,
            req.name.trim(),
            &generated.hash,
            &generated.prefix,
            &generated.last4,
            user_id,
            &req.allowed_models,
        )
        .await?;

    Ok(Json(CreateApiKeyResponse {
        id: id.as_uuid().to_string(),
        name: req.name,
        plaintext: generated.plaintext.to_string(),
        prefix: generated.prefix,
        last4: generated.last4,
    }))
}

async fn list(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ApiKeySummary>>> {
    let org = OrgId::from(org_id);
    let project = ProjectId::from(project_id);
    require!(
        ctx,
        Permission::ApiKeyRead,
        Scope::Project {
            org: &org,
            project: &project
        }
    );

    // 路径越权防护
    let p = app.repos.projects.find_by_id(project).await?;
    if p.org_id != org {
        return Err(AppError::NotFound);
    }

    let records = app.repos.api_keys.list_in_project(project).await?;
    // Repo 返回的 record 没有 prefix/last4（在 PG schema 里有，但 ApiKeyRecord 字段精简了）
    // 这里列表层先返回基本信息；后续可在 Repo 加 list_summaries() 拓展
    Ok(Json(
        records
            .into_iter()
            .map(|r| ApiKeySummary {
                id: r.api_key_id.as_uuid().to_string(),
                name: r.name,
                prefix: String::new(),
                last4: String::new(),
                allowed_models: r.allowed_models,
            })
            .collect(),
    ))
}

async fn revoke(
    State(app): State<AppState>,
    Path((org_id, project_id, key_id)): Path<(Uuid, Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);

    let org = OrgId::from(org_id);
    let project = ProjectId::from(project_id);
    require!(
        ctx,
        Permission::ApiKeyRevoke,
        Scope::Project {
            org: &org,
            project: &project
        }
    );

    let user_id = ctx
        .user_id()
        .ok_or_else(|| AppError::BadRequest("user subject required".into()))?;

    app.repos
        .api_keys
        .revoke(ApiKeyId::from(key_id), user_id, None)
        .await?;

    Ok(Json(serde_json::json!({"revoked": true})))
}
