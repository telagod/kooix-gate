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
use chrono::Utc;
use sqlx::{PgPool, Row};
use tracing::Instrument;
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
        let span = outbox_enqueue_span(event);
        let span_for_inner = span.clone();
        async move { self.enqueue_inner(event, &span_for_inner).await }
            .instrument(span)
            .await
    }

    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
        let span = tracing::info_span!(
            "billing.outbox.fetch_batch",
            limit = limit,
            batch_size = tracing::field::Empty,
            worker_id = %self.worker_id,
        );
        let span_for_inner = span.clone();
        async move { self.fetch_batch_inner(limit, &span_for_inner).await }
            .instrument(span)
            .await
    }

    async fn mark_done(&self, id: OutboxId) -> BillingResult<()> {
        let span = tracing::info_span!("billing.outbox.mark_done", outbox_id = id);
        sqlx::query(
            "UPDATE outbox_events \
             SET processed_at = NOW(), locked_until = NULL, locked_by = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .instrument(span)
        .await?;
        Ok(())
    }

    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()> {
        let span =
            tracing::info_span!("billing.outbox.mark_failed", outbox_id = id, error = %error);
        sqlx::query(
            "UPDATE outbox_events \
             SET retry_count = retry_count + 1, last_error = $2, locked_until = NULL, locked_by = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .instrument(span)
        .await?;
        Ok(())
    }
}

impl PgOutboxRepo {
    async fn enqueue_inner(
        &self,
        event: &UsageEvent,
        span: &tracing::Span,
    ) -> BillingResult<OutboxId> {
        let payload = serde_json::to_value(event)?;
        let row = sqlx::query(
            "INSERT INTO outbox_events (topic, payload) VALUES ('usage', $1) RETURNING id",
        )
        .bind(payload)
        .fetch_one(&self.pool)
        .await?;
        let id: OutboxId = row.try_get("id")?;
        span.record("outbox_id", id);
        record_outbox_enqueued(event);
        Ok(id)
    }

    async fn fetch_batch_inner(
        &self,
        limit: i64,
        span: &tracing::Span,
    ) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
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
        } else if ids.is_empty() {
            metrics::gauge!("billing_outbox_lag_seconds").set(0.0);
        }
        tx.commit().await?;
        span.record("batch_size", result.len());
        Ok(result)
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
        let span = outbox_enqueue_span(event);
        let span_for_inner = span.clone();
        async move { self.enqueue_inner(event, &span_for_inner) }
            .instrument(span)
            .await
    }

    async fn fetch_batch(&self, limit: i64) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
        let span = tracing::info_span!(
            "billing.outbox.fetch_batch",
            limit = limit,
            batch_size = tracing::field::Empty,
            worker_id = "in_memory",
        );
        let span_for_inner = span.clone();
        async move { self.fetch_batch_inner(limit, &span_for_inner) }
            .instrument(span)
            .await
    }

    async fn mark_done(&self, id: OutboxId) -> BillingResult<()> {
        let _guard = tracing::info_span!("billing.outbox.mark_done", outbox_id = id).entered();
        let mut g = self.inner.lock();
        if let Some(r) = g.rows.iter_mut().find(|r| r.id == id) {
            r.processed = true;
        }
        Ok(())
    }

    async fn mark_failed(&self, id: OutboxId, error: &str) -> BillingResult<()> {
        let _guard =
            tracing::info_span!("billing.outbox.mark_failed", outbox_id = id, error = %error)
                .entered();
        let mut g = self.inner.lock();
        if let Some(r) = g.rows.iter_mut().find(|r| r.id == id) {
            r.retry_count += 1;
            r.last_error = Some(error.to_string());
        }
        Ok(())
    }
}

impl InMemoryOutboxRepo {
    fn enqueue_inner(&self, event: &UsageEvent, span: &tracing::Span) -> BillingResult<OutboxId> {
        let mut g = self.inner.lock();
        g.next_id += 1;
        let id = g.next_id;
        span.record("outbox_id", id);
        g.rows.push(InMemoryRow {
            id,
            event: event.clone(),
            processed: false,
            retry_count: 0,
            last_error: None,
        });
        record_outbox_enqueued(event);
        Ok(id)
    }

    fn fetch_batch_inner(
        &self,
        limit: i64,
        span: &tracing::Span,
    ) -> BillingResult<Vec<(OutboxId, UsageEvent)>> {
        let g = self.inner.lock();
        let out: Vec<_> = g
            .rows
            .iter()
            .filter(|r| !r.processed && r.retry_count < 3)
            .take(limit.max(0) as usize)
            .map(|r| (r.id, r.event.clone()))
            .collect();
        let lag_seconds = g
            .rows
            .iter()
            .filter(|r| !r.processed && r.retry_count < 3)
            .map(|r| (Utc::now() - r.event.occurred_at).num_milliseconds().max(0) as f64 / 1000.0)
            .fold(0.0, f64::max);
        metrics::gauge!("billing_outbox_lag_seconds").set(lag_seconds);
        span.record("batch_size", out.len());
        Ok(out)
    }
}

fn outbox_enqueue_span(event: &UsageEvent) -> tracing::Span {
    tracing::info_span!(
        "billing.outbox.enqueue",
        outbox_id = tracing::field::Empty,
        kooix.request_id = %event.request_id,
        kooix.org_id = %event.org_id,
        kooix.project_id = %event.project_id,
        kooix.api_key_id = %event.api_key_id,
        kooix.channel_id = display_opt_uuid(event.channel_id),
        kooix.group_id = display_opt_uuid(event.group_id),
        kooix.model = %event.model,
        request_id = %event.request_id,
        org_id = %event.org_id,
        project_id = %event.project_id,
        api_key_id = %event.api_key_id,
        channel_id = display_opt_uuid(event.channel_id),
        group_id = display_opt_uuid(event.group_id),
        model = %event.model,
        status = event.status,
    )
}

fn display_opt_uuid(value: Option<Uuid>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn record_outbox_enqueued(event: &UsageEvent) {
    metrics::counter!("billing_outbox_enqueued_total").increment(1);
    let lag_seconds = (Utc::now() - event.occurred_at).num_milliseconds().max(0) as f64 / 1000.0;
    metrics::gauge!("billing_outbox_lag_seconds").set(lag_seconds);
}
