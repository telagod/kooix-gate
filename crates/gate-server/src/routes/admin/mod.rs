//! /v1/admin/* — 平台运维接口
//!
//! 全部需 Platform 作用域权限。SuperAdmin 短路通过。
//!
//! # 模块拆分进度（B1，followup §4）
//!
//! 本文件 ~4250 行，按业务域规划成 7 块。截至 0.4.116：
//!
//! | 块 | 行范围（约） | 状态 | 拆分动作 |
//! |----|--------------|------|--------|
//! | channels & keys | 332-1190 | ⛔ 仍顶层 | god-tier，与 batch ops / drain 紧耦合 |
//! | users / sessions / audit | 1247-1700 | ⛔ 仍顶层 | 内部多 fn 跨调用 |
//! | identity providers (SSO) | 1816-2400 | ⛔ 仍顶层 | secret seal / OIDC discover 内聚 |
//! | groups & bindings | 2402-3190 | ⛔ 仍顶层 | fallback chain / canary 跨 handler 共享 helper |
//! | org members | 3197-3290 | ✅ `mod org_members` | 0.4.109 抽出 |
//! | invitations | 3291-3559 | ⛔ 仍顶层 | invitation_token_hash / accept_url helper 与 create/revoke handler 紧耦合 |
//! | probe / test / balance | 3559-3970 | ⛔ 仍顶层 | 与 channels CRUD 共享 channel_capabilities |
//! | pricing rules | ~4082-4252 | ✅ `mod pricing` | 0.4.72 抽出 |
//!
//! 拆分原则：先抽**独立性高的小块**（pricing 11 fn / org members 3 fn），
//! 验证 mod 化模式可行后才动跨块共享多的大块。
//!
//! ## 真拆物理文件（v0.5.x 计划）
//!
//! 内联 mod 完成后下一步是 `routes/admin/{mod.rs, channels.rs, groups.rs,
//! sso.rs, users.rs, invitations.rs, probe.rs, pricing.rs}` 目录化。届时
//! `use super::*` 改为显式 `use crate::admin::shared::*`，强制声明依赖。
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
        // 0.4.87（product-review B5）：暴露完整 provider capability 矩阵给前端。
        // 让 playground 能根据当前 channel/provider 联动节点禁用状态。
        .route("/providers/capabilities", get(list_provider_capabilities))
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
            axum::routing::post(probe::probe_channel_models),
        )
        .route("/channels/:id/test", get(probe::test_channel))
        .route("/channels/:id/balance", get(probe::get_channel_balance))
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
            axum::routing::post(sso::discover_identity_provider),
        )
        .route(
            "/identity-providers",
            get(sso::list_identity_providers).post(sso::create_identity_provider),
        )
        .route(
            "/identity-providers/:id",
            axum::routing::put(sso::update_identity_provider).delete(sso::delete_identity_provider),
        )
        .route("/groups", get(groups::list_groups).post(groups::create_group))
        .route(
            "/groups/:id",
            axum::routing::put(groups::update_group).delete(groups::delete_group),
        )
        .route(
            "/groups/:id/bindings",
            get(groups::list_group_bindings).post(groups::add_group_binding),
        )
        .route(
            "/groups/:id/bindings/:channel_id",
            axum::routing::put(groups::update_group_binding).delete(groups::remove_group_binding),
        )
        .route("/groups/:id/detail", get(groups::get_group_detail))
        .route(
            "/projects/:id/default-group",
            axum::routing::put(groups::set_project_default_group),
        )
        .route(
            "/orgs/:org_id/members",
            get(org_members::list_org_members).post(org_members::add_org_member),
        )
        .route(
            "/orgs/:org_id/invitations",
            get(invitations::list_org_invitations).post(invitations::create_org_invitation),
        )
        .route(
            "/orgs/:org_id/invitations/:invitation_id",
            axum::routing::delete(invitations::revoke_org_invitation),
        )
        .route(
            "/orgs/:org_id/projects/:project_id/invitations",
            get(invitations::list_project_invitations).post(invitations::create_project_invitation),
        )
        .route(
            "/orgs/:org_id/projects/:project_id/invitations/:invitation_id",
            axum::routing::delete(invitations::revoke_project_invitation),
        )
        .route(
            "/orgs/:org_id/members/:user_id",
            axum::routing::delete(org_members::remove_org_member_handler),
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
    #[serde(default = "invitations::default_invitation_ttl_hours")]
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


// ─── Pricing Rules CRUD ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 0.4.88（B5 step 2）：list_provider_capabilities 不依赖 DB / state，
    // 直接验证它返回 4 编译期 + 7 plugin preset 共 11 项，且每条字段齐全。
    // 用 minimal AuthContext 绕过权限检查。
    #[tokio::test]
    async fn provider_capabilities_returns_full_matrix() {
        // 模拟一个 system AuthContext —— Authed 仅做 deserialize-style
        // 提取，不真校验权限（list_provider_capabilities 没用 require!）
        let ctx = gate_auth::AuthContext::system();
        let Json(rows) = list_provider_capabilities(crate::auth::Authed(ctx))
            .await
            .expect("call ok");
        // 应该至少 4 编译期 + ≥1 plugin preset
        assert!(rows.len() >= 5, "expected >= 5 entries, got {}", rows.len());

        // 编译期 4 条都在
        for p in ["openai", "anthropic", "azure", "bedrock"] {
            assert!(
                rows.iter().any(|r| r.id == p && r.kind == "compile_time"),
                "missing compile_time provider: {p}"
            );
        }
        // plugin preset 至少包含 openai_compatible
        assert!(
            rows.iter()
                .any(|r| r.id == "plugin:openai_compatible" && r.kind == "plugin_preset"),
            "missing plugin preset openai_compatible"
        );

        // capabilities 字段非空（compile-time provider 至少有 chat true）
        let openai = rows.iter().find(|r| r.id == "openai").unwrap();
        assert!(openai.capabilities.chat, "openai must support chat");
    }

    #[test]
    fn channel_key_alias_validation_matches_plugin_secret_slots() {
        assert!(validate_channel_key_alias("client_id").is_ok());
        assert!(validate_channel_key_alias("aws-secret-key").is_ok());
        assert_eq!(probe::normalize_probe_secret_slot("api_key"), "primary");
        assert_eq!(probe::normalize_probe_secret_slot("Client_ID"), "client_id");
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

// 0.4.87（product-review B5）：完整 provider capability 矩阵，前端 playground
// / channel drawer 可一次拉到全部已知 provider + plugin preset 的能力清单。
// 不依赖 channel 状态，所有 Authed 用户都能查（gate-providers 编译期静态）。
#[derive(Serialize)]
struct ProviderCapabilityEntry {
    /// 标识符：编译期 provider 用 provider_type，plugin preset 用 `plugin:{preset_name}`。
    id: String,
    /// 人类可读 label。
    name: String,
    capabilities: gate_providers::ProviderCapabilities,
    /// 推荐 base_url（None 即没有标准推荐，比如 plugin 自定义 endpoint）。
    base_url_hint: Option<String>,
    /// `compile_time` | `plugin_preset`。
    kind: &'static str,
}

async fn list_provider_capabilities(
    Authed(_ctx): Authed,
) -> AppResult<Json<Vec<ProviderCapabilityEntry>>> {
    // 编译期 provider（4 个 fast-path）
    let compile_time = ["openai", "anthropic", "azure", "bedrock"];
    // 已知 plugin preset
    let plugin_presets = [
        "openai_compatible",
        "anthropic_messages",
        "google_gemini",
        "cohere",
        "mistral",
        "deepseek",
        "ollama",
    ];

    let mut out = Vec::with_capacity(compile_time.len() + plugin_presets.len());
    for p in compile_time {
        out.push(ProviderCapabilityEntry {
            id: p.to_string(),
            name: p.to_string(),
            capabilities: gate_providers::provider_capabilities(p),
            base_url_hint: gate_providers::provider_base_url_suggestion(p)
                .map(str::to_string),
            kind: "compile_time",
        });
    }
    for preset in plugin_presets {
        if let Some(caps) = gate_providers::plugin_preset_capabilities(preset) {
            out.push(ProviderCapabilityEntry {
                id: format!("plugin:{preset}"),
                name: preset.to_string(),
                capabilities: caps,
                base_url_hint: gate_providers::plugin_preset_base_url_suggestion(preset)
                    .map(str::to_string),
                kind: "plugin_preset",
            });
        }
    }
    Ok(Json(out))
}


// 0.4.122：pricing 块物理拆出到 admin/pricing.rs。
mod probe;
mod invitations;
mod groups;
mod sso;
mod org_members;
mod pricing;
