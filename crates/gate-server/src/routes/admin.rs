//! /v1/admin/* — 平台运维接口
//!
//! 全部需 Platform 作用域权限。SuperAdmin 短路通过。
//!
//! Channels CRUD:
//! - GET    /channels         — 列出全部 channels
//! - POST   /channels         — 创建 channel
//! - PUT    /channels/:id     — 更新 channel
//! - DELETE /channels/:id     — 软删除 channel
//!
//! Channel Keys:
//! - GET    /channels/:id/keys        — 列出 key 元数据（不含明文）
//! - POST   /channels/:id/keys        — 添加 key（服务端加密）
//! - POST   /channels/:id/keys/rotate — 轮转 key
//! - DELETE /channels/:id/keys/:key_id — 撤销 key

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::flex_uuid::FlexUuid;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::id::{ChannelId, ChannelKeyId, UserId};
use gate_core::rbac::{Permission, Scope};
use gate_providers::ProviderCapabilities;
use gate_providers::types::{ChatMessage, ChatRequest, MessageContent, Role};
use gate_storage::{CreateChannel, ListChannelsQuery, UpdateChannel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    pub supported_models: Vec<String>,
    pub rpm_limit: Option<i32>,
    pub tpm_limit: Option<i32>,
    pub timeout_ms: i32,
    pub max_retries: i32,
    pub tags: Vec<String>,
    pub capabilities: ProviderCapabilities,
    pub model_mapping: serde_json::Value,
    pub balance: Option<f64>,
    pub balance_updated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct PaginatedChannelsResponse {
    pub data: Vec<ChannelSummary>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Deserialize)]
pub struct ChannelListParams {
    pub search: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub health: Option<String>,
    pub tag: Option<String>,
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
}

fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    20
}
fn default_sort_by() -> String {
    "created_at".into()
}
fn default_sort_dir() -> String {
    "asc".into()
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
    #[serde(default)]
    pub rpm_limit: Option<i32>,
    #[serde(default)]
    pub tpm_limit: Option<i32>,
    #[serde(default)]
    pub timeout_ms: Option<i32>,
    #[serde(default)]
    pub max_retries: Option<i32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub model_mapping: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

fn default_replay_base_url() -> String {
    "https://example.com".to_string()
}

fn default_replay_model() -> String {
    "replay-model".to_string()
}

#[derive(Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub supported_models: Option<Vec<String>>,
    #[serde(default)]
    pub rpm_limit: Option<i32>,
    #[serde(default)]
    pub tpm_limit: Option<i32>,
    #[serde(default)]
    pub timeout_ms: Option<i32>,
    #[serde(default)]
    pub max_retries: Option<i32>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub model_mapping: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct BatchChannelRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub struct BatchResult {
    pub affected: u64,
}

#[derive(Deserialize)]
pub struct PluginReplayRequest {
    pub manifest: serde_json::Value,
    pub raw_sse: String,
    #[serde(default = "default_replay_base_url")]
    pub base_url: String,
    #[serde(default = "default_replay_model")]
    pub model: String,
}

#[derive(Serialize)]
pub struct PluginReplayResponse {
    pub chunks: Vec<gate_providers::ChatStreamChunk>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/plugin-manifest/schema", get(plugin_manifest_schema))
        .route(
            "/plugin-manifest/replay",
            axum::routing::post(plugin_manifest_replay),
        )
        .route("/channels", get(list_channels).post(create_channel))
        .route(
            "/channels/:id",
            axum::routing::put(update_channel).delete(delete_channel),
        )
        .route(
            "/channels/batch-enable",
            axum::routing::post(batch_enable_channels),
        )
        .route(
            "/channels/batch-disable",
            axum::routing::post(batch_disable_channels),
        )
        .route(
            "/channels/batch-delete",
            axum::routing::post(batch_delete_channels),
        )
        .route(
            "/channels/:id/keys",
            get(list_channel_keys).post(create_channel_key),
        )
        .route(
            "/channels/:id/keys/rotate",
            axum::routing::post(rotate_channel_key),
        )
        .route(
            "/channels/:id/keys/:key_id",
            axum::routing::delete(revoke_channel_key),
        )
        .route("/channels/:id/stats", get(get_channel_stats))
        .route(
            "/channels/:id/probe",
            axum::routing::post(probe_channel_models),
        )
        .route("/channels/:id/test", get(test_channel))
        .route("/channels/:id/balance", get(get_channel_balance))
        .route("/audit-logs", get(list_audit_logs))
        .route("/orgs", get(list_all_orgs).post(create_org))
        .route("/orgs/:org_id", axum::routing::put(update_org))
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id/status", axum::routing::put(update_user_status))
        .route(
            "/users/:id/password",
            axum::routing::put(reset_user_password),
        )
        .route("/groups", get(list_groups).post(create_group))
        .route(
            "/groups/:id",
            axum::routing::put(update_group).delete(delete_group),
        )
        .route(
            "/groups/:id/bindings",
            get(list_group_bindings).post(add_group_binding),
        )
        .route(
            "/groups/:id/bindings/:channel_id",
            axum::routing::put(update_group_binding).delete(remove_group_binding),
        )
        .route("/groups/:id/detail", get(get_group_detail))
        .route(
            "/projects/:id/default-group",
            axum::routing::put(set_project_default_group),
        )
        .route(
            "/orgs/:org_id/members",
            get(list_org_members).post(add_org_member),
        )
        .route(
            "/orgs/:org_id/members/:user_id",
            axum::routing::delete(remove_org_member_handler),
        )
        .route(
            "/pricing-rules",
            get(list_pricing_rules).post(upsert_pricing_rule),
        )
        .route(
            "/pricing-rules/:id",
            axum::routing::delete(delete_pricing_rule),
        )
}

async fn plugin_manifest_schema(Authed(ctx): Authed) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);
    Ok(Json(gate_providers::plugin_manifest_schema_json()))
}

