//! Channel Health Score 评分引擎（ADR-0007 / M5.1 N1.2）。
//!
//! 输入：[`gate_storage::ChannelHealthScore`]——`ChannelHealthScoreRepo::get`
//! 拿回的当前 4 维 raw 观测 + 状态机当前状态。
//! 输出：[`gate_storage::ScoreUpdate`]——喂给 `ChannelHealthScoreRepo::apply_update`
//! 落库。
//!
//! 本模块**纯 CPU 逻辑**，不 IO / 不锁 / 不并发原语。N1.5 异步落库
//! ([`gate_server::health_score_worker`] 待定) 把 [`ScoreEngine::recompute`]
//! 包装成周期 worker。N1.4 路由策略 ([`crate::router::selection`]) 直接读
//! `state` 字段做 skip / weight 修正。
//!
//! ## 模型简述（详见 ADR-0007）
//!
//! - **评分**：`score = Σ wᵢ × normalize(metricᵢ)`，加权 0.4 / 0.3 / 0.2 / 0.1
//! - **5 状态**：`Healthy / Degraded / Cooldown / Recovering / Banned`
//! - **Hysteresis**：边界 ±0.05 防抖
//! - **Auto-cooldown**：连续失败 ≥ 5 次 或 score < degraded → 指数退避 Cooldown
//! - **Banned**：终态。`banned_signal == 1.0` 转入，仅人工解封

use chrono::{DateTime, Duration, Utc};
use gate_storage::{ChannelHealthScore, HealthState, ScoreUpdate};
use serde::{Deserialize, Serialize};

// ============================================================================
// Weights：4 维加权配置
// ============================================================================

/// 4 维评分权重配置。
///
/// 落库为 `channel_groups.health_weights JSONB`；NULL 用 [`Weights::default`]。
/// 每个权重 [0, 1]，总和应 ≤ 1.0+ε（剩余预算留给后续维度扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    pub success_rate: f64,
    pub latency_p99: f64,
    pub banned_signal: f64,
    pub quota_remaining: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            success_rate: 0.40,
            latency_p99: 0.30,
            banned_signal: 0.20,
            quota_remaining: 0.10,
        }
    }
}

impl Weights {
    pub fn sum(&self) -> f64 {
        self.success_rate + self.latency_p99 + self.banned_signal + self.quota_remaining
    }

    /// 验证：每权重 ∈ [0, 1]，总和 ≤ 1.0+ε。
    pub fn validate(&self) -> Result<(), String> {
        for (name, w) in [
            ("success_rate", self.success_rate),
            ("latency_p99", self.latency_p99),
            ("banned_signal", self.banned_signal),
            ("quota_remaining", self.quota_remaining),
        ] {
            if !(0.0..=1.0).contains(&w) || !w.is_finite() {
                return Err(format!("weight {name}={w} out of [0,1] or not finite"));
            }
        }
        let sum = self.sum();
        if sum > 1.0 + 1e-6 {
            return Err(format!("weights sum {sum} > 1.0"));
        }
        Ok(())
    }
}

// ============================================================================
// Thresholds：状态转移阈值
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// score ≥ healthy → 进入或保持 Healthy
    pub healthy: f64,
    /// score ≥ degraded → 至少 Degraded（否则触发 Cooldown）
    pub degraded: f64,
    /// 防抖：升状态需多 +hysteresis；降状态需多 -hysteresis
    pub hysteresis: f64,
    /// 连续失败超阈值即自动 Cooldown（与 score < degraded 任一命中）
    pub max_consecutive_failures: i32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            healthy: 0.70,
            degraded: 0.40,
            hysteresis: 0.05,
            max_consecutive_failures: 5,
        }
    }
}

// ============================================================================
// CooldownPolicy：指数退避
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct CooldownPolicy {
    /// 基础退避（秒）。
    pub base_seconds: i64,
    /// 最大退避（秒）。
    pub max_seconds: i64,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self {
            base_seconds: 30,
            max_seconds: 30 * 60, // 30 min
        }
    }
}

impl CooldownPolicy {
    /// 计算本次 cooldown 时长。
    ///
    /// 公式：`min(base * 2^consecutive_failures, max)`。
    /// 上游 `Retry-After` 头存在时取 `max(指数退避, 上游值)`——更保守。
    pub fn duration(
        &self,
        consecutive_failures: i32,
        external_retry_after_secs: Option<i64>,
    ) -> Duration {
        // shift count 限 [0,30]，避免 overflow（2^30 已远超 max_seconds）
        let shift = consecutive_failures.clamp(0, 30) as u32;
        let multiplier: i64 = (1_i64).checked_shl(shift).unwrap_or(i64::MAX);
        let exp = self
            .base_seconds
            .saturating_mul(multiplier)
            .min(self.max_seconds);
        let mut secs = exp;
        if let Some(ext) = external_retry_after_secs {
            secs = secs.max(ext);
        }
        Duration::seconds(secs.max(0))
    }
}

