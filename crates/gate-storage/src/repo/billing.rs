//! BillingRepo — 月度账单聚合 + 用量 CSV/JSON 导出。
//!
//! `usage_records` 仍是控制台 analytics projection；P1.5 引入
//! `billing_ledger_events` 作为不可变对账账本后，月账单优先按
//! `actual_settle` ledger 重建收入，再用 `usage_records` 补 tokens/model
//! 维度明细。
//! 月度范围按 UTC 自然月 [YYYY-MM-01 00:00:00, next-month-01 00:00:00) 划分。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use gate_core::id::{OrgId, ProjectId};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// DTO
// ============================================================================

/// 月度账单汇总。
#[derive(Debug, Clone)]
pub struct MonthlyBill {
    pub org_id: OrgId,
    pub month: String, // "2026-05"
    pub total_cost_usd: Decimal,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub total_requests: i64,
    pub breakdown_by_project: Vec<ProjectBillLine>,
    pub breakdown_by_model: Vec<ModelBillLine>,
}

#[derive(Debug, Clone)]
pub struct ProjectBillLine {
    pub project_id: ProjectId,
    pub cost_usd: Decimal,
    pub requests: i64,
}

#[derive(Debug, Clone)]
pub struct ModelBillLine {
    pub model: String,
    pub cost_usd: Decimal,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub requests: i64,
}

/// CSV 导出行。
#[derive(Debug, Clone)]
pub struct UsageExportRow {
    pub ts: DateTime<Utc>,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub api_key_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub model: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceStatus {
    Draft,
    Closed,
    Exported,
    Paid,
    Waived,
}

impl InvoiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Closed => "closed",
            Self::Exported => "exported",
            Self::Paid => "paid",
            Self::Waived => "waived",
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            (current, target) if current == target => true,
            (Self::Draft, Self::Closed) => true,
            (Self::Closed, Self::Exported) => true,
            (Self::Closed | Self::Exported, Self::Paid | Self::Waived) => true,
            _ => false,
        }
    }

    fn parse(value: &str) -> DbResult<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "closed" => Ok(Self::Closed),
            "exported" => Ok(Self::Exported),
            "paid" => Ok(Self::Paid),
            "waived" => Ok(Self::Waived),
            other => Err(DbError::Internal(format!(
                "unknown billing invoice status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BillingInvoice {
    pub org_id: OrgId,
    pub month: String,
    pub status: InvoiceStatus,
    pub total_cost_usd: Decimal,
    pub total_cost_micros: i64,
    pub export_digest: Option<String>,
    pub closed_at: Option<DateTime<Utc>>,
    pub exported_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub waived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// Helpers
// ============================================================================

/// 解析 "YYYY-MM" → (month_start_utc, next_month_start_utc)
fn parse_month_range(month: &str) -> DbResult<(DateTime<Utc>, DateTime<Utc>)> {
    let from_str = format!("{month}-01");
    let from_date = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
        .map_err(|e| DbError::Internal(format!("invalid month format: {e}")))?;
    let from_dt = from_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbError::Internal("invalid date".into()))?;
    let from_utc = DateTime::from_naive_utc_and_offset(from_dt, Utc);

    let (next_y, next_m) = if from_date.month() == 12 {
        (from_date.year() + 1, 1)
    } else {
        (from_date.year(), from_date.month() + 1)
    };
    let to_date = NaiveDate::from_ymd_opt(next_y, next_m, 1)
        .ok_or_else(|| DbError::Internal("invalid next month".into()))?;
    let to_dt = to_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbError::Internal("invalid date".into()))?;
    let to_utc = DateTime::from_naive_utc_and_offset(to_dt, Utc);

    Ok((from_utc, to_utc))
}

async fn table_exists(pool: &PgPool, table: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(table)
        .fetch_one(pool)
        .await
}

async fn usage_total_usd(
    pool: &PgPool,
    org_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> DbResult<Decimal> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(cost_usd), 0)::numeric \
         FROM usage_records \
         WHERE org_id = $1 AND ts >= $2 AND ts < $3",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await
    .map_err(DbError::from)
}

