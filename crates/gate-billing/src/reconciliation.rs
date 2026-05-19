//! Usage projection ↔ billing ledger reconciliation.

use crate::BillingResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerReconciliationDiff {
    pub request_id: Uuid,
    pub usage_micros: Option<i64>,
    pub ledger_micros: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerReconciliationReport {
    pub org_id: Option<Uuid>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub usage_count: i64,
    pub ledger_count: i64,
    pub usage_total_micros: i64,
    pub ledger_total_micros: i64,
    pub missing_ledger: Vec<LedgerReconciliationDiff>,
    pub orphan_ledger: Vec<LedgerReconciliationDiff>,
    pub amount_mismatches: Vec<LedgerReconciliationDiff>,
    pub checked_at: DateTime<Utc>,
}

impl LedgerReconciliationReport {
    pub fn is_balanced(&self) -> bool {
        self.usage_count == self.ledger_count
            && self.usage_total_micros == self.ledger_total_micros
            && self.missing_ledger.is_empty()
            && self.orphan_ledger.is_empty()
            && self.amount_mismatches.is_empty()
    }
}

pub async fn reconcile_usage_ledger(
    pool: &PgPool,
    org_id: Option<Uuid>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> BillingResult<LedgerReconciliationReport> {
    let usage_row = sqlx::query(
        "SELECT COUNT(*)::BIGINT AS count, \
                COALESCE(SUM(ROUND(cost_usd * 1000000)::BIGINT), 0)::BIGINT AS total_micros \
         FROM usage_records \
         WHERE ($1::UUID IS NULL OR org_id = $1) \
           AND ts >= $2 AND ts < $3",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    let usage_count: i64 = usage_row.try_get("count")?;
    let usage_total_micros: i64 = usage_row.try_get("total_micros")?;

    let ledger_row = sqlx::query(
        "SELECT COUNT(*)::BIGINT AS count, \
                COALESCE(SUM(amount_micros), 0)::BIGINT AS total_micros \
         FROM billing_ledger_events \
         WHERE ($1::UUID IS NULL OR org_id = $1) \
           AND occurred_at >= $2 AND occurred_at < $3 \
           AND event_type = 'actual_settle' \
           AND status = 'posted'",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    let ledger_count: i64 = ledger_row.try_get("count")?;
    let ledger_total_micros: i64 = ledger_row.try_get("total_micros")?;

    let diff_rows = sqlx::query(
        "WITH usage_rows AS ( \
             SELECT request_id, SUM(ROUND(cost_usd * 1000000)::BIGINT)::BIGINT AS usage_micros \
             FROM usage_records \
             WHERE ($1::UUID IS NULL OR org_id = $1) \
               AND ts >= $2 AND ts < $3 \
             GROUP BY request_id \
         ), ledger_rows AS ( \
             SELECT request_id, SUM(amount_micros)::BIGINT AS ledger_micros \
             FROM billing_ledger_events \
             WHERE ($1::UUID IS NULL OR org_id = $1) \
               AND occurred_at >= $2 AND occurred_at < $3 \
               AND event_type = 'actual_settle' \
               AND status = 'posted' \
               AND request_id IS NOT NULL \
             GROUP BY request_id \
         ) \
         SELECT COALESCE(u.request_id, l.request_id) AS request_id, \
                u.usage_micros, l.ledger_micros \
         FROM usage_rows u \
         FULL OUTER JOIN ledger_rows l USING (request_id) \
         WHERE u.request_id IS NULL \
            OR l.request_id IS NULL \
            OR u.usage_micros <> l.ledger_micros \
         ORDER BY request_id \
         LIMIT 500",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let mut missing_ledger = Vec::new();
    let mut orphan_ledger = Vec::new();
    let mut amount_mismatches = Vec::new();

    for row in diff_rows {
        let diff = LedgerReconciliationDiff {
            request_id: row.try_get("request_id")?,
            usage_micros: row.try_get("usage_micros")?,
            ledger_micros: row.try_get("ledger_micros")?,
        };
        match (diff.usage_micros, diff.ledger_micros) {
            (Some(_), None) => missing_ledger.push(diff),
            (None, Some(_)) => orphan_ledger.push(diff),
            (Some(_), Some(_)) => amount_mismatches.push(diff),
            (None, None) => {}
        }
    }

    Ok(LedgerReconciliationReport {
        org_id,
        from,
        to,
        usage_count,
        ledger_count,
        usage_total_micros,
        ledger_total_micros,
        missing_ledger,
        orphan_ledger,
        amount_mismatches,
        checked_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn report_balance_requires_counts_totals_and_diff_sets() {
        let base = LedgerReconciliationReport {
            org_id: None,
            from: Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            to: Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
            usage_count: 1,
            ledger_count: 1,
            usage_total_micros: 42,
            ledger_total_micros: 42,
            missing_ledger: vec![],
            orphan_ledger: vec![],
            amount_mismatches: vec![],
            checked_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
        };
        assert!(base.is_balanced());

        let mut mismatched = base.clone();
        mismatched.ledger_total_micros = 43;
        assert!(!mismatched.is_balanced());

        let mut missing = base;
        missing.missing_ledger.push(LedgerReconciliationDiff {
            request_id: Uuid::now_v7(),
            usage_micros: Some(42),
            ledger_micros: None,
        });
        assert!(!missing.is_balanced());
    }
}
