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
use uuid::Uuid;

/// outbox 行 ID（BIGSERIAL）
pub type OutboxId = i64;

#[async_trait]
pub trait OutboxRepo: Send + Sync + 'static {
    /// 将 UsageEvent 写入 outbox（topic = 'usage'）。
    async fn enqueue(&self, event: &UsageEvent) -> BillingResult<OutboxId>;

    /// 拉取最多 `limit` 条未处理的 outbox 行（processed_at IS NULL，按 id ASC）。
    ///
    /// Pg 实现会在事务中用 `FOR UPDATE SKIP LOCKED` 锁住本批行，并把
    /// `last_error` 标为 `processing:<id-list>`，避免多 worker 重复取同一批。
    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>>;

    /// 标记成功处理。
    async fn mark_done(&self, id: OutboxId) -> BillingResult<()>;

    /// 标记失败（递增 retry_count，记录 last_error）。
    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()>;
}

pub struct PgOutboxRepo {
    pool: PgPool,
    worker_id: String,
}

impl PgOutboxRepo {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            worker_id: format!("worker-{}", Uuid::now_v7()),
        }
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
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            "SELECT id, payload FROM outbox_events \
             WHERE topic = 'usage' \
               AND processed_at IS NULL \
               AND retry_count < 3 \
               AND (locked_until IS NULL OR locked_until < NOW()) \
             ORDER BY id ASC \
             LIMIT $1 \
             FOR UPDATE SKIP LOCKED",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;

        let mut result = Vec::with_capacity(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: OutboxId = row.try_get("id")?;
            let payload: serde_json::Value = row.try_get("payload")?;
            let event: UsageEvent = serde_json::from_value(payload)?;
            ids.push(id);
            result.push((id, event));
        }
        if !ids.is_empty() {
            let marker = format!(
                "processing:{}",
                ids.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            sqlx::query(
                "UPDATE outbox_events \
                 SET locked_until = NOW() + INTERVAL '5 minutes', locked_by = $2, last_error = $3 \
                 WHERE id = ANY($1)",
            )
            .bind(&ids)
            .bind(&self.worker_id)
            .bind(marker)
            .execute(&mut *tx)
            .await?;
        }
        if !ids.is_empty()
            && let Ok(Some(lag_seconds)) = sqlx::query_scalar::<_, Option<f64>>(
                "SELECT EXTRACT(EPOCH FROM (NOW() - MIN(created_at)))::float8 \
                 FROM outbox_events \
                 WHERE topic = 'usage' AND processed_at IS NULL AND retry_count < 3",
            )
            .fetch_one(&mut *tx)
            .await
        {
            metrics::gauge!("billing_outbox_lag_seconds").set(lag_seconds);
        }
        tx.commit().await?;
        Ok(result)
    }

    async fn mark_done(&self, id: OutboxId) -> BillingResult<()> {
        sqlx::query(
            "UPDATE outbox_events \
             SET processed_at = NOW(), locked_until = NULL, locked_by = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()> {
        sqlx::query(
            "UPDATE outbox_events \
             SET retry_count = retry_count + 1, last_error = $2, locked_until = NULL, locked_by = NULL \
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
