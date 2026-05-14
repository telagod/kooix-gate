//! GET /v1/usage — 控制台聚合用量端点。
//!
//! 设计要点：
//! - 默认作用域：`current_org`；带 `?org_id=` 时仅 SuperAdmin 可跨 Org（其他 403）。
//! - `range` 仅支持 `7d` / `30d`；未提供则默认 `7d`。窗口为 [now - N days, now]。
//! - `group_by` 支持 `day`（默认） / `model` / `channel`。
//! - 总计字段（`total_cost_usd` / `total_tokens_in/out`）单独走一次 totals 聚合，
//!   避免前端再算一遍 series 求和（也避开了 model/channel 切片时漏算）。
//!
//! 鉴权：`Permission::UsageRead` on `Scope::Org`；user 必须是 User subject。

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Duration, Utc};
use gate_auth::{AuthError, require, require_user};
use gate_core::id::OrgId;
use gate_core::rbac::{Permission, Scope};
use gate_storage::{RequestFilter, UsageGroupBy};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub group_by: Option<String>,
    /// SuperAdmin 才能用：跨 Org 查询。
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct UsagePoint {
    pub key: String,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

#[derive(Serialize)]
pub struct UsageResponse {
    pub range: String,
    pub group_by: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub total_cost_usd: f64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub series: Vec<UsagePoint>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/usage", get(get_usage))
        .route("/orgs/:org_id/requests", get(list_org_requests))
        .route("/orgs/:org_id/requests/:request_id", get(get_org_request))
}

async fn get_usage(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<UsageQuery>,
) -> AppResult<Json<UsageResponse>> {
    require_user!(ctx);

    // 解析 range：默认 7d
    let (range_label, days) = match q.range.as_deref() {
        None | Some("7d") => ("7d", 7),
        Some("30d") => ("30d", 30),
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "unsupported range: {other} (allowed: 7d|30d)"
            )));
        }
    };

    // 解析 group_by：默认 day
    let (group_label, group_by) = match q.group_by.as_deref() {
        None | Some("day") => ("day", UsageGroupBy::Day),
        Some("model") => ("model", UsageGroupBy::Model),
        Some("channel") => ("channel", UsageGroupBy::Channel),
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "unsupported group_by: {other} (allowed: day|model|channel)"
            )));
        }
    };

    // 决定查哪个 Org（None = 跨 Org，SuperAdmin 专用）。
    let target_org = resolve_target_org(&ctx, q.org_id)?;

    // 非跨 Org 路径必须有 UsageRead；跨 Org 已被 SuperAdmin 检查放行（可跳过 require）。
    if let Some(org) = target_org {
        require!(ctx, Permission::UsageRead, Scope::Org(&org));
    }

    let to = Utc::now();
    let from = to - Duration::days(days);

    let buckets = app
        .repos
        .usage
        .aggregate(target_org, from, to, group_by)
        .await?;
    let totals = app.repos.usage.totals(target_org, from, to).await?;

    Ok(Json(UsageResponse {
        range: range_label.into(),
        group_by: group_label.into(),
        from,
        to,
        total_cost_usd: totals.cost_usd,
        total_tokens_in: totals.tokens_in,
        total_tokens_out: totals.tokens_out,
        series: buckets
            .into_iter()
            .map(|b| UsagePoint {
                key: b.key,
                cost_usd: b.cost_usd,
                tokens_in: b.tokens_in,
                tokens_out: b.tokens_out,
            })
            .collect(),
    }))
}

/// 解析查询目标 Org：
/// - `?org_id=` 显式指定 → 仅 SuperAdmin 可跨 Org，其他 user 必须自己在该 Org
/// - 否则用 `current_org`；没有就 400（控制台调用前应先选 Org）
fn resolve_target_org(
    ctx: &gate_auth::AuthContext,
    requested: Option<Uuid>,
) -> AppResult<Option<OrgId>> {
    if let Some(o) = requested {
        let org = OrgId::from(o);
        if ctx.is_super_admin() {
            return Ok(Some(org));
        }
        if ctx.accessible_orgs().contains(&org) {
            return Ok(Some(org));
        }
        return Err(AppError::Auth(AuthError::Forbidden {
            action: "usage.read".into(),
            resource: format!("org:{o}"),
        }));
    }

    if let Some(o) = ctx.current_org() {
        return Ok(Some(o));
    }

    // SuperAdmin 没指定 org_id 也没 current_org → 跨 Org 全量（用于平台仪表盘）
    if ctx.is_super_admin() {
        return Ok(None);
    }

    Err(AppError::BadRequest(
        "current_org missing; set X-Kooix-Org header or pass org_id=".into(),
    ))
}

// ============================================================================
// Org-scoped Request Logs (普通用户可看自己 Org 的请求)
// ============================================================================

#[derive(Deserialize)]
pub struct OrgRequestListQuery {
    pub project_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub model: Option<String>,
    pub status_min: Option<i16>,
    pub status_max: Option<i16>,
    pub error_only: Option<bool>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    #[serde(default = "default_request_limit")]
    pub limit: i64,
}

fn default_request_limit() -> i64 { 50 }

async fn list_org_requests(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(org_id): Path<Uuid>,
    Query(q): Query<OrgRequestListQuery>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::UsageRead, Scope::Org(&org));

    let filter = RequestFilter {
        org_id: Some(org_id),
        project_id: q.project_id,
        channel_id: q.channel_id,
        api_key_id: None,
        model: q.model,
        status_min: q.status_min,
        status_max: q.status_max,
        error_only: q.error_only,
        from: q.from,
        to: q.to,
        search: q.search,
    };

    let limit = q.limit.clamp(1, 100);
    let page = app
        .repos
        .request_logs
        .list(&filter, q.cursor.as_deref(), limit)
        .await?;

    Ok(Json(serde_json::json!({
        "data": page.data,
        "next_cursor": page.next_cursor,
        "has_more": page.has_more,
    })))
}

async fn get_org_request(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path((org_id, request_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    let org = OrgId::from(org_id);
    require!(ctx, Permission::UsageRead, Scope::Org(&org));

    let record = app.repos.request_logs.find_by_request_id(request_id).await?;

    // 确保请求属于该 Org
    if record.org_id != org_id {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::to_value(&record).unwrap_or_default()))
}
