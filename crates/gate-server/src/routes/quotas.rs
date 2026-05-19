//! /v1/orgs/:org_id/quotas — Org-scoped quota CRUD（admin 视角）
//!
//! Endpoints:
//! - GET    /v1/orgs/:org_id/quotas              — 列出 Org 及其下属 project/api_key/user 的所有 quota
//! - POST   /v1/orgs/:org_id/quotas              — 创建/更新一行 quota（UPSERT）
//! - GET    /v1/orgs/:org_id/quotas/explain      — 解释某个 scope 当前会命中哪些规则
//! - GET    /v1/orgs/:org_id/quotas/reconcile    — Redis counter 与 PG usage 对账视图
//! - DELETE /v1/orgs/:org_id/quotas/:quota_id    — 硬删一行
//!
//! 权限：QuotaWrite / QuotaRead at `Scope::Org(&org)`。
//!
//! 越权防御（POST）：scope_id 必须可归属当前 Org：
//! - org：scope_id == org_id
//! - project：projects.org_id == org_id
//! - api_key：api_key 所属 project 的 org_id == org_id
//! - user：用户必须属于当前 Org
//! - membership / platform：直接拒绝

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::flex_uuid::FlexUuid;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{
    Json, Router,
    routing::{delete, get},
};
use chrono::{Datelike, TimeZone, Utc};
use gate_auth::require;
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
use gate_core::rbac::{Permission, Scope};
use gate_storage::{DbError, QuotaRecord, QuotaUpsert, ScopeUsageFilter, UsageTotals};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/orgs/:org_id/quotas", get(list_quotas).post(upsert_quota))
        .route("/orgs/:org_id/quotas/explain", get(explain_quota))
        .route("/orgs/:org_id/quotas/reconcile", get(reconcile_quotas))
        .route("/orgs/:org_id/quotas/:quota_id", delete(delete_quota))
}

#[derive(Serialize)]
pub struct QuotaView {
    pub id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub limit_value: String,
    pub window_seconds: Option<i32>,
    pub mode: String,
    pub enabled: bool,
}

impl From<gate_storage::QuotaRecord> for QuotaView {
    fn from(r: gate_storage::QuotaRecord) -> Self {
        Self {
            id: r.id.to_string(),
            scope_kind: r.scope_kind,
            scope_id: r.scope_id.to_string(),
            dimension: r.dimension,
            model_filter: r.model_filter,
            limit_value: r.limit_value.normalize().to_string(),
            window_seconds: r.window_seconds,
            mode: r.mode,
            enabled: r.enabled,
        }
    }
}

#[derive(Deserialize)]
pub struct UpsertQuotaRequest {
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub dimension: String,
    pub model_filter: Option<String>,
    /// 数值字符串避免 JSON f64 精度损失（rust_decimal 用 serde-with-str）
    pub limit_value: Decimal,
    pub window_seconds: Option<i32>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Deserialize)]
pub struct ExplainQuotaQuery {
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub dimension: Option<String>,
    pub model: Option<String>,
    pub estimated_tokens: Option<i64>,
    pub estimated_cost_micros: Option<i64>,
}

#[derive(Serialize)]
pub struct QuotaExplainResponse {
    pub org_id: String,
    pub rules: Vec<QuotaExplainRule>,
}

#[derive(Serialize)]
pub struct QuotaExplainRule {
    pub quota_id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub mode: String,
    pub limit: i64,
    pub current_used: i64,
    pub estimated: i64,
    pub remaining: i64,
    pub would_deny: bool,
    pub retry_after_ms: Option<i64>,
    pub reset_at: Option<String>,
}

#[derive(Serialize)]
pub struct QuotaReconcileResponse {
    pub org_id: String,
    pub rows: Vec<QuotaReconcileRow>,
}

#[derive(Serialize)]
pub struct QuotaReconcileRow {
    pub quota_id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub mode: String,
    pub redis_key: Option<String>,
    pub redis_used: Option<i64>,
    pub pg_used: i64,
    pub delta: Option<i64>,
    pub note: Option<String>,
}