async fn plugin_manifest_replay(
    Authed(ctx): Authed,
    Json(req): Json<PluginReplayRequest>,
) -> AppResult<Json<PluginReplayResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);
    if req.raw_sse.trim().is_empty() {
        return Err(AppError::BadRequest("raw_sse is required".into()));
    }
    let chunks =
        gate_providers::replay_plugin_sse(req.manifest, &req.base_url, req.raw_sse, &req.model)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(PluginReplayResponse { chunks }))
}

async fn list_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(params): Query<ChannelListParams>,
) -> AppResult<Json<PaginatedChannelsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let query = ListChannelsQuery {
        search: params.search,
        provider: params.provider,
        status: params.status,
        health: params.health,
        tag: params.tag,
        page: params.page,
        page_size: params.page_size,
        sort_by: params.sort_by,
        sort_dir: params.sort_dir,
    };

    let result = app.repos.channels.list_admin_paginated(query).await?;
    Ok(Json(PaginatedChannelsResponse {
        data: result.data.into_iter().map(record_to_summary).collect(),
        total: result.total,
        page: result.page,
        page_size: result.page_size,
    }))
}

fn record_to_summary(r: gate_storage::ChannelRecord) -> ChannelSummary {
    let capabilities = channel_capabilities(&r);
    ChannelSummary {
        id: r.channel_id.to_string(),
        code: r.code,
        name: r.name,
        provider_type: r.provider_type,
        base_url: r.base_url,
        status: r.status,
        health: r.health,
        supported_models: r.supported_models,
        rpm_limit: r.rpm_limit,
        tpm_limit: r.tpm_limit,
        timeout_ms: r.timeout_ms,
        max_retries: r.max_retries,
        tags: r.tags,
        capabilities,
        model_mapping: r.model_mapping,
        balance: r.balance,
        balance_updated_at: r.balance_updated_at,
        last_error: r.last_error,
        last_error_at: r.last_error_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

fn channel_capabilities(r: &gate_storage::ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&r.provider_type) {
        return gate_providers::plugin_manifest(r.model_mapping.clone(), &r.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| gate_providers::provider_capabilities(&r.provider_type));
    }
    gate_providers::provider_capabilities(&r.provider_type)
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
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || code.len() > 64
    {
        return Err(AppError::BadRequest(
            "code must be 1-64 chars: [a-zA-Z0-9_-]".into(),
        ));
    }

    let valid_types = [
        "openai",
        "anthropic",
        "gemini",
        "azure",
        "bedrock",
        "deepseek",
        "ollama",
        "mistral",
        "cohere",
        "groq",
        "together",
        "openrouter",
        "moonshot",
        "zhipu",
        "qwen",
        "yi",
        "plugin",
        "custom",
        "http",
        "http_plugin",
    ];
    if !valid_types.contains(&req.provider_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid provider_type: '{}', must be one of {:?}",
            req.provider_type, valid_types
        )));
    }

    let name = req.name.unwrap_or_else(|| code.clone());
    if is_plugin_provider(&req.provider_type) {
        let mapping = req
            .model_mapping
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        gate_providers::validate_plugin_manifest(mapping, &req.base_url)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }

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
            rpm_limit: req.rpm_limit,
            tpm_limit: req.tpm_limit,
            timeout_ms: req.timeout_ms,
            max_retries: req.max_retries,
            tags: req.tags,
            model_mapping: req.model_mapping,
        })
        .await?;

    app.audit.emit(
        &ctx,
        "channel.create",
        "channel",
        Some(*record.channel_id.as_uuid()),
        Some(serde_json::json!({"code": &record.code})),
    );

    Ok(Json(record_to_summary(record)))
}

async fn update_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    if req.model_mapping.is_some() || req.base_url.is_some() {
        let current = app.repos.channels.find_by_id(channel_id).await?;
        if is_plugin_provider(&current.provider_type) {
            let mapping = req
                .model_mapping
                .clone()
                .unwrap_or_else(|| current.model_mapping.clone());
            let base_url = req.base_url.as_deref().unwrap_or(&current.base_url);
            gate_providers::validate_plugin_manifest(mapping, base_url)
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
        }
    }
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
                rpm_limit: req.rpm_limit,
                tpm_limit: req.tpm_limit,
                timeout_ms: req.timeout_ms,
                max_retries: req.max_retries,
                tags: req.tags,
                model_mapping: req.model_mapping,
            },
        )
        .await?;

    app.audit
        .emit(&ctx, "channel.update", "channel", Some(*id), None);

    Ok(Json(record_to_summary(record)))
}

fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

async fn delete_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    app.repos.channels.soft_delete(channel_id).await?;

    app.audit
        .emit(&ctx, "channel.delete", "channel", Some(*id), None);

    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn batch_enable_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_set_enabled(&ids, true).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_enable",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

async fn batch_disable_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_set_enabled(&ids, false).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_disable",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

async fn batch_delete_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_soft_delete(&ids).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_delete",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

// ============================================================================
// Channel Keys
// ============================================================================

/// Key 摘要（不含 key_enc）。
#[derive(Serialize)]
pub struct ChannelKeySummary {
    pub id: String,
    pub channel_id: String,
    pub label: Option<String>,
    pub fingerprint: String,
    pub weight: i32,
    pub health: String,
    pub total_requests: i64,
    pub total_errors: i64,
    pub consecutive_errors: i32,
    pub last_error_code: Option<i32>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    /// 明文 API key，服务端加密后存储。
    pub secret: String,
    #[serde(default)]
    pub alias: Option<String>,
}

