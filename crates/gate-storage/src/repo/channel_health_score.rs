//! Channel Health Score — 号池中台护城河（ADR-0007 / M5.1 N1.1）。
//!
//! 4 维加权评分 + 5 状态机 + 自动 cooldown 的**持久层**。本模块只负责
//! schema 落库与读写；评分引擎（[`ScoreCalculator`]）和状态机
//! （[`StateMachine`]）实装属于 M5.1 N1.2，路由策略接入属于 N1.4。
//!
//! ## 字段语义
//!
//! - `score`：4 维加权后的综合健康度，归一 [0.0, 1.0]
//! - `success_rate / latency_p99_ms / banned_signal / quota_remaining_norm`：4 维原始观测
//! - `state`：5 状态机当前状态
//! - `cooldown_until`：仅 `Cooldown / Recovering` 状态下有意义，过期后转 Recovering
//! - `banned_reason`：`Banned` 状态时填封号原因
//! - `consecutive_failures`：连续失败计数，成功时清零，用于指数退避 cooldown
//! - `window_total / window_success / window_started_at`：滚动统计窗口的原始 counter
//!
//! ## 兼容窗口（ADR-0007 §9）
//!
//! v0.5.0：`channel_groups.use_health_score = FALSE` 默认，本表写入但路由不消费
//! v0.6.0：默认 `TRUE`，旧 `channels.health` 字段标 `#[deprecated]`
//! v0.7.0：删除 `channels.health`，强制走 score
//!
//! [`ScoreCalculator`]: crate::repo::channel_health_score::ScoreCalculator
//! [`StateMachine`]: crate::repo::channel_health_score::StateMachine

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::ChannelId;
use parking_lot::RwLock;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

// ============================================================================
// 类型
// ============================================================================

/// 5 状态机的当前状态。
///
/// 转移规则详见 [ADR-0007 §2](../../../docs/architecture/decisions/ADR-0007-channel-health-score.md#2-状态机)
/// 与未来的 [`StateMachine`] 实装（M5.1 N1.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthState {
    Healthy,
    Degraded,
    Cooldown,
    Recovering,
    Banned,
}

impl HealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Cooldown => "cooldown",
            Self::Recovering => "recovering",
            Self::Banned => "banned",
        }
    }

    /// 路由是否完全跳过本 channel。
    pub fn skip_in_routing(self) -> bool {
        matches!(self, Self::Cooldown | Self::Banned)
    }

    /// 是否需要 probe 流量探测恢复。
    pub fn needs_probe(self) -> bool {
        matches!(self, Self::Recovering)
    }
}

impl FromStr for HealthState {
    type Err = DbError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "healthy" => Self::Healthy,
            "degraded" => Self::Degraded,
            "cooldown" => Self::Cooldown,
            "recovering" => Self::Recovering,
            "banned" => Self::Banned,
            other => {
                return Err(DbError::Constraint(format!(
                    "invalid channel health state: {other}"
                )));
            }
        })
    }
}

/// 一条 channel 当前的健康快照。
///
/// 这是路由层、Admin API、Dashboard UI 共享的入口结构。
#[derive(Debug, Clone)]
pub struct ChannelHealthScore {
    pub channel_id: ChannelId,

    // 4 维归一观测
    pub score: f64,
    pub success_rate: f64,
    pub latency_p99_ms: i32,
    pub banned_signal: f64,
    pub quota_remaining_norm: f64,
    pub consecutive_failures: i32,

    // 状态机
    pub state: HealthState,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub banned_reason: Option<String>,
    pub last_transition_at: DateTime<Utc>,

    // 滚动窗口
    pub window_total: i32,
    pub window_success: i32,
    pub window_started_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,
}

impl ChannelHealthScore {
    /// 创建乐观初值（新 channel 入池时）。
    pub fn fresh(channel_id: ChannelId) -> Self {
        let now = Utc::now();
        Self {
            channel_id,
            score: 1.0,
            success_rate: 1.0,
            latency_p99_ms: 0,
            banned_signal: 0.0,
            quota_remaining_norm: 1.0,
            consecutive_failures: 0,
            state: HealthState::Healthy,
            cooldown_until: None,
            banned_reason: None,
            last_transition_at: now,
            window_total: 0,
            window_success: 0,
            window_started_at: now,
            updated_at: now,
        }
    }
}

