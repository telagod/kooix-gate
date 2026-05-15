//! QuotaRepo — 多维度配额持久层。
//!
//! quotas 表挂载点是 (scope_kind, scope_id) 复合键，scope_kind ∈
//! `platform | org | project | user | membership | api_key`。
//!
//! 关键不变式：
//! - `enabled=TRUE` 才会被 middleware 加载执行；列表接口看 admin 是否要看全量。
//! - 同一 (scope_kind, scope_id, dimension, model_filter) 唯一，UPSERT 走它。
//! - 删除走硬删（quotas 没有 deleted_at 列），调用方自行决定要不要先 disable 再删。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// quota 行的存储快照。
///
/// `scope_kind` / `dimension` 用 String 而非 enum 是为了兼容 schema 的 CHECK 约束变化；
/// 调用方自行用 [`QuotaDimension`](gate_core::quota::QuotaDimension) 解析。
#[derive(Debug, Clone)]
pub struct QuotaRecord {
    pub id: Uuid,
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub limit_value: Decimal,
    pub window_seconds: Option<i32>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 创建/更新 quota 的入参。
///
/// 没有 `enabled` 字段：UPSERT 时强制 `enabled=TRUE`，要禁用走单独的 update 路径
/// （未来再加，本版只暴露 upsert+delete）。
#[derive(Debug, Clone)]
pub struct QuotaUpsert {
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub limit_value: Decimal,
    pub window_seconds: Option<i32>,
}

#[async_trait]
pub trait QuotaRepo: Send + Sync + 'static {
    /// 加载主体 (scope_kind, scope_id) 上所有 enabled 的 quota 行。
    /// middleware 热路径用——要快、要无副作用。
    async fn find_active_for(&self, scope_kind: &str, scope_id: Uuid)
    -> DbResult<Vec<QuotaRecord>>;

    /// UPSERT：(scope_kind, scope_id, dimension, model_filter) 命中则改 limit/window。
    async fn upsert(&self, q: QuotaUpsert) -> DbResult<QuotaRecord>;

    /// 硬删一行。
    async fn delete(&self, id: Uuid) -> DbResult<()>;

    /// 列举主体上所有 quota（含 disabled），用于 admin 查看。
    async fn list_by_scope(&self, scope_kind: &str, scope_id: Uuid) -> DbResult<Vec<QuotaRecord>>;
}

// ============================================================================
// PgQuotaRepo
// ============================================================================

pub struct PgQuotaRepo {
    pool: PgPool,
}

impl PgQuotaRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const QUOTA_COLUMNS: &str = "id, scope_kind, scope_id, dimension, model_filter, \
    limit_value, window_seconds, enabled, created_at, updated_at";

