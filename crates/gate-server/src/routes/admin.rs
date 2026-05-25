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

use crate::audit::{AuditChange, AuditRequestMeta};
use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::flex_uuid::FlexUuid;
use crate::middleware::KooixRequestId;
use crate::routes::invitations::{
    invitation_token_hash, normalize_email, parse_org_invite_role, parse_project_invite_role,
};
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::{Json, Router, http::HeaderMap, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gate_auth::{require, require_user};
use gate_core::id::{ChannelGroupId, ChannelId, ChannelKeyId, OrgId, ProjectId, UserId};
use gate_core::rbac::{Permission, Scope};
use gate_providers::ProviderCapabilities;
use gate_providers::types::{ChatMessage, ChatRequest, MessageContent, Role};
use gate_storage::{
    AuditSortBy, ChannelStatus, CreateChannel, IdentityProviderCreate, IdentityProviderRecord,
    IdentityProviderUpdate, InvitationCreate, InvitationRecord, ListChannelsQuery, SortDirection,
    UpdateChannel, UpdateChannelBinding,
};
use rand::RngCore;
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
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

const CONFIRM_HEADER: &str = "x-kooix-confirm";

#[derive(Serialize)]
pub struct BatchResult {
    pub affected: u64,
}

#[derive(Serialize)]
pub struct ChannelDrainResponse {
    pub channel: ChannelSummary,
    pub inflight: i64,
    pub safe_to_disable: bool,
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
        .route("/channels/:id/drain", axum::routing::post(drain_channel))
        .route("/channels/:id/drain-status", get(get_channel_drain_status))
        .route(
            "/channels/:id/disable-when-idle",
            axum::routing::post(disable_channel_when_idle),
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
        .route(
            "/users/:id/sessions",
            get(list_user_sessions).delete(revoke_user_sessions),
        )
        .route(
            "/users/:id/sessions/:session_id",
            axum::routing::delete(revoke_user_session),
        )
        .route(
            "/identity-providers/discover",
            axum::routing::post(discover_identity_provider),
        )
        .route(
            "/identity-providers",
            get(list_identity_providers).post(create_identity_provider),
        )
        .route(
            "/identity-providers/:id",
            axum::routing::put(update_identity_provider).delete(delete_identity_provider),
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
            "/orgs/:org_id/invitations",
            get(list_org_invitations).post(create_org_invitation),
        )
        .route(
            "/orgs/:org_id/invitations/:invitation_id",
            axum::routing::delete(revoke_org_invitation),
        )
        .route(
            "/orgs/:org_id/projects/:project_id/invitations",
            get(list_project_invitations).post(create_project_invitation),
        )
        .route(
            "/orgs/:org_id/projects/:project_id/invitations/:invitation_id",
            axum::routing::delete(revoke_project_invitation),
        )
        .route(
            "/orgs/:org_id/members/:user_id",
            axum::routing::delete(remove_org_member_handler),
        )
        .route(
            "/pricing-rules",
            get(pricing::list_pricing_rules).post(pricing::upsert_pricing_rule),
        )
        .route(
            "/pricing-rules/:id",
            axum::routing::delete(pricing::delete_pricing_rule),
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

fn channel_audit_snapshot(r: &gate_storage::ChannelRecord) -> serde_json::Value {
    serde_json::json!({
        "id": r.channel_id.to_string(),
        "code": r.code,
        "name": r.name,
        "provider_type": r.provider_type,
        "base_url": r.base_url,
        "status": r.status,
        "health": r.health,
        "supported_models": r.supported_models,
        "rpm_limit": r.rpm_limit,
        "tpm_limit": r.tpm_limit,
        "timeout_ms": r.timeout_ms,
        "max_retries": r.max_retries,
        "tags": r.tags,
        "model_mapping": r.model_mapping,
        "balance": r.balance,
        "last_error": r.last_error,
    })
}

fn key_audit_snapshot(k: &gate_storage::ChannelKeyRecord) -> serde_json::Value {
    serde_json::json!({
        "id": k.id.to_string(),
        "channel_id": k.channel_id.to_string(),
        "label": k.label,
        "fingerprint": k.key_fingerprint,
        "weight": k.weight,
        "health": k.health,
        "total_requests": k.total_requests,
        "total_errors": k.total_errors,
        "consecutive_errors": k.consecutive_errors,
        "last_error_code": k.last_error_code,
    })
}

fn group_audit_snapshot(
    g: &gate_storage::ChannelGroupRecord,
    channel_count: i64,
) -> serde_json::Value {
    serde_json::json!({
        "id": g.group_id.to_string(),
        "name": g.name,
        "description": g.description,
        "strategy": g.strategy,
        "enabled": g.enabled,
        "fallback_group_id": g.fallback_group_id.map(|fb| fb.to_string()),
        "channel_count": channel_count,
    })
}

fn pricing_rule_audit_snapshot(r: &gate_billing::PricingRule) -> serde_json::Value {
    serde_json::json!({
        "id": r.id.to_string(),
        "channel_id": r.channel_id.map(|c| gate_core::id::ChannelId::from(c).to_string()),
        "model": r.model,
        "dimension": r.dimension,
        "unit": r.unit,
        "rate": r.rate,
        "conditions": r.conditions,
        "effective_from": r.effective_from,
        "effective_until": r.effective_until,
        "priority": r.priority,
        "description": r.description,
    })
}

fn user_audit_snapshot(u: &gate_core::identity::User) -> serde_json::Value {
    serde_json::json!({
        "id": u.id.to_string(),
        "email": u.email,
        "display_name": u.display_name,
        "status": format!("{:?}", u.status).to_lowercase(),
        "mfa_enabled": u.mfa_enabled,
        "last_login_at": u.last_login_at,
    })
}

fn confirmation_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONFIRM_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn require_confirmation(headers: &HeaderMap, expected: impl AsRef<str>) -> AppResult<()> {
    let expected = expected.as_ref();
    match confirmation_from_headers(headers) {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(AppError::BadRequest(format!(
            "confirmation required: set {CONFIRM_HEADER}: {expected}"
        ))),
    }
}

fn audit_meta(
    request_id: Option<Extension<KooixRequestId>>,
    headers: &HeaderMap,
) -> AuditRequestMeta {
    AuditRequestMeta::from_parts(request_id.map(|Extension(id)| id), headers, None)
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
        "vertex",
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

fn channel_inflight(app: &AppState, channel_id: ChannelId) -> i64 {
    app.provider_router
        .as_ref()
        .map(|router| router.inflight_tracker().current(channel_id))
        .unwrap_or(0)
}

async fn drain_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let record = app
        .repos
        .channels
        .set_status(channel_id, ChannelStatus::Draining)
        .await?;
    let inflight = channel_inflight(&app, channel_id);

    app.audit.emit(
        &ctx,
        "channel.drain",
        "channel",
        Some(*id),
        Some(serde_json::json!({"inflight": inflight})),
    );

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: inflight <= 0,
    }))
}

async fn get_channel_drain_status(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let record = app.repos.channels.find_by_id(channel_id).await?;
    let inflight = channel_inflight(&app, channel_id);

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: inflight <= 0,
    }))
}