/// 一条请求结果的观测，喂给 [`ChannelHealthScoreRepo::record_outcome`]。
///
/// 字段都是可选的：调用方按当前已知的维度填，未知留 `None`。
#[derive(Debug, Clone, Default)]
pub struct OutcomeObservation {
    /// 请求是否成功（HTTP 2xx + 业务层通过）。`None` 表示该次观测不参与 success_rate 更新。
    pub success: Option<bool>,
    /// 单次延迟，毫秒。
    pub latency_ms: Option<i32>,
    /// 封号信号（来自 N1.3 `BannedPatternMatcher`）。`Some(reason)` 即命中。
    pub banned_signal: Option<String>,
    /// quota 剩余归一观测（来自 balance probe 或 quota header）。
    pub quota_remaining_norm: Option<f64>,
}

/// 完整覆盖式更新（用于 score recompute 后写回）。
#[derive(Debug, Clone)]
pub struct ScoreUpdate {
    pub score: f64,
    pub success_rate: f64,
    pub latency_p99_ms: i32,
    pub banned_signal: f64,
    pub quota_remaining_norm: f64,
    pub consecutive_failures: i32,
    pub state: HealthState,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub banned_reason: Option<String>,
    pub window_total: i32,
    pub window_success: i32,
    pub window_started_at: DateTime<Utc>,
}

// ============================================================================
// Repo trait
// ============================================================================

#[async_trait]
pub trait ChannelHealthScoreRepo: Send + Sync + 'static {
    /// 读取单 channel 的快照。返回 `None` 表示该 channel 尚无 score 行
    /// （新建未初始化或 migration 前数据）。
    async fn get(&self, channel_id: ChannelId) -> DbResult<Option<ChannelHealthScore>>;

    /// 批量读取多 channel 的快照（路由热路径用）。
    async fn get_many(
        &self,
        channel_ids: &[ChannelId],
    ) -> DbResult<HashMap<ChannelId, ChannelHealthScore>>;

    /// 给指定 channel 落一条观测。**仅更新 raw counter**（window_total /
    /// window_success / consecutive_failures），不触发评分计算或状态转移
    /// ——那是 N1.2 `ScoreCalculator` + `StateMachine` 的职责。
    ///
    /// 若 channel 尚无 score 行，自动用 [`ChannelHealthScore::fresh`] 落初值。
    async fn record_outcome(&self, channel_id: ChannelId, obs: &OutcomeObservation)
    -> DbResult<()>;

    /// 完整覆盖式写回（score recompute 后调用）。
    async fn apply_update(&self, channel_id: ChannelId, update: &ScoreUpdate) -> DbResult<()>;

    /// 重置滚动统计窗口（rotate window，N1.2 周期任务调）。
    async fn reset_window(&self, channel_id: ChannelId) -> DbResult<()>;

    /// 给 ChannelId 确保有一行，幂等。新建 channel 时调；存量 migration 已覆盖。
    async fn ensure_row(&self, channel_id: ChannelId) -> DbResult<()>;
}

// ============================================================================
// PgChannelHealthScoreRepo
// ============================================================================

pub struct PgChannelHealthScoreRepo {
    pool: PgPool,
}

