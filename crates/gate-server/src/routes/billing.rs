//! /v1/orgs/:org_id/billing — 月度账单 + CSV 导出 + 配额告警
//!
//! Endpoints:
//! - GET /v1/orgs/:org_id/billing/:month       — 月度账单汇总 + invoice 状态
//! - POST /v1/orgs/:org_id/billing/:month/state — 月账单状态机推进
//! - GET /v1/orgs/:org_id/billing/export       — CSV 导出（?from=&to=）
//! - GET /v1/orgs/:org_id/billing/export.json  — signed JSON 导出（?from=&to=）
//! - GET /v1/orgs/:org_id/quota-alerts         — 当前配额告警

use crate::alerts::{self, QuotaAlert};
use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::flex_uuid::FlexUuid;
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
use gate_storage::{BillingInvoice, InvoiceStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/orgs/:org_id/billing/export", get(export_csv))
        .route("/orgs/:org_id/billing/export.json", get(export_json))
        .route("/orgs/:org_id/billing/:month", get(get_monthly_bill))
        .route(
            "/orgs/:org_id/billing/:month/state",
            axum::routing::post(transition_invoice_state),
        )
        .route("/orgs/:org_id/quota-alerts", get(get_quota_alerts))
}

// ============================================================================
// Monthly Bill
// ============================================================================

#[derive(Serialize)]
struct MonthlyBillResponse {
    org_id: String,
    month: String,
    invoice: InvoiceView,
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

#[derive(Serialize)]
struct InvoiceView {
    org_id: String,
    month: String,
    status: String,
    total_cost_usd: String,
    total_cost_micros: i64,
    export_digest: Option<String>,
    closed_at: Option<DateTime<Utc>>,
    exported_at: Option<DateTime<Utc>>,
    paid_at: Option<DateTime<Utc>>,
    waived_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct TransitionInvoiceRequest {
    status: String,
    export_digest: Option<String>,
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
    let invoice = app.repos.billing.get_or_create_invoice(org, &month).await?;

    Ok(Json(MonthlyBillResponse {
        org_id: org_id.to_string(),
        month: bill.month,
        invoice: invoice_to_view(invoice),
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

async fn transition_invoice_state(
    State(app): State<AppState>,
    Path((org_id, month)): Path<(FlexUuid, String)>,
    Authed(ctx): Authed,
    Json(req): Json<TransitionInvoiceRequest>,
) -> AppResult<Json<InvoiceView>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgBillingWrite, Scope::Org(&org));

    if !is_valid_month(&month) {
        return Err(AppError::BadRequest(
            "month must be in YYYY-MM format".into(),
        ));
    }

    let target_status = parse_invoice_status(&req.status)?;
    let invoice = app
        .repos
        .billing
        .transition_invoice(org, &month, target_status, req.export_digest.as_deref())
        .await?;

    app.audit.emit(
        &ctx,
        "billing.invoice.transition",
        "billing_invoice",
        Some(org_id.0),
        Some(serde_json::json!({
            "month": &month,
            "status": target_status.as_str(),
            "export_digest": req.export_digest,
        })),
    );

    Ok(Json(invoice_to_view(invoice)))
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

fn parse_invoice_status(status: &str) -> AppResult<InvoiceStatus> {
    match status {
        "draft" => Ok(InvoiceStatus::Draft),
        "closed" => Ok(InvoiceStatus::Closed),
        "exported" => Ok(InvoiceStatus::Exported),
        "paid" => Ok(InvoiceStatus::Paid),
        "waived" => Ok(InvoiceStatus::Waived),
        _ => Err(AppError::BadRequest(
            "status must be one of: draft, closed, exported, paid, waived".into(),
        )),
    }
}

fn invoice_to_view(invoice: BillingInvoice) -> InvoiceView {
    InvoiceView {
        org_id: invoice.org_id.to_string(),
        month: invoice.month,
        status: invoice.status.as_str().to_string(),
        total_cost_usd: invoice.total_cost_usd.normalize().to_string(),
        total_cost_micros: invoice.total_cost_micros,
        export_digest: invoice.export_digest,
        closed_at: invoice.closed_at,
        exported_at: invoice.exported_at,
        paid_at: invoice.paid_at,
        waived_at: invoice.waived_at,
        created_at: invoice.created_at,
        updated_at: invoice.updated_at,
    }
}

// ============================================================================
// CSV Export
// ============================================================================

#[derive(Deserialize)]
pub struct ExportQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug))]
struct SignedExportResponse {
    org_id: String,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    rows: Vec<SignedExportRow>,
    digest: ExportDigest,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug))]
struct SignedExportRow {
    ts: DateTime<Utc>,
    org_id: String,
    project_id: String,
    api_key_id: String,
    channel_id: Option<String>,
    model: String,
    tokens_in: i64,
    tokens_out: i64,
    cost_usd: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Debug))]
