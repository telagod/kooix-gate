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

// ============================================================================
// InMemoryOutboxRepo — 测试 / dev 用
// ============================================================================

use parking_lot::Mutex;

#[derive(Debug, Clone)]
struct InMemoryRow {
    id: OutboxId,
    event: UsageEvent,
    processed: bool,
    retry_count: i32,
    last_error: Option<String>,
}

#[derive(Default)]
pub struct InMemoryOutboxRepo {
    inner: Mutex<InMemoryInner>,
}

#[derive(Default)]
struct InMemoryInner {
    rows: Vec<InMemoryRow>,
    next_id: OutboxId,
}

impl InMemoryOutboxRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试辅助：拿到全部 enqueue 过的事件（不分 processed / pending）。
    pub fn snapshot(&self) -> Vec<UsageEvent> {
        self.inner
            .lock()
            .rows
            .iter()
            .map(|r| r.event.clone())
            .collect()
    }

    /// 测试辅助：未处理事件数。
    pub fn pending_count(&self) -> usize {
        self.inner
            .lock()
            .rows
            .iter()
            .filter(|r| !r.processed && r.retry_count < 3)
            .count()
    }
}

#[async_trait]
impl OutboxRepo for InMemoryOutboxRepo {
    async fn enqueue(&self, event: &UsageEvent) -> BillingResult<OutboxId> {
        let mut g = self.inner.lock();
        g.next_id += 1;
        let id = g.next_id;
        g.rows.push(InMemoryRow {
            id,
            event: event.clone(),
            processed: false,
            retry_count: 0,
            last_error: None,
        });
        Ok(id)
    }

    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
        let g = self.inner.lock();
        let out: Vec<_> = g
            .rows
            .iter()
            .filter(|r| !r.processed && r.retry_count < 3)
            .take(limit.max(0) as usize)
            .map(|r| (r.id, r.event.clone()))
            .collect();
        Ok(out)
    }

    async fn mark_done(&self, id: OutboxId) -> BillingResult<()> {
        let mut g = self.inner.lock();
        if let Some(r) = g.rows.iter_mut().find(|r| r.id == id) {
            r.processed = true;
        }
        Ok(())
    }

    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()> {
        let mut g = self.inner.lock();
        if let Some(r) = g.rows.iter_mut().find(|r| r.id == id) {
            r.retry_count += 1;
            r.last_error = Some(error.to_string());
        }
        Ok(())
    }
}
