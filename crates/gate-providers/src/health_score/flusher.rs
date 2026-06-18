//! Async batched score flusher (ADR-0007 / M5.1 N1.5)。
//!
//! 把 routing / request handler 的 [`OutcomeEvent`] 串成异步 batched 写入：
//!
//! ```text
//!   request handler ──tokio::mpsc──> ScoreFlusher worker
//!                                       │
//!                                       ▼  (周期 OR size 触发)
//!                          per channel:
//!                            1. repo.record_outcome(obs)
//!                            2. repo.get(ch_id)  → ChannelHealthScore
//!                            3. engine.recompute → ScoreUpdate
//!                            4. repo.apply_update
//!                            5. cache.put(ChannelHealthScore::from(update))
//! ```
//!
//! ## 设计
//!
//! - **不阻塞热路径**：handler 用 `try_send`，buffer 满则 drop（事件流允许少量丢失，
//!   滚动窗口会自行修正）。
//! - **批量收集**：每 batch_size 事件或每 flush_interval 触发一次落库；同 channel
//!   多事件聚合后单次 recompute（避免抖动）。
//! - **fail-open**：单条事件失败仅 warn，不影响后续。
//! - **优雅停止**：drop 发送端后 worker 自然结束。
//!
//! 调用方流程：
//!
//! ```ignore
//! let flusher = ScoreFlusher::new(repo, engine, cache, config);
//! let handle = flusher.spawn();  // returns ScoreFlusherHandle
//! // request handler 侧：
//! handle.try_observe(channel_id, observation);
//! ```

use super::ScoreEngine;
use super::cache::HealthScoreCache;
use chrono::Utc;
use gate_core::id::ChannelId;
use gate_storage::{ChannelHealthScoreRepo, OutcomeObservation};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// ============================================================================
// 配置 + 事件
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct ScoreFlusherConfig {
    /// 单 batch 最大事件数；触发立即 flush。
    pub batch_size: usize,
    /// 最大等待间隔；即使 batch 未满也触发 flush。
    pub flush_interval: Duration,
    /// mpsc 通道缓冲；buffer 满时 try_send 失败（即"丢事件"）。
    pub channel_buffer: usize,
}

impl Default for ScoreFlusherConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval: Duration::from_secs(1),
            channel_buffer: 1024,
        }
    }
}

/// 一条观测事件——request handler / probe 在请求结束时发。
#[derive(Debug, Clone)]
pub struct OutcomeEvent {
    pub channel_id: ChannelId,
    pub observation: OutcomeObservation,
    /// 上游 `Retry-After` 头存在时填秒数；用于 cooldown 决策。
    pub external_retry_after_secs: Option<i64>,
}

// ============================================================================
// ScoreFlusher worker
// ============================================================================

pub struct ScoreFlusher {
    repo: Arc<dyn ChannelHealthScoreRepo>,
    engine: ScoreEngine,
    cache: HealthScoreCache,
    config: ScoreFlusherConfig,
}

impl ScoreFlusher {
    pub fn new(
        repo: Arc<dyn ChannelHealthScoreRepo>,
        engine: ScoreEngine,
        cache: HealthScoreCache,
        config: ScoreFlusherConfig,
    ) -> Self {
        Self {
            repo,
            engine,
            cache,
            config,
        }
    }

    /// 起 worker。返回 handle 让调用方发事件。
    pub fn spawn(self) -> ScoreFlusherHandle {
        let (tx, rx) = mpsc::channel(self.config.channel_buffer);
        let join = tokio::spawn(self.run(rx));
        ScoreFlusherHandle { tx, join }
    }