async fn disable_channel_when_idle(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let current = app.repos.channels.find_by_id(channel_id).await?;
    let inflight = channel_inflight(&app, channel_id);
    if inflight > 0 {
        return Err(AppError::BadRequest(format!(
            "channel still has {inflight} inflight request(s)"
        )));
    }

    let record = app
        .repos
        .channels
        .set_status(channel_id, ChannelStatus::Disabled)
        .await?;

    app.audit.emit(
        &ctx,
        "channel.disable_when_idle",
        "channel",
        Some(*id),
        Some(serde_json::json!({"previous_status": current.status, "inflight": inflight})),
    );

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: true,
    }))
}

async fn update_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let before = app.repos.channels.find_by_id(channel_id).await?;
    if (req.model_mapping.is_some() || req.base_url.is_some())
        && is_plugin_provider(&before.provider_type)
    {
        let mapping = req
            .model_mapping
            .clone()
            .unwrap_or_else(|| before.model_mapping.clone());
        let base_url = req.base_url.as_deref().unwrap_or(&before.base_url);
        gate_providers::validate_plugin_manifest(mapping, base_url)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
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

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel.update",
        resource_kind: "channel",
        resource_id: Some(*id),
        before: Some(channel_audit_snapshot(&before)),
        after: Some(channel_audit_snapshot(&record)),
    });

    Ok(Json(record_to_summary(record)))
}

fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

async fn delete_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let before = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("delete:{}", before.code))?;
    app.repos.channels.soft_delete(channel_id).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel.delete",
        resource_kind: "channel",
        resource_id: Some(*id),
        before: Some(channel_audit_snapshot(&before)),
        after: Some(serde_json::json!({
            "id": before.channel_id.to_string(),
            "code": before.code,
            "deleted": true,
            "status": "disabled",
        })),
    });

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
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

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
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
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
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("rotate:{}", channel.code))?;
    let before_keys = app.repos.channel_keys.list_by_channel(channel_id).await?;

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
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_key.rotate",
        resource_kind: "channel_key",
        resource_id: Some(*key_id.as_uuid()),
        before: Some(serde_json::json!({
            "channel_id": channel_id.to_string(),
            "channel_code": channel.code,
            "keys": before_keys.iter().map(key_audit_snapshot).collect::<Vec<_>>(),
        })),
        after: Some(serde_json::json!({
            "channel_id": channel_id.to_string(),
            "new_key_id": key_id.to_string(),
            "new_fingerprint": fingerprint,
            "alias": req.alias,
        })),
    });

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
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    // 验证 channel 存在
    let channel_id = ChannelId::from(id.0);
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("revoke:{}", key_id.0))?;
    let before = app
        .repos
        .channel_keys
        .list_by_channel(channel_id)
        .await?
        .into_iter()
        .find(|k| *k.id.as_uuid() == key_id.0)
        .ok_or(AppError::NotFound)?;

    let ck_id = ChannelKeyId::from(key_id.0);
    app.repos.channel_keys.revoke(ck_id).await?;
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_key.revoke",
        resource_kind: "channel_key",
        resource_id: Some(*key_id),
        before: Some(key_audit_snapshot(&before)),
        after: Some(serde_json::json!({
            "id": key_id.0.to_string(),
            "channel_id": channel_id.to_string(),
            "channel_code": channel.code,
            "revoked": true,
            "health": "disabled",
        })),
    });

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
    #[serde(default = "default_audit_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_audit_sort_dir")]
    pub sort_dir: String,
}

fn default_limit() -> i64 {
    50
}

fn default_audit_sort_by() -> String {
    "ts".into()
}

fn default_audit_sort_dir() -> String {
    "desc".into()
}

fn parse_audit_sort_by(value: &str) -> AuditSortBy {
    match value {
        "actor_kind" => AuditSortBy::ActorKind,
        "action" => AuditSortBy::Action,
        "resource_kind" => AuditSortBy::ResourceKind,
        "outcome" => AuditSortBy::Outcome,
        _ => AuditSortBy::Ts,
    }
}

fn parse_sort_dir(value: &str, default: SortDirection) -> SortDirection {
    if value.eq_ignore_ascii_case("asc") {
        SortDirection::Asc
    } else if value.eq_ignore_ascii_case("desc") {
        SortDirection::Desc
    } else {
        default
    }
}

#[derive(Serialize)]
pub struct AuditLogView {
    pub id: String,
    pub ts: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub actor_ip: Option<String>,
    pub actor_user_agent: Option<String>,
    pub request_id: Option<String>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<String>,
    pub org_id: Option<String>,
    pub project_id: Option<String>,
    pub outcome: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub error_message: Option<String>,
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
    let sort_by = parse_audit_sort_by(&q.sort_by);
    let sort_dir = parse_sort_dir(&q.sort_dir, SortDirection::Desc);

    let records = if let Some(org_id) = q.org_id {
        app.repos
            .audit
            .list_by_org_sorted(org_id, limit, offset, sort_by, sort_dir)
            .await?
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
                actor_ip: r.actor_ip,
                actor_user_agent: r.actor_user_agent,
                request_id: r.request_id.map(|u| u.to_string()),
                action: r.action,
                resource_kind: r.resource_kind,
                resource_id: r.resource_id.map(|u| u.to_string()),
                org_id: r.org_id.map(|u| u.to_string()),
                project_id: r.project_id.map(|u| u.to_string()),
                outcome: r.outcome,
                before: r.before,
                after: r.after,
                error_message: r.error_message,
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

#[derive(Serialize)]
pub struct UserSessionView {
    pub id: String,
    pub user_id: String,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}

#[derive(Serialize)]
pub struct RevokeSessionsResponse {
    pub revoked: u64,
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
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
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

    let before = app.repos.users.find_by_id(user_id).await?;
    if before.status != gate_core::identity::UserStatus::Suspended && req.status == "suspended" {
        require_confirmation(&headers, format!("suspend:{}", before.email))?;
    }
    let user = app.repos.users.update_status(user_id, &req.status).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "user.update_status",
        resource_kind: "user",
        resource_id: Some(*id),
        before: Some(user_audit_snapshot(&before)),
        after: Some(user_audit_snapshot(&user)),
    });

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

async fn list_user_sessions(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<UserSessionView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    app.repos.users.find_by_id(user_id).await?;
    let current_session = ctx.session_id();
    let sessions = app.repos.sessions.list_active_for_user(user_id).await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|session| UserSessionView {
                id: session.id.to_string(),
                user_id: session.user_id.to_string(),
                user_agent: session.user_agent,
                ip: session.ip.map(|ip| ip.to_string()),
                created_at: session.created_at,
                last_used_at: session.last_used_at,
                expires_at: session.expires_at,
                current: current_session == Some(session.id),
            })
            .collect(),
    ))
}

