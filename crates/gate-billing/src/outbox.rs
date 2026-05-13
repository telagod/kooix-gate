//! OutboxRepo — 复用 outbox_events 表（sessions_outbox migration 已建）。
//!
//! 表结构：
//!   id BIGSERIAL, topic TEXT, payload JSONB,
//!   created_at TIMESTAMPTZ, processed_at TIMESTAMPTZ,
//!   retry_count INT, last_error TEXT
//!
//! topic 固定为 'usage'，payload 为 UsageEvent JSON。

use crate::BillingResult;
use crate::types::UsageEvent;
use async_trait::async_trait;
use sqlx::{PgPool, Row};

/// outbox 行 ID（BIGSERIAL）
pub type OutboxId = i64;

#[async_trait]
pub trait OutboxRepo: Send + Sync + 'static {
    /// 将 UsageEvent 写入 outbox（topic = 'usage'）。
    async fn enqueue(&self, event: &UsageEvent) -> BillingResult<OutboxId>;

    /// 拉取最多 `limit` 条未处理的 outbox 行（processed_at IS NULL，按 id ASC）。
    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>>;

    /// 标记成功处理。
    async fn mark_done(&self, id: OutboxId) -> BillingResult<()>;

    /// 标记失败（递增 retry_count，记录 last_error）。
    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()>;
}

pub struct PgOutboxRepo {
    pool: PgPool,
}

impl PgOutboxRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OutboxRepo for PgOutboxRepo {
    async fn enqueue(&self, event: &UsageEvent) -> BillingResult<OutboxId> {
        let payload = serde_json::to_value(event)?;
        let row = sqlx::query(
            "INSERT INTO outbox_events (topic, payload) VALUES ('usage', $1) RETURNING id",
        )
        .bind(payload)
        .fetch_one(&self.pool)
        .await?;
        let id: OutboxId = row.try_get("id")?;
        Ok(id)
    }

    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
        // C1 阶段：单消费者，不需要 FOR UPDATE SKIP LOCKED
        // 多消费者场景升级时用事务包裹 + SKIP LOCKED
        let rows = sqlx::query(
            "SELECT id, payload FROM outbox_events \
             WHERE topic = 'usage' \
               AND processed_at IS NULL \
               AND retry_count < 3 \
             ORDER BY id ASC \
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: OutboxId = row.try_get("id")?;
            let payload: serde_json::Value = row.try_get("payload")?;
            let event: UsageEvent = serde_json::from_value(payload)?;
            result.push((id, event));
        }
        Ok(result)
    }

    async fn mark_done(&self, id: OutboxId) -> BillingResult<()> {
        sqlx::query("UPDATE outbox_events SET processed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()> {
        sqlx::query(
            "UPDATE outbox_events \
             SET retry_count = retry_count + 1, last_error = $2 \
             WHERE id = $1",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