struct ExportDigest {
    alg: &'static str,
    value: String,
    rows: usize,
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
        return Err(AppError::BadRequest("'from' must be before 'to'".into()));
    }

    let rows = app
        .repos
        .billing
        .export_usage_csv(org, q.from, q.to)
        .await?;

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
    let digest = sha256_hex(&csv_bytes);

    let mut response = Response::new(Body::from(csv_bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"usage-export.csv\""),
    );
    response.headers_mut().insert(
        "x-kooix-export-digest",
        HeaderValue::from_str(&format!("sha256:{digest}"))
            .map_err(|e| AppError::Internal(format!("digest header: {e}")))?,
    );
    Ok(response)
}

async fn export_json(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Query(q): Query<ExportQuery>,
) -> AppResult<Json<SignedExportResponse>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::UsageExport, Scope::Org(&org));

    if q.from >= q.to {
        return Err(AppError::BadRequest("'from' must be before 'to'".into()));
    }

    let rows = app
        .repos
        .billing
        .export_usage_csv(org, q.from, q.to)
        .await?;

    let signed_rows: Vec<SignedExportRow> = rows
        .into_iter()
        .map(|r| SignedExportRow {
            ts: r.ts,
            org_id: r.org_id.to_string(),
            project_id: r.project_id.to_string(),
            api_key_id: r.api_key_id.to_string(),
            channel_id: r.channel_id.map(|c| c.to_string()),
            model: r.model,
            tokens_in: r.tokens_in,
            tokens_out: r.tokens_out,
            cost_usd: r.cost_usd.normalize().to_string(),
        })
        .collect();

    let canonical = serde_json::to_vec(&signed_rows)
        .map_err(|e| AppError::Internal(format!("json canonical export: {e}")))?;
    let digest = sha256_hex(&canonical);
    Ok(Json(SignedExportResponse {
        org_id: org_id.to_string(),
        from: q.from,
        to: q.to,
        digest: ExportDigest {
            alg: "sha256",
            value: digest,
            rows: signed_rows.len(),
        },
        rows: signed_rows,
    }))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn valid_month_accepts_normal_months() {
        assert!(is_valid_month("2026-05"));
        assert!(is_valid_month("2000-01"));
        assert!(is_valid_month("2100-12"));
    }

    #[test]
    fn valid_month_rejects_bad_months() {
        assert!(!is_valid_month("2026-5"));
        assert!(!is_valid_month("2026-00"));
        assert!(!is_valid_month("2026-13"));
        assert!(!is_valid_month("1999-12"));
        assert!(!is_valid_month("2101-01"));
    }

    #[test]
    fn parse_invoice_status_accepts_state_machine_values() {
        assert_eq!(parse_invoice_status("draft").unwrap(), InvoiceStatus::Draft);
        assert_eq!(
            parse_invoice_status("closed").unwrap(),
            InvoiceStatus::Closed
        );
        assert_eq!(
            parse_invoice_status("exported").unwrap(),
            InvoiceStatus::Exported
        );
        assert_eq!(parse_invoice_status("paid").unwrap(), InvoiceStatus::Paid);
        assert_eq!(
            parse_invoice_status("waived").unwrap(),
            InvoiceStatus::Waived
        );
        assert!(parse_invoice_status("void").is_err());
    }

    #[test]
    fn sha256_hex_is_stable_for_export_payloads() {
        let digest = sha256_hex(b"billing-export");
        assert_eq!(
            digest,
            "8109e06a40ecdc56db147ecdbbed6b655dddd8fae4c023f1ec6ca8cb3a651be2"
        );
    }

    #[test]
    fn signed_export_rows_have_stable_canonical_digest() {
        let rows = vec![SignedExportRow {
            ts: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
            org_id: "org".to_string(),
            project_id: "project".to_string(),
            api_key_id: "key".to_string(),
            channel_id: None,
            model: "gpt-4o-mini".to_string(),
            tokens_in: 10,
            tokens_out: 5,
            cost_usd: "0.00015".to_string(),
        }];
        let canonical = serde_json::to_vec(&rows).unwrap();
        assert_eq!(
            sha256_hex(&canonical),
            "4d742bcf3f5282d84bb2c00f86d940e5838a34516f6f57ab7c537dcbadb79fc7"
        );
    }

    #[test]
    fn signed_json_export_is_registered_in_manifest() {
        assert!(
            crate::route_manifest::all_routes().iter().any(|route| {
                route.method == "GET" && route.path == "/v1/orgs/:org_id/billing/export.json"
            }),
            "signed JSON billing export route must stay in route manifest"
        );
    }

    #[test]
    fn invoice_state_transition_route_is_registered_in_manifest() {
        assert!(
            crate::route_manifest::all_routes().iter().any(|route| {
                route.method == "POST" && route.path == "/v1/orgs/:org_id/billing/:month/state"
            }),
            "billing invoice state transition route must stay in route manifest"
        );
    }
}