fn row_to_record(row: &sqlx::postgres::PgRow) -> DbResult<QuotaRecord> {
    Ok(QuotaRecord {
        id: row.try_get("id")?,
        scope_kind: row.try_get("scope_kind")?,
        scope_id: row.try_get("scope_id")?,
        dimension: row.try_get("dimension")?,
        model_filter: row.try_get("model_filter")?,
        limit_value: row.try_get("limit_value")?,
        window_seconds: row.try_get("window_seconds")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl QuotaRepo for PgQuotaRepo {
    async fn find_active_for(
        &self,
        scope_kind: &str,
        scope_id: Uuid,
    ) -> DbResult<Vec<QuotaRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {QUOTA_COLUMNS} FROM quotas \
             WHERE scope_kind = $1 AND scope_id = $2 AND enabled = TRUE \
             ORDER BY dimension, model_filter NULLS FIRST"
        ))
        .bind(scope_kind)
        .bind(scope_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }

    async fn upsert(&self, q: QuotaUpsert) -> DbResult<QuotaRecord> {
        // model_filter NULL 参与唯一约束需要用 IS NOT DISTINCT FROM 风格的 ON CONFLICT。
        // PostgreSQL UNIQUE (a, b, c, d) 把 NULL 视为不同 → 简化处理：分两条路径。
        // 不过我们的 schema UNIQUE (scope_kind, scope_id, dimension, model_filter) 已生效，
        // 这里直接用 ON CONFLICT 指定列，依赖 PG 17 的 NULLS NOT DISTINCT 行为。
        // 兼容老版 PG：用 INSERT ... WHERE NOT EXISTS / SELECT 先查再决定。
        // 简化：先 SELECT 查存在性，再 UPDATE 或 INSERT。
        // 这避免了对 ON CONFLICT NULLS 行为的依赖。
        let existing = sqlx::query(&format!(
            "SELECT {QUOTA_COLUMNS} FROM quotas \
             WHERE scope_kind = $1 AND scope_id = $2 AND dimension = $3 \
               AND model_filter IS NOT DISTINCT FROM $4"
        ))
        .bind(&q.scope_kind)
        .bind(q.scope_id)
        .bind(&q.dimension)
        .bind(q.model_filter.as_deref())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = existing {
            let id: Uuid = row.try_get("id")?;
            let updated = sqlx::query(&format!(
                "UPDATE quotas SET limit_value = $1, window_seconds = $2, enabled = TRUE \
                 WHERE id = $3 RETURNING {QUOTA_COLUMNS}"
            ))
            .bind(q.limit_value)
            .bind(q.window_seconds)
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
            row_to_record(&updated)
        } else {
            let inserted = sqlx::query(&format!(
                "INSERT INTO quotas (scope_kind, scope_id, dimension, model_filter, \
                                     limit_value, window_seconds, enabled) \
                 VALUES ($1, $2, $3, $4, $5, $6, TRUE) \
                 RETURNING {QUOTA_COLUMNS}"
            ))
            .bind(&q.scope_kind)
            .bind(q.scope_id)
            .bind(&q.dimension)
            .bind(q.model_filter.as_deref())
            .bind(q.limit_value)
            .bind(q.window_seconds)
            .fetch_one(&self.pool)
            .await?;
            row_to_record(&inserted)
        }
    }

    async fn delete(&self, id: Uuid) -> DbResult<()> {
        let res = sqlx::query("DELETE FROM quotas WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn list_by_scope(&self, scope_kind: &str, scope_id: Uuid) -> DbResult<Vec<QuotaRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {QUOTA_COLUMNS} FROM quotas \
             WHERE scope_kind = $1 AND scope_id = $2 \
             ORDER BY dimension, model_filter NULLS FIRST"
        ))
        .bind(scope_kind)
        .bind(scope_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_record).collect()
    }
}

// ============================================================================
// InMemoryQuotaRepo（dev 模式 / 测试用）
// ============================================================================

#[derive(Default)]
pub struct InMemoryQuotaRepo {
    inner: RwLock<HashMap<Uuid, QuotaRecord>>,
}

impl InMemoryQuotaRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试快捷：直接 seed 一行（绕开 upsert 的唯一性检查）。
    pub fn seed(&self, record: QuotaRecord) {
        self.inner.write().insert(record.id, record);
    }
}

#[async_trait]
impl QuotaRepo for InMemoryQuotaRepo {
    async fn find_active_for(
        &self,
        scope_kind: &str,
        scope_id: Uuid,
    ) -> DbResult<Vec<QuotaRecord>> {
        Ok(self
            .inner
            .read()
            .values()
            .filter(|r| r.scope_kind == scope_kind && r.scope_id == scope_id && r.enabled)
            .cloned()
            .collect())
    }

    async fn upsert(&self, q: QuotaUpsert) -> DbResult<QuotaRecord> {
        let mut g = self.inner.write();
        // 先找现有
        let existing_id = g
            .values()
            .find(|r| {
                r.scope_kind == q.scope_kind
                    && r.scope_id == q.scope_id
                    && r.dimension == q.dimension
                    && r.model_filter == q.model_filter
            })
            .map(|r| r.id);

        let now = Utc::now();
        let record = if let Some(id) = existing_id {
            let mut rec = g.get(&id).unwrap().clone();
            rec.limit_value = q.limit_value;
            rec.window_seconds = q.window_seconds;
            rec.enabled = true;
            rec.updated_at = now;
            g.insert(id, rec.clone());
            rec
        } else {
            let id = Uuid::now_v7();
            let rec = QuotaRecord {
                id,
                scope_kind: q.scope_kind,
                scope_id: q.scope_id,
                dimension: q.dimension,
                model_filter: q.model_filter,
                limit_value: q.limit_value,
                window_seconds: q.window_seconds,
                enabled: true,
                created_at: now,
                updated_at: now,
            };
            g.insert(id, rec.clone());
            rec
        };
        Ok(record)
    }

    async fn delete(&self, id: Uuid) -> DbResult<()> {
        if self.inner.write().remove(&id).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    async fn list_by_scope(&self, scope_kind: &str, scope_id: Uuid) -> DbResult<Vec<QuotaRecord>> {
        Ok(self
            .inner
            .read()
            .values()
            .filter(|r| r.scope_kind == scope_kind && r.scope_id == scope_id)
            .cloned()
            .collect())
    }
}