async fn revoke_user_session(
    State(app): State<AppState>,
    Path((id, session_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<RevokeSessionsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    app.repos
        .sessions
        .revoke_for_user(user_id, session_id.0)
        .await?;

    app.audit.emit(
        &ctx,
        "user_session.revoke",
        "user_session",
        Some(session_id.0),
        Some(serde_json::json!({"user_id": user_id.to_string()})),
    );

    Ok(Json(RevokeSessionsResponse { revoked: 1 }))
}

async fn revoke_user_sessions(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<RevokeSessionsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let user_id = UserId::from(id.0);
    let revoked = app.repos.sessions.revoke_user_sessions(user_id).await?;

    app.audit.emit(
        &ctx,
        "user_session.revoke_all",
        "user",
        Some(*user_id.as_uuid()),
        Some(serde_json::json!({"revoked": revoked})),
    );

    Ok(Json(RevokeSessionsResponse { revoked }))
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
// Identity Providers / SSO (Admin)
// ============================================================================

#[derive(Deserialize)]
pub struct IdentityProvidersQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Serialize)]
pub struct IdentityProviderView {
    pub id: String,
    pub org_id: Option<String>,
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub name_claim: String,
    pub subject_claim: String,
    pub auto_create_users: bool,
    pub auto_join_org_role: Option<String>,
    pub email_domain_allowlist: Vec<String>,
    pub enabled: bool,
    pub redirect_policy: RedirectPolicyView,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RedirectPolicyView {
    #[serde(default = "default_true")]
    pub allow_relative: bool,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreateIdentityProviderRequest {
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub email_claim: Option<String>,
    #[serde(default)]
    pub name_claim: Option<String>,
    #[serde(default)]
    pub subject_claim: Option<String>,
    #[serde(default = "default_true")]
    pub auto_create_users: bool,
    #[serde(default)]
    pub auto_join_org_role: Option<String>,
    #[serde(default)]
    pub email_domain_allowlist: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub redirect_policy: Option<RedirectPolicyView>,
}

#[derive(Deserialize)]
pub struct UpdateIdentityProviderRequest {
    #[serde(default)]
    pub org_id: Option<Option<String>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub email_claim: Option<String>,
    #[serde(default)]
    pub name_claim: Option<String>,
    #[serde(default)]
    pub subject_claim: Option<String>,
    #[serde(default)]
    pub auto_create_users: Option<bool>,
    #[serde(default)]
    pub auto_join_org_role: Option<Option<String>>,
    #[serde(default)]
    pub email_domain_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub redirect_policy: Option<RedirectPolicyView>,
}

#[derive(Deserialize)]
pub struct DiscoverIdentityProviderRequest {
    pub issuer: String,
}

#[derive(Serialize)]
pub struct DiscoverIdentityProviderResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub scopes_supported: Vec<String>,
}

async fn list_identity_providers(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<IdentityProvidersQuery>,
) -> AppResult<Json<Vec<IdentityProviderView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let providers = app
        .repos
        .identity_providers
        .list(q.limit.clamp(1, 200), q.offset.max(0))
        .await?;
    Ok(Json(
        providers
            .into_iter()
            .map(identity_provider_to_view)
            .collect(),
    ))
}

async fn create_identity_provider(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateIdentityProviderRequest>,
) -> AppResult<Json<IdentityProviderView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let id = Uuid::now_v7();
    let org_id = parse_optional_org_id(req.org_id.as_deref())?;
    let name = normalize_non_empty(req.name, "name")?;
    let slug = normalize_slug(&req.slug)?;
    let issuer = normalize_https_url(req.issuer, "issuer")?;
    let client_id = normalize_non_empty(req.client_id, "client_id")?;
    let scopes = normalize_scopes(Some(req.scopes));
    let email_claim = normalize_claim(req.email_claim, "email")?;
    let name_claim = normalize_claim(req.name_claim, "name")?;
    let subject_claim = normalize_claim(req.subject_claim, "sub")?;
    let auto_join_org_role = normalize_optional_org_role(req.auto_join_org_role)?;
    let email_domain_allowlist = normalize_domain_allowlist(Some(req.email_domain_allowlist))?;
    let redirect_policy = normalize_redirect_policy(req.redirect_policy.unwrap_or_default())?;
    let client_secret_enc = seal_idp_secret(&app, id, &req.client_secret).await?;

    let provider = app
        .repos
        .identity_providers
        .create(IdentityProviderCreate {
            id,
            org_id,
            name,
            slug,
            issuer,
            client_id,
            client_secret_enc,
            scopes,
            email_claim,
            name_claim,
            subject_claim,
            auto_create_users: req.auto_create_users,
            auto_join_org_role,
            email_domain_allowlist,
            enabled: req.enabled,
            metadata: redirect_policy_metadata(&redirect_policy),
        })
        .await?;

    app.audit.emit(
        &ctx,
        "identity_provider.create",
        "identity_provider",
        Some(provider.id),
        Some(serde_json::json!({
            "slug": &provider.slug,
            "org_id": provider.org_id.map(|id| id.to_string()),
            "enabled": provider.enabled
        })),
    );

    Ok(Json(identity_provider_to_view(provider)))
}

async fn update_identity_provider(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateIdentityProviderRequest>,
) -> AppResult<Json<IdentityProviderView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let client_secret_enc = match req.client_secret {
        Some(secret) if !secret.trim().is_empty() => {
            Some(seal_idp_secret(&app, id.0, &secret).await?)
        }
        Some(_) => return Err(AppError::BadRequest("client_secret cannot be empty".into())),
        None => None,
    };
    let org_id = match req.org_id {
        Some(Some(raw)) => Some(parse_optional_org_id(Some(&raw))?),
        Some(None) => Some(None),
        None => None,
    };
    let auto_join_org_role = match req.auto_join_org_role {
        Some(role) => Some(normalize_optional_org_role(role)?),
        None => None,
    };
    let redirect_policy = req
        .redirect_policy
        .map(normalize_redirect_policy)
        .transpose()?;

    let provider = app
        .repos
        .identity_providers
        .update(
            id.0,
            IdentityProviderUpdate {
                org_id,
                name: req
                    .name
                    .map(|v| normalize_non_empty(v, "name"))
                    .transpose()?,
                slug: req.slug.map(|v| normalize_slug(&v)).transpose()?,
                issuer: req
                    .issuer
                    .map(|v| normalize_https_url(v, "issuer"))
                    .transpose()?,
                client_id: req
                    .client_id
                    .map(|v| normalize_non_empty(v, "client_id"))
                    .transpose()?,
                client_secret_enc,
                scopes: req.scopes.map(|v| normalize_scopes(Some(v))),
                email_claim: req
                    .email_claim
                    .map(|v| normalize_claim(Some(v), "email"))
                    .transpose()?,
                name_claim: req
                    .name_claim
                    .map(|v| normalize_claim(Some(v), "name"))
                    .transpose()?,
                subject_claim: req
                    .subject_claim
                    .map(|v| normalize_claim(Some(v), "sub"))
                    .transpose()?,
                auto_create_users: req.auto_create_users,
                auto_join_org_role,
                email_domain_allowlist: req
                    .email_domain_allowlist
                    .map(|v| normalize_domain_allowlist(Some(v)))
                    .transpose()?,
                enabled: req.enabled,
                metadata: redirect_policy.map(|p| redirect_policy_metadata(&p)),
            },
        )
        .await?;

    app.audit.emit(
        &ctx,
        "identity_provider.update",
        "identity_provider",
        Some(id.0),
        Some(serde_json::json!({
            "slug": &provider.slug,
            "enabled": provider.enabled
        })),
    );

    Ok(Json(identity_provider_to_view(provider)))
}

