//! /v1/admin/* — 平台运维接口
//!
//! 全部需 Platform 作用域权限。SuperAdmin 短路通过。
//!
//! Channels CRUD:
//! - GET    /channels         — 列出全部 channels
//! - POST   /channels         — 创建 channel
//! - PUT    /channels/:id     — 更新 channel
//! - DELETE /channels/:id     — 软删除 channel

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::id::ChannelId;
use gate_core::rbac::{Permission, Scope};
use gate_storage::{CreateChannel, UpdateChannel};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct ChannelSummary {
    pub id: String,
    pub code: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub status: String,
    pub health: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateChannelRequest {
    pub code: String,
    pub provider_type: String,
    pub base_url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub supported_models: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub supported_models: Option<Vec<String>>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/channels", get(list_channels).post(create_channel))
        .route(
            "/channels/:id",
            axum::routing::put(update_channel).delete(delete_channel),
        )
}

async fn list_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ChannelSummary>>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let records = app.repos.channels.list_admin_view().await?;
    Ok(Json(
        records
            .into_iter()
            .map(|r| ChannelSummary {
                id: r.channel_id.as_uuid().to_string(),
                code: r.code,
                name: r.name,
                provider_type: r.provider_type,
                base_url: r.base_url,
                status: r.status,
                health: r.health,
                updated_at: r.updated_at,
            })
            .collect(),
    ))
}

async fn create_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelCreate, Scope::Platform);

    let code = req.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest("code is required".into()));
    }

    let valid_types = ["openai", "anthropic", "gemini", "azure", "bedrock"];
    if !valid_types.contains(&req.provider_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid provider_type: '{}', must be one of {:?}",
            req.provider_type, valid_types
        )));
    }

    let name = req.name.unwrap_or_else(|| code.clone());

    let record = app
        .repos
        .channels
        .create(CreateChannel {
            code,
            name,
            provider_type: req.provider_type,
            base_url: req.base_url,
            supported_models: req.supported_models,
            enabled: req.enabled,
        })
        .await?;

    Ok(Json(ChannelSummary {
        id: record.channel_id.as_uuid().to_string(),
        code: record.code,
        name: record.name,
        provider_type: record.provider_type,
        base_url: record.base_url,
        status: record.status,
        health: record.health,
        updated_at: record.updated_at,
    }))
}

async fn update_channel(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id);
    let record = app
        .repos
        .channels
        .update(
            channel_id,
            UpdateChannel {
                name: req.name,
                base_url: req.base_url,
                supported_models: req.supported_models,
                enabled: req.enabled,
            },
        )
        .await?;

    Ok(Json(ChannelSummary {
        id: record.channel_id.as_uuid().to_string(),
        code: record.code,
        name: record.name,
        provider_type: record.provider_type,
        base_url: record.base_url,
        status: record.status,
        health: record.health,
        updated_at: record.updated_at,
    }))
}

async fn delete_channel(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);

    let channel_id = ChannelId::from(id);
    app.repos.channels.soft_delete(channel_id).await?;

    Ok(Json(serde_json::json!({"deleted": true})))
}
