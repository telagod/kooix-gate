use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::rbac::{Permission, Scope};
use gate_storage::RequestFilter;
use serde::Deserialize;
use uuid::Uuid;
use crate::flex_uuid::FlexUuid;

#[derive(Deserialize)]
pub struct RequestListQuery {
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub model: Option<String>,
    pub model_requested: Option<String>,
    pub status_min: Option<i16>,
    pub status_max: Option<i16>,
    pub status_category: Option<String>,
    pub error_only: Option<bool>,
    pub error_code: Option<String>,
    pub stream: Option<bool>,
    pub has_retries: Option<bool>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub latency_min: Option<i32>,
    pub latency_max: Option<i32>,
    pub ttfb_min: Option<i32>,
    pub ttfb_max: Option<i32>,
    pub cost_min: Option<f64>,
    pub cost_max: Option<f64>,
    pub tokens_min: Option<i64>,
    pub tokens_max: Option<i64>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Deserialize)]
pub struct DashboardStatsQuery {
    pub org_id: Option<Uuid>,
    #[serde(default = "default_hours")]
    pub hours: i64,
}

#[derive(Deserialize)]
pub struct FilterOptionsQuery {
    pub org_id: Option<Uuid>,
    #[serde(default = "default_filter_hours")]
    pub hours: i64,
}

fn default_hours() -> i64 {
    24
}

fn default_filter_hours() -> i64 {
    168
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/requests", get(list_requests))
        .route("/requests/filters", get(get_filter_options))
        .route("/requests/:request_id", get(get_request))
        .route("/dashboard-stats", get(dashboard_stats))
}

async fn list_requests(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<RequestListQuery>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let filter = RequestFilter {
        org_id: q.org_id,
        project_id: q.project_id,
        channel_id: q.channel_id,
        api_key_id: q.api_key_id,
        user_id: q.user_id,
        group_id: q.group_id,
        model: q.model,
        model_requested: q.model_requested,
        status_min: q.status_min,
        status_max: q.status_max,
        status_category: q.status_category,
        error_only: q.error_only,
        error_code: q.error_code,
        stream: q.stream,
        has_retries: q.has_retries,
        from: q.from,
        to: q.to,
        latency_min: q.latency_min,
        latency_max: q.latency_max,
        ttfb_min: q.ttfb_min,
        ttfb_max: q.ttfb_max,
        cost_min: q.cost_min,
        cost_max: q.cost_max,
        tokens_min: q.tokens_min,
        tokens_max: q.tokens_max,
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

async fn get_request(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(request_id): Path<FlexUuid>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let record = app.repos.request_logs.find_by_request_id(*request_id).await?;
    Ok(Json(serde_json::to_value(&record).unwrap_or_default()))
}

async fn get_filter_options(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<FilterOptionsQuery>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let hours = q.hours.clamp(1, 720);
    let options = app.repos.request_logs.filter_options(q.org_id, hours).await?;
    Ok(Json(serde_json::to_value(&options).unwrap_or_default()))
}

async fn dashboard_stats(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<DashboardStatsQuery>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::UsageRead, Scope::Platform);

    let hours = q.hours.clamp(1, 720);
    let stats = app.repos.request_logs.dashboard_stats(q.org_id, hours).await?;
    Ok(Json(serde_json::to_value(&stats).unwrap_or_default()))
}