impl PgChannelHealthScoreRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChannelHealthScoreRepo for PgChannelHealthScoreRepo {
    async fn get(&self, channel_id: ChannelId) -> DbResult<Option<ChannelHealthScore>> {
        let row = sqlx::query(
            "SELECT channel_id, score, success_rate, latency_p99_ms, banned_signal, \
                    quota_remaining_norm, consecutive_failures, state, cooldown_until, \
                    banned_reason, last_transition_at, window_total, window_success, \
                    window_started_at, updated_at \
             FROM channel_health_score WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_score).transpose()
    }

    async fn get_many(
        &self,
        channel_ids: &[ChannelId],
    ) -> DbResult<HashMap<ChannelId, ChannelHealthScore>> {
        if channel_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<Uuid> = channel_ids.iter().map(|id| *id.as_uuid()).collect();
        let rows = sqlx::query(
            "SELECT channel_id, score, success_rate, latency_p99_ms, banned_signal, \
                    quota_remaining_norm, consecutive_failures, state, cooldown_until, \
                    banned_reason, last_transition_at, window_total, window_success, \
                    window_started_at, updated_at \
             FROM channel_health_score WHERE channel_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let mut out = HashMap::with_capacity(rows.len());
        for row in rows {
            let score = row_to_score(row)?;
            out.insert(score.channel_id, score);
        }
        Ok(out)
    }

    async fn record_outcome(
        &self,
        channel_id: ChannelId,
        obs: &OutcomeObservation,
    ) -> DbResult<()> {
        self.ensure_row(channel_id).await?;

        // success/failure counter 用 SQL CASE 一次性原子更新；
        // banned_signal 和 quota_remaining_norm 是当前最新观测的快照覆盖。
        let success_inc: i32 = match obs.success {
            Some(true) => 1,
            _ => 0,
        };
        let total_inc: i32 = match obs.success {
            Some(_) => 1,
            None => 0,
        };
        let failure_reset = obs.success == Some(true);

        sqlx::query(
            "UPDATE channel_health_score SET \
                 window_total = window_total + $2, \
                 window_success = window_success + $3, \
                 consecutive_failures = CASE \
                     WHEN $4 THEN 0 \
                     WHEN $5 IS NOT NULL AND NOT $5 THEN consecutive_failures + 1 \
                     ELSE consecutive_failures END, \
                 banned_signal = COALESCE(\
                     CASE WHEN $6::TEXT IS NULL THEN NULL ELSE 1.0 END, banned_signal), \
                 quota_remaining_norm = COALESCE($7, quota_remaining_norm), \
                 updated_at = NOW() \
             WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .bind(total_inc)
        .bind(success_inc)
        .bind(failure_reset)
        .bind(obs.success)
        .bind(obs.banned_signal.as_deref())
        .bind(obs.quota_remaining_norm)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn apply_update(&self, channel_id: ChannelId, update: &ScoreUpdate) -> DbResult<()> {
        self.ensure_row(channel_id).await?;
        sqlx::query(
            "UPDATE channel_health_score SET \
                 score = $2, success_rate = $3, latency_p99_ms = $4, banned_signal = $5, \
                 quota_remaining_norm = $6, consecutive_failures = $7, \
                 state = $8, cooldown_until = $9, banned_reason = $10, \
                 last_transition_at = CASE WHEN state = $8 \
                     THEN last_transition_at ELSE NOW() END, \
                 window_total = $11, window_success = $12, window_started_at = $13, \
                 updated_at = NOW() \
             WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .bind(update.score)
        .bind(update.success_rate)
        .bind(update.latency_p99_ms)
        .bind(update.banned_signal)
        .bind(update.quota_remaining_norm)
        .bind(update.consecutive_failures)
        .bind(update.state.as_str())
        .bind(update.cooldown_until)
        .bind(update.banned_reason.as_deref())
        .bind(update.window_total)
        .bind(update.window_success)
        .bind(update.window_started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn reset_window(&self, channel_id: ChannelId) -> DbResult<()> {
        sqlx::query(
            "UPDATE channel_health_score SET \
                 window_total = 0, window_success = 0, window_started_at = NOW(), \
                 updated_at = NOW() \
             WHERE channel_id = $1",
        )
        .bind(channel_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_row(&self, channel_id: ChannelId) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO channel_health_score (channel_id) VALUES ($1) \
             ON CONFLICT (channel_id) DO NOTHING",
        )
        .bind(channel_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn row_to_score(row: sqlx::postgres::PgRow) -> DbResult<ChannelHealthScore> {
    let channel_id: Uuid = row.try_get("channel_id")?;
    let state_str: String = row.try_get("state")?;
    let state = HealthState::from_str(&state_str)?;
    Ok(ChannelHealthScore {
        channel_id: ChannelId::from(channel_id),
        score: row.try_get("score")?,
        success_rate: row.try_get("success_rate")?,
        latency_p99_ms: row.try_get("latency_p99_ms")?,
        banned_signal: row.try_get("banned_signal")?,
        quota_remaining_norm: row.try_get("quota_remaining_norm")?,
        consecutive_failures: row.try_get("consecutive_failures")?,
        state,
        cooldown_until: row.try_get("cooldown_until")?,
        banned_reason: row.try_get("banned_reason")?,
        last_transition_at: row.try_get("last_transition_at")?,
        window_total: row.try_get("window_total")?,
        window_success: row.try_get("window_success")?,
        window_started_at: row.try_get("window_started_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

// ============================================================================
// InMemoryChannelHealthScoreRepo（测试 + dev fallback）
// ============================================================================

#[derive(Default)]
pub struct InMemoryChannelHealthScoreRepo {
    inner: RwLock<HashMap<ChannelId, ChannelHealthScore>>,
}

impl InMemoryChannelHealthScoreRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ChannelHealthScoreRepo for InMemoryChannelHealthScoreRepo {
    async fn get(&self, channel_id: ChannelId) -> DbResult<Option<ChannelHealthScore>> {
        Ok(self.inner.read().get(&channel_id).cloned())
    }

    async fn get_many(
        &self,
        channel_ids: &[ChannelId],
    ) -> DbResult<HashMap<ChannelId, ChannelHealthScore>> {
        let inner = self.inner.read();
        Ok(channel_ids
            .iter()
            .filter_map(|id| inner.get(id).map(|s| (*id, s.clone())))
            .collect())
    }

    async fn record_outcome(
        &self,
        channel_id: ChannelId,
        obs: &OutcomeObservation,
    ) -> DbResult<()> {
        let mut inner = self.inner.write();
        let entry = inner
            .entry(channel_id)
            .or_insert_with(|| ChannelHealthScore::fresh(channel_id));
        if let Some(success) = obs.success {
            entry.window_total += 1;
            if success {
                entry.window_success += 1;
                entry.consecutive_failures = 0;
            } else {
                entry.consecutive_failures += 1;
            }
        }
        if obs.banned_signal.is_some() {
            entry.banned_signal = 1.0;
        }
        if let Some(quota) = obs.quota_remaining_norm {
            entry.quota_remaining_norm = quota.clamp(0.0, 1.0);
        }
        entry.updated_at = Utc::now();
        Ok(())
    }

    async fn apply_update(&self, channel_id: ChannelId, update: &ScoreUpdate) -> DbResult<()> {
        let mut inner = self.inner.write();
        let entry = inner
            .entry(channel_id)
            .or_insert_with(|| ChannelHealthScore::fresh(channel_id));
        let state_changed = entry.state != update.state;
        entry.score = update.score;
        entry.success_rate = update.success_rate;
        entry.latency_p99_ms = update.latency_p99_ms;
        entry.banned_signal = update.banned_signal;
        entry.quota_remaining_norm = update.quota_remaining_norm;
        entry.consecutive_failures = update.consecutive_failures;
        entry.state = update.state;
        entry.cooldown_until = update.cooldown_until;
        entry.banned_reason = update.banned_reason.clone();
        if state_changed {
            entry.last_transition_at = Utc::now();
        }
        entry.window_total = update.window_total;
        entry.window_success = update.window_success;
        entry.window_started_at = update.window_started_at;
        entry.updated_at = Utc::now();
        Ok(())
    }

    async fn reset_window(&self, channel_id: ChannelId) -> DbResult<()> {
        let mut inner = self.inner.write();
        if let Some(entry) = inner.get_mut(&channel_id) {
            entry.window_total = 0;
            entry.window_success = 0;
            entry.window_started_at = Utc::now();
            entry.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn ensure_row(&self, channel_id: ChannelId) -> DbResult<()> {
        self.inner
            .write()
            .entry(channel_id)
            .or_insert_with(|| ChannelHealthScore::fresh(channel_id));
        Ok(())
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn cid() -> ChannelId {
        ChannelId::from(Uuid::new_v4())
    }

    #[test]
    fn health_state_round_trip() {
        for state in [
            HealthState::Healthy,
            HealthState::Degraded,
            HealthState::Cooldown,
            HealthState::Recovering,
            HealthState::Banned,
        ] {
            assert_eq!(HealthState::from_str(state.as_str()).unwrap(), state);
        }
    }

    #[test]
    fn health_state_invalid_string_rejected() {
        assert!(HealthState::from_str("zombie").is_err());
        assert!(HealthState::from_str("").is_err());
    }

    #[test]
    fn skip_in_routing_matches_adr() {
        assert!(!HealthState::Healthy.skip_in_routing());
        assert!(!HealthState::Degraded.skip_in_routing());
        assert!(HealthState::Cooldown.skip_in_routing());
        assert!(HealthState::Banned.skip_in_routing());
        assert!(!HealthState::Recovering.skip_in_routing());
    }

    #[test]
    fn fresh_score_is_optimistic() {
        let s = ChannelHealthScore::fresh(cid());
        assert_eq!(s.score, 1.0);
        assert_eq!(s.success_rate, 1.0);
        assert_eq!(s.state, HealthState::Healthy);
        assert_eq!(s.consecutive_failures, 0);
        assert_eq!(s.window_total, 0);
    }

    #[tokio::test]
    async fn in_memory_ensure_row_idempotent() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();
        repo.ensure_row(id).await.unwrap();
        repo.ensure_row(id).await.unwrap();
        let score = repo.get(id).await.unwrap().unwrap();
        assert_eq!(score.state, HealthState::Healthy);
        assert_eq!(score.window_total, 0);
    }

    #[tokio::test]
    async fn in_memory_record_outcome_updates_counters() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();

        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let s = repo.get(id).await.unwrap().unwrap();
        assert_eq!(s.window_total, 3);
        assert_eq!(s.window_success, 1);
        assert_eq!(s.consecutive_failures, 2);
    }

    #[tokio::test]
    async fn in_memory_record_outcome_success_resets_failure_streak() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();

        for _ in 0..3 {
            repo.record_outcome(
                id,
                &OutcomeObservation {
                    success: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let s = repo.get(id).await.unwrap().unwrap();
        assert_eq!(s.consecutive_failures, 0);
        assert_eq!(s.window_total, 4);
        assert_eq!(s.window_success, 1);
    }

    #[tokio::test]
    async fn in_memory_record_outcome_banned_signal_sticky() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();
        repo.record_outcome(
            id,
            &OutcomeObservation {
                banned_signal: Some("account_deactivated".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let s = repo.get(id).await.unwrap().unwrap();
        assert_eq!(s.banned_signal, 1.0);
    }

    #[tokio::test]
    async fn in_memory_apply_update_changes_transition_timestamp() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();
        repo.ensure_row(id).await.unwrap();
        let before = repo.get(id).await.unwrap().unwrap().last_transition_at;

        // 同状态：last_transition_at 不变
        let update = ScoreUpdate {
            score: 0.95,
            success_rate: 0.95,
            latency_p99_ms: 200,
            banned_signal: 0.0,
            quota_remaining_norm: 1.0,
            consecutive_failures: 0,
            state: HealthState::Healthy,
            cooldown_until: None,
            banned_reason: None,
            window_total: 100,
            window_success: 95,
            window_started_at: Utc::now(),
        };
        repo.apply_update(id, &update).await.unwrap();
        let same_state = repo.get(id).await.unwrap().unwrap();
        assert_eq!(same_state.last_transition_at, before);

        // 状态变：last_transition_at 推进
        let update2 = ScoreUpdate {
            state: HealthState::Cooldown,
            cooldown_until: Some(Utc::now() + chrono::Duration::seconds(60)),
            ..update
        };
        repo.apply_update(id, &update2).await.unwrap();
        let new_state = repo.get(id).await.unwrap().unwrap();
        assert!(new_state.last_transition_at > before);
        assert_eq!(new_state.state, HealthState::Cooldown);
    }

    #[tokio::test]
    async fn in_memory_reset_window_zeros_counters() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let id = cid();
        for _ in 0..5 {
            repo.record_outcome(
                id,
                &OutcomeObservation {
                    success: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        repo.reset_window(id).await.unwrap();
        let s = repo.get(id).await.unwrap().unwrap();
        assert_eq!(s.window_total, 0);
        assert_eq!(s.window_success, 0);
    }

    #[tokio::test]
    async fn in_memory_get_many_returns_only_present() {
        let repo = InMemoryChannelHealthScoreRepo::new();
        let known = cid();
        let unknown = cid();
        repo.ensure_row(known).await.unwrap();

        let result = repo.get_many(&[known, unknown]).await.unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&known));
        assert!(!result.contains_key(&unknown));
    }
}
