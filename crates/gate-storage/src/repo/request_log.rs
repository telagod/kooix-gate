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
    pub channel_id: Option<Uuid>,
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RequestFilter {
    // scope
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    // model
    pub model: Option<String>,
    pub model_requested: Option<String>,
    // status
    pub status_min: Option<i16>,
    pub status_max: Option<i16>,
    pub status_category: Option<String>, // "2xx" | "4xx" | "5xx"
    pub error_only: Option<bool>,
    pub error_code: Option<String>,
    // stream / retries
    pub stream: Option<bool>,
    pub has_retries: Option<bool>,
    // time
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    // ranges
    pub latency_min: Option<i32>,
    pub latency_max: Option<i32>,
    pub ttfb_min: Option<i32>,
    pub ttfb_max: Option<i32>,
    pub cost_min: Option<f64>,
    pub cost_max: Option<f64>,
    pub tokens_min: Option<i64>,
    pub tokens_max: Option<i64>,
    // search
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRank {
    pub model: String,
    pub requests: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyBucket {
    pub hour: String,
    pub requests: i64,
    pub errors: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterOptions {
    pub models: Vec<String>,
    pub channels: Vec<FilterOptionItem>,
    pub projects: Vec<FilterOptionItem>,
    pub error_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterOptionItem {
    pub id: Uuid,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentSummary {
    pub top_failing_channels: Vec<TopFailingChannel>,
    pub upstream_error_classes: UpstreamErrorClasses,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopFailingChannel {
    pub channel_id: Option<Uuid>,
    pub channel_name: Option<String>,
    pub provider_type: Option<String>,
    pub requests: i64,
    pub errors: i64,
    pub error_rate: f64,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpstreamErrorClasses {
    pub auth_401: i64,
    pub rate_limit_429: i64,
    pub upstream_5xx: i64,
    pub other_4xx: i64,
    pub unknown: i64,
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

    async fn dashboard_stats(&self, org_id: Option<Uuid>, hours: i64) -> DbResult<DashboardStats>;

    async fn filter_options(&self, org_id: Option<Uuid>, hours: i64) -> DbResult<FilterOptions>;

    async fn incident_summary(&self, org_id: Option<Uuid>, hours: i64)
    -> DbResult<IncidentSummary>;

    async fn ensure_partitions(&self, _months_ahead: i32) -> DbResult<Vec<String>> {
        Ok(Vec::new())
    }

    async fn prune_partitions(&self, _retention_months: i32, _dry_run: bool) -> DbResult<u64> {
        Ok(0)
    }

    async fn prune_details(&self, _retention_days: i32) -> DbResult<u64> {
        Ok(0)
    }
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

const USAGE_COLS: &str = "ts, request_id, org_id, project_id, api_key_id, user_id, \
    channel_id, channel_key_id, group_id, model_requested, model_actual, stream, \
    tokens_in, tokens_out, tokens_cached, cost_usd::float8 AS cost_f64, \
    latency_ms, ttfb_ms, status, error_code, retries, client_ip, metadata";

const EVENT_COLS: &str = "ts, request_id, org_id, project_id, api_key_id, user_id, \
    channel_id, channel_key_id, group_id, model_requested, model_actual, stream, \
    tokens_in, tokens_out, tokens_cached, cost_usd::float8 AS cost_f64, \
    latency_ms, ttfb_ms, status, error_code, retries, NULL::inet AS client_ip, NULL::jsonb AS metadata";

const REQUEST_LOG_COLS: &str = EVENT_COLS;

const EVENT_DETAIL_COLS: &str = "e.ts, e.request_id, e.org_id, e.project_id, e.api_key_id, e.user_id, \
    e.channel_id, e.channel_key_id, e.group_id, e.model_requested, e.model_actual, e.stream, \
    e.tokens_in, e.tokens_out, e.tokens_cached, e.cost_usd::float8 AS cost_f64, \
    e.latency_ms, e.ttfb_ms, e.status, e.error_code, e.retries, d.client_ip, d.metadata";

const REQUEST_LOG_DETAIL_COLS: &str = EVENT_DETAIL_COLS;

fn table_exists_sql(table: &str) -> String {
    format!(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = '{table}')"
    )
}

async fn table_exists(pool: &PgPool, table: &str) -> DbResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(&table_exists_sql(table))
        .fetch_one(pool)
        .await?)
}

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
        client_ip: r
            .try_get::<Option<sqlx::types::ipnetwork::IpNetwork>, _>("client_ip")?
            .map(|ip| ip.to_string()),
        metadata: r.try_get("metadata")?,
    })
}

impl PgRequestLogRepo {
    fn source_for_incidents(source: &str) -> &'static str {
        match source {
            "request_log_events" => "request_log_events",
            "request_events" => "request_events",
            _ => "usage_records",
        }
    }

    async fn list_source(&self) -> (&'static str, &'static str) {
        if table_exists(&self.pool, "request_log_events")
            .await
            .unwrap_or(false)
        {
            ("request_log_events", REQUEST_LOG_COLS)
        } else if table_exists(&self.pool, "request_events")
            .await
            .unwrap_or(false)
        {
            ("request_events", EVENT_COLS)
        } else {
            ("usage_records", USAGE_COLS)
        }
    }

    async fn compact_source(&self) -> &'static str {
        if table_exists(&self.pool, "request_log_events")
            .await
            .unwrap_or(false)
        {
            "request_log_events"
        } else if table_exists(&self.pool, "request_events")
            .await
            .unwrap_or(false)
        {
            "request_events"
        } else {
            "usage_records"
        }
    }

    async fn dashboard_stats_from_rollups(
        &self,
        org_id: Option<Uuid>,
        hours: i64,
    ) -> DbResult<DashboardStats> {
        let org_filter = if org_id.is_some() {
            "AND org_id = $2"
        } else {
            ""
        };

        let stats_sql = format!(
            "WITH filtered AS (
                SELECT * FROM usage_hourly_rollups
                WHERE bucket >= date_trunc('hour', NOW() - make_interval(hours => $1::int)) {org_filter}
             ), model_rank AS (
                SELECT model_actual AS model,
                       COALESCE(SUM(request_count), 0)::bigint AS requests,
                       COALESCE(SUM(cost_micros), 0)::float8 / 1000000 AS cost_usd
                FROM filtered
                GROUP BY model_actual
                ORDER BY requests DESC
                LIMIT 5
             ), hourly AS (
                SELECT bucket, to_char(bucket, 'HH24:MI') AS hour,
                       COALESCE(SUM(request_count), 0)::bigint AS requests,
                       COALESCE(SUM(error_count), 0)::bigint AS errors,
                       COALESCE(SUM(cost_micros), 0)::float8 / 1000000 AS cost_usd
                FROM filtered
                GROUP BY bucket
                ORDER BY bucket ASC
             )
             SELECT
                COALESCE((SELECT SUM(request_count) FROM filtered), 0)::bigint AS total_requests,
                COALESCE((SELECT SUM(error_count) FROM filtered), 0)::bigint AS total_errors,
                COALESCE((SELECT SUM(cost_micros) FROM filtered), 0)::float8 / 1000000 AS total_cost,
                COALESCE((SELECT SUM(tokens_in + tokens_out) FROM filtered), 0)::bigint AS total_tokens,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('model', model, 'requests', requests, 'cost_usd', cost_usd) ORDER BY requests DESC) FROM model_rank), '[]'::jsonb) AS top_models,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('hour', hour, 'requests', requests, 'errors', errors, 'cost_usd', cost_usd) ORDER BY bucket ASC) FROM hourly), '[]'::jsonb) AS hourly_trend"
        );
        let mut q = sqlx::query(&stats_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let row = q.fetch_one(&self.pool).await?;
        let total_requests: i64 = row.try_get("total_requests")?;
        let total_errors: i64 = row.try_get("total_errors")?;
        let total_cost: f64 = row.try_get("total_cost")?;
        let total_tokens: i64 = row.try_get("total_tokens")?;
        let top_models_value: serde_json::Value = row.try_get("top_models")?;
        let hourly_trend_value: serde_json::Value = row.try_get("hourly_trend")?;
        let top_models: Vec<ModelRank> =
            serde_json::from_value(top_models_value).unwrap_or_default();
        let hourly_trend: Vec<HourlyBucket> =
            serde_json::from_value(hourly_trend_value).unwrap_or_default();
        let error_rate = if total_requests > 0 {
            total_errors as f64 / total_requests as f64
        } else {
            0.0
        };

        let compact_source = self.compact_source().await;
        let events_org_filter = if org_id.is_some() {
            "AND org_id = $2"
        } else {
            ""
        };
        let errors_sql = format!(
            "SELECT {EVENT_COLS} FROM {compact_source}
             WHERE status >= 400 AND ts >= NOW() - make_interval(hours => $1::int) {events_org_filter}
             ORDER BY ts DESC
             LIMIT 5"
        );
        let mut q = sqlx::query(&errors_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
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
            p50_latency_ms: None,
            p95_latency_ms: None,
            total_cost_usd: total_cost,
            total_tokens,
            top_models,
            hourly_trend,
            recent_errors,
        })
    }

    async fn filter_options_from_events(
        &self,
        org_id: Option<Uuid>,
        hours: i64,
        compact_source: &str,
    ) -> DbResult<FilterOptions> {
        let org_filter = if org_id.is_some() {
            "AND e.org_id = $2"
        } else {
            ""
        };

        let models_sql = format!(
            "SELECT DISTINCT e.model_actual AS model \
             FROM {compact_source} e \
             WHERE e.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY model"
        );
        let mut q = sqlx::query(&models_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let models = rows
            .iter()
            .filter_map(|r| r.try_get("model").ok())
            .collect();

        let channels_sql = format!(
            "SELECT DISTINCT e.channel_id AS id, c.name AS label \
             FROM {compact_source} e \
             LEFT JOIN channels c ON c.id = e.channel_id \
             WHERE e.channel_id IS NOT NULL AND e.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY label NULLS LAST"
        );
        let mut q = sqlx::query(&channels_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let channels = rows
            .iter()
            .map(|r| FilterOptionItem {
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").ok(),
            })
            .collect();

        let projects_sql = format!(
            "SELECT DISTINCT e.project_id AS id, p.name AS label \
             FROM {compact_source} e \
             LEFT JOIN projects p ON p.id = e.project_id \
             WHERE e.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY label NULLS LAST"
        );
        let mut q = sqlx::query(&projects_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let projects = rows
            .iter()
            .map(|r| FilterOptionItem {
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").ok(),
            })
            .collect();

        let errors_sql = format!(
            "SELECT DISTINCT e.error_code \
             FROM {compact_source} e \
             WHERE e.error_code IS NOT NULL AND e.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY e.error_code"
        );
        let mut q = sqlx::query(&errors_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let error_codes = rows
            .iter()
            .filter_map(|r| r.try_get("error_code").ok())
            .collect();

        Ok(FilterOptions {
            models,
            channels,
            projects,
            error_codes,
        })
    }

    async fn incident_summary_from_source(
        &self,
        source: &str,
        org_id: Option<Uuid>,
        hours: i64,
    ) -> DbResult<IncidentSummary> {
        let table = Self::source_for_incidents(source);
        let alias = if table == "usage_records" { "u" } else { "e" };
        let org_filter = if org_id.is_some() {
            format!("AND {alias}.org_id = $2")
        } else {
            String::new()
        };

        let top_sql = format!(
            "WITH filtered AS (
                SELECT {alias}.channel_id,
                       {alias}.status,
                       {alias}.error_code,
                       {alias}.ts
                FROM {table} {alias}
                WHERE {alias}.ts >= NOW() - make_interval(hours => $1::int) {org_filter}
             ), grouped AS (
                SELECT f.channel_id,
                       COUNT(*)::BIGINT AS requests,
                       COUNT(*) FILTER (WHERE f.status >= 400 OR f.error_code IS NOT NULL)::BIGINT AS errors,
                       (
                         SELECT f2.error_code
                         FROM filtered f2
                         WHERE f2.channel_id IS NOT DISTINCT FROM f.channel_id
                           AND (f2.status >= 400 OR f2.error_code IS NOT NULL)
                         ORDER BY f2.ts DESC
                         LIMIT 1
                       ) AS last_error_code,
                       MAX(f.ts) FILTER (WHERE f.status >= 400 OR f.error_code IS NOT NULL) AS last_error_at
                FROM filtered f
                GROUP BY f.channel_id
             )
             SELECT g.channel_id,
                    c.name AS channel_name,
                    c.provider_type AS provider_type,
                    g.requests,
                    g.errors,
                    CASE WHEN g.requests > 0 THEN g.errors::float8 / g.requests::float8 ELSE 0 END AS error_rate,
                    g.last_error_code,
                    g.last_error_at
             FROM grouped g
             LEFT JOIN channels c ON c.id = g.channel_id
             WHERE g.errors > 0
             ORDER BY g.errors DESC, error_rate DESC, g.last_error_at DESC NULLS LAST
             LIMIT 10"
        );
        let mut q = sqlx::query(&top_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let top_failing_channels = rows
            .iter()
            .map(|r| TopFailingChannel {
                channel_id: r.try_get("channel_id").ok().flatten(),
                channel_name: r.try_get("channel_name").ok().flatten(),
                provider_type: r.try_get("provider_type").ok().flatten(),
                requests: r.try_get("requests").unwrap_or(0),
                errors: r.try_get("errors").unwrap_or(0),
                error_rate: r.try_get("error_rate").unwrap_or(0.0),
                last_error_code: r.try_get("last_error_code").ok().flatten(),
                last_error_at: r.try_get("last_error_at").ok().flatten(),
            })
            .collect();

        let classes_sql = format!(
            "SELECT
                COUNT(*) FILTER (WHERE status = 401 OR error_code IN ('authentication_error', 'invalid_api_key', 'unauthorized'))::BIGINT AS auth_401,
                COUNT(*) FILTER (WHERE status = 429 OR error_code IN ('rate_limit_error', 'rate_limited', 'too_many_requests'))::BIGINT AS rate_limit_429,
                COUNT(*) FILTER (WHERE status >= 500)::BIGINT AS upstream_5xx,
                COUNT(*) FILTER (
                    WHERE status >= 400 AND status < 500
                      AND status NOT IN (401, 429)
                      AND COALESCE(error_code, '') NOT IN ('authentication_error', 'invalid_api_key', 'unauthorized', 'rate_limit_error', 'rate_limited', 'too_many_requests')
                )::BIGINT AS other_4xx,
                COUNT(*) FILTER (WHERE (status < 400 OR status IS NULL) AND error_code IS NOT NULL)::BIGINT AS unknown
             FROM {table} {alias}
             WHERE {alias}.ts >= NOW() - make_interval(hours => $1::int)
               AND ({alias}.status >= 400 OR {alias}.error_code IS NOT NULL) {org_filter}"
        );
        let mut q = sqlx::query(&classes_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let row = q.fetch_one(&self.pool).await?;
        let upstream_error_classes = UpstreamErrorClasses {
            auth_401: row.try_get("auth_401").unwrap_or(0),
            rate_limit_429: row.try_get("rate_limit_429").unwrap_or(0),
            upstream_5xx: row.try_get("upstream_5xx").unwrap_or(0),
            other_4xx: row.try_get("other_4xx").unwrap_or(0),
            unknown: row.try_get("unknown").unwrap_or(0),
        };

        Ok(IncidentSummary {
            top_failing_channels,
            upstream_error_classes,
        })
    }
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
        let (from_sql, cols) = self.list_source().await;

        let mut conditions = vec!["1=1".to_string()];
        let mut bind_idx = 0u32;

        macro_rules! next_idx {
            () => {{
                bind_idx += 1;
                format!("${bind_idx}")
            }};
        }

        // scope
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
        if filter.user_id.is_some() {
            conditions.push(format!("user_id = {}", next_idx!()));
        }
        if filter.group_id.is_some() {
            conditions.push(format!("group_id = {}", next_idx!()));
        }

        // model
        if filter.model.is_some() {
            conditions.push(format!("model_actual = {}", next_idx!()));
        }
        if filter.model_requested.is_some() {
            conditions.push(format!("model_requested = {}", next_idx!()));
        }

        // time
        if filter.from.is_some() {
            conditions.push(format!("ts >= {}", next_idx!()));
        }
        if filter.to.is_some() {
            conditions.push(format!("ts < {}", next_idx!()));
        }

        // status
        if let Some(cat) = &filter.status_category {
            match cat.as_str() {
                "2xx" => conditions.push("status >= 200 AND status < 300".to_string()),
                "4xx" => conditions.push("status >= 400 AND status < 500".to_string()),
                "5xx" => conditions.push("status >= 500".to_string()),
                _ => {}
            }
        } else if filter.error_only == Some(true) {
            conditions.push("status >= 400".to_string());
        } else {
            if filter.status_min.is_some() {
                conditions.push(format!("status >= {}", next_idx!()));
            }
            if filter.status_max.is_some() {
                conditions.push(format!("status <= {}", next_idx!()));
            }
        }

        // error_code exact match
        if filter.error_code.is_some() {
            conditions.push(format!("error_code = {}", next_idx!()));
        }

        // stream
        if filter.stream.is_some() {
            conditions.push(format!("stream = {}", next_idx!()));
        }

        // has_retries
        if filter.has_retries == Some(true) {
            conditions.push("retries > 0".to_string());
        } else if filter.has_retries == Some(false) {
            conditions.push("retries = 0".to_string());
        }

        // latency range
        if filter.latency_min.is_some() {
            conditions.push(format!("latency_ms >= {}", next_idx!()));
        }
        if filter.latency_max.is_some() {
            conditions.push(format!("latency_ms <= {}", next_idx!()));
        }

        // ttfb range
        if filter.ttfb_min.is_some() {
            conditions.push(format!("ttfb_ms >= {}", next_idx!()));
        }
        if filter.ttfb_max.is_some() {
            conditions.push(format!("ttfb_ms <= {}", next_idx!()));
        }

        // cost range (cast to float8 for comparison)
        if filter.cost_min.is_some() {
            conditions.push(format!("cost_usd::float8 >= {}", next_idx!()));
        }
        if filter.cost_max.is_some() {
            conditions.push(format!("cost_usd::float8 <= {}", next_idx!()));
        }

        // tokens range (tokens_in + tokens_out)
        if filter.tokens_min.is_some() {
            conditions.push(format!(
                "(tokens_in + tokens_out)::bigint >= {}",
                next_idx!()
            ));
        }
        if filter.tokens_max.is_some() {
            conditions.push(format!(
                "(tokens_in + tokens_out)::bigint <= {}",
                next_idx!()
            ));
        }

        // search (ILIKE or UUID exact match)
        if let Some(s) = &filter.search {
            if Uuid::parse_str(s.trim()).is_ok() {
                conditions.push(format!("request_id = {}", next_idx!()));
            } else {
                let idx = next_idx!();
                conditions.push(format!(
                    "(model_actual ILIKE {idx} OR model_requested ILIKE {idx} OR error_code ILIKE {idx})"
                ));
            }
        }

        // cursor
        let cursor_parsed = cursor.and_then(parse_cursor);
        if cursor_parsed.is_some() {
            let ts_idx = next_idx!();
            let id_idx = next_idx!();
            conditions.push(format!("(ts, request_id) < ({ts_idx}, {id_idx})"));
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT {cols} FROM {from_sql} WHERE {where_clause} ORDER BY ts DESC, request_id DESC LIMIT {fetch_limit}"
        );

        let mut q = sqlx::query(&sql);

        // bind: scope
        if let Some(v) = &filter.org_id {
            q = q.bind(v);
        }
        if let Some(v) = &filter.project_id {
            q = q.bind(v);
        }
        if let Some(v) = &filter.channel_id {
            q = q.bind(v);
        }
        if let Some(v) = &filter.api_key_id {
            q = q.bind(v);
        }
        if let Some(v) = &filter.user_id {
            q = q.bind(v);
        }
        if let Some(v) = &filter.group_id {
            q = q.bind(v);
        }

        // bind: model
        if let Some(v) = &filter.model {
            q = q.bind(v);
        }
        if let Some(v) = &filter.model_requested {
            q = q.bind(v);
        }

        // bind: time
        if let Some(v) = &filter.from {
            q = q.bind(v);
        }
        if let Some(v) = &filter.to {
            q = q.bind(v);
        }

        // bind: status (only when no category and not error_only)
        if filter.status_category.is_none() && filter.error_only != Some(true) {
            if let Some(v) = &filter.status_min {
                q = q.bind(v);
            }
            if let Some(v) = &filter.status_max {
                q = q.bind(v);
            }
        }

        // bind: error_code
        if let Some(v) = &filter.error_code {
            q = q.bind(v);
        }

        // bind: stream
        if let Some(v) = &filter.stream {
            q = q.bind(v);
        }

        // bind: latency
        if let Some(v) = &filter.latency_min {
            q = q.bind(v);
        }
        if let Some(v) = &filter.latency_max {
            q = q.bind(v);
        }

        // bind: ttfb
        if let Some(v) = &filter.ttfb_min {
            q = q.bind(v);
        }
        if let Some(v) = &filter.ttfb_max {
            q = q.bind(v);
        }

        // bind: cost
        if let Some(v) = &filter.cost_min {
            q = q.bind(v);
        }
        if let Some(v) = &filter.cost_max {
            q = q.bind(v);
        }

        // bind: tokens
        if let Some(v) = &filter.tokens_min {
            q = q.bind(v);
        }
        if let Some(v) = &filter.tokens_max {
            q = q.bind(v);
        }

        // bind: search
        if let Some(s) = &filter.search {
            if Uuid::parse_str(s.trim()).is_ok() {
                q = q.bind(Uuid::parse_str(s.trim()).unwrap());
            } else {
                let pattern = format!("%{s}%");
                q = q.bind(pattern);
            }
        }

        // bind: cursor
        if let Some((ts, id)) = &cursor_parsed {
            q = q.bind(ts);
            q = q.bind(id);
        }

        let rows = q.fetch_all(&self.pool).await?;

        let has_more = rows.len() as i64 > limit;
        let data_rows = if has_more {
            &rows[..limit as usize]
        } else {
            &rows
        };

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
        let sql = if table_exists(&self.pool, "request_log_events")
            .await
            .unwrap_or(false)
        {
            format!(
                "SELECT {REQUEST_LOG_DETAIL_COLS} FROM request_log_events e \
                 LEFT JOIN request_event_details d ON d.request_id = e.request_id \
                 WHERE e.request_id = $1 LIMIT 1"
            )
        } else if table_exists(&self.pool, "request_events")
            .await
            .unwrap_or(false)
        {
            format!(
                "SELECT {EVENT_DETAIL_COLS} FROM request_events e \
                 LEFT JOIN request_event_details d ON d.request_id = e.request_id \
                 WHERE e.request_id = $1 LIMIT 1"
            )
        } else {
            format!("SELECT {USAGE_COLS} FROM usage_records WHERE request_id = $1 LIMIT 1")
        };
        let row = sqlx::query(&sql)
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(DbError::NotFound)?;

        row_to_record(&row)
    }

    async fn dashboard_stats(&self, org_id: Option<Uuid>, hours: i64) -> DbResult<DashboardStats> {
        let hours = hours.clamp(1, 720);
        if table_exists(&self.pool, "usage_hourly_rollups")
            .await
            .unwrap_or(false)
        {
            return self.dashboard_stats_from_rollups(org_id, hours).await;
        }
        let org_filter = if org_id.is_some() {
            "AND org_id = $2"
        } else {
            ""
        };

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
        if let Some(o) = org_id {
            q = q.bind(o);
        }
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
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let model_rows = q.fetch_all(&self.pool).await?;
        let top_models: Vec<ModelRank> = model_rows
            .iter()
            .map(|r| ModelRank {
                model: r.try_get("model").unwrap_or_default(),
                requests: r.try_get("requests").unwrap_or(0),
                cost_usd: r.try_get("cost_usd").unwrap_or(0.0),
            })
            .collect();

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
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let trend_rows = q.fetch_all(&self.pool).await?;
        let hourly_trend: Vec<HourlyBucket> = trend_rows
            .iter()
            .map(|r| HourlyBucket {
                hour: r.try_get("hour").unwrap_or_default(),
                requests: r.try_get("requests").unwrap_or(0),
                errors: r.try_get("errors").unwrap_or(0),
                cost_usd: r.try_get("cost_usd").unwrap_or(0.0),
            })
            .collect();

        let errors_sql = format!(
            "SELECT {USAGE_COLS} FROM usage_records \
             WHERE status >= 400 AND ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY ts DESC \
             LIMIT 5"
        );
        let mut q = sqlx::query(&errors_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
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

    async fn filter_options(&self, org_id: Option<Uuid>, hours: i64) -> DbResult<FilterOptions> {
        let hours = hours.clamp(1, 720);
        let compact_source = self.compact_source().await;
        if compact_source != "usage_records" {
            return self
                .filter_options_from_events(org_id, hours, compact_source)
                .await;
        }
        let org_filter = if org_id.is_some() {
            "AND org_id = $2"
        } else {
            ""
        };

        let models_sql = format!(
            "SELECT DISTINCT model_actual AS model \
             FROM usage_records \
             WHERE ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY model"
        );
        let mut q = sqlx::query(&models_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let models: Vec<String> = rows
            .iter()
            .filter_map(|r| r.try_get("model").ok())
            .collect();

        let channels_sql = format!(
            "SELECT DISTINCT u.channel_id AS id, c.name AS label \
             FROM usage_records u \
             LEFT JOIN channels c ON c.id = u.channel_id \
             WHERE u.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY label NULLS LAST"
        );
        let mut q = sqlx::query(&channels_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let channels: Vec<FilterOptionItem> = rows
            .iter()
            .map(|r| FilterOptionItem {
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").ok(),
            })
            .collect();

        let projects_sql = format!(
            "SELECT DISTINCT u.project_id AS id, p.name AS label \
             FROM usage_records u \
             LEFT JOIN projects p ON p.id = u.project_id \
             WHERE u.ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY label NULLS LAST"
        );
        let mut q = sqlx::query(&projects_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let projects: Vec<FilterOptionItem> = rows
            .iter()
            .map(|r| FilterOptionItem {
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").ok(),
            })
            .collect();

        let errors_sql = format!(
            "SELECT DISTINCT error_code \
             FROM usage_records \
             WHERE error_code IS NOT NULL AND ts >= NOW() - make_interval(hours => $1::int) {org_filter} \
             ORDER BY error_code"
        );
        let mut q = sqlx::query(&errors_sql).bind(hours as i32);
        if let Some(o) = org_id {
            q = q.bind(o);
        }
        let rows = q.fetch_all(&self.pool).await?;
        let error_codes: Vec<String> = rows
            .iter()
            .filter_map(|r| r.try_get("error_code").ok())
            .collect();

        Ok(FilterOptions {
            models,
            channels,
            projects,
            error_codes,
        })
    }

    async fn incident_summary(
        &self,
        org_id: Option<Uuid>,
        hours: i64,
    ) -> DbResult<IncidentSummary> {
        let hours = hours.clamp(1, 720);
        let source = self.compact_source().await;
        self.incident_summary_from_source(source, org_id, hours)
            .await
    }

    async fn ensure_partitions(&self, months_ahead: i32) -> DbResult<Vec<String>> {
        if !table_exists(&self.pool, "request_log_events")
            .await
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }
        let partitions = sqlx::query_scalar::<_, String>(
            "SELECT partition_name FROM kooix_ensure_request_log_partitions($1)",
        )
        .bind(months_ahead.clamp(0, 24))
        .fetch_all(&self.pool)
        .await?;
        Ok(partitions)
    }

    async fn prune_partitions(&self, retention_months: i32, dry_run: bool) -> DbResult<u64> {
        if !table_exists(&self.pool, "request_log_events")
            .await
            .unwrap_or(false)
        {
            return Ok(0);
        }
        let rows = sqlx::query("SELECT dropped FROM kooix_prune_request_log_partitions($1, $2)")
            .bind(retention_months.clamp(1, 120))
            .bind(dry_run)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .filter(|row| row.try_get::<bool, _>("dropped").unwrap_or(false))
            .count() as u64)
    }

    async fn prune_details(&self, retention_days: i32) -> DbResult<u64> {
        if !table_exists(&self.pool, "request_event_details")
            .await
            .unwrap_or(false)
        {
            return Ok(0);
        }
        let deleted = sqlx::query_scalar::<_, i64>("SELECT kooix_prune_request_log_details($1)")
            .bind(retention_days.clamp(1, 3650))
            .fetch_one(&self.pool)
            .await?;
        Ok(deleted.max(0) as u64)
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
    async fn list(
        &self,
        _filter: &RequestFilter,
        _cursor: Option<&str>,
        _limit: i64,
    ) -> DbResult<RequestPage> {
        Ok(RequestPage {
            data: vec![],
            next_cursor: None,
            has_more: false,
        })
    }

    async fn find_by_request_id(&self, _request_id: Uuid) -> DbResult<RequestRecord> {
        Err(DbError::NotFound)
    }

    async fn dashboard_stats(
        &self,
        _org_id: Option<Uuid>,
        _hours: i64,
    ) -> DbResult<DashboardStats> {
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

    async fn filter_options(&self, _org_id: Option<Uuid>, _hours: i64) -> DbResult<FilterOptions> {
        Ok(FilterOptions {
            models: vec![],
            channels: vec![],
            projects: vec![],
            error_codes: vec![],
        })
    }

    async fn incident_summary(
        &self,
        _org_id: Option<Uuid>,
        _hours: i64,
    ) -> DbResult<IncidentSummary> {
        Ok(IncidentSummary {
            top_failing_channels: vec![],
            upstream_error_classes: UpstreamErrorClasses::default(),
        })
    }
}