async fn delete_identity_provider(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    app.repos.identity_providers.soft_delete(id.0).await?;
    app.audit.emit(
        &ctx,
        "identity_provider.delete",
        "identity_provider",
        Some(id.0),
        None,
    );
    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn discover_identity_provider(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<DiscoverIdentityProviderRequest>,
) -> AppResult<Json<DiscoverIdentityProviderResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let issuer = normalize_https_url(req.issuer, "issuer")?;
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let resp = client
        .get(&discovery_url)
        .send()
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::BadRequest(format!(
            "OIDC discovery returned HTTP {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery JSON invalid: {e}")))?;

    let discovered_issuer = required_json_string(&body, "issuer")?;
    if discovered_issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(AppError::BadRequest(
            "OIDC discovery issuer does not match requested issuer".into(),
        ));
    }
    let out = DiscoverIdentityProviderResponse {
        issuer: discovered_issuer,
        authorization_endpoint: required_json_string(&body, "authorization_endpoint")?,
        token_endpoint: required_json_string(&body, "token_endpoint")?,
        jwks_uri: required_json_string(&body, "jwks_uri")?,
        scopes_supported: body
            .get("scopes_supported")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect(),
    };

    app.audit.emit(
        &ctx,
        "identity_provider.discover",
        "identity_provider",
        None,
        Some(serde_json::json!({"issuer": &out.issuer})),
    );

    Ok(Json(out))
}

impl Default for RedirectPolicyView {
    fn default() -> Self {
        Self {
            allow_relative: true,
            allowed_origins: vec![],
        }
    }
}

fn identity_provider_to_view(p: IdentityProviderRecord) -> IdentityProviderView {
    IdentityProviderView {
        id: p.id.to_string(),
        org_id: p.org_id.map(|id| id.to_string()),
        name: p.name,
        slug: p.slug,
        issuer: p.issuer,
        client_id: p.client_id,
        scopes: p.scopes,
        email_claim: p.email_claim,
        name_claim: p.name_claim,
        subject_claim: p.subject_claim,
        auto_create_users: p.auto_create_users,
        auto_join_org_role: p.auto_join_org_role,
        email_domain_allowlist: p.email_domain_allowlist,
        enabled: p.enabled,
        redirect_policy: redirect_policy_from_metadata(&p.metadata),
    }
}

async fn seal_idp_secret(app: &AppState, provider_id: Uuid, secret: &str) -> AppResult<Vec<u8>> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(AppError::BadRequest("client_secret is required".into()));
    }
    let crypto = app
        .crypto
        .as_ref()
        .ok_or_else(|| AppError::Internal("crypto KMS not configured".into()))?;
    let aad = gate_crypto::aad::idp_secret(provider_id);
    crypto
        .seal(secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("client_secret encrypt: {e}")))
}

fn normalize_non_empty(value: String, field: &str) -> AppResult<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(format!("{field} is required")));
    }
    Ok(trimmed)
}

fn normalize_slug(raw: &str) -> AppResult<String> {
    let slug = raw.trim().to_ascii_lowercase();
    if slug.is_empty()
        || slug.len() > 64
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err(AppError::BadRequest(
            "slug must be 1-64 chars: lowercase letters, digits, hyphen".into(),
        ));
    }
    Ok(slug)
}

fn normalize_https_url(value: String, field: &str) -> AppResult<String> {
    let url = value.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("https://")
        || url.starts_with("http://localhost")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://[::1]"))
    {
        return Err(AppError::BadRequest(format!(
            "{field} must use https, except localhost development"
        )));
    }
    Ok(url)
}

fn normalize_scopes(scopes: Option<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = scopes
        .unwrap_or_default()
        .into_iter()
        .flat_map(|s| {
            s.split([',', ' ', '\n', '\t'])
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    if out.is_empty() {
        out = vec!["openid".into(), "email".into(), "profile".into()];
    }
    out.sort();
    out.dedup();
    out
}

fn normalize_claim(value: Option<String>, default: &str) -> AppResult<String> {
    let value = value.unwrap_or_else(|| default.to_string());
    normalize_non_empty(value, "claim")
}

fn normalize_optional_org_role(role: Option<String>) -> AppResult<Option<String>> {
    let Some(role) = role
        .map(|r| r.trim().to_ascii_lowercase())
        .filter(|r| !r.is_empty())
    else {
        return Ok(None);
    };
    let valid = ["owner", "admin", "billing_viewer", "member"];
    if !valid.contains(&role.as_str()) {
        return Err(AppError::BadRequest(format!(
            "auto_join_org_role must be one of: {valid:?}"
        )));
    }
    Ok(Some(role))
}

fn normalize_domain_allowlist(values: Option<Vec<String>>) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for value in values.unwrap_or_default() {
        for part in value.split([',', '\n', ' ', '\t']) {
            let domain = part.trim().trim_start_matches('@').to_ascii_lowercase();
            if domain.is_empty() {
                continue;
            }
            if domain.contains('/') || !domain.contains('.') {
                return Err(AppError::BadRequest(format!(
                    "invalid email domain: {domain}"
                )));
            }
            out.push(domain);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn normalize_redirect_policy(policy: RedirectPolicyView) -> AppResult<RedirectPolicyView> {
    let mut origins = Vec::new();
    for origin in policy.allowed_origins {
        let origin = normalize_origin(&origin)?;
        origins.push(origin);
    }
    origins.sort();
    origins.dedup();
    Ok(RedirectPolicyView {
        allow_relative: policy.allow_relative,
        allowed_origins: origins,
    })
}

fn normalize_origin(raw: &str) -> AppResult<String> {
    let value = raw.trim().trim_end_matches('/').to_ascii_lowercase();
    let (scheme, rest) = value
        .split_once("://")
        .ok_or_else(|| AppError::BadRequest("redirect origin must include scheme".into()))?;
    if scheme != "https" && scheme != "http" {
        return Err(AppError::BadRequest(
            "redirect origin scheme must be http or https".into(),
        ));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err(AppError::BadRequest("redirect origin host invalid".into()));
    }
    Ok(format!("{scheme}://{host}"))
}

fn redirect_policy_metadata(policy: &RedirectPolicyView) -> serde_json::Value {
    serde_json::json!({
        "redirect_policy": {
            "allow_relative": policy.allow_relative,
            "allowed_origins": policy.allowed_origins,
        }
    })
}

fn redirect_policy_from_metadata(metadata: &serde_json::Value) -> RedirectPolicyView {
    let obj = metadata.get("redirect_policy").unwrap_or(metadata);
    RedirectPolicyView {
        allow_relative: obj
            .get("allow_relative")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        allowed_origins: obj
            .get("allowed_origins")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .filter_map(|v| normalize_origin(v).ok())
            .collect(),
    }
}

fn parse_optional_org_id(raw: Option<&str>) -> AppResult<Option<gate_core::id::OrgId>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    raw.parse::<gate_core::id::OrgId>()
        .map(Some)
        .map_err(|_| AppError::BadRequest("invalid org_id".into()))
}

fn required_json_string(body: &serde_json::Value, key: &str) -> AppResult<String> {
    body.get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("OIDC discovery missing {key}")))
}

// ============================================================================
// Channel Groups (Admin)
// ============================================================================

#[derive(Clone, Serialize)]
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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fallback_group_id: Option<String>,
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
    pub canary_percent_bps: Option<i32>,
    pub model_filter: Vec<String>,
    pub enabled: bool,
    pub channel_status: String,
    pub channel_health: String,
}

