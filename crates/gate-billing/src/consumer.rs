//! Consumer loop — 拉取 outbox 批次，写 usage_records，标记完成。
//!
//! 设计要点：
//! - 每条独立处理，单条失败不影响其他
//! - 失败 >= MAX_RETRIES 后 mark_failed（不再重试）
//! - commit_usage 走幂等写（ON CONFLICT DO NOTHING on idempotency_key）

use crate::BillingResult;
use crate::ledger::{BillingLedgerEvent, insert_ledger_event_tx};
use crate::outbox::{OutboxId, OutboxRepo};
use crate::types::UsageEvent;
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

/// 失败次数上限（与 outbox.rs fetch_batch SQL 中的 retry_count < 3 对应）。
#[allow(dead_code)]
const MAX_RETRIES: i32 = 3;

pub struct Consumer {
    outbox: Arc<dyn OutboxRepo>,
    pool: PgPool,
    batch_size: i64,
    interval: Duration,
}

impl Consumer {
    pub fn new(
        outbox: Arc<dyn OutboxRepo>,
        pool: PgPool,
        batch_size: i64,
        interval: Duration,
    ) -> Self {
        Self {
            outbox,
            pool,
            batch_size,
            interval,
        }
    }

    /// 运行消费循环（阻塞，直到 task 被 cancel）。
    pub async fn run(&self) {
        self.run_until(CancellationToken::new()).await;
    }