/// 计算 key fingerprint：SHA-256 前 16 字节 hex。
fn key_fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(secret.as_bytes());
    hex::encode(&hash[..16])
}

fn validate_channel_key_alias(alias: &str) -> AppResult<()> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Ok(());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::BadRequest(
            "key alias must use [a-zA-Z0-9_-]".into(),
        ));
    }
    Ok(())
}

async fn list_channel_keys(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ChannelKeySummary>>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let records = app.repos.channel_keys.list_by_channel(channel_id).await?;
    Ok(Json(
        records
            .into_iter()
            .map(|r| ChannelKeySummary {
                id: r.id.to_string(),
                channel_id: r.channel_id.to_string(),
                label: r.label,
                fingerprint: r.key_fingerprint,
                weight: r.weight,
                health: r.health,
                total_requests: r.total_requests,
                total_errors: r.total_errors,
                consecutive_errors: r.consecutive_errors,
                last_error_code: r.last_error_code,
                last_error_at: r.last_error_at,
                cooldown_until: r.cooldown_until,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

async fn create_channel_key(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }
    if let Some(alias) = req.alias.as_deref() {
        validate_channel_key_alias(alias)?;
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id.0);
    // 先确认 channel 存在
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);

    // AAD 用 channel_id：同 channel 的所有 key 共享 AAD context，
    // 防止密文跨 channel 移植，同时避免先有 key_id 再加密的鸡生蛋问题。
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto
        .seal(req.secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("encrypt channel key failed: {e}")))?;

    let key_id = app
        .repos
        .channel_keys
        .create(channel_id, &key_enc, &fingerprint, req.alias.as_deref())
        .await?;

    app.audit.emit(
        &ctx,
        "channel_key.create",
        "channel_key",
        Some(*key_id.as_uuid()),
        Some(serde_json::json!({"channel_id": id.to_string()})),
    );

    Ok(Json(ChannelKeySummary {
        id: key_id.to_string(),
        channel_id: channel_id.to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        total_requests: 0,
        total_errors: 0,
        consecutive_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
    }))
}

async fn rotate_channel_key(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }
    if let Some(alias) = req.alias.as_deref() {
        validate_channel_key_alias(alias)?;
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id.0);
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto
        .seal(req.secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("encrypt channel key failed: {e}")))?;

    let key_id = app
        .repos
        .channel_keys
        .rotate(channel_id, &key_enc, &fingerprint, req.alias.as_deref())
        .await?;

    app.audit.emit(
        &ctx,
        "channel_key.rotate",
        "channel_key",
        Some(*key_id.as_uuid()),
        Some(serde_json::json!({"channel_id": id.to_string()})),
    );

    Ok(Json(ChannelKeySummary {
        id: key_id.to_string(),
        channel_id: channel_id.to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        total_requests: 0,
        total_errors: 0,
        consecutive_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
    }))
}

async fn revoke_channel_key(
    State(app): State<AppState>,
    Path((id, key_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    // 验证 channel 存在
    let channel_id = ChannelId::from(id.0);
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let ck_id = ChannelKeyId::from(key_id.0);
    app.repos.channel_keys.revoke(ck_id).await?;

    app.audit.emit(
        &ctx,
        "channel_key.revoke",
        "channel_key",
        Some(*key_id),
        Some(serde_json::json!({"channel_id": id.to_string()})),
    );

    Ok(Json(serde_json::json!({"revoked": true})))
}

// ============================================================================
// Channel Stats
// ============================================================================

#[derive(Serialize)]
pub struct ChannelStatsResponse {
    pub channel: ChannelSummary,
    pub keys_count: i64,
    pub keys_healthy: i64,
    pub total_requests: i64,
    pub total_errors: i64,
}

async fn get_channel_stats(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelStatsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    let keys = app.repos.channel_keys.list_by_channel(channel_id).await?;

    let keys_count = keys.len() as i64;
    let keys_healthy = keys.iter().filter(|k| k.health == "healthy").count() as i64;
    let total_requests: i64 = keys.iter().map(|k| k.total_requests).sum();
    let total_errors: i64 = keys.iter().map(|k| k.total_errors).sum();

    Ok(Json(ChannelStatsResponse {
        channel: record_to_summary(channel),
        keys_count,
        keys_healthy,
        total_requests,
        total_errors,
    }))
}

// ============================================================================
// Audit Logs
// ============================================================================

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub org_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Serialize)]
pub struct AuditLogView {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<String>,
    pub org_id: Option<String>,
    pub outcome: String,
    pub after: Option<serde_json::Value>,
}

async fn list_audit_logs(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<AuditLogQuery>,
) -> AppResult<Json<Vec<AuditLogView>>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);

    let records = if let Some(org_id) = q.org_id {
        app.repos.audit.list_by_org(org_id, limit, offset).await?
    } else {
        // No org filter — platform admin sees all (via org_id=nil trick won't work;
        // for now require org_id)
        return Err(AppError::BadRequest("org_id query param required".into()));
    };

    Ok(Json(
        records
            .into_iter()
            .map(|r| AuditLogView {
                id: r.id.to_string(),
                ts: r.ts,
                actor_kind: r.actor_kind,
                actor_id: r.actor_id.map(|u| u.to_string()),
                action: r.action,
                resource_kind: r.resource_kind,
                resource_id: r.resource_id.map(|u| u.to_string()),
                org_id: r.org_id.map(|u| u.to_string()),
                outcome: r.outcome,
                after: r.after,
            })
            .collect(),
    ))
}

