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
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::id::{ChannelId, ChannelKeyId};
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
        .route("/audit-logs", get(list_audit_logs))
        .route("/orgs", get(list_all_orgs).post(create_org))
        .route("/orgs/:id", axum::routing::put(update_org))
        .route("/users", get(list_users))
        .route("/users/:id/status", axum::routing::put(update_user_status))
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/:id", axum::routing::put(update_group).delete(delete_group))
        .route("/groups/:id/bindings", get(list_group_bindings).post(add_group_binding))
        .route("/groups/:id/bindings/:channel_id", axum::routing::delete(remove_group_binding))
        .route("/orgs/:org_id/members", get(list_org_members).post(add_org_member))
        .route("/orgs/:org_id/members/:user_id", axum::routing::delete(remove_org_member_handler))
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

    let valid_types = ["openai", "anthropic", "gemini", "azure", "bedrock", "deepseek", "ollama", "mistral", "cohere"];
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

    app.audit.emit(
        &ctx,
        "channel.create",
        "channel",
        Some(*record.channel_id.as_uuid()),
        Some(serde_json::json!({"code": &record.code})),
    );

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

    app.audit.emit(
        &ctx,
        "channel.update",
        "channel",
        Some(id),
        None,
    );

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

    app.audit.emit(
        &ctx,
        "channel.delete",
        "channel",
        Some(id),
        None,
    );

    Ok(Json(serde_json::json!({"deleted": true})))
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

async fn list_channel_keys(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ChannelKeySummary>>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    let channel_id = ChannelId::from(id);
    let records = app.repos.channel_keys.list_by_channel(channel_id).await?;
    Ok(Json(
        records
            .into_iter()
            .map(|r| ChannelKeySummary {
                id: r.id.as_uuid().to_string(),
                channel_id: r.channel_id.as_uuid().to_string(),
                label: r.label,
                fingerprint: r.key_fingerprint,
                weight: r.weight,
                health: r.health,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

async fn create_channel_key(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id);
    // 先确认 channel 存在
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);

    // AAD 用 channel_id：同 channel 的所有 key 共享 AAD context，
    // 防止密文跨 channel 移植，同时避免先有 key_id 再加密的鸡生蛋问题。
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto.seal(req.secret.as_bytes(), &aad).await.map_err(|e| {
        AppError::Internal(format!("encrypt channel key failed: {e}"))
    })?;

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
        id: key_id.as_uuid().to_string(),
        channel_id: channel_id.as_uuid().to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        created_at: Utc::now(),
    }))
}

async fn rotate_channel_key(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id);
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto.seal(req.secret.as_bytes(), &aad).await.map_err(|e| {
        AppError::Internal(format!("encrypt channel key failed: {e}"))
    })?;

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
        id: key_id.as_uuid().to_string(),
        channel_id: channel_id.as_uuid().to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        created_at: Utc::now(),
    }))
}

async fn revoke_channel_key(
    State(app): State<AppState>,
    Path((id, key_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    // 验证 channel 存在
    let channel_id = ChannelId::from(id);
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let ck_id = ChannelKeyId::from(key_id);
    app.repos.channel_keys.revoke(ck_id).await?;

    app.audit.emit(
        &ctx,
        "channel_key.revoke",
        "channel_key",
        Some(key_id),
        Some(serde_json::json!({"channel_id": id.to_string()})),
    );

    Ok(Json(serde_json::json!({"revoked": true})))
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
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateOrgRequest>,
) -> AppResult<Json<OrgView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org_id = gate_core::id::OrgId::from(id);
    let org = app
        .repos
        .orgs
        .update(org_id, req.name.as_deref(), req.billing_email.as_deref())
        .await?;

    app.audit.emit(&ctx, "org.update", "org", Some(id), None);

    Ok(Json(org_to_view(org)))
}

fn org_to_view(o: gate_core::identity::Organization) -> OrgView {
    OrgView {
        id: o.id.as_uuid().to_string(),
        name: o.name,
        slug: o.slug,
        owner_user_id: o.owner_user_id.as_uuid().to_string(),
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
pub struct UpdateStatusRequest {
    pub status: String,
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

async fn update_user_status(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateStatusRequest>,
) -> AppResult<Json<UserView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let valid = ["active", "suspended"];
    if !valid.contains(&req.status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of: {:?}",
            valid
        )));
    }

    let user_id = gate_core::id::UserId::from(id);
    let user = app.repos.users.update_status(user_id, &req.status).await?;

    app.audit.emit(
        &ctx,
        "user.update_status",
        "user",
        Some(id),
        Some(serde_json::json!({"status": &req.status})),
    );

    Ok(Json(user_to_view(user)))
}