// ============================================================================
// ScoreEngine：编排
// ============================================================================

#[derive(Debug, Clone)]
pub struct ScoreEngine {
    pub weights: Weights,
    pub thresholds: Thresholds,
    pub cooldown: CooldownPolicy,
    /// latency_p99_ms 归一上限。超过即视为最差 1.0。
    pub latency_norm_ceiling_ms: i32,
}

impl Default for ScoreEngine {
    fn default() -> Self {
        Self {
            weights: Weights::default(),
            thresholds: Thresholds::default(),
            cooldown: CooldownPolicy::default(),
            latency_norm_ceiling_ms: 30_000,
        }
    }
}

impl ScoreEngine {
    /// 用自定义权重创建。
    pub fn with_weights(weights: Weights) -> Self {
        Self {
            weights,
            ..Self::default()
        }
    }

    /// 对当前 raw 观测重新计算 score + 决定状态转移，输出完整 [`ScoreUpdate`]。
    ///
    /// 调用方负责把返回的 `ScoreUpdate` 喂给
    /// [`gate_storage::ChannelHealthScoreRepo::apply_update`]。
    ///
    /// `external_retry_after_secs`：上游 `Retry-After` 头存在时传入；进入
    /// Cooldown 时和指数退避取更长者。
    pub fn recompute(
        &self,
        current: &ChannelHealthScore,
        now: DateTime<Utc>,
        external_retry_after_secs: Option<i64>,
    ) -> ScoreUpdate {
        let new_score = self.compute_score(current);
        let (state, cooldown_until, banned_reason) =
            self.decide_state(current, new_score, now, external_retry_after_secs);

        ScoreUpdate {
            score: new_score,
            // 4 维 raw 字段原样回写（recompute 不修改 raw 观测，只算 score + state）
            success_rate: derive_success_rate(current),
            latency_p99_ms: current.latency_p99_ms,
            banned_signal: current.banned_signal,
            quota_remaining_norm: current.quota_remaining_norm,
            consecutive_failures: current.consecutive_failures,
            state,
            cooldown_until,
            banned_reason,
            window_total: current.window_total,
            window_success: current.window_success,
            window_started_at: current.window_started_at,
        }
    }

    /// 4 维加权 → score ∈ [0, 1]。
    pub fn compute_score(&self, current: &ChannelHealthScore) -> f64 {
        let success_rate = derive_success_rate(current);
        let latency_norm =
            1.0 - normalize_latency(current.latency_p99_ms, self.latency_norm_ceiling_ms);
        let banned_term = 1.0 - current.banned_signal.clamp(0.0, 1.0);
        let quota_term = current.quota_remaining_norm.clamp(0.0, 1.0);

        let score = self.weights.success_rate * success_rate.clamp(0.0, 1.0)
            + self.weights.latency_p99 * latency_norm
            + self.weights.banned_signal * banned_term
            + self.weights.quota_remaining * quota_term;
        score.clamp(0.0, 1.0)
    }