// ============================================================================
// Org Management (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct OrgView {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub owner_user_id: String,
    pub status: String,
    pub billing_email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Deserialize)]
pub struct UpdateOrgRequest {
    pub name: Option<String>,
    pub billing_email: Option<String>,
}

async fn list_all_orgs(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<OrgView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let orgs = app.repos.orgs.list_all().await?;
    Ok(Json(orgs.into_iter().map(org_to_view).collect()))
}

async fn create_org(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateOrgRequest>,
) -> AppResult<Json<OrgView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let name = req.name.trim();
    let slug = req.slug.trim();
    if name.is_empty() || slug.is_empty() {
        return Err(AppError::BadRequest("name and slug required".into()));
    }

    let owner_id = match ctx.subject().unwrap() {
        gate_auth::Subject::User { user_id, .. } => user_id,
        _ => return Err(AppError::Forbidden("only user subjects".into())),
    };

    let org = app.repos.orgs.create(name, slug, *owner_id).await?;

    app.repos
        .memberships
        .add_org_member(org.id, *owner_id, gate_core::identity::OrgRole::Owner)
        .await?;

    app.audit.emit(
        &ctx,
        "org.create",
        "org",
        Some(*org.id.as_uuid()),
        Some(serde_json::json!({"slug": &org.slug})),
    );

    Ok(Json(org_to_view(org)))
}

async fn update_org(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateOrgRequest>,
) -> AppResult<Json<OrgView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org_id = gate_core::id::OrgId::from(id.0);
    let org = app
        .repos
        .orgs
        .update(org_id, req.name.as_deref(), req.billing_email.as_deref())
        .await?;

    app.audit.emit(&ctx, "org.update", "org", Some(*id), None);

    Ok(Json(org_to_view(org)))
}

fn org_to_view(o: gate_core::identity::Organization) -> OrgView {
    OrgView {
        id: o.id.to_string(),
        name: o.name,
        slug: o.slug,
        owner_user_id: o.owner_user_id.to_string(),
        status: format!("{:?}", o.status).to_lowercase(),
        billing_email: o.billing_email,
        created_at: o.created_at,
        updated_at: o.updated_at,
    }
}

// ============================================================================
// User Management (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub status: String,
    pub mfa_enabled: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct UsersQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub password: String,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateStatusRequest {
    pub status: String,
}

#[derive(Deserialize)]
pub struct ResetUserPasswordRequest {
    pub password: String,
}

fn normalize_user_email(email: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || email.len() > 320 || !email.contains('@') {
        return Err(AppError::BadRequest("valid email is required".into()));
    }
    Ok(email)
}

fn normalize_display_name(display_name: Option<String>) -> Option<String> {
    display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn validate_admin_user_status(status: &str) -> AppResult<()> {
    let valid = ["active", "suspended", "pending_verification"];
    if !valid.contains(&status) {
        return Err(AppError::BadRequest(format!(
            "status must be one of: {valid:?}"
        )));
    }
    Ok(())
}

fn current_user_id(ctx: &gate_auth::AuthContext) -> AppResult<UserId> {
    ctx.user_id()
        .ok_or_else(|| AppError::Forbidden("user subject required".into()))
}

async fn list_users(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<UsersQuery>,
) -> AppResult<Json<Vec<UserView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let users = app.repos.users.list_all(limit, offset).await?;
    Ok(Json(users.into_iter().map(user_to_view).collect()))
}

async fn create_user(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let email = normalize_user_email(&req.email)?;
    let display_name = normalize_display_name(req.display_name);
    let status = req.status.unwrap_or_else(|| "active".into());
    validate_admin_user_status(&status)?;

    let password_hash = gate_auth::password::hash(&req.password)?;
    let user = app
        .repos
        .users
        .create(
            &email,
            Some(&password_hash),
            display_name.as_deref(),
            Some(&status),
        )
        .await?;

    app.audit.emit(
        &ctx,
        "user.create",
        "user",
        Some(*user.id.as_uuid()),
        Some(serde_json::json!({"email": &user.email, "status": status})),
    );

    Ok(Json(user_to_view(user)))
}

async fn update_user_status(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateStatusRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    validate_admin_user_status(&req.status)?;

    let user_id = UserId::from(id.0);
    if user_id == current_user_id(&ctx)? && req.status != "active" {
        return Err(AppError::BadRequest(
            "cannot suspend or deactivate the current admin user".into(),
        ));
    }

    let user = app.repos.users.update_status(user_id, &req.status).await?;

    app.audit.emit(
        &ctx,
        "user.update_status",
        "user",
        Some(*id),
        Some(serde_json::json!({"status": &req.status})),
    );

    Ok(Json(user_to_view(user)))
}

async fn reset_user_password(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<ResetUserPasswordRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let password_hash = gate_auth::password::hash(&req.password)?;
    let user_id = UserId::from(id.0);
    let user = app
        .repos
        .users
        .reset_password(user_id, &password_hash)
        .await?;

    app.audit.emit(
        &ctx,
        "user.reset_password",
        "user",
        Some(*id),
        Some(serde_json::json!({"email": &user.email})),
    );

    Ok(Json(user_to_view(user)))
}

fn user_to_view(u: gate_core::identity::User) -> UserView {
    UserView {
        id: u.id.to_string(),
        email: u.email,
        display_name: u.display_name,
        status: format!("{:?}", u.status).to_lowercase(),
        mfa_enabled: u.mfa_enabled,
        last_login_at: u.last_login_at,
        created_at: u.created_at,
    }
}

