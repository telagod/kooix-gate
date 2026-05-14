use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct RequestRecord {
    pub ts: DateTime<Utc>,
    pub request_id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub channel_id: Uuid,
    pub channel_key_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub model_requested: String,
    pub model_actual: String,
    pub stream: bool,
    pub tokens_in: i32,
    pub tokens_out: i32,
    pub tokens_cached: i32,
    pub cost_usd: f64,
    pub latency_ms: Option<i32>,
    pub ttfb_ms: Option<i32>,
    pub status: i16,
    pub error_code: Option<String>,
    pub retries: i16,
    pub client_ip: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestFilter {
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
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestPage {
    pub data: Vec<RequestRecord>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardStats {
    pub total_requests: i64,
    pub total_errors: i64,
    pub error_rate: f64,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub total_cost_usd: f64,
    pub total_tokens: i64,
    pub top_models: Vec<ModelRank>,
    pub hourly_trend: Vec<HourlyBucket>,
    pub recent_errors: Vec<RequestRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRank {
    pub model: String,
    pub requests: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HourlyBucket {
    pub hour: String,
    pub requests: i64,
    pub errors: i64,
    pub cost_usd: f64,
}

#[async_trait]
pub trait RequestLogRepo: Send + Sync + 'static {
    async fn list(
        &self,
        filter: &RequestFilter,
        cursor: Option<&str>,
        limit: i64,
    ) -> DbResult<RequestPage>;

    async fn find_by_request_id(&self, request_id: Uuid) -> DbResult<RequestRecord>;

    async fn dashboard_stats(
        &self,
        org_id: Option<Uuid>,
        hours: i64,
    ) -> DbResult<DashboardStats>;
}

pub struct PgRequestLogRepo {
    pool: PgPool,
}

impl PgRequestLogRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_cursor(cursor: &str) -> Option<(DateTime<Utc>, Uuid)> {
    let parts: Vec<&str> = cursor.splitn(2, '|').collect();
    if parts.len() != 2 {
        return None;
    }
    let ts = parts[0].parse::<DateTime<Utc>>().ok()?;
    let id = parts[1].parse::<Uuid>().ok()?;
    Some((ts, id))
}

fn encode_cursor(ts: &DateTime<Utc>, request_id: &Uuid) -> String {
    format!("{}|{}", ts.to_rfc3339(), request_id)
}

const COLS: &str = "ts, request_id, org_id, project_id, api_key_id, user_id, \
    channel_id, channel_key_id, group_id, model_requested, model_actual, stream, \
    tokens_in, tokens_out, tokens_cached, cost_usd::float8 AS cost_f64, \
    latency_ms, ttfb_ms, status, error_code, retries, client_ip, metadata";

fn row_to_record(r: &sqlx::postgres::PgRow) -> DbResult<RequestRecord> {
    use sqlx::Row;
    Ok(RequestRecord {
        ts: r.try_get("ts")?,
        request_id: r.try_get("request_id")?,
        org_id: r.try_get("org_id")?,
        project_id: r.try_get("project_id")?,
        api_key_id: r.try_get("api_key_id")?,
        user_id: r.try_get("user_id")?,
        channel_id: r.try_get("channel_id")?,
        channel_key_id: r.try_get("channel_key_id")?,
        group_id: r.try_get("group_id")?,
        model_requested: r.try_get("model_requested")?,
        model_actual: r.try_get("model_actual")?,
        stream: r.try_get("stream")?,
        tokens_in: r.try_get("tokens_in")?,
        tokens_out: r.try_get("tokens_out")?,
        tokens_cached: r.try_get("tokens_cached")?,
        cost_usd: r.try_get::<f64, _>("cost_f64").unwrap_or(0.0),
        latency_ms: r.try_get("latency_ms")?,
        ttfb_ms: r.try_get("ttfb_ms")?,
        status: r.try_get("status")?,
        error_code: r.try_get("error_code")?,
        retries: r.try_get("retries")?,
        client_ip: r.try_get::<Option<sqlx::types::ipnetwork::IpNetwork>, _>("client_ip")?
            .map(|ip| ip.to_string()),
        metadata: r.try_get("metadata")?,
    })
}

#[async_trait]
impl RequestLogRepo for PgRequestLogRepo {
    async fn list(
        &self,
        filter: &RequestFilter,
        cursor: Option<&str>,
        limit: i64,
    ) -> DbResult<RequestPage> {
        let limit = limit.clamp(1, 100);
        let fetch_limit = limit + 1;

        let mut conditions = vec!["1=1".to_string()];
        let mut bind_idx = 0u32;

        macro_rules! next_idx {
            () => {{ bind_idx += 1; format!("${bind_idx}") }};
        }

        if filter.org_id.is_some() {
            conditions.push(format!("org_id = {}", next_idx!()));
        }
        if filter.project_id.is_some() {
            conditions.push(format!("project_id = {}", next_idx!()));
        }
        if filter.channel_id.is_some() {
            conditions.push(format!("channel_id = {}", next_idx!()));
        }
        if filter.api_key_id.is_some() {
            conditions.push(format!("api_key_id = {}", next_idx!()));
        }
        if filter.model.is_some() {
            conditions.push(format!("model_actual = {}", next_idx!()));
        }
        if filter.from.is_some() {
            conditions.push(format!("ts >= {}", next_idx!()));
        }
        if filter.to.is_some() {
            conditions.push(format!("ts < {}", next_idx!()));
        }
        if filter.error_only == Some(true) {
            conditions.push("status >= 400".to_string());
        } else {
            if filter.status_min.is_some() {
                conditions.push(format!("status >= {}", next_idx!()));
            }
            if filter.status_max.is_some() {
                conditions.push(format!("status <= {}", next_idx!()));
            }
        }
        if filter.search.is_some() {
            let idx = next_idx!();
            conditions.push(format!(
                "(model_actual ILIKE {idx} OR model_requested ILIKE {idx} OR error_code ILIKE {idx})"
            ));
        }

        let cursor_parsed = cursor.and_then(parse_cursor);
        if cursor_parsed.is_some() {
            let ts_idx = next_idx!();
            let id_idx = next_idx!();
            conditions.push(format!("(ts, request_id) < ({ts_idx}, {id_idx})"));
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT {COLS} FROM usage_records WHERE {where_clause} ORDER BY ts DESC, request_id DESC LIMIT {fetch_limit}"
        );

        let mut q = sqlx::query(&sql);

        if let Some(v) = &filter.org_id { q = q.bind(v); }
        if let Some(v) = &filter.project_id { q = q.bind(v); }
        if let Some(v) = &filter.channel_id { q = q.bind(v); }
        if let Some(v) = &filter.api_key_id { q = q.bind(v); }
        if let Some(v) = &filter.model { q = q.bind(v); }
        if let Some(v) = &filter.from { q = q.bind(v); }
        if let Some(v) = &filter.to { q = q.bind(v); }
        if filter.error_only != Some(true) {
            if let Some(v) = &filter.status_min { q = q.bind(v); }
            if let Some(v) = &filter.status_max { q = q.bind(v); }
        }
        if let Some(v) = &filter.search {
            let pattern = format!("%{v}%");
            q = q.bind(pattern);
        }
        if let Some((ts, id)) = &cursor_parsed {
            q = q.bind(ts);
            q = q.bind(id);
        }

        let rows = q.fetch_all(&self.pool).await?;

        let has_more = rows.len() as i64 > limit;
        let data_rows = if has_more { &rows[..limit as usize] } else { &rows };

        let mut data = Vec::with_capacity(data_rows.len());
        for r in data_rows {
            data.push(row_to_record(r)?);
        }

        let next_cursor = if has_more {
            data.last().map(|r| encode_cursor(&r.ts, &r.request_id))
        } else {
            None
        };

        Ok(RequestPage {
            data,
            next_cursor,
            has_more,
        })
    }

    async fn find_by_request_id(&self, request_id: Uuid) -> DbResult<RequestRecord> {
        let sql = format!("SELECT {COLS} FROM usage_records WHERE request_id = $1 LIMIT 1");
        let row = sqlx::query(&sql)
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;

        row_to_record(&row)
    }

    async fn dashboard_stats(
        &self,
        org_id: Option<Uuid>,
        hours: i64,
    ) -> DbResult<DashboardStats> {
        let hours = hours.clamp(1, 720);
        let org_filter = if org_id.is_some() { "AND org_id = $2" } else { "" };

        // Totals + error count + percentiles
        let stats_sql = format!(
            "SELECT \
                COUNT(*) AS total_requests, \
                COUNT(*) FILTER (WHERE status >= 400) AS total_errors, \
                COALESCE(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY latency_ms) FILTER (WHERE latency_ms IS NOT NULL), 0) AS p50, \
                COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms) FILTER (WHERE latency_ms IS NOT NULL), 0) AS p95, \
                COALESCE(SUM(cost_usd), 0)::float8 AS total_cost, \
                COALESCE(SUM(tokens_in + tokens_out), 0)::bigint AS total_tokens \
             FROM usage_records \
             WHERE ts >= NOW() - make_interval(hours => $1::int) {org_filter}"
        );
        let mut q = sqlx::query(&stats_sql).bind(hours as i32);
        if let Some(o) = org_id { q = q.bind(o); }
        let row = q.fetch_one(&self.pool).await?;

        let total_requests: i64 = row.try_get("total_requests")?;
        let total_errors: i64 = row.try_get("total_errors")?;
        let p50: f64 = row.try_get("p50")?;
        let p95: f64 = row.try_get("p95")?;
        let total_cost: f64 = row.try_get("total_cost")?;
        let total_tokens: i64 = row.try_get("total_tokens")?;
        let error_rate = if total_requests > 0 {
            total_errors as f64 / total_requests as f64
        } else {
            0.0
        };

        // Top models
        let models_sql = format!(
            "SELECT model_actual AS model, COUNT(*) AS requests, \
                    COALESCE(SUM(cost_usd), 0)::float8 AS cost_usd \
             FROM usage_records \
             WHERE ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             GROUP BY model_actual \
             ORDER BY requests DESC \
             LIMIT 5"
        );
        let mut q = sqlx::query(&models_sql).bind(hours as i32);
        if let Some(o) = org_id { q = q.bind(o); }
        let model_rows = q.fetch_all(&self.pool).await?;
        let top_models: Vec<ModelRank> = model_rows.iter().map(|r| {
            ModelRank {
                model: r.try_get("model").unwrap_or_default(),
                requests: r.try_get("requests").unwrap_or(0),
                cost_usd: r.try_get("cost_usd").unwrap_or(0.0),
            }
        }).collect();

        // Hourly trend
        let trend_sql = format!(
            "SELECT to_char(date_trunc('hour', ts), 'HH24:MI') AS hour, \
                    COUNT(*) AS requests, \
                    COUNT(*) FILTER (WHERE status >= 400) AS errors, \
                    COALESCE(SUM(cost_usd), 0)::float8 AS cost_usd \
             FROM usage_records \
             WHERE ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             GROUP BY date_trunc('hour', ts) \
             ORDER BY date_trunc('hour', ts) ASC"
        );
        let mut q = sqlx::query(&trend_sql).bind(hours as i32);
        if let Some(o) = org_id { q = q.bind(o); }
        let trend_rows = q.fetch_all(&self.pool).await?;
        let hourly_trend: Vec<HourlyBucket> = trend_rows.iter().map(|r| {
            HourlyBucket {
                hour: r.try_get("hour").unwrap_or_default(),
                requests: r.try_get("requests").unwrap_or(0),
                errors: r.try_get("errors").unwrap_or(0),
                cost_usd: r.try_get("cost_usd").unwrap_or(0.0),
            }
        }).collect();

        // Recent errors (last 5)
        let errors_sql = format!(
            "SELECT {COLS} FROM usage_records \
             WHERE status >= 400 AND ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY ts DESC \
             LIMIT 5"
        );
        let mut q = sqlx::query(&errors_sql).bind(hours as i32);
        if let Some(o) = org_id { q = q.bind(o); }
        let error_rows = q.fetch_all(&self.pool).await?;
        let mut recent_errors = Vec::new();
        for r in &error_rows {
            if let Ok(rec) = row_to_record(r) {
                recent_errors.push(rec);
            }
        }

        Ok(DashboardStats {
            total_requests,
            total_errors,
            error_rate,
            p50_latency_ms: if p50 > 0.0 { Some(p50) } else { None },
            p95_latency_ms: if p95 > 0.0 { Some(p95) } else { None },
            total_cost_usd: total_cost,
            total_tokens,
            top_models,
            hourly_trend,
            recent_errors,
        })
    }
}

// InMemory stub for dev/test
#[derive(Default)]
pub struct InMemoryRequestLogRepo;

impl InMemoryRequestLogRepo {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RequestLogRepo for InMemoryRequestLogRepo {
    async fn list(&self, _filter: &RequestFilter, _cursor: Option<&str>, _limit: i64) -> DbResult<RequestPage> {
        Ok(RequestPage { data: vec![], next_cursor: None, has_more: false })
    }

    async fn find_by_request_id(&self, _request_id: Uuid) -> DbResult<RequestRecord> {
        Err(DbError::NotFound)
    }

    async fn dashboard_stats(&self, _org_id: Option<Uuid>, _hours: i64) -> DbResult<DashboardStats> {
        Ok(DashboardStats {
            total_requests: 0,
            total_errors: 0,
            error_rate: 0.0,
            p50_latency_ms: None,
            p95_latency_ms: None,
            total_cost_usd: 0.0,
            total_tokens: 0,
            top_models: vec![],
            hourly_trend: vec![],
            recent_errors: vec![],
        })
    }
}
