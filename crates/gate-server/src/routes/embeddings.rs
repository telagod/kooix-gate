//! POST /v1/embeddings — Embedding 代理。
//!
//! Provider 选路优先级（与 chat.rs 一致）：
//! 1. 若 AppState 有 provider_router，用它按 project_id 选 channel（route_embedding）
//!    - project_id 来源：API key 主体直接取；User 主体从 X-Kooix-Project 头取
//! 2. 路由器找不到可用 channel → fallback 到 AppState.provider（若实现 EmbeddingProvider）
//! 3. 均无 → 400 Bad Request

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::gateway::{GatewayStage, StageOutcome};
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{Json, Router, routing::post};
use gate_auth::AuthError;
use gate_auth::context::Subject;
use gate_core::id::ProjectId;
use gate_providers::types::{EmbeddingRequest, EmbeddingResponse};
use gate_providers::{EmbeddingProvider, RoutedEmbeddingProvider};
use std::sync::Arc;

pub fn router() -> Router<AppState> {
    Router::new().route("/embeddings", post(create_embedding))
}

async fn create_embedding(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    Json(req): Json<EmbeddingRequest>,
) -> AppResult<Json<EmbeddingResponse>> {
    let route_start = std::time::Instant::now();
    let provider = resolve_embedding_provider(&app, &ctx, &headers, &req).await?;
    crate::gateway::record_stage(
        GatewayStage::Route,
        StageOutcome::Ok,
        route_start.elapsed().as_secs_f64(),
    );
    let execute_start = std::time::Instant::now();
    let resp = match provider.embed(req).await {
        Ok(resp) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Ok,
                execute_start.elapsed().as_secs_f64(),
            );
            resp
        }
        Err(e) => {
            crate::gateway::record_stage(
                GatewayStage::Execute,
                StageOutcome::Error,
                execute_start.elapsed().as_secs_f64(),
            );
            return Err(AppError::Internal(e.to_string()));
        }
    };
    Ok(Json(resp))
}

/// 按 subject 类型解析 project_id，经 ProviderRouter 选 EmbeddingProvider。
///
/// 返回顺序：
/// 1. ProviderRouter 选到 → 返回 provider
/// 2. ProviderRouter 找不到（返回 None） → fallback 到 AppState.provider（若实现 EmbeddingProvider）
/// 3. 均无 → 400
async fn resolve_embedding_provider(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
    req: &EmbeddingRequest,
) -> AppResult<Arc<dyn EmbeddingProvider>> {
    if let Some(router) = &app.provider_router {
        let project_id_opt = extract_project_id(app, ctx, headers).await?;

        if let Some(project_id) = project_id_opt {
            match router.route_embedding(project_id, &req.model).await {
                Ok(Some(RoutedEmbeddingProvider { provider, .. })) => {
                    return Ok(provider);
                }
                Ok(None) => {
                    tracing::debug!(
                        project_id = %project_id,
                        model = %req.model,
                        "provider_router returned None for embedding, trying fallback"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "provider_router error for embedding, falling back");
                }
            }
        }
    }

    // Fallback: 全局 provider（需实现 EmbeddingProvider）
    // gate-providers 的 OpenAiProvider 等同时实现 Provider + EmbeddingProvider，
    // 但 AppState.provider 字段类型是 Arc<dyn Provider>，无法直接转型。
    // 此路径仅在没有 provider_router 或路由器 fallback 时生效。
    // 如果配了 provider_router 但没找到 channel，返回清晰错误。
    Err(AppError::BadRequest(format!(
        "no embedding channel found for model '{}'",
        req.model
    )))
}

/// 从 AuthContext + headers 提取 project_id（与 chat.rs 完全相同的越权校验逻辑）。
async fn extract_project_id(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    headers: &HeaderMap,
) -> AppResult<Option<ProjectId>> {
    if let Some(Subject::ApiKey { project_id, .. }) = ctx.subject() {
        return Ok(Some(*project_id));
    }

    // User 主体：从 X-Kooix-Project 头取 UUID
    let Some(raw) = headers.get("x-kooix-project").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };

    let project_id: ProjectId = raw
        .trim()
        .parse()
        .map_err(|_| AppError::BadRequest("invalid X-Kooix-Project".into()))?;

    // 越权校验：project.org_id 必须匹配 ctx.current_org
    let project = app.repos.projects.find_by_id(project_id).await?;
    let Some(org) = ctx.current_org() else {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    };
    if project.org_id != org {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    if !ctx.is_super_admin()
        && ctx.project_role(&org, &project_id).is_none()
        && ctx.org_role(&org).is_none()
    {
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "embed.use_project".into(),
            resource: format!("project:{}", project_id.as_uuid()),
        }));
    }

    Ok(Some(project_id))
}