#[derive(Serialize)]
pub struct CanaryStatsView {
    pub channel_id: String,
    pub channel_code: String,
    pub channel_name: String,
    pub provider_type: String,
    pub canary_percent_bps: Option<i32>,
    pub is_canary: bool,
    pub requests: i64,
    pub error_rate: f64,
    pub avg_latency_ms: Option<f64>,
    pub avg_cost_micros: Option<f64>,
}

#[derive(Serialize)]
pub struct FallbackChainNodeView {
    pub id: String,
    pub name: String,
    pub strategy: String,
    pub enabled: bool,
    pub channel_count: i64,
    pub requests: i64,
    pub share: f64,
    pub is_fallback: bool,
}

#[derive(Serialize)]
pub struct FallbackStatsView {
    pub window_hours: i64,
    pub total_requests: i64,
    pub primary_requests: i64,
    pub fallback_requests: i64,
    pub fallback_hit_rate: f64,
    pub has_cycle: bool,
    pub cycle_at: Option<String>,
}

const VALID_GROUP_STRATEGIES: [&str; 5] = [
    "priority",
    "weighted_random",
    "round_robin",
    "least_conn",
    "least_latency",
];
const MAX_FALLBACK_DEPTH: usize = 5;
const FALLBACK_STATS_WINDOW_HOURS: i64 = 24;

#[derive(Deserialize)]
pub struct AddBindingRequest {
    pub channel_id: Uuid,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    #[serde(default)]
    pub canary_percent_bps: Option<i32>,
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

fn validate_group_strategy(strategy: &str) -> AppResult<()> {
    if !VALID_GROUP_STRATEGIES.contains(&strategy) {
        return Err(AppError::BadRequest(format!(
            "strategy must be one of: {VALID_GROUP_STRATEGIES:?}"
        )));
    }
    Ok(())
}

fn parse_channel_group_id(value: &str, field: &str) -> AppResult<ChannelGroupId> {
    value
        .parse::<ChannelGroupId>()
        .map_err(|_| AppError::BadRequest(format!("invalid {field} UUID")))
}

async fn ensure_group_exists(app: &AppState, gid: ChannelGroupId, message: &str) -> AppResult<()> {
    app.repos
        .channel_groups
        .find_by_id(gid)
        .await
        .map(|_| ())
        .map_err(|e| match e {
            gate_storage::DbError::NotFound => AppError::BadRequest(message.into()),
            other => AppError::Db(other),
        })
}

async fn validate_fallback_target(
    app: &AppState,
    gid: ChannelGroupId,
    fallback: Option<ChannelGroupId>,
) -> AppResult<()> {
    validate_fallback_chain(app, Some(gid), fallback).await
}

async fn validate_fallback_chain(
    app: &AppState,
    source: Option<ChannelGroupId>,
    fallback: Option<ChannelGroupId>,
) -> AppResult<()> {
    let Some(fallback) = fallback else {
        return Ok(());
    };

    if Some(fallback) == source {
        return Err(AppError::BadRequest(
            "fallback_group_id cannot point to itself".into(),
        ));
    }
    ensure_group_exists(app, fallback, "fallback group not found").await?;

    let mut visited = source.into_iter().collect::<HashSet<_>>();
    let mut current = fallback;
    let mut depth = 1usize;
    loop {
        if !visited.insert(current) {
            return Err(AppError::BadRequest(format!(
                "fallback cycle detected at {current}"
            )));
        }
        if depth >= MAX_FALLBACK_DEPTH {
            return Err(AppError::BadRequest(format!(
                "fallback chain exceeds max depth {MAX_FALLBACK_DEPTH}"
            )));
        }
        let group = app
            .repos
            .channel_groups
            .find_by_id(current)
            .await
            .map_err(|e| match e {
                gate_storage::DbError::NotFound => {
                    AppError::BadRequest("fallback group not found".into())
                }
                other => AppError::Db(other),
            })?;
        match group.fallback_group_id {
            Some(next) => {
                current = next;
                depth += 1;
            }
            None => return Ok(()),
        }
    }
}

async fn build_fallback_chain_records(
    app: &AppState,
    root: gate_storage::ChannelGroupRecord,
) -> AppResult<(
    Vec<gate_storage::ChannelGroupRecord>,
    bool,
    Option<ChannelGroupId>,
)> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = root;
    let mut depth = 0usize;

    loop {
        if !visited.insert(current.group_id) {
            return Ok((chain, true, Some(current.group_id)));
        }
        let next = current.fallback_group_id;
        chain.push(current);
        let Some(next_id) = next else {
            return Ok((chain, false, None));
        };
        if depth >= MAX_FALLBACK_DEPTH {
            return Ok((chain, true, Some(next_id)));
        }
        current = match app.repos.channel_groups.find_by_id(next_id).await {
            Ok(group) => group,
            Err(gate_storage::DbError::NotFound) => return Ok((chain, false, None)),
            Err(e) => return Err(AppError::Db(e)),
        };
        depth += 1;
    }
}