async fn list_quotas(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<QuotaView>>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaRead, Scope::Org(&org));

    // Org 本身
    let mut out: Vec<QuotaView> = app
        .repos
        .quotas
        .list_by_scope("org", *org_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    // 下属 Project 的 quota
    let projects = app.repos.projects.list_in_org(org).await?;
    for p in &projects {
        let rows = app
            .repos
            .quotas
            .list_by_scope("project", *p.id.as_uuid())
            .await?;
        out.extend(rows.into_iter().map(Into::into));

        // Project 下的 api_keys
        let keys = app.repos.api_keys.list_in_project(p.id).await?;
        for k in &keys {
            let rows = app
                .repos
                .quotas
                .list_by_scope("api_key", *k.api_key_id.as_uuid())
                .await?;
            out.extend(rows.into_iter().map(Into::into));
        }
    }
    for m in app.repos.memberships.list_org_members(org).await? {
        let rows = app
            .repos
            .quotas
            .list_by_scope("user", *m.user_id.as_uuid())
            .await?;
        out.extend(rows.into_iter().map(Into::into));
    }

    Ok(Json(out))
}

async fn explain_quota(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Query(query): Query<ExplainQuotaQuery>,
    Authed(ctx): Authed,
) -> AppResult<Json<QuotaExplainResponse>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaRead, Scope::Org(&org));
    ensure_scope_owned(&app, org, &query.scope_kind, query.scope_id).await?;

    let mut rules = Vec::new();
    for q in app
        .repos
        .quotas
        .list_by_scope(&query.scope_kind, query.scope_id)
        .await?
        .into_iter()
        .filter(|q| q.enabled)
    {
        if let Some(dim) = query.dimension.as_deref()
            && q.dimension != dim
        {
            continue;
        }
        if !model_matches(q.model_filter.as_deref(), query.model.as_deref()) {
            continue;
        }
        rules.push(explain_rule(&app, &q, &query).await);
    }

    Ok(Json(QuotaExplainResponse {
        org_id: org.to_string(),
        rules,
    }))
}

async fn reconcile_quotas(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<QuotaReconcileResponse>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaRead, Scope::Org(&org));

    let quotas = owned_quota_records(&app, org, *org_id).await?;
    let mut rows = Vec::with_capacity(quotas.len());
    for q in quotas {
        rows.push(reconcile_rule(&app, org, &q).await?);
    }
    Ok(Json(QuotaReconcileResponse {
        org_id: org.to_string(),
        rows,
    }))
}

async fn upsert_quota(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpsertQuotaRequest>,
) -> AppResult<Json<QuotaView>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaWrite, Scope::Org(&org));

    // 验 Org 存在
    let _ = app.repos.orgs.find_by_id(org).await?;

    // 维度白名单（schema CHECK 也会拦，提前拦能给 400 而非 db constraint）
    if !matches!(
        req.dimension.as_str(),
        "rpm"
            | "tpm"
            | "concurrent"
            | "daily_budget_usd"
            | "monthly_budget_usd"
            | "lifetime_budget_usd"
            | "lifetime_tokens"
    ) {
        return Err(AppError::BadRequest(format!(
            "unknown dimension: {}",
            req.dimension
        )));
    }

    if req.limit_value < Decimal::ZERO {
        return Err(AppError::BadRequest("limit_value must be >= 0".into()));
    }
    let mode = req.mode.unwrap_or_else(|| "enforce".into());
    if !matches!(mode.as_str(), "enforce" | "dry_run") {
        return Err(AppError::BadRequest(
            "mode must be enforce or dry_run".into(),
        ));
    }

    // 越权写防御：scope_id 必须可归属当前 Org
    match req.scope_kind.as_str() {
        "org" => {
            if req.scope_id != *org_id {
                return Err(AppError::BadRequest(
                    "scope_id must equal path org_id when scope_kind=org".into(),
                ));
            }
        }
        "project" => {
            let p = app
                .repos
                .projects
                .find_by_id(ProjectId::from(req.scope_id))
                .await
                .map_err(|_| AppError::NotFound)?;
            if p.org_id != org {
                return Err(AppError::NotFound);
            }
        }
        "api_key" => {
            ensure_api_key_owned(&app, org, ApiKeyId::from(req.scope_id)).await?;
        }
        "user" => {
            let target = UserId::from(req.scope_id);
            if app.repos.users.find_by_id(target).await.is_err() {
                return Err(AppError::NotFound);
            }
            let members = app.repos.memberships.load_for_user(target).await?;
            if !members.orgs.contains_key(&org)
                && !members
                    .projects
                    .keys()
                    .any(|(member_org, _)| *member_org == org)
            {
                return Err(AppError::NotFound);
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "scope_kind '{other}' not manageable via Org endpoint (use platform admin)"
            )));
        }
    }

    let rec = app
        .repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: req.scope_kind,
            scope_id: req.scope_id,
            dimension: req.dimension,
            model_filter: req.model_filter,
            limit_value: req.limit_value,
            window_seconds: req.window_seconds,
            mode,
        })
        .await?;

    app.audit.emit(
        &ctx,
        "quota.upsert",
        "quota",
        Some(rec.id),
        Some(serde_json::json!({
            "scope_kind": &rec.scope_kind,
            "scope_id": rec.scope_id.to_string(),
            "dimension": &rec.dimension,
            "mode": &rec.mode,
        })),
    );

    Ok(Json(rec.into()))
}