// ============================================================================
// Channel Groups (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub strategy: String,
    pub enabled: bool,
    pub fallback_group_id: Option<String>,
    pub channel_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub strategy: String,
}

#[derive(Deserialize)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    pub strategy: Option<String>,
    pub enabled: Option<bool>,
    pub fallback_group_id: Option<Option<String>>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct BindingView {
    pub channel_id: String,
    pub channel_code: String,
    pub channel_name: String,
    pub provider_type: String,
    pub capabilities: ProviderCapabilities,
    pub priority: i32,
    pub weight: i32,
    pub model_filter: Vec<String>,
    pub enabled: bool,
    pub channel_status: String,
    pub channel_health: String,
}

#[derive(Deserialize)]
pub struct AddBindingRequest {
    pub channel_id: Uuid,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

async fn list_groups(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<GroupView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let groups = app.repos.channel_groups.list_all().await?;
    let mut views = Vec::with_capacity(groups.len());
    for g in groups {
        let bindings = app.repos.channel_groups.list_bindings(g.group_id).await?;
        views.push(GroupView {
            id: g.group_id.to_string(),
            name: g.name,
            description: g.description,
            strategy: g.strategy,
            enabled: g.enabled,
            fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
            channel_count: bindings.len() as i64,
            created_at: g.created_at,
            updated_at: g.updated_at,
        });
    }
    Ok(Json(views))
}

async fn create_group(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let valid = [
        "priority",
        "weighted_random",
        "round_robin",
        "least_conn",
        "least_latency",
    ];
    if !valid.contains(&req.strategy.as_str()) {
        return Err(AppError::BadRequest(format!(
            "strategy must be one of: {valid:?}"
        )));
    }

    let g = app
        .repos
        .channel_groups
        .create(&req.name, &req.strategy)
        .await?;
    app.audit.emit(
        &ctx,
        "channel_group.create",
        "channel_group",
        Some(*g.group_id.as_uuid()),
        None,
    );

    Ok(Json(GroupView {
        id: g.group_id.to_string(),
        name: g.name,
        description: g.description,
        strategy: g.strategy,
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: 0,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }))
}

async fn update_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    // Validate strategy if provided
    if let Some(ref s) = req.strategy {
        let valid = [
            "priority",
            "weighted_random",
            "round_robin",
            "least_conn",
            "least_latency",
        ];
        if !valid.contains(&s.as_str()) {
            return Err(AppError::BadRequest(format!(
                "strategy must be one of: {valid:?}"
            )));
        }
    }

    let gid = gate_core::id::ChannelGroupId::from(id.0);

    // Parse fallback_group_id: Option<Option<String>> -> Option<Option<ChannelGroupId>>
    let fallback: Option<Option<gate_core::id::ChannelGroupId>> = match req.fallback_group_id {
        None => None,             // don't change
        Some(None) => Some(None), // clear
        Some(Some(ref s)) => {
            let fb_uuid = s
                .parse::<Uuid>()
                .map_err(|_| AppError::BadRequest("invalid fallback_group_id UUID".into()))?;
            Some(Some(gate_core::id::ChannelGroupId::from(fb_uuid)))
        }
    };

    let g = app
        .repos
        .channel_groups
        .update(
            gid,
            req.name.as_deref(),
            req.strategy.as_deref(),
            req.enabled,
            fallback,
            req.description.as_deref(),
        )
        .await?;
    app.audit.emit(
        &ctx,
        "channel_group.update",
        "channel_group",
        Some(*id),
        None,
    );

    let bindings = app.repos.channel_groups.list_bindings(gid).await?;

    Ok(Json(GroupView {
        id: g.group_id.to_string(),
        name: g.name,
        description: g.description,
        strategy: g.strategy,
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: bindings.len() as i64,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }))
}

async fn delete_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    app.repos.channel_groups.delete(gid).await?;
    app.audit.emit(
        &ctx,
        "channel_group.delete",
        "channel_group",
        Some(*id),
        None,
    );

    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn list_group_bindings(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<BindingView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;
    Ok(Json(bindings.into_iter().map(binding_to_view).collect()))
}

fn binding_to_view(b: gate_storage::ChannelBinding) -> BindingView {
    let capabilities = channel_capabilities(&b.channel);
    BindingView {
        channel_id: b.channel.channel_id.to_string(),
        channel_code: b.channel.code,
        channel_name: b.channel.name,
        provider_type: b.channel.provider_type,
        capabilities,
        priority: b.priority,
        weight: b.weight,
        model_filter: b.model_filter,
        enabled: b.enabled,
        channel_status: b.channel.status,
        channel_health: b.channel.health,
    }
}

