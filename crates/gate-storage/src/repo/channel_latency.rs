//! Channel latency samples — persistent sliding window for `least_latency`.
//!
//! Data-plane routing should not depend solely on one process' in-memory
//! `ChannelMetrics`: health probes and requests from multiple replicas all append
//! compact samples here, then router reads one bounded average query per routing
//! decision.

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gate_core::id::ChannelId;
use parking_lot::RwLock;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

const IN_MEMORY_MAX_SAMPLES_PER_CHANNEL: usize = 4096;
const SOURCE_REQUEST: &str = "request";
const SOURCE_HEALTH_PROBE: &str = "health_probe";

#[async_trait]
pub trait ChannelLatencyRepo: Send + Sync + 'static {
    /// Append one latency observation.
    ///
    /// `source` is intentionally low-cardinality and must be either `request`
    /// or `health_probe`.
    async fn record_sample(
        &self,
        channel_id: ChannelId,
        latency_ms: u64,
        success: bool,
        source: &str,
    ) -> DbResult<()>;

    /// Return successful average latency per channel within a recent window.
    async fn avg_latency_ms(
        &self,
        channel_ids: &[ChannelId],
        window_secs: i64,
    ) -> DbResult<HashMap<ChannelId, u64>>;

    /// Prune old samples; meant for cron/maintenance hooks.
    async fn prune_older_than(&self, retention_secs: i64) -> DbResult<u64>;
}

pub struct PgChannelLatencyRepo {
    pool: PgPool,
}

impl PgChannelLatencyRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChannelLatencyRepo for PgChannelLatencyRepo {
    async fn record_sample(
        &self,
        channel_id: ChannelId,
        latency_ms: u64,
        success: bool,
        source: &str,
    ) -> DbResult<()> {
        validate_source(source)?;
        let latency_ms = i64::try_from(latency_ms).unwrap_or(i64::MAX);
        sqlx::query(
            "INSERT INTO channel_latency_samples (channel_id, latency_ms, success, source) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(channel_id.as_uuid())
        .bind(latency_ms)
        .bind(success)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn avg_latency_ms(
        &self,
        channel_ids: &[ChannelId],
        window_secs: i64,
    ) -> DbResult<HashMap<ChannelId, u64>> {
        if channel_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = channel_ids.iter().map(|id| *id.as_uuid()).collect();
        let rows = sqlx::query(
            "SELECT channel_id, FLOOR(AVG(latency_ms))::BIGINT AS avg_latency_ms \
             FROM channel_latency_samples \
             WHERE channel_id = ANY($1) \
               AND ts >= NOW() - ($2::BIGINT * INTERVAL '1 second') \
               AND success = TRUE \
             GROUP BY channel_id",
        )
        .bind(&ids)
        .bind(window_secs.max(1))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.try_get("channel_id")?;
                let avg: i64 = row.try_get("avg_latency_ms")?;
                Ok((ChannelId::from(id), avg.max(0) as u64))
            })
            .collect()
    }

    async fn prune_older_than(&self, retention_secs: i64) -> DbResult<u64> {
        let result = sqlx::query(
            "DELETE FROM channel_latency_samples \
             WHERE ts < NOW() - ($1::BIGINT * INTERVAL '1 second')",
        )
        .bind(retention_secs.max(1))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Debug, Clone)]
struct LatencySample {
    ts: DateTime<Utc>,
    latency_ms: u64,
    success: bool,
}

#[derive(Default)]
pub struct InMemoryChannelLatencyRepo {
    inner: RwLock<HashMap<ChannelId, VecDeque<LatencySample>>>,
}

impl InMemoryChannelLatencyRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ChannelLatencyRepo for InMemoryChannelLatencyRepo {
    async fn record_sample(
        &self,
        channel_id: ChannelId,
        latency_ms: u64,
        success: bool,
        source: &str,
    ) -> DbResult<()> {
        validate_source(source)?;
        let mut inner = self.inner.write();
        let samples = inner.entry(channel_id).or_default();
        if samples.len() >= IN_MEMORY_MAX_SAMPLES_PER_CHANNEL {
            samples.pop_front();
        }
        samples.push_back(LatencySample {
            ts: Utc::now(),
            latency_ms,
            success,
        });
        Ok(())
    }

    async fn avg_latency_ms(
        &self,
        channel_ids: &[ChannelId],
        window_secs: i64,
    ) -> DbResult<HashMap<ChannelId, u64>> {
        let cutoff = Utc::now() - ChronoDuration::seconds(window_secs.max(1));
        let inner = self.inner.read();
        let mut out = HashMap::new();
        for channel_id in channel_ids {
            let Some(samples) = inner.get(channel_id) else {
                continue;
            };
            let mut sum = 0u64;
            let mut count = 0u64;
            for sample in samples {
                if sample.success && sample.ts >= cutoff {
                    sum = sum.saturating_add(sample.latency_ms);
                    count += 1;
                }
            }
            if let Some(avg) = sum.checked_div(count) {
                out.insert(*channel_id, avg);
            }
        }
        Ok(out)
    }

    async fn prune_older_than(&self, retention_secs: i64) -> DbResult<u64> {
        let cutoff = Utc::now() - ChronoDuration::seconds(retention_secs.max(1));
        let mut removed = 0u64;
        let mut inner = self.inner.write();
        for samples in inner.values_mut() {
            let before = samples.len();
            samples.retain(|sample| sample.ts >= cutoff);
            removed += (before - samples.len()) as u64;
        }
        Ok(removed)
    }
}

fn validate_source(source: &str) -> DbResult<()> {
    match source {
        SOURCE_REQUEST | SOURCE_HEALTH_PROBE => Ok(()),
        other => Err(DbError::Constraint(format!(
            "invalid channel latency sample source: {other}"
        ))),
    }
}