fn user_to_view(u: gate_core::identity::User) -> UserView {
    UserView {
        id: u.id.as_uuid().to_string(),
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
    pub strategy: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
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
}

#[derive(Serialize)]
pub struct BindingView {
    pub channel_id: String,
    pub channel_code: String,
    pub channel_name: String,
    pub provider_type: String,
    pub priority: i32,
    pub weight: i32,
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
    Ok(Json(groups.into_iter().map(|g| GroupView {
        id: g.group_id.as_uuid().to_string(),
        name: g.name,
        strategy: g.strategy,
        enabled: g.enabled,
        created_at: g.created_at,
    }).collect()))
}

async fn create_group(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let valid = ["priority", "weighted_random", "round_robin", "least_conn"];
    if !valid.contains(&req.strategy.as_str()) {
        return Err(AppError::BadRequest(format!("strategy must be one of: {valid:?}")));
    }

    let g = app.repos.channel_groups.create(&req.name, &req.strategy).await?;
    app.audit.emit(&ctx, "channel_group.create", "channel_group", Some(*g.group_id.as_uuid()), None);

    Ok(Json(GroupView {
        id: g.group_id.as_uuid().to_string(),
        name: g.name, strategy: g.strategy, enabled: g.enabled, created_at: g.created_at,
    }))
}

async fn update_group(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id);
    let g = app.repos.channel_groups.update(gid, req.name.as_deref(), req.strategy.as_deref(), req.enabled).await?;
    app.audit.emit(&ctx, "channel_group.update", "channel_group", Some(id), None);

    Ok(Json(GroupView {
        id: g.group_id.as_uuid().to_string(),
        name: g.name, strategy: g.strategy, enabled: g.enabled, created_at: g.created_at,
    }))
}

async fn delete_group(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id);
    app.repos.channel_groups.delete(gid).await?;
    app.audit.emit(&ctx, "channel_group.delete", "channel_group", Some(id), None);

    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn list_group_bindings(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<BindingView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id);
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;
    Ok(Json(bindings.into_iter().map(|b| BindingView {
        channel_id: b.channel.channel_id.as_uuid().to_string(),
        channel_code: b.channel.code,
        channel_name: b.channel.name,
        provider_type: b.channel.provider_type,
        priority: b.priority,
        weight: b.weight,
    }).collect()))
}

async fn add_group_binding(
    State(app): State<AppState>,
    Path(id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<AddBindingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id);
    let cid = gate_core::id::ChannelId::from(req.channel_id);
    app.repos.channel_groups.add_binding(gid, cid, req.priority.unwrap_or(100), req.weight.unwrap_or(1)).await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn remove_group_binding(
    State(app): State<AppState>,
    Path((id, channel_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id);
    let cid = gate_core::id::ChannelId::from(channel_id);
    app.repos.channel_groups.remove_binding(gid, cid).await?;

    Ok(Json(serde_json::json!({"removed": true})))
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
    Path(org_id): Path<Uuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<MemberView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org = gate_core::id::OrgId::from(org_id);
    let members = app.repos.memberships.list_org_members(org).await?;
    Ok(Json(members.into_iter().map(|m| MemberView {
        user_id: m.user_id.as_uuid().to_string(),
        email: m.email,
        display_name: m.display_name,
        role: m.role,
        joined_at: m.joined_at,
    }).collect()))
}

async fn add_org_member(
    State(app): State<AppState>,
    Path(org_id): Path<Uuid>,
    Authed(ctx): Authed,
    Json(req): Json<AddMemberRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let valid_roles = ["owner", "admin", "billing_viewer", "member"];
    if !valid_roles.contains(&req.role.as_str()) {
        return Err(AppError::BadRequest(format!("role must be one of: {valid_roles:?}")));
    }

    let user = app.repos.users.find_by_email(&req.email).await
        .map_err(|_| AppError::BadRequest(format!("user '{}' not found", req.email)))?;

    let role = match req.role.as_str() {
        "owner" => gate_core::identity::OrgRole::Owner,
        "admin" => gate_core::identity::OrgRole::Admin,
        "billing_viewer" => gate_core::identity::OrgRole::BillingViewer,
        _ => gate_core::identity::OrgRole::Member,
    };

    let org = gate_core::id::OrgId::from(org_id);
    app.repos.memberships.add_org_member(org, user.id, role).await?;

    app.audit.emit(&ctx, "membership.add", "membership", None,
        Some(serde_json::json!({"org_id": org_id.to_string(), "email": req.email})));

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn remove_org_member_handler(
    State(app): State<AppState>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let org = gate_core::id::OrgId::from(org_id);
    let uid = gate_core::id::UserId::from(user_id);
    app.repos.memberships.remove_org_member(org, uid).await?;

    app.audit.emit(&ctx, "membership.remove", "membership", Some(user_id), None);

    Ok(Json(serde_json::json!({"removed": true})))
}