async fn ledger_total_usd(
    pool: &PgPool,
    org_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> DbResult<Decimal> {
    sqlx::query_scalar(
        "SELECT (COALESCE(SUM(amount_micros), 0)::numeric / 1000000)::numeric \
         FROM billing_ledger_events \
         WHERE org_id = $1 AND occurred_at >= $2 AND occurred_at < $3 \
           AND event_type = 'actual_settle' AND status = 'posted'",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await
    .map_err(DbError::from)
}

async fn ledger_total_micros(
    pool: &PgPool,
    org_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_micros), 0)::bigint \
         FROM billing_ledger_events \
         WHERE org_id = $1 AND occurred_at >= $2 AND occurred_at < $3 \
           AND event_type = 'actual_settle' AND status = 'posted'",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await
    .map_err(DbError::from)
}

fn decimal_to_micros(value: Decimal) -> i64 {
    (value * Decimal::from(1_000_000_i64))
        .round()
        .try_into()
        .unwrap_or_default()
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn invoice_from_row(row: &sqlx::postgres::PgRow) -> DbResult<BillingInvoice> {
    let status: String = row.try_get("status")?;
    Ok(BillingInvoice {
        org_id: OrgId::from(row.try_get::<Uuid, _>("org_id")?),
        month: row.try_get("month")?,
        status: InvoiceStatus::parse(&status)?,
        total_cost_usd: row.try_get("total_cost_usd")?,
        total_cost_micros: row.try_get("total_cost_micros")?,
        export_digest: row.try_get("export_digest")?,
        closed_at: row.try_get("closed_at")?,
        exported_at: row.try_get("exported_at")?,
        paid_at: row.try_get("paid_at")?,
        waived_at: row.try_get("waived_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

// ============================================================================
// Trait
// ============================================================================

#[async_trait]
pub trait BillingRepo: Send + Sync + 'static {
    /// 按月聚合 Org 级别账单。`month` 格式 "YYYY-MM"。
    async fn monthly_bill(&self, org_id: OrgId, month: &str) -> DbResult<MonthlyBill>;

    /// 获取或创建月账单状态行。
    async fn get_or_create_invoice(&self, org_id: OrgId, month: &str) -> DbResult<BillingInvoice>;

    /// 按严格状态机推进月账单状态。
    async fn transition_invoice(
        &self,
        org_id: OrgId,
        month: &str,
        target_status: InvoiceStatus,
        export_digest: Option<&str>,
    ) -> DbResult<BillingInvoice>;

    /// 导出用量明细（时间范围内的所有原始行）。
    async fn export_usage_csv(
        &self,
        org_id: OrgId,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<Vec<UsageExportRow>>;
}

// ============================================================================
// PgBillingRepo
// ============================================================================

pub struct PgBillingRepo {
    pool: PgPool,
}

impl PgBillingRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BillingRepo for PgBillingRepo {
    async fn monthly_bill(&self, org_id: OrgId, month: &str) -> DbResult<MonthlyBill> {
        let (from_utc, to_utc) = parse_month_range(month)?;

        let ledger_exists = table_exists(&self.pool, "billing_ledger_events").await?;

        // 总计：费用优先从 immutable ledger 重建；tokens/request 仍来自 usage projection。
        let totals_row = sqlx::query(
            "SELECT COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
                    COALESCE(SUM(tokens_out), 0)::bigint AS tokens_out, \
                    COUNT(*)::bigint AS requests \
             FROM usage_records \
             WHERE org_id = $1 AND ts >= $2 AND ts < $3",
        )
        .bind(*org_id.as_uuid())
        .bind(from_utc)
        .bind(to_utc)
        .fetch_one(&self.pool)
        .await?;

        let total_tokens_in: i64 = totals_row.try_get("tokens_in")?;
        let total_tokens_out: i64 = totals_row.try_get("tokens_out")?;
        let total_requests: i64 = totals_row.try_get("requests")?;
        let total_cost_usd = if ledger_exists {
            ledger_total_usd(&self.pool, *org_id.as_uuid(), from_utc, to_utc).await?
        } else {
            usage_total_usd(&self.pool, *org_id.as_uuid(), from_utc, to_utc).await?
        };

        // 按 project 分组
        let project_rows = if ledger_exists {
            sqlx::query(
                "SELECT COALESCE(u.project_id, l.project_id) AS project_id, \
                        COALESCE(l.cost_usd, 0)::numeric AS cost_usd, \
                        COALESCE(u.requests, 0)::bigint AS requests \
                 FROM ( \
                    SELECT project_id, COUNT(*)::bigint AS requests \
                    FROM usage_records \
                    WHERE org_id = $1 AND ts >= $2 AND ts < $3 \
                    GROUP BY project_id \
                 ) u \
                 FULL OUTER JOIN ( \
                    SELECT project_id, (SUM(amount_micros)::numeric / 1000000)::numeric AS cost_usd \
                    FROM billing_ledger_events \
                    WHERE org_id = $1 AND occurred_at >= $2 AND occurred_at < $3 \
                      AND event_type = 'actual_settle' AND status = 'posted' \
                    GROUP BY project_id \
                 ) l USING (project_id) \
                 WHERE COALESCE(u.project_id, l.project_id) IS NOT NULL \
                 ORDER BY cost_usd DESC",
            )
        } else {
            sqlx::query(
                "SELECT project_id, \
                        COALESCE(SUM(cost_usd), 0)::numeric AS cost_usd, \
                        COUNT(*)::bigint AS requests \
                 FROM usage_records \
                 WHERE org_id = $1 AND ts >= $2 AND ts < $3 \
                 GROUP BY project_id \
                 ORDER BY cost_usd DESC",
            )
        }
        .bind(*org_id.as_uuid())
        .bind(from_utc)
        .bind(to_utc)
        .fetch_all(&self.pool)
        .await?;

        let breakdown_by_project: Vec<ProjectBillLine> = project_rows
            .iter()
            .map(|r| {
                Ok(ProjectBillLine {
                    project_id: ProjectId::from(r.try_get::<Uuid, _>("project_id")?),
                    cost_usd: r.try_get("cost_usd")?,
                    requests: r.try_get("requests")?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        // 按 model 分组
        let model_rows = if ledger_exists {
            sqlx::query(
                "WITH joined AS ( \
                    SELECT u.model_actual, u.tokens_in, u.tokens_out, \
                           COALESCE(l.cost_micros, ROUND(u.cost_usd * 1000000)::bigint) AS cost_micros \
                    FROM usage_records u \
                    LEFT JOIN ( \
                        SELECT request_id, SUM(amount_micros)::bigint AS cost_micros \
                        FROM billing_ledger_events \
                        WHERE org_id = $1 AND occurred_at >= $2 AND occurred_at < $3 \
                          AND event_type = 'actual_settle' AND status = 'posted' \
                        GROUP BY request_id \
                    ) l USING (request_id) \
                    WHERE u.org_id = $1 AND u.ts >= $2 AND u.ts < $3 \
                 ) \
                 SELECT model_actual, \
                        (COALESCE(SUM(cost_micros), 0)::numeric / 1000000)::numeric AS cost_usd, \
                        COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
                        COALESCE(SUM(tokens_out), 0)::bigint AS tokens_out, \
                        COUNT(*)::bigint AS requests \
                 FROM joined \
                 GROUP BY model_actual \
                 ORDER BY cost_usd DESC",
            )
        } else {
            sqlx::query(
                "SELECT model_actual, \
                        COALESCE(SUM(cost_usd), 0)::numeric AS cost_usd, \
                        COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
                        COALESCE(SUM(tokens_out), 0)::bigint AS tokens_out, \
                        COUNT(*)::bigint AS requests \
                 FROM usage_records \
                 WHERE org_id = $1 AND ts >= $2 AND ts < $3 \
                 GROUP BY model_actual \
                 ORDER BY cost_usd DESC",
            )
        }
        .bind(*org_id.as_uuid())
        .bind(from_utc)
        .bind(to_utc)
        .fetch_all(&self.pool)
        .await?;

        let breakdown_by_model: Vec<ModelBillLine> = model_rows
            .iter()
            .map(|r| {
                Ok(ModelBillLine {
                    model: r.try_get("model_actual")?,
                    cost_usd: r.try_get("cost_usd")?,
                    tokens_in: r.try_get("tokens_in")?,
                    tokens_out: r.try_get("tokens_out")?,
                    requests: r.try_get("requests")?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(MonthlyBill {
            org_id,
            month: month.to_string(),
            total_cost_usd,
            total_tokens_in,
            total_tokens_out,
            total_requests,
            breakdown_by_project,
            breakdown_by_model,
        })
    }

    async fn get_or_create_invoice(&self, org_id: OrgId, month: &str) -> DbResult<BillingInvoice> {
        let (from_utc, to_utc) = parse_month_range(month)?;
        let ledger_exists = table_exists(&self.pool, "billing_ledger_events").await?;
        let total_cost_usd = if ledger_exists {
            ledger_total_usd(&self.pool, *org_id.as_uuid(), from_utc, to_utc).await?
        } else {
            usage_total_usd(&self.pool, *org_id.as_uuid(), from_utc, to_utc).await?
        };
        let total_cost_micros = if ledger_exists {
            ledger_total_micros(&self.pool, *org_id.as_uuid(), from_utc, to_utc).await?
        } else {
            decimal_to_micros(total_cost_usd)
        };

        let row = sqlx::query(
            "INSERT INTO billing_invoices (org_id, month, status, total_cost_usd, total_cost_micros) \
             VALUES ($1, $2, 'draft', $3, $4) \
             ON CONFLICT (org_id, month) DO UPDATE SET \
               total_cost_usd = CASE WHEN billing_invoices.status = 'draft' THEN EXCLUDED.total_cost_usd ELSE billing_invoices.total_cost_usd END, \
               total_cost_micros = CASE WHEN billing_invoices.status = 'draft' THEN EXCLUDED.total_cost_micros ELSE billing_invoices.total_cost_micros END, \
               updated_at = CASE WHEN billing_invoices.status = 'draft' THEN NOW() ELSE billing_invoices.updated_at END \
             RETURNING org_id, month, status, total_cost_usd, total_cost_micros, export_digest, \
                       closed_at, exported_at, paid_at, waived_at, created_at, updated_at",
        )
        .bind(*org_id.as_uuid())
        .bind(month)
        .bind(total_cost_usd)
        .bind(total_cost_micros)
        .fetch_one(&self.pool)
        .await?;

        invoice_from_row(&row)
    }

    async fn transition_invoice(
        &self,
        org_id: OrgId,
        month: &str,
        target_status: InvoiceStatus,
        export_digest: Option<&str>,
    ) -> DbResult<BillingInvoice> {
        if target_status == InvoiceStatus::Draft {
            return Err(DbError::Conflict(
                "billing invoice cannot transition back to draft".into(),
            ));
        }
        if target_status == InvoiceStatus::Exported && export_digest.is_none() {
            return Err(DbError::Constraint(
                "export_digest is required when marking invoice exported".into(),
            ));
        }
        if let Some(digest) = export_digest
            && !is_sha256_digest(digest)
        {
            return Err(DbError::Constraint(
                "export_digest must match sha256:<64 lowercase hex chars>".into(),
            ));
        }

        let current = self.get_or_create_invoice(org_id, month).await?;
        if !current.status.can_transition_to(target_status) {
            return Err(DbError::Conflict(format!(
                "invalid billing invoice transition: {} -> {}",
                current.status.as_str(),
                target_status.as_str()
            )));
        }

        if current.status == target_status {
            return Ok(current);
        }

        let row = sqlx::query(
            "UPDATE billing_invoices SET \
               status = $3, \
               export_digest = COALESCE($4, export_digest), \
               closed_at = CASE WHEN $3 IN ('closed', 'exported', 'paid', 'waived') THEN COALESCE(closed_at, NOW()) ELSE closed_at END, \
               exported_at = CASE WHEN $3 = 'exported' THEN COALESCE(exported_at, NOW()) ELSE exported_at END, \
               paid_at = CASE WHEN $3 = 'paid' THEN COALESCE(paid_at, NOW()) ELSE paid_at END, \
               waived_at = CASE WHEN $3 = 'waived' THEN COALESCE(waived_at, NOW()) ELSE waived_at END, \
               updated_at = NOW() \
             WHERE org_id = $1 AND month = $2 \
             RETURNING org_id, month, status, total_cost_usd, total_cost_micros, export_digest, \
                       closed_at, exported_at, paid_at, waived_at, created_at, updated_at",
        )
        .bind(*org_id.as_uuid())
        .bind(month)
        .bind(target_status.as_str())
        .bind(export_digest)
        .fetch_one(&self.pool)
        .await?;

        invoice_from_row(&row)
    }

    async fn export_usage_csv(
        &self,
        org_id: OrgId,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<Vec<UsageExportRow>> {
        let rows = sqlx::query(
            "SELECT ts, org_id, project_id, api_key_id, channel_id, \
                    model_actual, tokens_in::bigint, tokens_out::bigint, cost_usd::numeric \
             FROM usage_records \
             WHERE org_id = $1 AND ts >= $2 AND ts < $3 \
             ORDER BY ts ASC \
             LIMIT 100000",
        )
        .bind(*org_id.as_uuid())
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .map(|r| {
                Ok(UsageExportRow {
                    ts: r.try_get("ts")?,
                    org_id: r.try_get("org_id")?,
                    project_id: r.try_get("project_id")?,
                    api_key_id: r.try_get("api_key_id")?,
                    channel_id: r.try_get("channel_id")?,
                    model: r.try_get("model_actual")?,
                    tokens_in: r.try_get("tokens_in")?,
                    tokens_out: r.try_get("tokens_out")?,
                    cost_usd: r.try_get("cost_usd")?,
                })
            })
            .collect()
    }
}

// ============================================================================
// InMemoryBillingRepo（测试 / dev）
// ============================================================================

/// 模拟 usage_records 的一行（与 UsageSeed 类似但带 project_id/api_key_id）。
#[derive(Debug, Clone)]
pub struct BillingSeed {
    pub ts: DateTime<Utc>,
    pub org_id: OrgId,
    pub project_id: ProjectId,
    pub api_key_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub model: String,
    pub cost_usd: Decimal,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

#[derive(Default)]
pub struct InMemoryBillingRepo {
    inner: RwLock<Vec<BillingSeed>>,
    invoices: RwLock<HashMap<(Uuid, String), BillingInvoice>>,
}

impl InMemoryBillingRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, rec: BillingSeed) {
        self.inner.write().push(rec);
    }
}

#[async_trait]
impl BillingRepo for InMemoryBillingRepo {
    async fn monthly_bill(&self, org_id: OrgId, month: &str) -> DbResult<MonthlyBill> {
        let (from_utc, to_utc) = parse_month_range(month)?;

        let inner = self.inner.read();
        let filtered: Vec<&BillingSeed> = inner
            .iter()
            .filter(|r| r.org_id == org_id && r.ts >= from_utc && r.ts < to_utc)
            .collect();

        let total_cost_usd: Decimal = filtered.iter().map(|r| r.cost_usd).sum();
        let total_tokens_in: i64 = filtered.iter().map(|r| r.tokens_in).sum();
        let total_tokens_out: i64 = filtered.iter().map(|r| r.tokens_out).sum();
        let total_requests = filtered.len() as i64;

        // By project — use HashMap keyed on Uuid (ProjectId has no Ord)
        let mut proj_map: HashMap<Uuid, (ProjectId, Decimal, i64)> = HashMap::new();
        for r in &filtered {
            let e =
                proj_map
                    .entry(*r.project_id.as_uuid())
                    .or_insert((r.project_id, Decimal::ZERO, 0));
            e.1 += r.cost_usd;
            e.2 += 1;
        }
        let mut breakdown_by_project: Vec<ProjectBillLine> = proj_map
            .into_values()
            .map(|(project_id, cost_usd, requests)| ProjectBillLine {
                project_id,
                cost_usd,
                requests,
            })
            .collect();
        breakdown_by_project.sort_by_key(|b| std::cmp::Reverse(b.cost_usd));

        // By model
        let mut model_map: HashMap<String, (Decimal, i64, i64, i64)> = HashMap::new();
        for r in &filtered {
            let e = model_map
                .entry(r.model.clone())
                .or_insert((Decimal::ZERO, 0, 0, 0));
            e.0 += r.cost_usd;
            e.1 += r.tokens_in;
            e.2 += r.tokens_out;
            e.3 += 1;
        }
        let mut breakdown_by_model: Vec<ModelBillLine> = model_map
            .into_iter()
            .map(
                |(model, (cost_usd, tokens_in, tokens_out, requests))| ModelBillLine {
                    model,
                    cost_usd,
                    tokens_in,
                    tokens_out,
                    requests,
                },
            )
            .collect();
        breakdown_by_model.sort_by_key(|b| std::cmp::Reverse(b.cost_usd));

        Ok(MonthlyBill {
            org_id,
            month: month.to_string(),
            total_cost_usd,
            total_tokens_in,
            total_tokens_out,
            total_requests,
            breakdown_by_project,
            breakdown_by_model,
        })
    }

    async fn get_or_create_invoice(&self, org_id: OrgId, month: &str) -> DbResult<BillingInvoice> {
        parse_month_range(month)?;
        let key = (*org_id.as_uuid(), month.to_string());
        if let Some(invoice) = self.invoices.read().get(&key).cloned() {
            return Ok(invoice);
        }

        let bill = self.monthly_bill(org_id, month).await?;
        let now = Utc::now();
        let invoice = BillingInvoice {
            org_id,
            month: month.to_string(),
            status: InvoiceStatus::Draft,
            total_cost_usd: bill.total_cost_usd,
            total_cost_micros: decimal_to_micros(bill.total_cost_usd),
            export_digest: None,
            closed_at: None,
            exported_at: None,
            paid_at: None,
            waived_at: None,
            created_at: now,
            updated_at: now,
        };
        self.invoices.write().insert(key, invoice.clone());
        Ok(invoice)
    }

    async fn transition_invoice(
        &self,
        org_id: OrgId,
        month: &str,
        target_status: InvoiceStatus,
        export_digest: Option<&str>,
    ) -> DbResult<BillingInvoice> {
        if target_status == InvoiceStatus::Draft {
            return Err(DbError::Conflict(
                "billing invoice cannot transition back to draft".into(),
            ));
        }
        if target_status == InvoiceStatus::Exported && export_digest.is_none() {
            return Err(DbError::Constraint(
                "export_digest is required when marking invoice exported".into(),
            ));
        }
        if let Some(digest) = export_digest
            && !is_sha256_digest(digest)
        {
            return Err(DbError::Constraint(
                "export_digest must match sha256:<64 lowercase hex chars>".into(),
            ));
        }

        let current = self.get_or_create_invoice(org_id, month).await?;
        if !current.status.can_transition_to(target_status) {
            return Err(DbError::Conflict(format!(
                "invalid billing invoice transition: {} -> {}",
                current.status.as_str(),
                target_status.as_str()
            )));
        }
        if current.status == target_status {
            return Ok(current);
        }

        let mut invoice = current;
        let now = Utc::now();
        invoice.status = target_status;
        if let Some(digest) = export_digest {
            invoice.export_digest = Some(digest.to_string());
        }
        match target_status {
            InvoiceStatus::Draft => {}
            InvoiceStatus::Closed => invoice.closed_at = Some(invoice.closed_at.unwrap_or(now)),
            InvoiceStatus::Exported => {
                invoice.closed_at = Some(invoice.closed_at.unwrap_or(now));
                invoice.exported_at = Some(invoice.exported_at.unwrap_or(now));
            }
            InvoiceStatus::Paid => {
                invoice.closed_at = Some(invoice.closed_at.unwrap_or(now));
                invoice.paid_at = Some(invoice.paid_at.unwrap_or(now));
            }
            InvoiceStatus::Waived => {
                invoice.closed_at = Some(invoice.closed_at.unwrap_or(now));
                invoice.waived_at = Some(invoice.waived_at.unwrap_or(now));
            }
        }
        invoice.updated_at = now;
        self.invoices
            .write()
            .insert((*org_id.as_uuid(), month.to_string()), invoice.clone());
        Ok(invoice)
    }

    async fn export_usage_csv(
        &self,
        org_id: OrgId,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<Vec<UsageExportRow>> {
        let inner = self.inner.read();
        let mut rows: Vec<UsageExportRow> = inner
            .iter()
            .filter(|r| r.org_id == org_id && r.ts >= from && r.ts < to)
            .map(|r| UsageExportRow {
                ts: r.ts,
                org_id: *r.org_id.as_uuid(),
                project_id: *r.project_id.as_uuid(),
                api_key_id: r.api_key_id,
                channel_id: r.channel_id,
                model: r.model.clone(),
                tokens_in: r.tokens_in,
                tokens_out: r.tokens_out,
                cost_usd: r.cost_usd,
            })
            .collect();
        rows.sort_by_key(|r| r.ts);
        Ok(rows)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn inmemory_monthly_bill_aggregation() {
        let repo = InMemoryBillingRepo::new();
        let org = OrgId::new();
        let proj_a = ProjectId::new();
        let proj_b = ProjectId::new();
        let key_id = Uuid::now_v7();

        repo.seed(BillingSeed {
            ts: dt(2026, 5, 3),
            org_id: org,
            project_id: proj_a,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o".into(),
            cost_usd: Decimal::new(50, 2), // 0.50
            tokens_in: 100,
            tokens_out: 50,
        });
        repo.seed(BillingSeed {
            ts: dt(2026, 5, 10),
            org_id: org,
            project_id: proj_b,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o-mini".into(),
            cost_usd: Decimal::new(20, 2), // 0.20
            tokens_in: 200,
            tokens_out: 100,
        });
        repo.seed(BillingSeed {
            ts: dt(2026, 5, 15),
            org_id: org,
            project_id: proj_a,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o".into(),
            cost_usd: Decimal::new(30, 2), // 0.30
            tokens_in: 150,
            tokens_out: 75,
        });
        // Different month — should not appear
        repo.seed(BillingSeed {
            ts: dt(2026, 6, 1),
            org_id: org,
            project_id: proj_a,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o".into(),
            cost_usd: Decimal::new(999, 2), // 9.99
            tokens_in: 999,
            tokens_out: 999,
        });

        let bill = repo.monthly_bill(org, "2026-05").await.unwrap();
        assert_eq!(bill.month, "2026-05");
        assert_eq!(bill.total_cost_usd, Decimal::new(100, 2)); // 1.00
        assert_eq!(bill.total_tokens_in, 450);
        assert_eq!(bill.total_tokens_out, 225);
        assert_eq!(bill.total_requests, 3);

        // Project breakdown
        assert_eq!(bill.breakdown_by_project.len(), 2);
        assert_eq!(bill.breakdown_by_project[0].project_id, proj_a);
        assert_eq!(bill.breakdown_by_project[0].cost_usd, Decimal::new(80, 2)); // 0.80
        assert_eq!(bill.breakdown_by_project[0].requests, 2);

        // Model breakdown
        assert_eq!(bill.breakdown_by_model.len(), 2);
        assert_eq!(bill.breakdown_by_model[0].model, "gpt-4o");
        assert_eq!(bill.breakdown_by_model[0].cost_usd, Decimal::new(80, 2)); // 0.80
        assert_eq!(bill.breakdown_by_model[0].requests, 2);
    }

    #[tokio::test]
    async fn inmemory_invoice_state_machine_enforces_forward_only_transitions() {
        let repo = InMemoryBillingRepo::new();
        let org = OrgId::new();
        let proj = ProjectId::new();
        let key_id = Uuid::now_v7();

        repo.seed(BillingSeed {
            ts: dt(2026, 5, 3),
            org_id: org,
            project_id: proj,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o-mini".into(),
            cost_usd: Decimal::new(25, 2),
            tokens_in: 100,
            tokens_out: 50,
        });

        let draft = repo.get_or_create_invoice(org, "2026-05").await.unwrap();
        assert_eq!(draft.status, InvoiceStatus::Draft);
        assert_eq!(draft.total_cost_micros, 250_000);

        let closed = repo
            .transition_invoice(org, "2026-05", InvoiceStatus::Closed, None)
            .await
            .unwrap();
        assert_eq!(closed.status, InvoiceStatus::Closed);
        assert!(closed.closed_at.is_some());

        let back_to_draft = repo
            .transition_invoice(org, "2026-05", InvoiceStatus::Draft, None)
            .await;
        assert!(back_to_draft.is_err());

        let exported = repo
            .transition_invoice(
                org,
                "2026-05",
                InvoiceStatus::Exported,
                Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            )
            .await
            .unwrap();
        assert_eq!(exported.status, InvoiceStatus::Exported);
        assert_eq!(
            exported.export_digest.as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        let paid = repo
            .transition_invoice(org, "2026-05", InvoiceStatus::Paid, None)
            .await
            .unwrap();
        assert_eq!(paid.status, InvoiceStatus::Paid);

        let waived_after_paid = repo
            .transition_invoice(org, "2026-05", InvoiceStatus::Waived, None)
            .await;
        assert!(waived_after_paid.is_err());
    }

    #[tokio::test]
    async fn inmemory_export_csv() {
        let repo = InMemoryBillingRepo::new();
        let org = OrgId::new();
        let proj = ProjectId::new();
        let key_id = Uuid::now_v7();

        repo.seed(BillingSeed {
            ts: dt(2026, 5, 3),
            org_id: org,
            project_id: proj,
            api_key_id: key_id,
            channel_id: None,
            model: "gpt-4o".into(),
            cost_usd: Decimal::new(10, 2), // 0.10
            tokens_in: 10,
            tokens_out: 5,
        });
        repo.seed(BillingSeed {
            ts: dt(2026, 5, 5),
            org_id: org,
            project_id: proj,
            api_key_id: key_id,
            channel_id: Some(Uuid::now_v7()),
            model: "gpt-4o-mini".into(),
            cost_usd: Decimal::new(5, 2), // 0.05
            tokens_in: 20,
            tokens_out: 10,
        });

        let rows = repo
            .export_usage_csv(org, dt(2026, 5, 1), dt(2026, 6, 1))
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by ts ascending
        assert!(rows[0].ts < rows[1].ts);
        assert_eq!(rows[0].model, "gpt-4o");
        assert_eq!(rows[1].model, "gpt-4o-mini");
    }
}