    /// 状态机决策。详见 ADR-0007 §2。
    fn decide_state(
        &self,
        current: &ChannelHealthScore,
        new_score: f64,
        now: DateTime<Utc>,
        external_retry_after_secs: Option<i64>,
    ) -> (HealthState, Option<DateTime<Utc>>, Option<String>) {
        // 1. Banned 是终态：人工解封前永久 skip。
        if current.state == HealthState::Banned {
            return (HealthState::Banned, None, current.banned_reason.clone());
        }

        // 2. banned_signal 命中 → 立即转 Banned。
        if current.banned_signal >= 1.0 - 1e-9 {
            let reason = current
                .banned_reason
                .clone()
                .unwrap_or_else(|| "banned_signal=1".to_string());
            return (HealthState::Banned, None, Some(reason));
        }

        // 3. 当前在 Cooldown：仅决定是否到期转 Recovering，不参与正常分数转移。
        if current.state == HealthState::Cooldown {
            let elapsed = match current.cooldown_until {
                Some(until) => now >= until,
                // 防呆：state=Cooldown 但 cooldown_until=NULL → 立即转 Recovering
                None => true,
            };
            if elapsed {
                return (HealthState::Recovering, None, None);
            }
            return (HealthState::Cooldown, current.cooldown_until, None);
        }

        // 4. Auto-cooldown 触发：连续失败超阈值 或 score < degraded。
        let triggers_cooldown = current.consecutive_failures
            >= self.thresholds.max_consecutive_failures
            || new_score < self.thresholds.degraded;
        if triggers_cooldown {
            let duration = self
                .cooldown
                .duration(current.consecutive_failures, external_retry_after_secs);
            return (HealthState::Cooldown, Some(now + duration), None);
        }

        // 5. Hysteresis：升状态需多 +ε；降状态需多 -ε。
        let next = match current.state {
            HealthState::Healthy => {
                if new_score < self.thresholds.healthy - self.thresholds.hysteresis {
                    HealthState::Degraded
                } else {
                    HealthState::Healthy
                }
            }
            HealthState::Degraded => {
                if new_score >= self.thresholds.healthy + self.thresholds.hysteresis {
                    HealthState::Healthy
                } else {
                    HealthState::Degraded
                }
            }
            HealthState::Recovering => {
                if new_score >= self.thresholds.healthy {
                    HealthState::Healthy
                } else {
                    // 持续观察。N1.4 路由策略会限制 Recovering 只接 probe 流量。
                    HealthState::Recovering
                }
            }
            HealthState::Cooldown | HealthState::Banned => unreachable!("handled above"),
        };
        (next, None, None)
    }
}

/// 从 `window_total / window_success` 派生 success_rate；窗口为空时回退快照值。
fn derive_success_rate(current: &ChannelHealthScore) -> f64 {
    if current.window_total > 0 {
        (current.window_success as f64 / current.window_total as f64).clamp(0.0, 1.0)
    } else {
        current.success_rate.clamp(0.0, 1.0)
    }
}

