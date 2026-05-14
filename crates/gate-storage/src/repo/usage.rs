//! UsageRepo — 只读聚合 usage_records，供控制台仪表盘用。
//!
//! 核心查询：按时间范围 + 维度（day/model/channel）group by，
//! 返回 (key, cost_usd, tokens_in, tokens_out)。返回结构扁平，前端直接画图。
//!
//! 性能取舍：
//! - day 维度走现有 `usage_records_org_ts_idx`（按 org_id + ts DESC），
//!   在 30d 级别可接受线性扫描。
//! - model / channel 维度没有专门的 (org_id, model, ts) 索引，但 org_ts_idx
//!   + filter 也能压榨足够性能。若 org 数据量巨大再考虑冷热分层。
//! - cost_usd 是 `NUMERIC(12, 8)`，SUM 后用 `::float8` 转 f64，前端展示够用。
//! - tokens_in/out 是 INT，SUM 回 BIGINT。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::OrgId;
use sqlx::{PgPool, Row};
use parking_lot::RwLock;
use uuid::Uuid;

// ============================================================================
// DTO
// ============================================================================

/// 聚合维度。`Day` 按 UTC 日分桶；`Model` / `Channel` 按对应字段分桶。
#[derive(Debug, Clone, Copy)]
pub enum GroupBy {
    Day,
    Model,
    Channel,
}

/// 单个 bucket 的聚合结果。
#[derive(Debug, Clone)]
pub struct UsageBucket {
    /// 分桶 key：day → `YYYY-MM-DD`；model → 模型名；channel → `<channel_id>` 或 `unknown`。
    pub key: String,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

/// 汇总（series 之和）。
#[derive(Debug, Clone, Default)]
pub struct UsageTotals {
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

// ============================================================================
// Trait
// ============================================================================

#[async_trait]
pub trait UsageRepo: Send + Sync + 'static {
    /// `org_id = None` 表示跨 Org 聚合（SuperAdmin 才能用）。
    async fn aggregate(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        group_by: GroupBy,
    ) -> DbResult<Vec<UsageBucket>>;

    /// 同一时间窗口的总和（不分桶），一次 DB round-trip。
    async fn totals(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<UsageTotals>;
}

// ============================================================================
// PgUsageRepo
// ============================================================================

pub struct PgUsageRepo {
    pool: PgPool,
}

impl PgUsageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsageRepo for PgUsageRepo {
    async fn aggregate(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        group_by: GroupBy,
    ) -> DbResult<Vec<UsageBucket>> {
        // 分桶表达式 + ORDER BY 表达式
        let (bucket_expr, order_expr) = match group_by {
            GroupBy::Day => (
                "to_char(date_trunc('day', ts), 'YYYY-MM-DD')",
                "to_char(date_trunc('day', ts), 'YYYY-MM-DD') ASC",
            ),
            GroupBy::Model => ("model_actual", "SUM(cost_usd) DESC"),
            GroupBy::Channel => (
                "COALESCE(channel_id::text, 'unknown')",
                "SUM(cost_usd) DESC",
            ),
        };

        let org_filter = if org_id.is_some() {
            "AND org_id = $3"
        } else {
            ""
        };

        let sql = format!(
            "SELECT {bucket_expr} AS bucket, \
                    COALESCE(SUM(cost_usd), 0)::float8 AS cost_usd, \
                    COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
                    COALESCE(SUM(tokens_out), 0)::bigint AS tokens_out \
             FROM usage_records \
             WHERE ts >= $1 AND ts < $2 {org_filter} \
             GROUP BY bucket \
             ORDER BY {order_expr}"
        );

        let mut q = sqlx::query(&sql).bind(from).bind(to);
        if let Some(o) = org_id {
            q = q.bind(*o.as_uuid());
        }
        let rows = q.fetch_all(&self.pool).await?;

        rows.iter()
            .map(|r| {
                Ok(UsageBucket {
                    key: r.try_get("bucket")?,
                    cost_usd: r.try_get("cost_usd")?,
                    tokens_in: r.try_get("tokens_in")?,
                    tokens_out: r.try_get("tokens_out")?,
                })
            })
            .collect()
    }

    async fn totals(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<UsageTotals> {
        let org_filter = if org_id.is_some() {
            "AND org_id = $3"
        } else {
            ""
        };

        let sql = format!(
            "SELECT COALESCE(SUM(cost_usd), 0)::float8 AS cost_usd, \
                    COALESCE(SUM(tokens_in), 0)::bigint AS tokens_in, \
                    COALESCE(SUM(tokens_out), 0)::bigint AS tokens_out \
             FROM usage_records \
             WHERE ts >= $1 AND ts < $2 {org_filter}"
        );

        let mut q = sqlx::query(&sql).bind(from).bind(to);
        if let Some(o) = org_id {
            q = q.bind(*o.as_uuid());
        }
        let row = q.fetch_one(&self.pool).await?;
        Ok(UsageTotals {
            cost_usd: row.try_get("cost_usd")?,
            tokens_in: row.try_get("tokens_in")?,
            tokens_out: row.try_get("tokens_out")?,
        })
    }
}

// ============================================================================
// InMemoryUsageRepo（测试 / dev）
// ============================================================================

/// 模拟 usage_records 的一行，只保留聚合相关字段。
#[derive(Debug, Clone)]
pub struct UsageSeed {
    pub ts: DateTime<Utc>,
    pub org_id: OrgId,
    pub model: String,
    pub channel_id: Option<Uuid>,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

#[derive(Default)]
pub struct InMemoryUsageRepo {
    inner: RwLock<Vec<UsageSeed>>,
}

impl InMemoryUsageRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, rec: UsageSeed) {
        self.inner.write().push(rec);
    }

