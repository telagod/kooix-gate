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

#[derive(Deserialize)]
pub struct RequestListQuery {
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub model: Option<String>,
    pub status_min: Option<i16>,
    pub status_max: Option<i16>,
    pub error_only: Option<bool>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
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

fn default_hours() -> i64 {
    24
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/requests", get(list_requests))
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

async fn get_request(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(request_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::AuditRead, Scope::Platform);

    let record = app.repos.request_logs.find_by_request_id(request_id).await?;
    Ok(Json(serde_json::to_value(&record).unwrap_or_default()))
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