    /// 运行消费循环，直到 `shutdown` 被触发。
    pub async fn run_until(&self, shutdown: CancellationToken) {
        loop {
            if shutdown.is_cancelled() {
                tracing::info!("billing consumer shutdown requested");
                break;
            }

            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "billing consumer tick error");
                metrics::counter!("billing_outbox_tick_errors_total").increment(1);
            }

            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!("billing consumer stopped");
                    break;
                }
                () = tokio::time::sleep(self.interval) => {}
            }
        }
    }

    /// 单次 tick：拉一批 → 批量 settle → 批量 mark done。
    pub async fn tick(&self) -> BillingResult<()> {
        let span = tracing::info_span!(
            "billing.consumer.tick",
            configured_batch_size = self.batch_size,
            batch_size = tracing::field::Empty,
        );
        let span_for_inner = span.clone();
        async move { self.tick_inner(&span_for_inner).await }
            .instrument(span)
            .await
    }

    async fn tick_inner(&self, span: &tracing::Span) -> BillingResult<()> {
        let batch = self.outbox.fetch_batch(self.batch_size).await?;
        span.record("batch_size", batch.len());
        if batch.is_empty() {
            metrics::gauge!("billing_outbox_batch_size").set(0.0);
            return Ok(());
        }
        metrics::gauge!("billing_outbox_batch_size").set(batch.len() as f64);
        tracing::debug!(count = batch.len(), "billing consumer processing batch");

        let outcomes = self.process_batch(&batch).await;
        let mut successful_ids = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            match outcome {
                ProcessOutcome::Settled { outbox_id } => {
                    successful_ids.push(outbox_id);
                }
                ProcessOutcome::Failed {
                    outbox_id,
                    request_id,
                    error,
                } => {
                    self.handle_process_failure(outbox_id, request_id, &error)
                        .await;
                }
            }
        }

        if !successful_ids.is_empty()
            && let Err(e) = self.outbox.mark_done_batch(&successful_ids).await
        {
            tracing::warn!(
                count = successful_ids.len(),
                error = %e,
                "mark_done_batch failed"
            );
            metrics::counter!("billing_outbox_mark_done_failures_total")
                .increment(successful_ids.len() as u64);
            for outbox_id in successful_ids {
                if let Err(e) = self.outbox.mark_done(outbox_id).await {
                    tracing::warn!(outbox_id, error = %e, "mark_done fallback failed");
                }
            }
        }
        Ok(())
    }

    async fn process_batch(&self, batch: &[(OutboxId, UsageEvent)]) -> Vec<ProcessOutcome> {
        let span = tracing::info_span!("billing.consumer.process_batch", batch_size = batch.len());
        let events = batch.iter().map(|(_, event)| event).collect::<Vec<_>>();
        let settled_ids = async move { commit_usage_batch(&self.pool, &events).await }
            .instrument(span)
            .await;

        match settled_ids {
            Ok(settled_ids) => {
                metrics::counter!("billing_outbox_processed_total").increment(batch.len() as u64);
                tracing::debug!(
                    outbox_rows = batch.len(),
                    inserted_request_events = settled_ids.len(),
                    "billing batch settled"
                );
                batch
                    .iter()
                    .map(|(outbox_id, _)| ProcessOutcome::Settled {
                        outbox_id: *outbox_id,
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!(
                    count = batch.len(),
                    error = %e,
                    "commit_usage_batch failed; falling back to per-event settlement"
                );
                metrics::counter!("billing_settle_failures_total", "reason" => "commit_usage_batch")
                    .increment(1);
                self.process_batch_individually(batch).await
            }
        }
    }

    async fn process_batch_individually(
        &self,
        batch: &[(OutboxId, UsageEvent)],
    ) -> Vec<ProcessOutcome> {
        let mut outcomes = Vec::with_capacity(batch.len());
        for (outbox_id, event) in batch {
            let span = usage_event_span("billing.consumer.process_one", event);
            span.record("outbox_id", *outbox_id);
            let outcome = async move {
                match commit_usage(&self.pool, event).await {
                    Ok(()) => {
                        metrics::counter!("billing_outbox_processed_total").increment(1);
                        ProcessOutcome::Settled {
                            outbox_id: *outbox_id,
                        }
                    }
                    Err(e) => ProcessOutcome::Failed {
                        outbox_id: *outbox_id,
                        request_id: event.request_id,
                        error: e.to_string(),
                    },
                }
            }
            .instrument(span)
            .await;
            outcomes.push(outcome);
        }
        outcomes
    }

    async fn handle_process_failure(
        &self,
        outbox_id: OutboxId,
        request_id: uuid::Uuid,
        error: &str,
    ) {
        metrics::counter!("billing_outbox_failed_total").increment(1);
        metrics::counter!("billing_settle_failures_total", "reason" => "commit_usage").increment(1);
        tracing::warn!(
            outbox_id,
            request_id = %request_id,
            error,
            "commit_usage failed"
        );
        // 检查 retry_count：先 mark_failed，如果超限就留着（已 mark_failed = no
        // processed_at，下次 fetch_batch 还会捞到；业务决定是否要阻断）
        // 实际阻断逻辑：fetch_batch 里的 FOR UPDATE SKIP LOCKED 保证并发安全。
        // 超限判断：需查 retry_count，简化：mark_failed 本身会递增，
        // 消费者在 fetch_batch 里 filter retry_count < MAX_RETRIES。
        if let Err(me) = self.outbox.mark_failed(outbox_id, error).await {
            tracing::warn!(outbox_id, error = %me, "mark_failed itself failed");
        }
    }
}

enum ProcessOutcome {
    Settled {
        outbox_id: OutboxId,
    },
    Failed {
        outbox_id: OutboxId,
        request_id: uuid::Uuid,
        error: String,
    },
}

/// 把 UsageEvent 写到 usage_records + read model + ledger。
///
/// channel_id 为 None 时直接写 NULL（fallback provider 路径无 channel 归属）。
pub async fn commit_usage(pool: &PgPool, event: &UsageEvent) -> BillingResult<()> {
    let span = usage_event_span("billing.commit_usage", event);
    async move { commit_usage_inner(pool, event).await }
        .instrument(span)
        .await
}

/// 批量把 UsageEvent 写入 request_events / usage_records / rollups / ledger。
///
/// 返回已落库的新 request_id；duplicate idempotency key 会被跳过并计数，但 consumer
/// 仍可 mark done，因为幂等重复事件已经完成语义上的 settlement。
pub async fn commit_usage_batch(
    pool: &PgPool,
    events: &[&UsageEvent],
) -> BillingResult<Vec<uuid::Uuid>> {
    let span = tracing::info_span!("billing.commit_usage_batch", batch_size = events.len());
    async move { commit_usage_batch_inner(pool, events).await }
        .instrument(span)
        .await
}

async fn commit_usage_inner(pool: &PgPool, event: &UsageEvent) -> BillingResult<()> {
    let idem = event
        .idempotency_key
        .clone()
        .unwrap_or_else(|| event.request_id.to_string());
    let mut tx = pool.begin().await?;
    let inserted = sqlx::query_scalar::<_, bool>(
        "INSERT INTO request_events \
         (ts, request_id, idempotency_key, org_id, project_id, api_key_id, channel_id, group_id, \
          model_requested, model_actual, tokens_in, tokens_out, tokens_cached, cost_micros, cost_usd, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $14::numeric / 1000000, $15) \
         ON CONFLICT (idempotency_key) DO NOTHING \
         RETURNING TRUE",
    )
    .bind(event.occurred_at)
    .bind(event.request_id)
    .bind(&idem)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id)
    .bind(event.group_id)
    .bind(&event.model)
    .bind(&event.model)
    .bind(event.prompt_tokens)
    .bind(event.completion_tokens)
    .bind(event.cached_tokens)
    .bind(event.cost_micros)
    .bind(event.status as i32)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(false);
    if !inserted {
        tx.commit().await?;
        metrics::counter!("billing_settle_duplicates_total").increment(1);
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO usage_records \
         (ts, request_id, org_id, project_id, api_key_id, channel_id, group_id, \
          model_requested, model_actual, tokens_in, tokens_out, tokens_cached, cost_usd, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::numeric / 1000000, $14) \
         ON CONFLICT (ts, request_id) DO NOTHING",
    )
    .bind(event.occurred_at)
    .bind(event.request_id)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id)
    .bind(event.group_id)
    .bind(&event.model)
    .bind(&event.model)
    .bind(event.prompt_tokens)
    .bind(event.completion_tokens)
    .bind(event.cached_tokens)
    .bind(event.cost_micros)
    .bind(event.status as i32)
    .execute(&mut *tx)
    .await?;

    let status_class = (event.status / 100) * 100;
    let is_error = i64::from(event.status >= 400);
    let channel_key = event.channel_id.unwrap_or_else(uuid::Uuid::nil);
    sqlx::query(
        "INSERT INTO usage_hourly_rollups \
         (bucket, channel_key, org_id, project_id, api_key_id, channel_id, model_actual, status_class, \
          request_count, error_count, tokens_in, tokens_out, tokens_cached, cost_micros) \
         VALUES (date_trunc('hour', $1::timestamptz), $2, $3, $4, $5, $6, $7, $8, 1, $9, $10, $11, $12, $13) \
         ON CONFLICT (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class) \
         DO UPDATE SET request_count = usage_hourly_rollups.request_count + 1, \
                       error_count = usage_hourly_rollups.error_count + EXCLUDED.error_count, \
                       tokens_in = usage_hourly_rollups.tokens_in + EXCLUDED.tokens_in, \
                       tokens_out = usage_hourly_rollups.tokens_out + EXCLUDED.tokens_out, \
                       tokens_cached = usage_hourly_rollups.tokens_cached + EXCLUDED.tokens_cached, \
                       cost_micros = usage_hourly_rollups.cost_micros + EXCLUDED.cost_micros, \
                       updated_at = NOW()",
    )
    .bind(event.occurred_at)
    .bind(channel_key)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id)
    .bind(&event.model)
    .bind(status_class as i32)
    .bind(is_error)
    .bind(event.prompt_tokens as i64)
    .bind(event.completion_tokens as i64)
    .bind(event.cached_tokens as i64)
    .bind(event.cost_micros)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO usage_daily_rollups \
         (bucket, channel_key, org_id, project_id, api_key_id, channel_id, model_actual, status_class, \
          request_count, error_count, tokens_in, tokens_out, tokens_cached, cost_micros) \
         VALUES ($1::date, $2, $3, $4, $5, $6, $7, $8, 1, $9, $10, $11, $12, $13) \
         ON CONFLICT (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class) \
         DO UPDATE SET request_count = usage_daily_rollups.request_count + 1, \
                       error_count = usage_daily_rollups.error_count + EXCLUDED.error_count, \
                       tokens_in = usage_daily_rollups.tokens_in + EXCLUDED.tokens_in, \
                       tokens_out = usage_daily_rollups.tokens_out + EXCLUDED.tokens_out, \
                       tokens_cached = usage_daily_rollups.tokens_cached + EXCLUDED.tokens_cached, \
                       cost_micros = usage_daily_rollups.cost_micros + EXCLUDED.cost_micros, \
                       updated_at = NOW()",
    )
    .bind(event.occurred_at.date_naive())
    .bind(channel_key)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id)
    .bind(&event.model)
    .bind(status_class as i32)
    .bind(is_error)
    .bind(event.prompt_tokens as i64)
    .bind(event.completion_tokens as i64)
    .bind(event.cached_tokens as i64)
    .bind(event.cost_micros)
    .execute(&mut *tx)
    .await?;

    let lag_seconds = (Utc::now() - event.occurred_at).num_milliseconds().max(0) as f64 / 1000.0;
    metrics::gauge!("usage_rollup_lag_seconds").set(lag_seconds);
    metrics::gauge!("billing_settle_lag_seconds").set(lag_seconds);

    let ledger_event = BillingLedgerEvent::actual_settle(event, idem);
    insert_ledger_event_tx(&mut tx, &ledger_event).await?;

    tx.commit().await?;
    Ok(())
}