async fn fallback_request_counts(
    app: &AppState,
    group_ids: &[ChannelGroupId],
    window_hours: i64,
) -> AppResult<HashMap<ChannelGroupId, i64>> {
    let Some(pool) = app.repos.pool() else {
        return Ok(HashMap::new());
    };
    if group_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let ids: Vec<Uuid> = group_ids.iter().map(|id| *id.as_uuid()).collect();
    let rows = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT group_id, COUNT(*)::BIGINT AS requests \
         FROM request_events \
         WHERE group_id = ANY($1) \
           AND ts >= NOW() - ($2::BIGINT * INTERVAL '1 hour') \
         GROUP BY group_id",
    )
    .bind(&ids)
    .bind(window_hours)
    .fetch_all(pool)
    .await
    .map_err(gate_storage::DbError::from)?;

    Ok(rows
        .into_iter()
        .map(|(id, count)| (ChannelGroupId::from(id), count))
        .collect())
}

async fn canary_stats_for_bindings(
    app: &AppState,
    group_id: ChannelGroupId,
    bindings: &[BindingView],
    window_hours: i64,
) -> AppResult<Vec<CanaryStatsView>> {
    let mut metrics: HashMap<Uuid, (i64, i64, Option<f64>, Option<f64>)> = HashMap::new();
    if let Some(pool) = app.repos.pool()
        && !bindings.is_empty()
    {
        let channel_ids: Vec<Uuid> = bindings
            .iter()
            .filter_map(|binding| binding.channel_id.parse::<ChannelId>().ok())
            .map(|id| *id.as_uuid())
            .collect();
        if !channel_ids.is_empty() {
            let rows = sqlx::query(
                "SELECT channel_id, \
                        COUNT(*)::BIGINT AS requests, \
                        COUNT(*) FILTER (WHERE status >= 400 OR error_code IS NOT NULL)::BIGINT AS errors, \
                        AVG(latency_ms)::float8 AS avg_latency_ms, \
                        AVG(cost_micros)::float8 AS avg_cost_micros \
                 FROM request_events \
                 WHERE group_id = $1 \
                   AND channel_id = ANY($2) \
                   AND ts >= NOW() - ($3::BIGINT * INTERVAL '1 hour') \
                 GROUP BY channel_id",
            )
            .bind(group_id.as_uuid())
            .bind(&channel_ids)
            .bind(window_hours)
            .fetch_all(pool)
            .await
            .map_err(gate_storage::DbError::from)?;

            for row in rows {
                let channel_id: Uuid = row
                    .try_get("channel_id")
                    .map_err(gate_storage::DbError::from)?;
                let requests: i64 = row
                    .try_get("requests")
                    .map_err(gate_storage::DbError::from)?;
                let errors: i64 = row.try_get("errors").map_err(gate_storage::DbError::from)?;
                let avg_latency_ms: Option<f64> = row.try_get("avg_latency_ms").unwrap_or(None);
                let avg_cost_micros: Option<f64> = row.try_get("avg_cost_micros").unwrap_or(None);
                metrics.insert(
                    channel_id,
                    (requests, errors, avg_latency_ms, avg_cost_micros),
                );
            }
        }
    }

    Ok(bindings
        .iter()
        .filter_map(|binding| {
            let channel_id = binding.channel_id.parse::<ChannelId>().ok()?;
            let (requests, errors, avg_latency_ms, avg_cost_micros) = metrics
                .get(channel_id.as_uuid())
                .copied()
                .unwrap_or((0, 0, None, None));
            Some(CanaryStatsView {
                channel_id: binding.channel_id.clone(),
                channel_code: binding.channel_code.clone(),
                channel_name: binding.channel_name.clone(),
                provider_type: binding.provider_type.clone(),
                canary_percent_bps: binding.canary_percent_bps,
                is_canary: binding.canary_percent_bps.is_some(),
                requests,
                error_rate: if requests > 0 {
                    errors as f64 / requests as f64
                } else {
                    0.0
                },
                avg_latency_ms,
                avg_cost_micros,
            })
        })
        .collect())
}

async fn group_channel_count(app: &AppState, gid: ChannelGroupId) -> AppResult<i64> {
    Ok(app.repos.channel_groups.list_bindings(gid).await?.len() as i64)
}

fn validate_canary_percent_bps(canary: Option<i32>) -> AppResult<()> {
    if let Some(bps) = canary
        && !(100..=500).contains(&bps)
    {
        return Err(AppError::BadRequest(
            "canary_percent_bps must be between 100 and 500 (1%-5%), or null".into(),
        ));
    }
    Ok(())
}

async fn create_group(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    validate_group_strategy(&req.strategy)?;
    let requested_fallback = req
        .fallback_group_id
        .as_deref()
        .map(|id| parse_channel_group_id(id, "fallback_group_id"))
        .transpose()?;
    validate_fallback_chain(&app, None, requested_fallback).await?;

    let mut g = app
        .repos
        .channel_groups
        .create(&req.name, &req.strategy)
        .await?;
    if req.description.is_some() || requested_fallback.is_some() {
        g = app
            .repos
            .channel_groups
            .update(
                g.group_id,
                None,
                None,
                None,
                if requested_fallback.is_some() {
                    Some(requested_fallback)
                } else {
                    None
                },
                req.description.as_deref(),
            )
            .await?;
    }
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
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpdateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    if let Some(ref s) = req.strategy {
        validate_group_strategy(s)?;
    }

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let before = app.repos.channel_groups.find_by_id(gid).await?;
    let before_count = group_channel_count(&app, gid).await?;
    if before.enabled && req.enabled == Some(false) {
        require_confirmation(&headers, format!("disable:{}", before.name))?;
    }

    // Parse fallback_group_id: Option<Option<String>> -> Option<Option<ChannelGroupId>>
    let fallback: Option<Option<ChannelGroupId>> = match req.fallback_group_id {
        None => None,             // don't change
        Some(None) => Some(None), // clear
        Some(Some(ref s)) => {
            let fb = parse_channel_group_id(s, "fallback_group_id")?;
            Some(Some(fb))
        }
    };
    if fallback.is_some() {
        validate_fallback_target(&app, gid, fallback.flatten()).await?;
    }

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
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_group.update",
        resource_kind: "channel_group",
        resource_id: Some(*id),
        before: Some(group_audit_snapshot(&before, before_count)),
        after: Some(group_audit_snapshot(&g, bindings.len() as i64)),
    });

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
        canary_percent_bps: b.canary_percent_bps,
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
    validate_canary_percent_bps(req.canary_percent_bps)?;
    app.repos
        .channel_groups
        .add_binding(
            gid,
            cid,
            req.priority.unwrap_or(100),
            req.weight.unwrap_or(1),
            req.canary_percent_bps,
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
    #[serde(default, deserialize_with = "deserialize_optional_json_patch")]
    pub canary_percent_bps: Option<serde_json::Value>,
    pub model_filter: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

fn deserialize_optional_json_patch<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

fn parse_canary_percent_bps_patch(
    value: Option<serde_json::Value>,
) -> AppResult<Option<Option<i32>>> {
    match value {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::Number(n)) => {
            let bps = n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| {
                    AppError::BadRequest("canary_percent_bps must be an integer".into())
                })?;
            Ok(Some(Some(bps)))
        }
        Some(_) => Err(AppError::BadRequest(
            "canary_percent_bps must be an integer or null".into(),
        )),
    }
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
    let canary_percent_bps = parse_canary_percent_bps_patch(req.canary_percent_bps)?;
    if let Some(canary) = canary_percent_bps {
        validate_canary_percent_bps(canary)?;
    }
    app.repos
        .channel_groups
        .update_binding(
            gid,
            cid,
            UpdateChannelBinding {
                priority: req.priority,
                weight: req.weight,
                canary_percent_bps,
                model_filter: req.model_filter,
                enabled: req.enabled,
            },
        )
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Serialize)]
pub struct GroupDetailView {
    #[serde(flatten)]
    pub group_fields: GroupView,
    pub group: GroupView,
    pub bindings: Vec<BindingView>,
    pub projects_using: Vec<String>,
    pub project_ids: Vec<String>,
    pub fallback_chain: Vec<FallbackChainNodeView>,
    pub fallback_stats: FallbackStatsView,
    pub canary_stats: Vec<CanaryStatsView>,
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
    let canary_stats =
        canary_stats_for_bindings(&app, gid, &binding_views, FALLBACK_STATS_WINDOW_HOURS).await?;
    let group_view = GroupView {
        id: g.group_id.to_string(),
        name: g.name.clone(),
        description: g.description.clone(),
        strategy: g.strategy.clone(),
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: binding_views.len() as i64,
        created_at: g.created_at,
        updated_at: g.updated_at,
    };