    /// worker loop。接收 OutcomeEvent；按 batch_size 或 flush_interval 触发 flush。
    async fn run(self, mut rx: mpsc::Receiver<OutcomeEvent>) {
        let mut buffer: Vec<OutcomeEvent> = Vec::with_capacity(self.config.batch_size);
        let mut ticker = tokio::time::interval(self.config.flush_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                maybe_event = rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            buffer.push(event);
                            if buffer.len() >= self.config.batch_size {
                                self.flush(&mut buffer).await;
                            }
                        }
                        None => {
                            // 发送端全部 drop → 收尾 flush 剩余 buffer
                            if !buffer.is_empty() {
                                self.flush(&mut buffer).await;
                            }
                            tracing::debug!("ScoreFlusher worker exiting (channel closed)");
                            return;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        self.flush(&mut buffer).await;
                    }
                }
            }
        }
    }

    /// 真正落库：按 channel 聚合事件，调 record_outcome + recompute + apply_update + cache。
    async fn flush(&self, buffer: &mut Vec<OutcomeEvent>) {
        let mut by_channel: HashMap<ChannelId, Vec<OutcomeEvent>> = HashMap::new();
        for event in buffer.drain(..) {
            by_channel.entry(event.channel_id).or_default().push(event);
        }

        for (channel_id, events) in by_channel {
            if let Err(e) = self.flush_channel(channel_id, events).await {
                tracing::warn!(
                    error = %e,
                    %channel_id,
                    "ScoreFlusher: per-channel flush failed (events dropped, will retry on next event)"
                );
            }
        }
    }

    /// 单 channel：累计所有事件 → 1 次 recompute → 1 次 apply_update。
    async fn flush_channel(
        &self,
        channel_id: ChannelId,
        events: Vec<OutcomeEvent>,
    ) -> Result<(), gate_storage::DbError> {
        // 1. record_outcome 累计 raw counter
        // 2. 用最后一个事件的 retry_after 作为本次 cooldown 决策的 external 值
        let last_retry_after = events
            .iter()
            .rev()
            .find_map(|e| e.external_retry_after_secs);
        for event in &events {
            self.repo
                .record_outcome(channel_id, &event.observation)
                .await?;
        }

        // 3. 读最新快照
        let current = self
            .repo
            .get(channel_id)
            .await?
            .ok_or(gate_storage::DbError::NotFound)?;

        // 4. recompute
        let update = self
            .engine
            .recompute(&current, Utc::now(), last_retry_after);

        // 5. apply_update
        self.repo.apply_update(channel_id, &update).await?;

        // 6. cache.put：合成最新 ChannelHealthScore 写回 cache
        let cached_snapshot = merge_update(&current, &update);
        self.cache.put(cached_snapshot);
        Ok(())
    }
}

/// 把 `ScoreUpdate` 合并回 `ChannelHealthScore` 用于 cache。
fn merge_update(
    current: &gate_storage::ChannelHealthScore,
    update: &gate_storage::ScoreUpdate,
) -> gate_storage::ChannelHealthScore {
    let mut next = current.clone();
    next.score = update.score;
    next.success_rate = update.success_rate;
    next.latency_p99_ms = update.latency_p99_ms;
    next.banned_signal = update.banned_signal;
    next.quota_remaining_norm = update.quota_remaining_norm;
    next.consecutive_failures = update.consecutive_failures;
    next.state = update.state;
    next.cooldown_until = update.cooldown_until;
    next.banned_reason = update.banned_reason.clone();
    next.window_total = update.window_total;
    next.window_success = update.window_success;
    next.window_started_at = update.window_started_at;
    next.updated_at = Utc::now();
    next
}

// ============================================================================
// Handle：调用方接口
// ============================================================================

#[derive(Debug)]
pub struct ScoreFlusherHandle {
    tx: mpsc::Sender<OutcomeEvent>,
    join: JoinHandle<()>,
}

impl ScoreFlusherHandle {
    /// 非阻塞发送。buffer 满时返回 `Err(event)`，调用方可丢弃或重试。
    ///
    /// 路由热路径推荐用此 API：失败即丢，不阻塞请求。
    pub fn try_observe(&self, event: OutcomeEvent) -> Result<(), OutcomeEvent> {
        self.tx.try_send(event).map_err(|e| match e {
            mpsc::error::TrySendError::Full(ev) | mpsc::error::TrySendError::Closed(ev) => ev,
        })
    }

    /// 阻塞发送（async）。control plane / admin operations 用。
    pub async fn observe(
        &self,
        event: OutcomeEvent,
    ) -> Result<(), mpsc::error::SendError<OutcomeEvent>> {
        self.tx.send(event).await
    }