async fn commit_usage_batch_inner(
    pool: &PgPool,
    events: &[&UsageEvent],
) -> BillingResult<Vec<uuid::Uuid>> {
    if events.is_empty() {
        return Ok(Vec::new());
    }

    let mut tx = pool.begin().await?;
    let inserted = insert_request_events_batch(&mut tx, events).await?;
    let inserted_ids = inserted
        .iter()
        .map(|event| event.request_id)
        .collect::<Vec<_>>();
    let duplicate_count = events.len().saturating_sub(inserted.len());
    if duplicate_count > 0 {
        metrics::counter!("billing_settle_duplicates_total").increment(duplicate_count as u64);
    }
    if inserted.is_empty() {
        tx.commit().await?;
        return Ok(inserted_ids);
    }

    insert_usage_records_batch(&mut tx, &inserted).await?;
    upsert_hourly_rollups_batch(&mut tx, &inserted).await?;
    upsert_daily_rollups_batch(&mut tx, &inserted).await?;

    let mut ledger_events = Vec::with_capacity(inserted.len());
    for event in &inserted {
        let idem = idempotency_key(event);
        ledger_events.push(BillingLedgerEvent::actual_settle(event, idem));
        let lag_seconds =
            (Utc::now() - event.occurred_at).num_milliseconds().max(0) as f64 / 1000.0;
        metrics::gauge!("usage_rollup_lag_seconds").set(lag_seconds);
        metrics::gauge!("billing_settle_lag_seconds").set(lag_seconds);
    }
    insert_ledger_events_batch(&mut tx, &ledger_events).await?;

    tx.commit().await?;
    Ok(inserted_ids)
}

