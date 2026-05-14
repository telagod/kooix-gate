//! BillingRepo — 月度账单聚合 + 用量 CSV 导出。
//!
//! 所有聚合直接查 `usage_records`，无物化视图，够用即可。
//! 月度范围按 UTC 自然月 [YYYY-MM-01 00:00:00, next-month-01 00:00:00) 划分。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use gate_core::id::{OrgId, ProjectId};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use parking_lot::RwLock;
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

// ============================================================================
// Trait
// ============================================================================

#[async_trait]
pub trait BillingRepo: Send + Sync + 'static {
    /// 按月聚合 Org 级别账单。`month` 格式 "YYYY-MM"。
    async fn monthly_bill(&self, org_id: OrgId, month: &str) -> DbResult<MonthlyBill>;

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

        // 总计
        let totals_row = sqlx::query(
            "SELECT COALESCE(SUM(cost_usd), 0)::numeric AS cost_usd, \
                    COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
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

        let total_cost_usd: Decimal = totals_row.try_get("cost_usd")?;
        let total_tokens_in: i64 = totals_row.try_get("tokens_in")?;
        let total_tokens_out: i64 = totals_row.try_get("tokens_out")?;
        let total_requests: i64 = totals_row.try_get("requests")?;

        // 按 project 分组
        let project_rows = sqlx::query(
            "SELECT project_id, \
                    COALESCE(SUM(cost_usd), 0)::numeric AS cost_usd, \
                    COUNT(*)::bigint AS requests \
             FROM usage_records \
             WHERE org_id = $1 AND ts >= $2 AND ts < $3 \
             GROUP BY project_id \
             ORDER BY cost_usd DESC",
        )
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
        let model_rows = sqlx::query(
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
            let e = proj_map
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