    let (chain_records, has_cycle, cycle_at) = build_fallback_chain_records(&app, g).await?;
    let chain_group_ids: Vec<ChannelGroupId> =
        chain_records.iter().map(|group| group.group_id).collect();
    let request_counts =
        fallback_request_counts(&app, &chain_group_ids, FALLBACK_STATS_WINDOW_HOURS).await?;
    let total_requests: i64 = chain_group_ids
        .iter()
        .map(|gid| request_counts.get(gid).copied().unwrap_or_default())
        .sum();
    let primary_requests = request_counts.get(&gid).copied().unwrap_or_default();
    let fallback_requests = total_requests.saturating_sub(primary_requests);
    let fallback_hit_rate = if total_requests > 0 {
        fallback_requests as f64 / total_requests as f64
    } else {
        0.0
    };

    let mut fallback_chain = Vec::with_capacity(chain_records.len());
    for (index, group) in chain_records.into_iter().enumerate() {
        let requests = request_counts
            .get(&group.group_id)
            .copied()
            .unwrap_or_default();
        let share = if total_requests > 0 {
            requests as f64 / total_requests as f64
        } else {
            0.0
        };
        let channel_count = if group.group_id == gid {
            binding_views.len() as i64
        } else {
            group_channel_count(&app, group.group_id).await?
        };
        fallback_chain.push(FallbackChainNodeView {
            id: group.group_id.to_string(),
            name: group.name,
            strategy: group.strategy,
            enabled: group.enabled,
            channel_count,
            requests,
            share,
            is_fallback: index > 0,
        });
    }

    let project_ids: Vec<String> = projects.into_iter().map(|p| p.to_string()).collect();

    Ok(Json(GroupDetailView {
        group_fields: group_view.clone(),
        group: group_view,
        bindings: binding_views,
        projects_using: project_ids.clone(),
        project_ids,
        fallback_chain,
        fallback_stats: FallbackStatsView {
            window_hours: FALLBACK_STATS_WINDOW_HOURS,
            total_requests,
            primary_requests,
            fallback_requests,
            fallback_hit_rate,
            has_cycle,
            cycle_at: cycle_at.map(|id| id.to_string()),
        },
        canary_stats,
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

#[derive(Deserialize)]
pub struct InvitationQuery {
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: String,
    #[serde(default = "default_invitation_ttl_hours")]
    pub ttl_hours: i64,
}

#[derive(Serialize)]
pub struct InvitationView {
    pub id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub email: String,
    pub role: String,
    pub invited_by: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub accepted_by: Option<String>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Serialize)]
pub struct CreatedInvitationView {
    #[serde(flatten)]
    pub invitation: InvitationView,
    pub token: String,
    pub accept_url: Option<String>,
}

fn default_invitation_ttl_hours() -> i64 {
    168
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

async fn list_org_invitations(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Query(q): Query<InvitationQuery>,
) -> AppResult<Json<Vec<InvitationView>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberInvite, Scope::Org(&org));
    let records = app
        .repos
        .invitations
        .list_scope("org", org_id.0, q.include_inactive)
        .await?;
    Ok(Json(records.into_iter().map(invitation_to_view).collect()))
}

async fn create_org_invitation(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateInvitationRequest>,
) -> AppResult<Json<CreatedInvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberInvite, Scope::Org(&org));
    let _ = app.repos.orgs.find_by_id(org).await?;
    let role = parse_org_invite_role(&req.role)?;

    create_invitation(
        &app,
        &ctx,
        "org",
        org_id.0,
        normalize_email(&req.email)?,
        org_role_to_invite_str(role),
        req.ttl_hours,
    )
    .await
}

async fn revoke_org_invitation(
    State(app): State<AppState>,
    Path((org_id, invitation_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<InvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgMemberRemove, Scope::Org(&org));
    revoke_invitation(&app, &ctx, "org", org_id.0, invitation_id.0).await
}

async fn list_project_invitations(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Query(q): Query<InvitationQuery>,
) -> AppResult<Json<Vec<InvitationView>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberInvite,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    let records = app
        .repos
        .invitations
        .list_scope("project", project_id.0, q.include_inactive)
        .await?;
    Ok(Json(records.into_iter().map(invitation_to_view).collect()))
}