    /// 便捷 seed：给定 org/ts/model/cost/tokens 快速塞一条。
    pub fn seed_usage(
        &self,
        org_id: OrgId,
        ts: DateTime<Utc>,
        model: impl Into<String>,
        cost_usd: f64,
        tokens_in: i64,
        tokens_out: i64,
    ) {
        self.seed(UsageSeed {
            ts,
            org_id,
            model: model.into(),
            channel_id: None,
            cost_usd,
            tokens_in,
            tokens_out,
        });
    }
}

#[async_trait]
impl UsageRepo for InMemoryUsageRepo {
    async fn aggregate(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        group_by: GroupBy,
    ) -> DbResult<Vec<UsageBucket>> {
        let inner = self.inner.read();
        let filtered = inner
            .iter()
            .filter(|r| r.ts >= from && r.ts < to)
            .filter(|r| org_id.is_none_or(|o| r.org_id == o));

        // 聚合：HashMap<key, (cost, in, out)>
        use std::collections::BTreeMap;
        let mut agg: BTreeMap<String, (f64, i64, i64)> = BTreeMap::new();
        for r in filtered {
            let key = match group_by {
                GroupBy::Day => r.ts.format("%Y-%m-%d").to_string(),
                GroupBy::Model => r.model.clone(),
                GroupBy::Channel => r
                    .channel_id
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into()),
            };
            let e = agg.entry(key).or_insert((0.0, 0, 0));
            e.0 += r.cost_usd;
            e.1 += r.tokens_in;
            e.2 += r.tokens_out;
        }

        let mut buckets: Vec<UsageBucket> = agg
            .into_iter()
            .map(|(key, (cost_usd, tokens_in, tokens_out))| UsageBucket {
                key,
                cost_usd,
                tokens_in,
                tokens_out,
            })
            .collect();

        // Day 已按 key 升序；Model / Channel 按 cost 倒序（对齐 PG 实现）
        match group_by {
            GroupBy::Day => {}
            GroupBy::Model | GroupBy::Channel => {
                buckets.sort_by(|a, b| {
                    b.cost_usd
                        .partial_cmp(&a.cost_usd)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }

        Ok(buckets)
    }

    async fn totals(
        &self,
        org_id: Option<OrgId>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DbResult<UsageTotals> {
        let inner = self.inner.read();
        let mut t = UsageTotals::default();
        for r in inner.iter() {
            if r.ts < from || r.ts >= to {
                continue;
            }
            if let Some(o) = org_id
                && r.org_id != o
            {
                continue;
            }
            t.cost_usd += r.cost_usd;
            t.tokens_in += r.tokens_in;
            t.tokens_out += r.tokens_out;
        }
        // 避免 dead_code 告警：DbError 导入未用的兜底
        let _ = DbError::NotFound;
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn inmemory_aggregate_by_day() {
        let repo = InMemoryUsageRepo::new();
        let org = OrgId::new();
        repo.seed_usage(org, dt(2026, 5, 13), "gpt-4o-mini", 0.10, 100, 50);
        repo.seed_usage(org, dt(2026, 5, 13), "gpt-4o-mini", 0.20, 200, 100);
        repo.seed_usage(org, dt(2026, 5, 14), "gpt-4o-mini", 0.05, 50, 25);

        let buckets = repo
            .aggregate(Some(org), dt(2026, 5, 12), dt(2026, 5, 15), GroupBy::Day)
            .await
            .unwrap();

        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].key, "2026-05-13");
        assert!((buckets[0].cost_usd - 0.30).abs() < 1e-9);
        assert_eq!(buckets[0].tokens_in, 300);
        assert_eq!(buckets[0].tokens_out, 150);
        assert_eq!(buckets[1].key, "2026-05-14");
    }

    #[tokio::test]
    async fn inmemory_totals_respects_org_filter() {
        let repo = InMemoryUsageRepo::new();
        let a = OrgId::new();
        let b = OrgId::new();
        repo.seed_usage(a, dt(2026, 5, 13), "gpt-4o", 1.0, 10, 5);
        repo.seed_usage(b, dt(2026, 5, 13), "gpt-4o", 2.0, 20, 10);

        let only_a = repo
            .totals(Some(a), dt(2026, 5, 12), dt(2026, 5, 14))
            .await
            .unwrap();
        assert!((only_a.cost_usd - 1.0).abs() < 1e-9);

        let all = repo
            .totals(None, dt(2026, 5, 12), dt(2026, 5, 14))
            .await
            .unwrap();
        assert!((all.cost_usd - 3.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn inmemory_aggregate_by_model() {
        let repo = InMemoryUsageRepo::new();
        let org = OrgId::new();
        repo.seed_usage(org, dt(2026, 5, 13), "gpt-4o", 3.0, 100, 50);
        repo.seed_usage(org, dt(2026, 5, 13), "gpt-4o-mini", 1.0, 200, 100);

        let buckets = repo
            .aggregate(Some(org), dt(2026, 5, 12), dt(2026, 5, 14), GroupBy::Model)
            .await
            .unwrap();

        assert_eq!(buckets.len(), 2);
        // 按 cost 倒序
        assert_eq!(buckets[0].key, "gpt-4o");
        assert_eq!(buckets[1].key, "gpt-4o-mini");
    }
}
