//! /v1/orgs/:org_id/projects/:project_id/api-keys
//!
//! POST 创建 - 需 ApiKeyCreate 权限 + 必须是 User subject（拒绝 API key 自我复制）
//! GET  列出 - 需 ApiKeyRead

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::Path;
use axum::{
    routing::post,
    Json, Router,
};
use gate_auth::{require, require_user};
use gate_core::id::{OrgId, ProjectId};
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
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/orgs/:org_id/projects/:project_id/api-keys",
            post(create).get(list),
        )
}

async fn create(
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
        Scope::Project { org: &org, project: &project }
    );

    if req.name.trim().is_empty() {
        return Err(crate::error::AppError::BadRequest("name required".into()));
    }

    let generated = gate_auth::api_key::generate();

    // TODO: 写入数据库（api_keys 表） — 现仅返回示意
    Ok(Json(CreateApiKeyResponse {
        id: gate_core::id::ApiKeyId::new().to_string(),
        name: req.name,
        plaintext: generated.plaintext.to_string(),
        prefix: generated.prefix,
        last4: generated.last4,
    }))
}

async fn list(
    Path((org_id, project_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ApiKeySummary>>> {
    let org = OrgId::from(org_id);
    let project = ProjectId::from(project_id);
    require!(
        ctx,
        Permission::ApiKeyRead,
        Scope::Project { org: &org, project: &project }
    );
    Ok(Json(vec![]))
}
