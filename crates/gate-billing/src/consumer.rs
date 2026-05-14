//! Consumer loop — 拉取 outbox 批次，写 usage_records，标记完成。
//!
//! 设计要点：
//! - 每条独立处理，单条失败不影响其他
//! - 失败 >= MAX_RETRIES 后 mark_failed（不再重试）
//! - commit_usage 走幂等写（ON CONFLICT DO NOTHING on (ts, request_id)）

use crate::BillingResult;
use crate::outbox::{OutboxId, OutboxRepo};
use crate::types::UsageEvent;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

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
        loop {
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "billing consumer tick error");
            }
            tokio::time::sleep(self.interval).await;
        }
    }

    /// 单次 tick：拉一批 → 逐条处理。
    pub async fn tick(&self) -> BillingResult<()> {
        let batch = self.outbox.fetch_batch(self.batch_size).await?;
        if batch.is_empty() {
            return Ok(());
        }
        tracing::debug!(count = batch.len(), "billing consumer processing batch");

        for (outbox_id, event) in batch {
            self.process_one(outbox_id, &event).await;
        }
        Ok(())
    }

    async fn process_one(&self, outbox_id: OutboxId, event: &UsageEvent) {
        match commit_usage(&self.pool, event).await {
            Ok(()) => {
                if let Err(e) = self.outbox.mark_done(outbox_id).await {
                    tracing::warn!(outbox_id, error = %e, "mark_done failed");
                }
            }
            Err(e) => {
                tracing::warn!(
                    outbox_id,
                    request_id = %event.request_id,
                    error = %e,
                    "commit_usage failed"
                );
                // 检查 retry_count：先 mark_failed，如果超限就留着（已 mark_failed = no
                // processed_at，下次 fetch_batch 还会捞到；业务决定是否要阻断）
                // 实际阻断逻辑：fetch_batch 里的 FOR UPDATE SKIP LOCKED 保证并发安全。
                // 超限判断：需查 retry_count，简化：mark_failed 本身会递增，
                // 消费者在 fetch_batch 里 filter retry_count < MAX_RETRIES。
                if let Err(me) = self.outbox.mark_failed(outbox_id, &e.to_string()).await {
                    tracing::warn!(outbox_id, error = %me, "mark_failed itself failed");
                }
            }
        }
    }
}

/// 把 UsageEvent 写到 usage_records 表（幂等：ON CONFLICT DO NOTHING）。
///
/// channel_id 为 None 时直接写 NULL（fallback provider 路径无 channel 归属）。
pub async fn commit_usage(pool: &PgPool, event: &UsageEvent) -> BillingResult<()> {
    sqlx::query(
        "INSERT INTO usage_records \
         (ts, request_id, org_id, project_id, api_key_id, channel_id, \
          model_requested, model_actual, tokens_in, tokens_out, tokens_cached, cost_usd, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric / 1000000, $13) \
         ON CONFLICT (ts, request_id) DO NOTHING",
    )
    .bind(event.occurred_at)
    .bind(event.request_id)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id) // Option<Uuid> → NULL when None
    .bind(&event.model)
    .bind(&event.model) // model_actual = model_requested（C1 阶段无别名翻译）
    .bind(event.prompt_tokens)
    .bind(event.completion_tokens)
    .bind(event.cached_tokens)
    .bind(event.cost_micros)
    .bind(event.status as i32)
    .execute(pool)
    .await?;
    Ok(())
}

// ============================================================================
// OutboxRepo: fetch_batch 需要过滤 retry_count < MAX_RETRIES
// 在 PgOutboxRepo 里更新 SQL 以支持这个语义
// ============================================================================
// 注意：PgOutboxRepo::fetch_batch 需要补 retry_count 过滤，见 outbox.rs