    /// 优雅停止：drop 发送端，worker 自然结束。
    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        drop(self.tx);
        self.join.await
    }

    /// 主动等待 worker（不 close 发送端，用于调试）。
    pub fn join_handle(&self) -> &JoinHandle<()> {
        &self.join
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_score::ScoreEngine;
    use gate_storage::{
        ChannelHealthScoreRepo, HealthState, InMemoryChannelHealthScoreRepo, OutcomeObservation,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use uuid::Uuid;

    fn cid() -> ChannelId {
        ChannelId::from(Uuid::new_v4())
    }

    fn mk_flusher(
        batch_size: usize,
        flush_interval_ms: u64,
    ) -> (
        Arc<InMemoryChannelHealthScoreRepo>,
        HealthScoreCache,
        ScoreFlusherHandle,
    ) {
        let repo = Arc::new(InMemoryChannelHealthScoreRepo::new());
        let cache = HealthScoreCache::default();
        let cfg = ScoreFlusherConfig {
            batch_size,
            flush_interval: Duration::from_millis(flush_interval_ms),
            channel_buffer: 256,
        };
        let flusher = ScoreFlusher::new(
            repo.clone() as Arc<dyn ChannelHealthScoreRepo>,
            ScoreEngine::default(),
            cache.clone(),
            cfg,
        );
        let handle = flusher.spawn();
        (repo, cache, handle)
    }

    #[tokio::test]
    async fn batch_size_triggers_flush() {
        let (repo, cache, handle) = mk_flusher(3, 30_000); // 大 interval，只靠 size 触发
        let id = cid();
        repo.ensure_row(id).await.unwrap();

        for _ in 0..3 {
            handle
                .try_observe(OutcomeEvent {
                    channel_id: id,
                    observation: OutcomeObservation {
                        success: Some(true),
                        ..Default::default()
                    },
                    external_retry_after_secs: None,
                })
                .unwrap();
        }
        // 让 worker 跑一会
        tokio::time::sleep(Duration::from_millis(80)).await;
        let snap = repo.get(id).await.unwrap().unwrap();
        assert_eq!(snap.window_total, 3);
        assert_eq!(snap.window_success, 3);
        assert!(cache.get(id).is_some());
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn interval_triggers_flush_without_full_batch() {
        let (repo, _cache, handle) = mk_flusher(1000, 50);
        let id = cid();
        repo.ensure_row(id).await.unwrap();

        handle
            .try_observe(OutcomeEvent {
                channel_id: id,
                observation: OutcomeObservation {
                    success: Some(false),
                    ..Default::default()
                },
                external_retry_after_secs: None,
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        let snap = repo.get(id).await.unwrap().unwrap();
        assert_eq!(snap.window_total, 1);
        assert_eq!(snap.window_success, 0);
        assert_eq!(snap.consecutive_failures, 1);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cache_warms_up_after_flush() {
        let (repo, cache, handle) = mk_flusher(1, 30_000);
        let id = cid();
        repo.ensure_row(id).await.unwrap();
        assert!(cache.get(id).is_none());
        handle
            .try_observe(OutcomeEvent {
                channel_id: id,
                observation: OutcomeObservation {
                    success: Some(true),
                    ..Default::default()
                },
                external_retry_after_secs: None,
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(cache.get(id).is_some());
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn multi_channel_isolation() {
        let (repo, _cache, handle) = mk_flusher(2, 30_000);
        let a = cid();
        let b = cid();
        repo.ensure_row(a).await.unwrap();
        repo.ensure_row(b).await.unwrap();
        for _ in 0..2 {
            handle
                .try_observe(OutcomeEvent {
                    channel_id: a,
                    observation: OutcomeObservation {
                        success: Some(true),
                        ..Default::default()
                    },
                    external_retry_after_secs: None,
                })
                .unwrap();
            handle
                .try_observe(OutcomeEvent {
                    channel_id: b,
                    observation: OutcomeObservation {
                        success: Some(false),
                        ..Default::default()
                    },
                    external_retry_after_secs: None,
                })
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
        let sa = repo.get(a).await.unwrap().unwrap();
        let sb = repo.get(b).await.unwrap().unwrap();
        assert_eq!(sa.window_success, 2);
        assert_eq!(sb.window_success, 0);
        assert_eq!(sb.consecutive_failures, 2);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn banned_signal_transitions_to_banned_state() {
        let (repo, cache, handle) = mk_flusher(1, 30_000);
        let id = cid();
        repo.ensure_row(id).await.unwrap();
        handle
            .try_observe(OutcomeEvent {
                channel_id: id,
                observation: OutcomeObservation {
                    banned_signal: Some("openai:account_deactivated".to_string()),
                    ..Default::default()
                },
                external_retry_after_secs: None,
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let snap = repo.get(id).await.unwrap().unwrap();
        assert_eq!(snap.state, HealthState::Banned);
        let cached = cache.get(id).unwrap();
        assert_eq!(cached.state, HealthState::Banned);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_flushes_remaining_buffer() {
        let (repo, _cache, handle) = mk_flusher(1000, 30_000);
        let id = cid();
        repo.ensure_row(id).await.unwrap();
        handle
            .try_observe(OutcomeEvent {
                channel_id: id,
                observation: OutcomeObservation {
                    success: Some(true),
                    ..Default::default()
                },
                external_retry_after_secs: None,
            })
            .unwrap();
        // 立即 shutdown：worker 应该收尾 flush 剩余 buffer
        handle.shutdown().await.unwrap();
        let snap = repo.get(id).await.unwrap().unwrap();
        assert_eq!(snap.window_total, 1);
    }

    #[tokio::test]
    async fn buffer_full_drops_event_gracefully() {
        let (_repo, _cache, handle) = mk_flusher(1000, 30_000);
        // ScoreFlusherConfig.channel_buffer = 256；连发 300 个理论会有 drop
        let id = cid();
        let mut dropped = 0;
        for _ in 0..300 {
            if handle
                .try_observe(OutcomeEvent {
                    channel_id: id,
                    observation: OutcomeObservation {
                        success: Some(true),
                        ..Default::default()
                    },
                    external_retry_after_secs: None,
                })
                .is_err()
            {
                dropped += 1;
            }
        }
        // 至少有一些被 drop，证明 try_send 不阻塞
        assert!(
            dropped > 0,
            "expected some drops with buffer 256 + 300 events"
        );
        handle.shutdown().await.unwrap();
    }
}