fn idempotency_key(event: &UsageEvent) -> String {
    event
        .idempotency_key
        .clone()
        .unwrap_or_else(|| event.request_id.to_string())
}

async fn insert_request_events_batch<'a>(
    tx: &mut Transaction<'_, Postgres>,
    events: &[&'a UsageEvent],
) -> BillingResult<Vec<&'a UsageEvent>> {
    let mut query = QueryBuilder::new(
        "INSERT INTO request_events \
         (ts, request_id, idempotency_key, org_id, project_id, api_key_id, channel_id, group_id, \
          model_requested, model_actual, tokens_in, tokens_out, tokens_cached, cost_micros, cost_usd, status) ",
    );
    query.push_values(events, |mut row, event| {
        row.push_bind(event.occurred_at)
            .push_bind(event.request_id)
            .push_bind(idempotency_key(event))
            .push_bind(event.org_id)
            .push_bind(event.project_id)
            .push_bind(event.api_key_id)
            .push_bind(event.channel_id)
            .push_bind(event.group_id)
            .push_bind(&event.model)
            .push_bind(&event.model)
            .push_bind(event.prompt_tokens)
            .push_bind(event.completion_tokens)
            .push_bind(event.cached_tokens)
            .push_bind(event.cost_micros)
            .push_bind(cost_usd_decimal(event.cost_micros))
            .push_bind(event.status as i32);
    });
    query.push(" ON CONFLICT (idempotency_key) DO NOTHING RETURNING idempotency_key");

    let inserted_keys = query
        .build_query_scalar::<String>()
        .fetch_all(&mut **tx)
        .await?;
    let mut inserted_key_set = inserted_keys
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    Ok(events
        .iter()
        .copied()
        .filter(|event| inserted_key_set.remove(&idempotency_key(event)))
        .collect())
}