async fn delete_quota(
    State(app): State<AppState>,
    Path((org_id, quota_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaWrite, Scope::Org(&org));

    // 防越权：删除前先确认目标 quota 归属当前 Org
    // list 全部 Org 维度（含子层）然后定位 ID；规模可控因为 Org 级 quota 行数有限。
    let mut owned = false;
    for r in app.repos.quotas.list_by_scope("org", *org_id).await? {
        if r.id == *quota_id {
            owned = true;
            break;
        }
    }
    if !owned {
        for p in app.repos.projects.list_in_org(org).await? {
            for r in app
                .repos
                .quotas
                .list_by_scope("project", *p.id.as_uuid())
                .await?
            {
                if r.id == *quota_id {
                    owned = true;
                    break;
                }
            }
            if owned {
                break;
            }
            for k in app.repos.api_keys.list_in_project(p.id).await? {
                for r in app
                    .repos
                    .quotas
                    .list_by_scope("api_key", *k.api_key_id.as_uuid())
                    .await?
                {
                    if r.id == *quota_id {
                        owned = true;
                        break;
                    }
                }
                if owned {
                    break;
                }
            }
            if owned {
                break;
            }
        }
    }
    if !owned {
        for m in app.repos.memberships.list_org_members(org).await? {
            for r in app
                .repos
                .quotas
                .list_by_scope("user", *m.user_id.as_uuid())
                .await?
            {
                if r.id == *quota_id {
                    owned = true;
                    break;
                }
            }
            if owned {
                break;
            }
        }
    }
    if !owned {
        return Err(AppError::NotFound);
    }

    match app.repos.quotas.delete(*quota_id).await {
        Ok(()) => {
            app.audit
                .emit(&ctx, "quota.delete", "quota", Some(*quota_id), None);
            Ok(Json(serde_json::json!({"deleted": true})))
        }
        Err(DbError::NotFound) => Err(AppError::NotFound),
        Err(e) => Err(e.into()),
    }
}

async fn ensure_api_key_owned(app: &AppState, org: OrgId, target: ApiKeyId) -> AppResult<()> {
    let projects = app.repos.projects.list_in_org(org).await?;
    for p in projects {
        let keys = app.repos.api_keys.list_in_project(p.id).await?;
        if keys.iter().any(|k| k.api_key_id == target) {
            return Ok(());
        }
    }
    Err(AppError::NotFound)
}

async fn ensure_scope_owned(
    app: &AppState,
    org: OrgId,
    scope_kind: &str,
    scope_id: Uuid,
) -> AppResult<()> {
    match scope_kind {
        "org" => {
            if scope_id == *org.as_uuid() {
                Ok(())
            } else {
                Err(AppError::NotFound)
            }
        }
        "project" => {
            let p = app
                .repos
                .projects
                .find_by_id(ProjectId::from(scope_id))
                .await
                .map_err(|_| AppError::NotFound)?;
            if p.org_id == org {
                Ok(())
            } else {
                Err(AppError::NotFound)
            }
        }
        "api_key" => ensure_api_key_owned(app, org, ApiKeyId::from(scope_id)).await,
        "user" => {
            let user = UserId::from(scope_id);
            if app.repos.users.find_by_id(user).await.is_err() {
                return Err(AppError::NotFound);
            }
            let memberships = app.repos.memberships.load_for_user(user).await?;
            if memberships.orgs.contains_key(&org)
                || memberships
                    .projects
                    .keys()
                    .any(|(member_org, _)| *member_org == org)
            {
                Ok(())
            } else {
                Err(AppError::NotFound)
            }
        }
        _ => Err(AppError::BadRequest(format!(
            "scope_kind '{scope_kind}' not manageable via Org endpoint"
        ))),
    }
}

async fn owned_quota_records(
    app: &AppState,
    org: OrgId,
    org_uuid: Uuid,
) -> AppResult<Vec<QuotaRecord>> {
    let mut out = app.repos.quotas.list_by_scope("org", org_uuid).await?;
    let projects = app.repos.projects.list_in_org(org).await?;
    for p in &projects {
        out.extend(
            app.repos
                .quotas
                .list_by_scope("project", *p.id.as_uuid())
                .await?,
        );
        for k in app.repos.api_keys.list_in_project(p.id).await? {
            out.extend(
                app.repos
                    .quotas
                    .list_by_scope("api_key", *k.api_key_id.as_uuid())
                    .await?,
            );
        }
    }
    for m in app.repos.memberships.list_org_members(org).await? {
        out.extend(
            app.repos
                .quotas
                .list_by_scope("user", *m.user_id.as_uuid())
                .await?,
        );
    }
    Ok(out)
}

async fn explain_rule(
    app: &AppState,
    q: &QuotaRecord,
    query: &ExplainQuotaQuery,
) -> QuotaExplainRule {
    let limit = quota_limit_units(q).unwrap_or(0);
    let estimated = match q.dimension.as_str() {
        "rpm" | "concurrent" => 1,
        "tpm" | "lifetime_tokens" => query.estimated_tokens.unwrap_or(0).max(0),
        "daily_budget_usd" | "monthly_budget_usd" | "lifetime_budget_usd" => {
            query.estimated_cost_micros.unwrap_or(0).max(0)
        }
        _ => 0,
    };
    let key = quota_key_for_view(q);
    let mut current_used = 0;
    let mut retry_after_ms = None;
    let mut reset_at = None;
    if q.dimension == "rpm" || q.dimension == "tpm" {
        if let Some(rl) = &app.rate_limiter {
            let window_ms = i64::from(q.window_seconds.unwrap_or(60).max(1)) * 1000;
            current_used = rl.peek_count(&key, window_ms as u64).await.unwrap_or(0) as i64;
            if current_used + estimated > limit {
                retry_after_ms = Some(window_ms);
            }
            reset_at = Some((Utc::now() + chrono::Duration::milliseconds(window_ms)).to_rfc3339());
        }
    } else if let Some(qc) = &app.quota_counter {
        current_used = qc.peek(&key).await.unwrap_or(0);
        if let Ok(ttl) = qc.pttl_ms(&key).await
            && ttl > 0
        {
            retry_after_ms = Some(ttl);
            reset_at = Some((Utc::now() + chrono::Duration::milliseconds(ttl)).to_rfc3339());
        }
    }
    let remaining = limit.saturating_sub(current_used);
    QuotaExplainRule {
        quota_id: q.id.to_string(),
        scope_kind: q.scope_kind.clone(),
        scope_id: q.scope_id.to_string(),
        dimension: q.dimension.clone(),
        model_filter: q.model_filter.clone(),
        mode: q.mode.clone(),
        limit,
        current_used,
        estimated,
        remaining,
        would_deny: current_used.saturating_add(estimated) > limit,
        retry_after_ms,
        reset_at,
    }
}

async fn reconcile_rule(
    app: &AppState,
    org: OrgId,
    q: &QuotaRecord,
) -> AppResult<QuotaReconcileRow> {
    let redis_key = counter_dimension(&q.dimension).then(|| quota_key_for_view(q));
    let redis_used = match (&app.quota_counter, redis_key.as_deref()) {
        (Some(qc), Some(key)) => Some(qc.peek(key).await.unwrap_or(0)),
        _ => None,
    };
    let pg_totals = usage_totals_for_quota(app, org, q).await?;
    let pg_used = match q.dimension.as_str() {
        "daily_budget_usd" | "monthly_budget_usd" | "lifetime_budget_usd" => {
            (pg_totals.cost_usd * 1_000_000.0).round() as i64
        }
        "lifetime_tokens" | "tpm" => pg_totals.tokens_in + pg_totals.tokens_out,
        // concurrent/rpm 是 runtime-only，PG usage_records 不保存分钟窗口/并发状态。
        _ => 0,
    };
    Ok(QuotaReconcileRow {
        quota_id: q.id.to_string(),
        scope_kind: q.scope_kind.clone(),
        scope_id: q.scope_id.to_string(),
        dimension: q.dimension.clone(),
        model_filter: q.model_filter.clone(),
        mode: q.mode.clone(),
        redis_key,
        redis_used,
        pg_used,
        delta: redis_used.map(|used| used - pg_used),
        note: reconcile_note(q),
    })
}

async fn usage_totals_for_quota(
    app: &AppState,
    org: OrgId,
    q: &QuotaRecord,
) -> AppResult<UsageTotals> {
    let (from, to) = usage_window(&q.dimension);
    Ok(app
        .repos
        .usage
        .totals_for_scope(ScopeUsageFilter {
            org_id: org,
            scope_kind: q.scope_kind.clone(),
            scope_id: q.scope_id,
            model_filter: q.model_filter.clone(),
            from,
            to,
        })
        .await?)
}

fn usage_window(dimension: &str) -> (Option<chrono::DateTime<Utc>>, Option<chrono::DateTime<Utc>>) {
    let now = Utc::now();
    match dimension {
        "daily_budget_usd" => {
            let start = Utc
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .unwrap();
            (Some(start), Some(start + chrono::Duration::days(1)))
        }
        "monthly_budget_usd" => {
            let start = Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .unwrap();
            let (next_year, next_month) = if now.month() == 12 {
                (now.year() + 1, 1)
            } else {
                (now.year(), now.month() + 1)
            };
            let end = Utc
                .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                .unwrap();
            (Some(start), Some(end))
        }
        _ => (None, None),
    }
}

fn quota_limit_units(q: &QuotaRecord) -> Option<i64> {
    match q.dimension.as_str() {
        "daily_budget_usd" | "monthly_budget_usd" | "lifetime_budget_usd" => {
            (q.limit_value * Decimal::from(1_000_000)).to_i64()
        }
        _ => q.limit_value.to_i64(),
    }
}

fn quota_key_for_view(q: &QuotaRecord) -> String {
    if q.dimension == "rpm" || q.dimension == "tpm" {
        let mf = q.model_filter.as_deref().unwrap_or("*");
        format!(
            "qt:{}:{}:{}:{}:{}",
            q.scope_kind, q.scope_id, q.dimension, mf, q.id
        )
    } else {
        crate::middleware::quota::quota_counter_key(q)
    }
}

fn counter_dimension(dimension: &str) -> bool {
    matches!(
        dimension,
        "concurrent"
            | "daily_budget_usd"
            | "monthly_budget_usd"
            | "lifetime_budget_usd"
            | "lifetime_tokens"
    )
}

fn model_matches(filter: Option<&str>, model: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    if filter == "*" {
        return true;
    }
    let Some(model) = model else {
        return false;
    };
    if !filter.contains('*') {
        return filter == model;
    }
    let parts: Vec<&str> = filter.split('*').collect();
    let mut rest = model;
    let mut first = true;
    for part in parts.iter().copied().filter(|p| !p.is_empty()) {
        if first && !filter.starts_with('*') {
            if let Some(stripped) = rest.strip_prefix(part) {
                rest = stripped;
            } else {
                return false;
            }
        } else if let Some(idx) = rest.find(part) {
            rest = &rest[idx + part.len()..];
        } else {
            return false;
        }
        first = false;
    }
    filter.ends_with('*') || parts.last().is_none_or(|last| model.ends_with(last))
}

fn reconcile_note(q: &QuotaRecord) -> Option<String> {
    match q.dimension.as_str() {
        "rpm" | "concurrent" => Some("runtime-only dimension; PG usage has no equivalent".into()),
        "tpm" => Some("PG value is current persisted usage, not sliding Redis window".into()),
        _ if q.model_filter.as_deref().is_some_and(|m| m.contains('*')) => {
            Some("PG reconciliation for glob model_filter is best-effort".into())
        }
        _ => None,
    }
}