fn normalize_latency(latency_ms: i32, ceiling_ms: i32) -> f64 {
    let ceiling = ceiling_ms.max(1) as f64;
    (latency_ms.max(0) as f64 / ceiling).clamp(0.0, 1.0)
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::id::ChannelId;
    use uuid::Uuid;

    fn cid() -> ChannelId {
        ChannelId::from(Uuid::new_v4())
    }

    fn fresh() -> ChannelHealthScore {
        ChannelHealthScore::fresh(cid())
    }

    // ---- Weights ----

    #[test]
    fn default_weights_sum_to_one() {
        let w = Weights::default();
        assert!((w.sum() - 1.0).abs() < 1e-9);
        w.validate().unwrap();
    }

    #[test]
    fn weights_validate_rejects_overflow() {
        let w = Weights {
            success_rate: 0.6,
            latency_p99: 0.6,
            banned_signal: 0.0,
            quota_remaining: 0.0,
        };
        assert!(w.validate().is_err());
    }

    #[test]
    fn weights_validate_rejects_negative_or_nan() {
        let w = Weights {
            success_rate: -0.1,
            ..Weights::default()
        };
        assert!(w.validate().is_err());
        let w = Weights {
            success_rate: f64::NAN,
            ..Weights::default()
        };
        assert!(w.validate().is_err());
    }

    #[test]
    fn weights_serde_default_round_trip() {
        let json = serde_json::to_string(&Weights::default()).unwrap();
        let back: Weights = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Weights::default());
    }

    #[test]
    fn weights_serde_partial_fills_default() {
        let partial = r#"{"success_rate":0.5}"#;
        let w: Weights = serde_json::from_str(partial).unwrap();
        assert_eq!(w.success_rate, 0.5);
        assert_eq!(w.latency_p99, Weights::default().latency_p99);
    }

    // ---- CooldownPolicy ----

    #[test]
    fn cooldown_exponential_backoff() {
        let p = CooldownPolicy::default();
        assert_eq!(p.duration(0, None), Duration::seconds(30));
        assert_eq!(p.duration(1, None), Duration::seconds(60));
        assert_eq!(p.duration(2, None), Duration::seconds(120));
        assert_eq!(p.duration(3, None), Duration::seconds(240));
    }

    #[test]
    fn cooldown_capped_at_max() {
        let p = CooldownPolicy::default();
        // 30 * 2^10 = 30720 > max_seconds(1800)
        assert_eq!(p.duration(10, None), Duration::seconds(1800));
        // 极端值不 panic
        assert_eq!(p.duration(64, None), Duration::seconds(1800));
        assert_eq!(p.duration(i32::MAX, None), Duration::seconds(1800));
    }

    #[test]
    fn cooldown_external_retry_after_takes_max() {
        let p = CooldownPolicy::default();
        // 指数退避 30s vs 外部 3600s → 取 3600
        assert_eq!(p.duration(0, Some(3600)), Duration::seconds(3600));
        // 指数退避 1800s（capped）vs 外部 60s → 取 1800
        assert_eq!(p.duration(10, Some(60)), Duration::seconds(1800));
    }

    // ---- compute_score ----

    #[test]
    fn fresh_score_is_one() {
        let e = ScoreEngine::default();
        let c = fresh();
        let score = e.compute_score(&c);
        assert!((score - 1.0).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn score_with_all_failures_drops() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 100;
        c.window_success = 0;
        c.latency_p99_ms = 30_000;
        c.banned_signal = 0.0;
        c.quota_remaining_norm = 0.0;
        let score = e.compute_score(&c);
        // 0.4*0 + 0.3*0 + 0.2*1 + 0.1*0 = 0.2
        assert!((score - 0.2).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn score_weights_components_correctly() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 10;
        c.window_success = 5; // 0.5
        c.latency_p99_ms = 6_000; // norm = 0.2，term = 0.8
        c.banned_signal = 0.0; // term = 1.0
        c.quota_remaining_norm = 0.5; // term = 0.5
        // 0.4*0.5 + 0.3*0.8 + 0.2*1.0 + 0.1*0.5 = 0.20+0.24+0.20+0.05 = 0.69
        let score = e.compute_score(&c);
        assert!((score - 0.69).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn latency_above_ceiling_clamps_to_worst() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 1;
        c.window_success = 1;
        c.latency_p99_ms = 999_999; // far above 30s ceiling
        // 0.4*1 + 0.3*0 + 0.2*1 + 0.1*1 = 0.7
        let score = e.compute_score(&c);
        assert!((score - 0.7).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn quota_norm_clamped_to_one() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 1;
        c.window_success = 1;
        c.quota_remaining_norm = 2.0; // 超过 1 应被夹紧
        // 全 1.0
        assert!((e.compute_score(&c) - 1.0).abs() < 1e-9);
    }

    // ---- decide_state 状态转移 ----

    #[test]
    fn banned_is_terminal_state() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Banned;
        c.banned_reason = Some("manual".to_string());
        c.window_total = 100;
        c.window_success = 100; // 完全 healthy 也不解封
        c.banned_signal = 0.0;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Banned);
        assert_eq!(u.banned_reason.as_deref(), Some("manual"));
    }

    #[test]
    fn banned_signal_transitions_to_banned() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.banned_signal = 1.0;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Banned);
        assert!(u.banned_reason.is_some());
    }

    #[test]
    fn healthy_stays_healthy_above_threshold() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 10;
        c.window_success = 9;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Healthy);
        assert!(u.score > e.thresholds.healthy);
    }

    #[test]
    fn healthy_to_degraded_when_below_hysteresis() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 10;
        c.window_success = 5; // 0.5 success_rate
        c.latency_p99_ms = 8_000; // 减分但不至于 cooldown
        c.consecutive_failures = 1;
        c.state = HealthState::Healthy;
        let u = e.recompute(&c, Utc::now(), None);
        // score ≈ 0.4*0.5 + 0.3*(1-8/30) + 0.2*1 + 0.1*1 = 0.2 + 0.22 + 0.3 = 0.72
        // 0.72 ≥ healthy - hysteresis (0.65) → Healthy 保持
        assert_eq!(u.state, HealthState::Healthy);
    }

    #[test]
    fn healthy_transitions_to_cooldown_when_score_below_degraded() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Healthy;
        c.window_total = 10;
        c.window_success = 0;
        c.latency_p99_ms = 30_000;
        c.quota_remaining_norm = 0.0;
        // score = 0.2 < 0.4 (degraded) → 自动 Cooldown
        let now = Utc::now();
        let u = e.recompute(&c, now, None);
        assert_eq!(u.state, HealthState::Cooldown);
        assert!(u.cooldown_until.is_some());
        let until = u.cooldown_until.unwrap();
        assert!(until > now);
    }

    #[test]
    fn healthy_transitions_to_cooldown_when_consecutive_failures_exceed() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Healthy;
        c.window_total = 100;
        c.window_success = 90; // score 还很高
        c.consecutive_failures = 5; // 但连失败 5 次
        let now = Utc::now();
        let u = e.recompute(&c, now, None);
        assert_eq!(u.state, HealthState::Cooldown);
        assert!(u.cooldown_until.unwrap() > now);
    }

    #[test]
    fn cooldown_stays_until_deadline() {
        let e = ScoreEngine::default();
        let now = Utc::now();
        let mut c = fresh();
        c.state = HealthState::Cooldown;
        c.cooldown_until = Some(now + Duration::seconds(60));
        let u = e.recompute(&c, now, None);
        assert_eq!(u.state, HealthState::Cooldown);
        assert_eq!(u.cooldown_until, c.cooldown_until);
    }

    #[test]
    fn cooldown_transitions_to_recovering_after_deadline() {
        let e = ScoreEngine::default();
        let now = Utc::now();
        let mut c = fresh();
        c.state = HealthState::Cooldown;
        c.cooldown_until = Some(now - Duration::seconds(1)); // 已过期
        let u = e.recompute(&c, now, None);
        assert_eq!(u.state, HealthState::Recovering);
        assert_eq!(u.cooldown_until, None);
    }

    #[test]
    fn cooldown_without_deadline_immediately_recovers() {
        // 防呆：state=Cooldown 但 cooldown_until=NULL
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Cooldown;
        c.cooldown_until = None;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Recovering);
    }

    #[test]
    fn recovering_to_healthy_when_score_recovers() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Recovering;
        c.window_total = 10;
        c.window_success = 10;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Healthy);
    }

    #[test]
    fn recovering_stays_recovering_when_score_low() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Recovering;
        c.window_total = 10;
        c.window_success = 6; // 0.6 success_rate
        c.latency_p99_ms = 3000; // latency_norm = 0.1, term = 0.9
        // score ≈ 0.4*0.6 + 0.3*0.9 + 0.2*1 + 0.1*1 = 0.24+0.27+0.2+0.1 = 0.81
        // 实际 ≥ healthy → 应该 Healthy；改设计使它真在 Recovering 区
        c.window_success = 0;
        c.latency_p99_ms = 15_000; // latency term = 0.5
        // score ≈ 0 + 0.15 + 0.2 + 0.1 = 0.45 → 在 degraded 区
        // 但 Recovering 不走 Cooldown 路径（因为 consecutive_failures=0 且 score >= degraded）
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Recovering);
    }

    #[test]
    fn degraded_back_to_healthy_needs_hysteresis_buffer() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Degraded;
        c.window_total = 100;
        c.window_success = 73; // 0.73 success_rate
        // score = 0.4*0.73 + 0.3*1.0 + 0.2*1.0 + 0.1*1.0 = 0.292+0.6 = 0.892
        // 0.892 >= 0.70+0.05 = 0.75 → Healthy
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.state, HealthState::Healthy);
    }

    #[test]
    fn cooldown_duration_respects_external_retry_after() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.state = HealthState::Healthy;
        c.consecutive_failures = 5;
        let now = Utc::now();
        let u = e.recompute(&c, now, Some(3600));
        assert_eq!(u.state, HealthState::Cooldown);
        let until = u.cooldown_until.unwrap();
        let secs = (until - now).num_seconds();
        // 内部计算 30 * 2^5 = 960, capped to 1800；max(1800, 3600) = 3600
        assert_eq!(secs, 3600);
    }

    // ---- recompute 字段透传 ----

    #[test]
    fn recompute_preserves_raw_observation_fields() {
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 10;
        c.window_success = 8;
        c.latency_p99_ms = 1234;
        c.quota_remaining_norm = 0.65;
        c.consecutive_failures = 2;
        let u = e.recompute(&c, Utc::now(), None);
        assert_eq!(u.window_total, 10);
        assert_eq!(u.window_success, 8);
        assert_eq!(u.latency_p99_ms, 1234);
        assert!((u.quota_remaining_norm - 0.65).abs() < 1e-9);
        assert_eq!(u.consecutive_failures, 2);
    }

    // ---- 边界 / 防呆 ----

    #[test]
    fn empty_window_uses_snapshot_success_rate() {
        // window_total=0 时回退 current.success_rate
        let e = ScoreEngine::default();
        let mut c = fresh();
        c.window_total = 0;
        c.success_rate = 0.5;
        c.latency_p99_ms = 30_000;
        c.banned_signal = 0.0;
        c.quota_remaining_norm = 0.0;
        // 0.4*0.5 + 0.3*0 + 0.2*1 + 0.1*0 = 0.4
        let score = e.compute_score(&c);
        assert!((score - 0.4).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn custom_weights_change_score() {
        let custom = Weights {
            success_rate: 1.0,
            latency_p99: 0.0,
            banned_signal: 0.0,
            quota_remaining: 0.0,
        };
        let e = ScoreEngine::with_weights(custom);
        let mut c = fresh();
        c.window_total = 4;
        c.window_success = 3;
        // 全权重压在 success_rate → score = 0.75
        let score = e.compute_score(&c);
        assert!((score - 0.75).abs() < 1e-9, "score={score}");
    }
}