async fn insert_usage_records_batch(
    tx: &mut Transaction<'_, Postgres>,
    events: &[&UsageEvent],
) -> BillingResult<()> {
    let mut query = QueryBuilder::new(
        "INSERT INTO usage_records \
         (ts, request_id, org_id, project_id, api_key_id, channel_id, group_id, \
          model_requested, model_actual, tokens_in, tokens_out, tokens_cached, cost_usd, status) ",
    );
    query.push_values(events, |mut row, event| {
        row.push_bind(event.occurred_at)
            .push_bind(event.request_id)
            .push_bind(event.org_id)
            .push_bind(event.project_id)
            .push_bind(event.api_key_id)
            .push_bind(event.channel_id)
            .push_bind(event.group_id)
            .push_bind(&event.model)
            .push_bind(&event.model)
            .push_bind(event.prompt_tokens)
            .push_bind(event.completion_tokens)
            .push_bind(event.cached_tokens)
            .push_bind(cost_usd_decimal(event.cost_micros))
            .push_bind(event.status as i32);
    });
    query.push(" ON CONFLICT (ts, request_id) DO NOTHING");
    query.build().execute(&mut **tx).await?;
    Ok(())
}

async fn upsert_hourly_rollups_batch(
    tx: &mut Transaction<'_, Postgres>,
    events: &[&UsageEvent],
) -> BillingResult<()> {
    let mut query = QueryBuilder::new(
        "INSERT INTO usage_hourly_rollups \
         (bucket, channel_key, org_id, project_id, api_key_id, channel_id, model_actual, status_class, \
          request_count, error_count, tokens_in, tokens_out, tokens_cached, cost_micros) ",
    );
    query.push_values(events, |mut row, event| {
        let status_class = (event.status / 100) * 100;
        let is_error = i64::from(event.status >= 400);
        let channel_key = event.channel_id.unwrap_or_else(uuid::Uuid::nil);
        row.push("date_trunc('hour', ")
            .push_bind(event.occurred_at)
            .push("::timestamptz)")
            .push_bind(channel_key)
            .push_bind(event.org_id)
            .push_bind(event.project_id)
            .push_bind(event.api_key_id)
            .push_bind(event.channel_id)
            .push_bind(&event.model)
            .push_bind(status_class as i32)
            .push_bind(1_i64)
            .push_bind(is_error)
            .push_bind(event.prompt_tokens as i64)
            .push_bind(event.completion_tokens as i64)
            .push_bind(event.cached_tokens as i64)
            .push_bind(event.cost_micros);
    });
    query.push(
        " ON CONFLICT (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class) \
         DO UPDATE SET request_count = usage_hourly_rollups.request_count + EXCLUDED.request_count, \
                       error_count = usage_hourly_rollups.error_count + EXCLUDED.error_count, \
                       tokens_in = usage_hourly_rollups.tokens_in + EXCLUDED.tokens_in, \
                       tokens_out = usage_hourly_rollups.tokens_out + EXCLUDED.tokens_out, \
                       tokens_cached = usage_hourly_rollups.tokens_cached + EXCLUDED.tokens_cached, \
                       cost_micros = usage_hourly_rollups.cost_micros + EXCLUDED.cost_micros, \
                       updated_at = NOW()",
    );
    query.build().execute(&mut **tx).await?;
    Ok(())
}