async fn create_project_invitation(
    State(app): State<AppState>,
    Path((org_id, project_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Json(req): Json<CreateInvitationRequest>,
) -> AppResult<Json<CreatedInvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberInvite,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    let role = parse_project_invite_role(&req.role)?;

    create_invitation(
        &app,
        &ctx,
        "project",
        project_id.0,
        normalize_email(&req.email)?,
        project_role_to_invite_str(role),
        req.ttl_hours,
    )
    .await
}

async fn revoke_project_invitation(
    State(app): State<AppState>,
    Path((org_id, project_id, invitation_id)): Path<(FlexUuid, FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<InvitationView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    let project = ProjectId::from(project_id.0);
    require!(
        ctx,
        Permission::ProjectMemberRemove,
        Scope::Project {
            org: &org,
            project: &project
        }
    );
    ensure_project_in_org(&app, org, project).await?;
    revoke_invitation(&app, &ctx, "project", project_id.0, invitation_id.0).await
}

async fn create_invitation(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    scope_kind: &str,
    scope_id: Uuid,
    email: String,
    role: &str,
    ttl_hours: i64,
) -> AppResult<Json<CreatedInvitationView>> {
    let invited_by = ctx
        .user_id()
        .ok_or_else(|| AppError::Forbidden("only user subjects".into()))?;
    let ttl_hours = ttl_hours.clamp(1, 24 * 30);
    let token = generate_invitation_token();
    let rec = app
        .repos
        .invitations
        .create(InvitationCreate {
            id: Uuid::now_v7(),
            scope_kind: scope_kind.to_string(),
            scope_id,
            email: email.clone(),
            role: role.to_string(),
            token_hash: invitation_token_hash(&token),
            invited_by,
            expires_at: Utc::now() + ChronoDuration::hours(ttl_hours),
        })
        .await?;

    app.audit.emit(
        ctx,
        "invitation.create",
        "invitation",
        Some(rec.id),
        Some(serde_json::json!({
            "scope_kind": scope_kind,
            "scope_id": scope_id.to_string(),
            "email": email,
            "role": role,
            "expires_at": rec.expires_at
        })),
    );

    Ok(Json(CreatedInvitationView {
        invitation: invitation_to_view(rec),
        accept_url: invitation_accept_url(app, &token),
        token,
    }))
}

async fn revoke_invitation(
    app: &AppState,
    ctx: &gate_auth::AuthContext,
    scope_kind: &str,
    scope_id: Uuid,
    invitation_id: Uuid,
) -> AppResult<Json<InvitationView>> {
    let existing = app.repos.invitations.find_by_id(invitation_id).await?;
    if existing.scope_kind != scope_kind || existing.scope_id != scope_id {
        return Err(AppError::NotFound);
    }
    let revoked = app.repos.invitations.revoke(invitation_id).await?;
    app.audit.emit(
        ctx,
        "invitation.revoke",
        "invitation",
        Some(invitation_id),
        Some(serde_json::json!({
            "scope_kind": scope_kind,
            "scope_id": scope_id.to_string(),
            "email": revoked.email,
        })),
    );
    Ok(Json(invitation_to_view(revoked)))
}

async fn ensure_project_in_org(app: &AppState, org: OrgId, project: ProjectId) -> AppResult<()> {
    let p = app.repos.projects.find_by_id(project).await?;
    if p.org_id != org {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn invitation_to_view(rec: InvitationRecord) -> InvitationView {
    let status = rec.status_at(Utc::now()).to_string();
    InvitationView {
        id: rec.id.to_string(),
        scope_kind: rec.scope_kind,
        scope_id: rec.scope_id.to_string(),
        email: rec.email,
        role: rec.role,
        invited_by: rec.invited_by.to_string(),
        expires_at: rec.expires_at,
        accepted_at: rec.accepted_at,
        accepted_by: rec.accepted_by.map(|id| id.to_string()),
        revoked_at: rec.revoked_at,
        created_at: rec.created_at,
        status,
    }
}

fn generate_invitation_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("kg_inv_{}", B64.encode(buf))
}

fn invitation_accept_url(app: &AppState, token: &str) -> Option<String> {
    app.public_origin.as_ref().map(|origin| {
        format!(
            "{}/invite/accept?token={}",
            origin.trim_end_matches('/'),
            token
        )
    })
}

fn org_role_to_invite_str(role: gate_core::identity::OrgRole) -> &'static str {
    match role {
        gate_core::identity::OrgRole::Owner => "owner",
        gate_core::identity::OrgRole::Admin => "admin",
        gate_core::identity::OrgRole::BillingViewer => "billing_viewer",
        gate_core::identity::OrgRole::Member => "member",
    }
}

fn project_role_to_invite_str(role: gate_core::identity::ProjectRole) -> &'static str {
    match role {
        gate_core::identity::ProjectRole::Owner => "owner",
        gate_core::identity::ProjectRole::Admin => "admin",
        gate_core::identity::ProjectRole::Developer => "developer",
        gate_core::identity::ProjectRole::Viewer => "viewer",
    }
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

// 0.4.72（product-review B1 step 1）：pricing 块拆为内联子模块，缩小本文件
// 顶层符号表面，方便未来真正拆到 admin/pricing.rs。三个 handler + 类型 + 工厂
// 都搬进 mod pricing 内，主 router 直接引用 pricing::* handler。
mod pricing {
    use super::*;

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

    pub(super) async fn list_pricing_rules(
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
    pub(super) struct UpsertPricingRuleRequest {
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

    pub(super) async fn upsert_pricing_rule(
        State(app): State<AppState>,
        Authed(ctx): Authed,
        headers: HeaderMap,
        request_id: Option<Extension<KooixRequestId>>,
        Json(req): Json<UpsertPricingRuleRequest>,
    ) -> AppResult<Json<PricingRuleRow>> {
        require!(ctx, Permission::PlatformAdmin, Scope::Platform);
        let pricing = app
            .pricing
            .as_ref()
            .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
        let rule_id = req.id.unwrap_or_else(Uuid::now_v7);
        require_confirmation(
            &headers,
            format!("pricing:{}:{}", req.model.trim(), req.dimension.trim()),
        )?;
        let before = pricing
            .list_rules(None, None)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .into_iter()
            .find(|r| r.id == rule_id);

        let rule = gate_billing::PricingRule {
            id: rule_id,
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

        app.audit.emit_change(AuditChange {
            ctx: &ctx,
            meta: audit_meta(request_id, &headers),
            action: "pricing_rule.upsert",
            resource_kind: "pricing_rule",
            resource_id: Some(saved.id),
            before: before.as_ref().map(pricing_rule_audit_snapshot),
            after: Some(pricing_rule_audit_snapshot(&saved)),
        });

        Ok(Json(rule_to_row(&saved)))
    }

    pub(super) async fn delete_pricing_rule(
        State(app): State<AppState>,
        Authed(ctx): Authed,
        Path(id): Path<FlexUuid>,
        headers: HeaderMap,
        request_id: Option<Extension<KooixRequestId>>,
    ) -> AppResult<Json<serde_json::Value>> {
        require!(ctx, Permission::PlatformAdmin, Scope::Platform);
        let pricing = app
            .pricing
            .as_ref()
            .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
        let before = pricing
            .list_rules(None, None)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .into_iter()
            .find(|r| r.id == *id)
            .ok_or(AppError::NotFound)?;
        require_confirmation(
            &headers,
            format!("pricing:{}:{}", before.model, before.dimension),
        )?;

        let deleted = pricing
            .delete_rule(*id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        if !deleted {
            return Err(AppError::NotFound);
        }

        app.audit.emit_change(AuditChange {
            ctx: &ctx,
            meta: audit_meta(request_id, &headers),
            action: "pricing_rule.delete",
            resource_kind: "pricing_rule",
            resource_id: Some(*id),
            before: Some(pricing_rule_audit_snapshot(&before)),
            after: Some(serde_json::json!({
                "id": id.0.to_string(),
                "deleted": true,
            })),
        });

        Ok(Json(serde_json::json!({ "deleted": true })))
    }
}