async fn add_group_binding(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<AddBindingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(req.channel_id);
    app.repos
        .channel_groups
        .add_binding(
            gid,
            cid,
            req.priority.unwrap_or(100),
            req.weight.unwrap_or(1),
        )
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn remove_group_binding(
    State(app): State<AppState>,
    Path((id, channel_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(channel_id.0);
    app.repos.channel_groups.remove_binding(gid, cid).await?;

    Ok(Json(serde_json::json!({"removed": true})))
}

#[derive(Deserialize)]
pub struct UpdateBindingRequest {
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    pub model_filter: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

async fn update_group_binding(
    State(app): State<AppState>,
    Path((id, channel_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateBindingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(channel_id.0);
    app.repos
        .channel_groups
        .update_binding(
            gid,
            cid,
            req.priority,
            req.weight,
            req.model_filter,
            req.enabled,
        )
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Serialize)]
pub struct GroupDetailView {
    #[serde(flatten)]
    pub group: GroupView,
    pub bindings: Vec<BindingView>,
    pub projects_using: Vec<String>,
}

async fn get_group_detail(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<GroupDetailView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let g = app.repos.channel_groups.find_by_id(gid).await?;
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;
    let projects = app
        .repos
        .channel_groups
        .list_projects_using_group(gid)
        .await?;

    let binding_views: Vec<BindingView> = bindings.into_iter().map(binding_to_view).collect();

    Ok(Json(GroupDetailView {
        group: GroupView {
            id: g.group_id.to_string(),
            name: g.name,
            description: g.description,
            strategy: g.strategy,
            enabled: g.enabled,
            fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
            channel_count: binding_views.len() as i64,
            created_at: g.created_at,
            updated_at: g.updated_at,
        },
        bindings: binding_views,
        projects_using: projects.into_iter().map(|p| p.to_string()).collect(),
    }))
}

#[derive(Deserialize)]
pub struct SetDefaultGroupRequest {
    pub group_id: Option<String>,
}

async fn set_project_default_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<SetDefaultGroupRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let project_id = gate_core::id::ProjectId::from(id.0);
    // Validate the project exists
    let _ = app.repos.projects.find_by_id(project_id).await?;

    let group_id = match req.group_id {
        None => None,
        Some(ref s) => {
            let gid_uuid = s
                .parse::<Uuid>()
                .map_err(|_| AppError::BadRequest("invalid group_id UUID".into()))?;
            let gid = gate_core::id::ChannelGroupId::from(gid_uuid);
            // Validate the group exists
            let _ = app.repos.channel_groups.find_by_id(gid).await?;
            Some(gid)
        }
    };

    app.repos
        .channel_groups
        .set_project_default_group(project_id, group_id)
        .await?;

    app.audit.emit(
        &ctx,
        "project.set_default_group",
        "project",
        Some(*id),
        Some(serde_json::json!({"group_id": req.group_id})),
    );

    Ok(Json(serde_json::json!({"ok": true})))
}

// ============================================================================
// Org Membership Management (Admin)
// ============================================================================

#[derive(Serialize)]
pub struct MemberView {
    pub user_id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AddMemberRequest {
    pub email: String,
    pub role: String,
}

async fn list_org_members(
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

async fn add_org_member(
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

async fn remove_org_member_handler(
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

// ============================================================================
// Channel Probe & Test (P2.1 + P2.2)
// ============================================================================

/// POST /v1/admin/channels/:id/probe — 调用上游模型端点或 plugin manifest probe 获取可用模型列表。
async fn probe_channel_models(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<ProbeResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let channel_id_typed = ChannelId::from(channel_id.0);
    let (url, method, headers, body, success_status, probe_model, max_cost_micros) =
        if is_plugin_provider(&ch.provider_type) {
            let provider = gate_providers::CustomHttpProvider::new_with_secret_slots(
                &ch.base_url,
                resolve_probe_secrets(&app, channel_id_typed, &ch.code).await,
                ch.model_mapping.clone(),
                gate_providers::ProviderOpts {
                    timeout_ms: (ch.timeout_ms as u64).max(5_000),
                },
            )
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
            let probe = provider
                .build_probe_request()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            (
                probe.url,
                probe.method,
                probe.headers,
                probe.body,
                probe.success_status,
                Some(probe.model),
                probe.max_cost_micros,
            )
        } else {
            let api_key = resolve_probe_key(&app, channel_id_typed, &ch.code).await;
            let base = ch.base_url.trim_end_matches('/');
            let (url, headers) = match ch.provider_type.as_str() {
                "anthropic" => {
                    let url = format!("{base}/v1/models");
                    let mut h = reqwest::header::HeaderMap::new();
                    if let Ok(v) = api_key.parse() {
                        h.insert("x-api-key", v);
                    }
                    if let Ok(v) = "2023-06-01".parse() {
                        h.insert("anthropic-version", v);
                    }
                    (url, h)
                }
                _ => {
                    let url = format!("{base}/models");
                    let mut h = reqwest::header::HeaderMap::new();
                    if !api_key.is_empty()
                        && let Ok(v) = format!("Bearer {api_key}").parse()
                    {
                        h.insert("authorization", v);
                    }
                    (url, h)
                }
            };
            (
                url,
                reqwest::Method::GET,
                headers,
                None,
                vec![200],
                None,
                None,
            )
        };

    let resp = client
        .request(method, &url)
        .headers(headers)
        .body(body.unwrap_or_default())
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("probe failed: {e}")))?;

    let status = resp.status().as_u16();
    if !success_status.contains(&status) {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            channel_id = %channel_id,
            status = status,
            body = %body,
            "probe upstream returned error"
        );
        return Err(AppError::Internal(format!(
            "probe failed: upstream returned {status}"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("probe parse error: {e}")))?;

    let models = extract_probe_model_ids(&body);

    Ok(Json(ProbeResponse {
        channel_id: channel_id.to_string(),
        provider_type: ch.provider_type.clone(),
        models,
        probe_model,
        max_cost_micros,
    }))
}

#[derive(Serialize)]
struct ProbeResponse {
    channel_id: String,
    provider_type: String,
    models: Vec<String>,
    probe_model: Option<String>,
    max_cost_micros: Option<i64>,
}

fn extract_probe_model_ids(body: &serde_json::Value) -> Vec<String> {
    let mut models: Vec<String> = body
        .get("data")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| {
            m.get("id")
                .or_else(|| m.get("name"))
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect();
    if models.is_empty() {
        models = body
            .get("models")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|m| {
                m.as_str().map(str::to_string).or_else(|| {
                    m.get("id")
                        .or_else(|| m.get("name"))
                        .and_then(|id| id.as_str())
                        .map(str::to_string)
                })
            })
            .collect();
    }
    models.sort();
    models.dedup();
    models
}

/// GET /v1/admin/channels/:id/test — 发送最小 chat completion 验证渠道可用性。
async fn test_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
    Query(query): Query<TestChannelQuery>,
) -> AppResult<Json<TestResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;

    let test_model = query.model.clone().unwrap_or_else(|| {
        ch.supported_models
            .first()
            .cloned()
            .unwrap_or_else(|| "gpt-4o-mini".to_string())
    });

    let api_key = resolve_probe_key(&app, ChannelId::from(channel_id.0), &ch.code).await;

    let timeout_ms = (ch.timeout_ms as u64).max(5000);
    let opts = gate_providers::ProviderOpts { timeout_ms };

    let provider: Arc<dyn gate_providers::Provider> = match ch.provider_type.as_str() {
        "anthropic" => Arc::new(
            gate_providers::anthropic::AnthropicProvider::new_with_opts(
                &ch.base_url,
                &api_key,
                opts,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        "azure" => Arc::new(
            gate_providers::azure::AzureProvider::new_with_opts(&ch.base_url, &api_key, None, opts)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        "plugin" | "custom" | "http" | "http_plugin" => Arc::new(
            gate_providers::CustomHttpProvider::new_with_secret_slots(
                &ch.base_url,
                resolve_probe_secrets(&app, ChannelId::from(channel_id.0), &ch.code).await,
                ch.model_mapping.clone(),
                opts,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        _ => Arc::new(
            gate_providers::openai::OpenAiProvider::new_with_opts(&ch.base_url, &api_key, opts)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
    };

    let start = std::time::Instant::now();
    let req = ChatRequest {
        model: test_model.clone(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("Hi".to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        max_tokens: Some(1),
        temperature: Some(0.0),
        stream: false,
        ..Default::default()
    };

    let result = provider.chat(req).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => Ok(Json(TestResponse {
            success: true,
            model: test_model,
            response_time_ms: elapsed_ms,
            message: resp
                .choices
                .first()
                .and_then(|c| c.message.content.as_ref())
                .map(|c| c.to_text()),
            error: None,
        })),
        Err(e) => Ok(Json(TestResponse {
            success: false,
            model: test_model,
            response_time_ms: elapsed_ms,
            message: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Deserialize)]
struct TestChannelQuery {
    model: Option<String>,
}

#[derive(Serialize)]
struct TestResponse {
    success: bool,
    model: String,
    response_time_ms: u64,
    message: Option<String>,
    error: Option<String>,
}

// ============================================================================
// Channel Balance (P4.5)
// ============================================================================

/// GET /v1/admin/channels/:id/balance — 查询上游账户余额。
/// 目前仅 OpenAI 支持；其他 provider 返回 supported=false。
async fn get_channel_balance(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<BalanceResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;
    let api_key = resolve_probe_key(&app, ChannelId::from(channel_id.0), &ch.code).await;

    match ch.provider_type.as_str() {
        "openai" => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let url = format!(
                "{}/dashboard/billing/subscription",
                ch.base_url.trim_end_matches('/')
            );
            let resp = client.get(&url).bearer_auth(&api_key).send().await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let body: serde_json::Value = r
                        .json()
                        .await
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    let hard_limit = body.get("hard_limit_usd").and_then(|v| v.as_f64());
                    let used = body.get("soft_limit_usd").and_then(|v| v.as_f64());

                    if let Some(limit) = hard_limit {
                        update_channel_balance(&app, ChannelId::from(channel_id.0), limit).await;
                    }

                    Ok(Json(BalanceResponse {
                        channel_id: channel_id.to_string(),
                        provider_type: ch.provider_type.clone(),
                        supported: true,
                        balance_usd: hard_limit,
                        used_usd: used,
                        message: None,
                    }))
                }
                Ok(r) => {
                    let status = r.status().as_u16();
                    Ok(Json(BalanceResponse {
                        channel_id: channel_id.to_string(),
                        provider_type: ch.provider_type.clone(),
                        supported: true,
                        balance_usd: None,
                        used_usd: None,
                        message: Some(format!("billing API returned {status}")),
                    }))
                }
                Err(e) => Ok(Json(BalanceResponse {
                    channel_id: channel_id.to_string(),
                    provider_type: ch.provider_type.clone(),
                    supported: true,
                    balance_usd: None,
                    used_usd: None,
                    message: Some(format!("failed to reach billing API: {e}")),
                })),
            }
        }
        _ => Ok(Json(BalanceResponse {
            channel_id: channel_id.to_string(),
            provider_type: ch.provider_type.clone(),
            supported: false,
            balance_usd: None,
            used_usd: None,
            message: Some("balance checking not supported for this provider type".into()),
        })),
    }
}

#[derive(Serialize)]
struct BalanceResponse {
    channel_id: String,
    provider_type: String,
    supported: bool,
    balance_usd: Option<f64>,
    used_usd: Option<f64>,
    message: Option<String>,
}

async fn update_channel_balance(app: &AppState, id: ChannelId, balance: f64) {
    if let Some(pool) = app.repos.pool() {
        let _ = sqlx::query(
            "UPDATE channels SET balance = $1, balance_updated_at = NOW() WHERE id = $2",
        )
        .bind(balance)
        .bind(id.as_uuid())
        .execute(pool)
        .await;
    }
}

/// 解析 channel 探测/测试用的 API key。
/// 优先从 DB 加密 key 池取；fallback 到环境变量。
async fn resolve_probe_key(app: &AppState, channel_id: ChannelId, code: &str) -> String {
    if let (Ok(record), Some(crypto)) = (
        app.repos
            .channel_keys
            .find_active_for_channel(channel_id)
            .await,
        app.crypto.as_ref(),
    ) {
        let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
        if let Ok(plaintext) = crypto.open(&record.key_enc, &aad).await
            && let Ok(s) = String::from_utf8(plaintext.to_vec())
        {
            return s;
        }
    }
    // Fallback: 环境变量
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default()
}

async fn resolve_probe_secrets(
    app: &AppState,
    channel_id: ChannelId,
    code: &str,
) -> HashMap<String, String> {
    let mut secrets = gate_providers::CustomHttpProvider::env_secret_slots(code);
    let Some(crypto) = app.crypto.as_ref() else {
        return secrets;
    };
    let Ok(records) = app.repos.channel_keys.list_by_channel(channel_id).await else {
        return secrets;
    };

    let mut active: Vec<_> = records
        .into_iter()
        .filter(|record| record.health == "healthy")
        .filter(|record| {
            record
                .cooldown_until
                .is_none_or(|until| until < chrono::Utc::now())
        })
        .collect();
    active.sort_by_key(|record| (-record.weight, record.created_at));

    let mut best_active: Option<String> = None;
    let mut selected_primary: Option<String> = None;
    for record in active {
        let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
        let Ok(plaintext) = crypto.open(&record.key_enc, &aad).await else {
            continue;
        };
        let Ok(secret) = String::from_utf8(plaintext.to_vec()) else {
            continue;
        };
        best_active.get_or_insert_with(|| secret.clone());
        let slot = record
            .label
            .as_deref()
            .map(normalize_probe_secret_slot)
            .unwrap_or_else(|| "primary".to_string());
        secrets
            .entry(slot.clone())
            .or_insert_with(|| secret.clone());
        if slot == "primary" && selected_primary.is_none() {
            selected_primary = Some(secret);
        }
    }

    if let Some(primary) = selected_primary.or(best_active)
        && !primary.is_empty()
    {
        secrets.insert("primary".to_string(), primary);
    }
    secrets
}

pub(crate) fn normalize_probe_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

// ─── Pricing Rules CRUD ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_key_alias_validation_matches_plugin_secret_slots() {
        assert!(validate_channel_key_alias("client_id").is_ok());
        assert!(validate_channel_key_alias("aws-secret-key").is_ok());
        assert_eq!(normalize_probe_secret_slot("api_key"), "primary");
        assert_eq!(normalize_probe_secret_slot("Client_ID"), "client_id");
        assert!(validate_channel_key_alias("client.id").is_err());
        assert!(validate_channel_key_alias("client id").is_err());
    }
}

#[derive(Deserialize)]
struct PricingRulesQuery {
    channel_id: Option<Uuid>,
    model: Option<String>,
}

#[derive(Serialize)]
struct PricingRuleRow {
    id: String,
    channel_id: Option<String>,
    model: String,
    dimension: String,
    unit: String,
    rate: f64,
    conditions: serde_json::Value,
    effective_from: DateTime<Utc>,
    effective_until: Option<DateTime<Utc>>,
    priority: i32,
    description: Option<String>,
}

fn rule_to_row(r: &gate_billing::PricingRule) -> PricingRuleRow {
    PricingRuleRow {
        id: r.id.to_string(),
        channel_id: r
            .channel_id
            .map(|c| gate_core::id::ChannelId::from(c).to_string()),
        model: r.model.clone(),
        dimension: r.dimension.clone(),
        unit: r.unit.clone(),
        rate: r.rate,
        conditions: r.conditions.clone(),
        effective_from: r.effective_from,
        effective_until: r.effective_until,
        priority: r.priority,
        description: r.description.clone(),
    }
}

async fn list_pricing_rules(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<PricingRulesQuery>,
) -> AppResult<Json<Vec<PricingRuleRow>>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
    let rules = pricing
        .list_rules(q.channel_id, q.model.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(rules.iter().map(rule_to_row).collect()))
}

#[derive(Deserialize)]
struct UpsertPricingRuleRequest {
    id: Option<Uuid>,
    channel_id: Option<Uuid>,
    model: String,
    dimension: String,
    unit: String,
    rate: f64,
    #[serde(default)]
    conditions: serde_json::Value,
    effective_from: Option<DateTime<Utc>>,
    effective_until: Option<DateTime<Utc>>,
    #[serde(default)]
    priority: i32,
    description: Option<String>,
}

async fn upsert_pricing_rule(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<UpsertPricingRuleRequest>,
) -> AppResult<Json<PricingRuleRow>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;

    let rule = gate_billing::PricingRule {
        id: req.id.unwrap_or_else(Uuid::now_v7),
        channel_id: req.channel_id,
        model: req.model,
        dimension: req.dimension,
        unit: req.unit,
        rate: req.rate,
        conditions: if req.conditions.is_null() {
            serde_json::json!({})
        } else {
            req.conditions
        },
        effective_from: req.effective_from.unwrap_or_else(Utc::now),
        effective_until: req.effective_until,
        priority: req.priority,
        description: req.description,
    };

    let saved = pricing
        .upsert_rule(&rule)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    app.audit
        .emit(&ctx, "pricing_rule.upsert", "pricing_rule", None, None);

    Ok(Json(rule_to_row(&saved)))
}

async fn delete_pricing_rule(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<FlexUuid>,
) -> AppResult<Json<serde_json::Value>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;

    let deleted = pricing
        .delete_rule(*id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !deleted {
        return Err(AppError::NotFound);
    }

    app.audit
        .emit(&ctx, "pricing_rule.delete", "pricing_rule", None, None);

    Ok(Json(serde_json::json!({ "deleted": true })))
}