async fn upsert_daily_rollups_batch(
    tx: &mut Transaction<'_, Postgres>,
    events: &[&UsageEvent],
) -> BillingResult<()> {
    let mut query = QueryBuilder::new(
        "INSERT INTO usage_daily_rollups \
         (bucket, channel_key, org_id, project_id, api_key_id, channel_id, model_actual, status_class, \
          request_count, error_count, tokens_in, tokens_out, tokens_cached, cost_micros) ",
    );
    query.push_values(events, |mut row, event| {
        let status_class = (event.status / 100) * 100;
        let is_error = i64::from(event.status >= 400);
        let channel_key = event.channel_id.unwrap_or_else(uuid::Uuid::nil);
        row.push_bind(event.occurred_at.date_naive())
            .push_bind(channel_key)
            .push_bind(event.org_id)
            .push_bind(event.project_id)
            .push_bind(event.api_key_id)
            .push_bind(event.channel_id)
            .push_bind(&event.model)
            .push_bind(status_class as i32)
            .push_bind(1_i64)
            .push_bind(is_error)
            .push_bind(event.prompt_tokens as i64)
            .push_bind(event.completion_tokens as i64)
            .push_bind(event.cached_tokens as i64)
            .push_bind(event.cost_micros);
    });
    query.push(
        " ON CONFLICT (bucket, org_id, project_id, api_key_id, channel_key, model_actual, status_class) \
         DO UPDATE SET request_count = usage_daily_rollups.request_count + EXCLUDED.request_count, \
                       error_count = usage_daily_rollups.error_count + EXCLUDED.error_count, \
                       tokens_in = usage_daily_rollups.tokens_in + EXCLUDED.tokens_in, \
                       tokens_out = usage_daily_rollups.tokens_out + EXCLUDED.tokens_out, \
                       tokens_cached = usage_daily_rollups.tokens_cached + EXCLUDED.tokens_cached, \
                       cost_micros = usage_daily_rollups.cost_micros + EXCLUDED.cost_micros, \
                       updated_at = NOW()",
    );
    query.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_ledger_events_batch(
    tx: &mut Transaction<'_, Postgres>,
    events: &[BillingLedgerEvent],
) -> BillingResult<()> {
    if events.is_empty() {
        return Ok(());
    }

    let mut query = QueryBuilder::new(
        "INSERT INTO billing_ledger_events \
         (id, idempotency_key, request_id, occurred_at, org_id, project_id, api_key_id, channel_id, \
          event_type, direction, amount_micros, source_type, source_id, status, invoice_month, metadata) ",
    );
    query.push_values(events, |mut row, event| {
        row.push_bind(event.id)
            .push_bind(&event.idempotency_key)
            .push_bind(event.request_id)
            .push_bind(event.occurred_at)
            .push_bind(event.org_id)
            .push_bind(event.project_id)
            .push_bind(event.api_key_id)
            .push_bind(event.channel_id)
            .push_bind(event.event_type.as_str())
            .push_bind(event.direction.as_str())
            .push_bind(event.amount_micros)
            .push_bind(&event.source_type)
            .push_bind(&event.source_id)
            .push_bind(event.status.as_str())
            .push_bind(&event.invoice_month)
            .push_bind(&event.metadata);
    });
    query.push(" ON CONFLICT (idempotency_key) DO NOTHING");
    query.build().execute(&mut **tx).await?;
    Ok(())
}

fn cost_usd_decimal(cost_micros: i64) -> Decimal {
    Decimal::from(cost_micros) / Decimal::from(1_000_000_i64)
}

fn usage_event_span(name: &'static str, event: &UsageEvent) -> tracing::Span {
    match name {
        "billing.consumer.process_one" => tracing::info_span!(
            "billing.consumer.process_one",
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
        ),
        "billing.commit_usage" => tracing::info_span!(
            "billing.commit_usage",
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
        ),
        _ => tracing::info_span!(
            "billing.usage_event",
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
        ),
    }
}

fn display_opt_uuid(value: Option<uuid::Uuid>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

// ============================================================================
// OutboxRepo: fetch_batch 需要过滤 retry_count < MAX_RETRIES
// 在 PgOutboxRepo 里更新 SQL 以支持这个语义
// ============================================================================
// 注意：PgOutboxRepo::fetch_batch 需要补 retry_count 过滤，见 outbox.rs
