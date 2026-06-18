//! /v1/admin/* — 平台运维接口
//!
//! 全部需 Platform 作用域权限。SuperAdmin 短路通过。
//!
//! # 模块拆分完成（B1，第三刀 0.4.121-0.4.129）
//!
//! 0.4.120 admin.rs 4368 行 god file → 0.4.129 admin/mod.rs **553 行**（-87%）。
//! 真减 3815 行，分 9 个子文件：
//!
//! | 文件 | 行数 | handler 数 | 抽出版本 |
//! |------|-----|---|---|
//! | `mod.rs` | 553 | — | 共享 types + router + tests |
//! | `channels.rs` | 853 | 17 | 0.4.129（最大块） |
//! | `groups.rs` | 846 | 10 | 0.4.127 |
//! | `sso.rs` | 600 | 5 | 0.4.126 |
//! | `users.rs` | 529 | 11 | 0.4.128 |
//! | `probe.rs` | 488 | 3 | 0.4.125 |
//! | `invitations.rs` | 278 | 9 | 0.4.124 |
//! | `pricing.rs` | 169 | 3 | 0.4.122 |
//! | `org_members.rs` | 100 | 3 | 0.4.123 |
//!
//! ## 跨文件共享 helper
//!
//! 0.4.151-0.4.155 已把 13 个共享 helper 物理迁到 `admin/shared.rs`，
//! sibling 文件头声明 `use super::shared::{...}`，channels.rs 不再被反向依赖。
//! channels.rs 内部保留 17 个 thin wrapper（`super::shared::xxx`）兼容自身 handler。
//!
//! ## Channels CRUD（in channels.rs）
//!
//! - GET    /channels         — 列出全部 channels
//! - POST   /channels         — 创建 channel
//! - PUT    /channels/:id     — 更新 channel
//! - DELETE /channels/:id     — 软删除 channel
//!
//! ## Channel Keys（in channels.rs）
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
        .route(
            "/plugin-manifest/schema",
            get(channels::plugin_manifest_schema),
        )
        .route(
            "/plugin-manifest/replay",
            axum::routing::post(channels::plugin_manifest_replay),
        )
        .route(
            "/channels",
            get(channels::list_channels).post(channels::create_channel),
        )
        .route(
            "/channels/:id",
            axum::routing::put(channels::update_channel).delete(channels::delete_channel),
        )
        .route(
            "/channels/:id/drain",
            axum::routing::post(channels::drain_channel),
        )
        .route(
            "/channels/:id/drain-status",
            get(channels::get_channel_drain_status),
        )
        .route(
            "/channels/:id/disable-when-idle",
            axum::routing::post(channels::disable_channel_when_idle),
        )
        .route(
            "/channels/batch-enable",
            axum::routing::post(channels::batch_enable_channels),
        )
        .route(
            "/channels/batch-disable",
            axum::routing::post(channels::batch_disable_channels),
        )
        .route(
            "/channels/batch-delete",
            axum::routing::post(channels::batch_delete_channels),
        )
        .route(
            "/channels/:id/keys",
            get(channels::list_channel_keys).post(channels::create_channel_key),
        )
        .route(
            "/channels/:id/keys/rotate",
            axum::routing::post(channels::rotate_channel_key),
        )
        .route(
            "/channels/:id/keys/:key_id",
            axum::routing::delete(channels::revoke_channel_key),
        )
        .route("/channels/:id/stats", get(channels::get_channel_stats))
        .route(
            "/channels/:id/probe",
            axum::routing::post(probe::probe_channel_models),
        )
        .route("/channels/:id/test", get(probe::test_channel))
        .route("/channels/:id/balance", get(probe::get_channel_balance))
        .route("/audit-logs", get(users::list_audit_logs))
        .route("/orgs", get(users::list_all_orgs).post(users::create_org))
        .route("/orgs/:org_id", axum::routing::put(users::update_org))
        .route("/users", get(users::list_users).post(users::create_user))
        .route(
            "/users/:id/status",
            axum::routing::put(users::update_user_status),
        )
        .route(
            "/users/:id/password",
            axum::routing::put(users::reset_user_password),
        )
        .route(
            "/users/:id/sessions",
            get(users::list_user_sessions).delete(users::revoke_user_sessions),
        )
        .route(
            "/users/:id/sessions/:session_id",
            axum::routing::delete(users::revoke_user_session),
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
        .route(
            "/groups",
            get(groups::list_groups).post(groups::create_group),
        )
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

    #[test]
    fn channel_key_alias_validation_matches_plugin_secret_slots() {
        assert!(channels::validate_channel_key_alias("client_id").is_ok());
        assert!(channels::validate_channel_key_alias("aws-secret-key").is_ok());
        assert_eq!(probe::normalize_probe_secret_slot("api_key"), "primary");
        assert_eq!(probe::normalize_probe_secret_slot("Client_ID"), "client_id");
        assert!(channels::validate_channel_key_alias("client.id").is_err());
        assert!(channels::validate_channel_key_alias("client id").is_err());
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

// 0.4.122：pricing 块物理拆出到 admin/pricing.rs。
mod channels;
mod groups;
mod invitations;
mod org_members;
mod pricing;
mod probe;
#[allow(dead_code)]
mod shared;
mod sso;
mod users;
