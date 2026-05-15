//! /v1/orgs/:org_id/billing — 月度账单 + CSV 导出 + 配额告警
//!
//! Endpoints:
//! - GET /v1/orgs/:org_id/billing/:month       — 月度账单汇总
//! - GET /v1/orgs/:org_id/billing/export       — CSV 导出（?from=&to=）
//! - GET /v1/orgs/:org_id/quota-alerts         — 当前配额告警

use crate::alerts::{self, QuotaAlert};
use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::id::OrgId;
use gate_core::rbac::{Permission, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::flex_uuid::FlexUuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/orgs/:org_id/billing/export", get(export_csv))
        .route("/orgs/:org_id/billing/:month", get(get_monthly_bill))
        .route("/orgs/:org_id/quota-alerts", get(get_quota_alerts))
}

// ============================================================================
// Monthly Bill
// ============================================================================

#[derive(Serialize)]
struct MonthlyBillResponse {
    org_id: String,
    month: String,
    total_cost_usd: String,
    total_tokens_in: i64,
    total_tokens_out: i64,
    total_requests: i64,
    breakdown_by_project: Vec<ProjectLineView>,
    breakdown_by_model: Vec<ModelLineView>,
}

#[derive(Serialize)]
struct ProjectLineView {
    project_id: String,
    cost_usd: String,
    requests: i64,
}

#[derive(Serialize)]
struct ModelLineView {
    model: String,
    cost_usd: String,
    tokens_in: i64,
    tokens_out: i64,
    requests: i64,
}

async fn get_monthly_bill(
    State(app): State<AppState>,
    Path((org_id, month)): Path<(FlexUuid, String)>,
    Authed(ctx): Authed,
) -> AppResult<Json<MonthlyBillResponse>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgBillingRead, Scope::Org(&org));

    // Validate month format: YYYY-MM
    if !is_valid_month(&month) {
        return Err(AppError::BadRequest(
            "month must be in YYYY-MM format".into(),
        ));
    }

    let bill = app.repos.billing.monthly_bill(org, &month).await?;

    Ok(Json(MonthlyBillResponse {
        org_id: org_id.to_string(),
        month: bill.month,
        total_cost_usd: bill.total_cost_usd.normalize().to_string(),
        total_tokens_in: bill.total_tokens_in,
        total_tokens_out: bill.total_tokens_out,
        total_requests: bill.total_requests,
        breakdown_by_project: bill
            .breakdown_by_project
            .into_iter()
            .map(|p| ProjectLineView {
                project_id: p.project_id.to_string(),
                cost_usd: p.cost_usd.normalize().to_string(),
                requests: p.requests,
            })
            .collect(),
        breakdown_by_model: bill
            .breakdown_by_model
            .into_iter()
            .map(|m| ModelLineView {
                model: m.model,
                cost_usd: m.cost_usd.normalize().to_string(),
                tokens_in: m.tokens_in,
                tokens_out: m.tokens_out,
                requests: m.requests,
            })
            .collect(),
    }))
}

fn is_valid_month(s: &str) -> bool {
    if s.len() != 7 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return false;
    }
    let y: i32 = match parts[0].parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let m: u32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    (2000..=2100).contains(&y) && (1..=12).contains(&m)
}

// ============================================================================
// CSV Export
// ============================================================================

#[derive(Deserialize)]
pub struct ExportQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

async fn export_csv(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Query(q): Query<ExportQuery>,
) -> AppResult<Response> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::UsageExport, Scope::Org(&org));

    if q.from >= q.to {
        return Err(AppError::BadRequest(
            "'from' must be before 'to'".into(),
        ));
    }

    let rows = app.repos.billing.export_usage_csv(org, q.from, q.to).await?;

    // Build CSV
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record([
        "ts",
        "org_id",
        "project_id",
        "api_key_id",
        "channel_id",
        "model",
        "tokens_in",
        "tokens_out",
        "cost_usd",
    ])
    .map_err(|e| AppError::Internal(format!("csv header write: {e}")))?;

    for r in &rows {
        wtr.write_record([
            r.ts.to_rfc3339(),
            r.org_id.to_string(),
            r.project_id.to_string(),
            r.api_key_id.to_string(),
            r.channel_id.map(|c| c.to_string()).unwrap_or_default(),
            r.model.clone(),
            r.tokens_in.to_string(),
            r.tokens_out.to_string(),
            r.cost_usd.normalize().to_string(),
        ])
        .map_err(|e| AppError::Internal(format!("csv row write: {e}")))?;
    }

    let csv_bytes = wtr
        .into_inner()
        .map_err(|e| AppError::Internal(format!("csv flush: {e}")))?;

    let mut response = Response::new(Body::from(csv_bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"usage-export.csv\""),
    );
    Ok(response)
}

// ============================================================================
// Quota Alerts
// ============================================================================

async fn get_quota_alerts(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<QuotaAlert>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaRead, Scope::Org(&org));

    let alert_list = alerts::compute_alerts(org, &app.repos.quotas, &app.repos.usage).await;
    Ok(Json(alert_list))
}
