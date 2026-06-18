//! Channel selection strategies — priority / weighted_random / round_robin / least_conn / least_latency。
//!
//! 这些都是纯函数：拿一组候选 channel + strategy + metrics，挑一个或排序整个候选列表返回。
//! 路由决策的入口是 `select_channel`（单选）或 `order_channels_by_strategy`（排序后用于 fallback）。
//!
//! ## ADR-0007 / M5.1 N1.4 — health-aware
//!
//! [`order_channels_by_strategy`] 接受一个可选 `health_view` 参数：
//!
//! - `None` → 当前 v0.4.x 兼容路径，5 策略行为完全不变（opt-out 默认）
//! - `Some(view)` → 5 策略统一消费 score：
//!   - `Cooldown / Banned` 跳过整条 channel
//!   - `weighted_random`: `effective_weight = weight × max(score, MIN_WEIGHT_FLOOR)`
//!   - `least_conn`: 同 inflight 取 score 高者
//!   - `least_latency`: `effective_latency = latency × (2 - score)`
//!   - `priority / round_robin`: 同优先级/同 RR 索引下按 score 二级排序

use super::metrics::{ChannelMetrics, InflightTracker};
use gate_core::id::ChannelId;
use gate_storage::{ChannelBinding, HealthState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// 低分 channel 也至少留 5% 探针流量做 score 恢复探测，
/// 避免低分 channel 永久饿死。ADR-0007 §5。
const MIN_WEIGHT_FLOOR: f64 = 0.05;

/// channel-id → (state, score) 视图，路由热路径用。
///
/// `None` 表示 group 没启 health-aware（兼容 v0.4.x 路径）。
pub type HealthView = HashMap<ChannelId, (HealthState, f64)>;

/// 是否完全跳过该 channel（Cooldown/Banned）。
fn is_routable(state: HealthState) -> bool {
    !state.skip_in_routing()
}

/// 拿 score；缺数据按"乐观假设"=1.0（与 channel_health_score 初值一致）。
fn score_of(health: Option<&HealthView>, ch: &ChannelBinding) -> (HealthState, f64) {
    health
        .and_then(|m| m.get(&ch.channel.channel_id).copied())
        .unwrap_or((HealthState::Healthy, 1.0))
}

/// 过滤掉 Cooldown/Banned；health=None 时透传全部。
fn filter_routable<'a>(
    compatible: &[&'a ChannelBinding],
    health: Option<&HealthView>,
) -> Vec<&'a ChannelBinding> {
    match health {
        None => compatible.to_vec(),
        Some(view) => compatible
            .iter()
            .filter(|ch| {
                view.get(&ch.channel.channel_id)
                    .map(|(state, _)| is_routable(*state))
                    .unwrap_or(true)
            })
            .copied()
            .collect(),
    }
}

// ============================================================================
// Strategy selection functions
// ============================================================================

/// 根据策略名选择 channel。
///
/// 保留用于未来可能需要「单选」的场景（如 health probe 等）。
/// `health=None` 时与 v0.4.x 完全一致；`Some(view)` 时按 ADR-0007 §5 5 策略统一接 score。
#[allow(dead_code)]
pub(super) fn select_channel<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
    health: Option<&HealthView>,
) -> Option<&'a ChannelBinding> {
    let routable = filter_routable(compatible, health);
    if routable.is_empty() {
        return None;
    }
    let refs: Vec<&ChannelBinding> = routable.to_vec();
    let picked = match strategy {
        "weighted_random" => select_weighted_random(&refs, health),
        "round_robin" => select_round_robin(&refs, rr_counter),
        "least_conn" => select_least_conn(&refs, inflight, health),
        "least_latency" => select_least_latency(&refs, metrics, health),
        // "priority" + 未知 strategy：默认已按 priority ASC 排序；
        // health=Some 时同 priority 按 score DESC 二级排序，取第一条
        _ => {
            if health.is_some() {
                let mut sorted: Vec<&ChannelBinding> = refs.clone();
                sorted.sort_by(|a, b| {
                    let (_, sa) = score_of(health, a);
                    let (_, sb) = score_of(health, b);
                    let by_prio = a.priority.cmp(&b.priority);
                    if by_prio != std::cmp::Ordering::Equal {
                        return by_prio;
                    }
                    sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                });
                sorted[0]
            } else {
                refs[0]
            }
        }
    };
    // Map back to the original lifetime
    Some(
        compatible
            .iter()
            .find(|c| c.channel.channel_id == picked.channel.channel_id)
            .copied()
            .unwrap(),
    )
}

/// 按 weight 做加权随机选择。`health=Some` 时按 ADR-0007 §5
/// `effective_weight = weight × max(score, MIN_WEIGHT_FLOOR)` 修正。
fn select_weighted_random<'a>(
    channels: &[&'a ChannelBinding],
    health: Option<&HealthView>,
) -> &'a ChannelBinding {
    use rand::Rng;
    // 权重统一放到 f64，方便接 score 修正
    let effective: Vec<(usize, f64)> = channels
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let base = ch.weight.max(1) as f64;
            let mult = match health {
                None => 1.0,
                Some(_) => {
                    let (_, score) = score_of(health, ch);
                    score.max(MIN_WEIGHT_FLOOR)
                }
            };
            (i, base * mult)
        })
        .collect();
    let total: f64 = effective.iter().map(|(_, w)| *w).sum();
    if total <= 0.0 {
        return channels.last().unwrap();
    }
    let mut rng = rand::thread_rng();
    let mut roll = rng.gen_range(0.0..total);
    for (i, w) in &effective {
        if roll < *w {
            return channels[*i];
        }
        roll -= *w;
    }
    channels.last().unwrap()
}

/// 循环轮转：用 AtomicU64 取模。
fn select_round_robin<'a>(
    channels: &[&'a ChannelBinding],
    counter: &AtomicU64,
) -> &'a ChannelBinding {
    let idx = counter.fetch_add(1, Ordering::Relaxed) as usize % channels.len();
    channels[idx]
}

/// 选 inflight 最少的 channel；同分时取第一个（即 priority 最高的）。
/// `health=Some` 时同 inflight 用 score DESC 二级排序（取健康者）。
#[allow(dead_code)]
fn select_least_conn<'a>(
    channels: &[&'a ChannelBinding],
    inflight: &InflightTracker,
    health: Option<&HealthView>,
) -> &'a ChannelBinding {
    channels
        .iter()
        .min_by(|a, b| {
            let ia = inflight.current(a.channel.channel_id);
            let ib = inflight.current(b.channel.channel_id);
            match ia.cmp(&ib) {
                std::cmp::Ordering::Equal if health.is_some() => {
                    // 同 inflight 取 score 高者
                    let (_, sa) = score_of(health, a);
                    let (_, sb) = score_of(health, b);
                    sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                }
                other => other,
            }
        })
        .unwrap()
}

/// 选平均延迟最低的 channel；无延迟数据时 fallback 到第一条。
/// `health=Some` 时按 `effective_latency = latency × (2 − score)` 修正。
#[allow(dead_code)]
fn select_least_latency<'a>(
    channels: &[&'a ChannelBinding],
    metrics: Option<&ChannelMetrics>,
    health: Option<&HealthView>,
) -> &'a ChannelBinding {
    let Some(m) = metrics else {
        return channels[0];
    };
    channels
        .iter()
        .min_by(|a, b| {
            let la = effective_latency(m.avg_latency(a.channel.channel_id), health, a);
            let lb = effective_latency(m.avg_latency(b.channel.channel_id), health, b);
            la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap()
}

/// `effective_latency = latency × (2 − score)` 修正。无 health view 时透传。
fn effective_latency(raw_ms: u64, health: Option<&HealthView>, ch: &ChannelBinding) -> f64 {
    let base = raw_ms as f64;
    let mult = match health {
        None => 1.0,
        Some(_) => {
            let (_, score) = score_of(health, ch);
            (2.0 - score).max(0.0)
        }
    };
    base * mult
}

/// 按策略返回所有 compatible channels 的有序列表（首选在前）。
///
/// 与 `select_channel` 不同，这里返回全部候选而非单个。
/// 用于 rate limit fallback：首选被限速时依次尝试后续。
///
/// `health=None` 时 v0.4.x 行为完全不变。`Some(view)`：
/// - `Cooldown / Banned` 整条 filter 掉，不进入有序列表
/// - 5 策略统一接 score（ADR-0007 §5）
pub(super) fn order_channels_by_strategy<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
    persistent_latencies: Option<&HashMap<ChannelId, u64>>,
    health: Option<&HealthView>,
) -> Vec<&'a ChannelBinding> {
    let routable = filter_routable(compatible, health);
    if routable.len() <= 1 {
        return routable;
    }

    match strategy {
        "weighted_random" => {
            // 首选 = weighted random pick（含 score 修正）；其余按 priority 排，
            // health=Some 时同 priority 二级按 score DESC。
            let first = select_weighted_random(&routable, health);
            let mut rest: Vec<_> = routable
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            sort_by_priority_then_score(&mut rest, health);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "round_robin" => {
            // 首选 = round_robin pick；其余按 priority + score 二级
            let first = select_round_robin(&routable, rr_counter);
            let mut rest: Vec<_> = routable
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            sort_by_priority_then_score(&mut rest, health);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "least_conn" => {
            // 按 inflight 升序；同 inflight + health=Some 时 score DESC 二级
            let mut sorted: Vec<_> = routable.clone();
            sorted.sort_by(|a, b| {
                let ia = inflight.current(a.channel.channel_id);
                let ib = inflight.current(b.channel.channel_id);
                match ia.cmp(&ib) {
                    std::cmp::Ordering::Equal if health.is_some() => {
                        let (_, sa) = score_of(health, a);
                        let (_, sb) = score_of(health, b);
                        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    other => other,
                }
            });
            sorted
        }
        "least_latency" => {
            // 按 effective_latency 升序；health=Some 应用 (2 - score) 乘子
            let mut sorted: Vec<_> = routable.clone();
            sorted.sort_by(|a, b| {
                let raw_a = persistent_latencies
                    .and_then(|l| l.get(&a.channel.channel_id).copied())
                    .or_else(|| metrics.map(|m| m.avg_latency(a.channel.channel_id)))
                    .unwrap_or(u64::MAX);
                let raw_b = persistent_latencies
                    .and_then(|l| l.get(&b.channel.channel_id).copied())
                    .or_else(|| metrics.map(|m| m.avg_latency(b.channel.channel_id)))
                    .unwrap_or(u64::MAX);
                let ea = effective_latency(raw_a, health, a);
                let eb = effective_latency(raw_b, health, b);
                ea.partial_cmp(&eb).unwrap_or(std::cmp::Ordering::Equal)
            });
            sorted
        }
        // "priority" + 未知：按 priority ASC；health=Some 时同 priority 二级按 score DESC
        _ => {
            let mut sorted: Vec<_> = routable.clone();
            sort_by_priority_then_score(&mut sorted, health);
            sorted
        }
    }
}

/// 按 priority ASC 主序排，health=Some 时同 priority 按 score DESC 二级排。
fn sort_by_priority_then_score(channels: &mut Vec<&ChannelBinding>, health: Option<&HealthView>) {
    channels.sort_by(|a, b| {
        let by_prio = a.priority.cmp(&b.priority);
        if by_prio != std::cmp::Ordering::Equal || health.is_none() {
            return by_prio;
        }
        let (_, sa) = score_of(health, a);
        let (_, sb) = score_of(health, b);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ============================================================================
// 单元测试：5 策略 × 5 状态矩阵
// ============================================================================

#[cfg(test)]
mod health_aware_tests {
    use super::*;
    use chrono::Utc;
    use gate_storage::{ChannelRecord, ChannelStatus};
    use uuid::Uuid;

    fn make_channel(code: &str, priority: i32, weight: i32) -> ChannelBinding {
        let id = ChannelId::from(Uuid::new_v4());
        let now = Utc::now();
        ChannelBinding {
            channel: ChannelRecord {
                channel_id: id,
                code: code.to_string(),
                name: code.to_string(),
                provider_type: "openai".to_string(),
                base_url: "https://example.invalid".to_string(),
                supported_models: vec!["gpt-4o".to_string()],
                status: ChannelStatus::Active.as_str().to_string(),
                health: "healthy".to_string(),
                timeout_ms: 30_000,
                max_retries: 3,
                rpm_limit: None,
                tpm_limit: None,
                tags: vec![],
                model_mapping: serde_json::Value::Null,
                balance: None,
                balance_updated_at: None,
                last_error: None,
                last_error_at: None,
                created_at: now,
                updated_at: now,
            },
            priority,
            weight,
            canary_percent_bps: None,
            model_filter: vec![],
            enabled: true,
        }
    }

    fn view_of<I: IntoIterator<Item = (ChannelId, HealthState, f64)>>(items: I) -> HealthView {
        items
            .into_iter()
            .map(|(id, st, sc)| (id, (st, sc)))
            .collect()
    }

    fn rr() -> AtomicU64 {
        AtomicU64::new(0)
    }

    // ---- filter_routable / is_routable ----

    #[test]
    fn skip_cooldown_and_banned_when_health_present() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        let c = make_channel("c", 1, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Cooldown, 0.2),
            (c.channel.channel_id, HealthState::Banned, 0.0),
        ]);
        let compatible = vec![&a, &b, &c];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            Some(&view),
        );
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);
    }

    #[test]
    fn no_health_view_preserves_v04_behaviour() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 2, 1);
        let compatible = vec![&a, &b];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            None,
        );
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);
        assert_eq!(ordered[1].channel.channel_id, b.channel.channel_id);
    }

    #[test]
    fn empty_routable_returns_empty() {
        let a = make_channel("a", 1, 1);
        let view = view_of([(a.channel.channel_id, HealthState::Banned, 0.0)]);
        let compatible = vec![&a];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            Some(&view),
        );
        assert!(ordered.is_empty());
    }

    // ---- priority 策略 + score 二级排序 ----

    #[test]
    fn priority_uses_score_for_tie_break() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 0.4),
            (b.channel.channel_id, HealthState::Healthy, 0.9),
        ]);
        let compatible = vec![&a, &b];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            Some(&view),
        );
        // 同 priority → score 高者优先
        assert_eq!(ordered[0].channel.channel_id, b.channel.channel_id);
        assert_eq!(ordered[1].channel.channel_id, a.channel.channel_id);
    }

    #[test]
    fn priority_higher_priority_beats_higher_score() {
        let a = make_channel("a", 1, 1); // higher priority (1 < 2)
        let b = make_channel("b", 2, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 0.5),
            (b.channel.channel_id, HealthState::Healthy, 1.0),
        ]);
        let compatible = vec![&a, &b];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            Some(&view),
        );
        assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);
    }

    // ---- weighted_random + score 修正 ----

    #[test]
    fn weighted_random_floor_applies_to_low_score() {
        let a = make_channel("a", 1, 100);
        let b = make_channel("b", 1, 100);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Recovering, 0.01),
        ]);
        let compatible = vec![&a, &b];
        // 多次抽样：低分 b 也应有少量出现（5% floor）
        let mut b_first = 0;
        for _ in 0..2000 {
            let ordered = order_channels_by_strategy(
                "weighted_random",
                &compatible,
                &rr(),
                &InflightTracker::new(),
                None,
                None,
                Some(&view),
            );
            if ordered[0].channel.channel_id == b.channel.channel_id {
                b_first += 1;
            }
        }
        // 期望 ≈ 100*0.05 / (100*1.0 + 100*0.05) = 5/105 ≈ 4.8%；样本量 2000 给点 buffer
        assert!(
            b_first > 30,
            "floor should give b at least some traffic; got {b_first}"
        );
        assert!(b_first < 250, "but not the majority; got {b_first}");
    }

    #[test]
    fn weighted_random_high_score_dominates() {
        let a = make_channel("a", 1, 100);
        let b = make_channel("b", 1, 100);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Degraded, 0.5),
        ]);
        let compatible = vec![&a, &b];
        let mut a_first = 0;
        for _ in 0..2000 {
            let ordered = order_channels_by_strategy(
                "weighted_random",
                &compatible,
                &rr(),
                &InflightTracker::new(),
                None,
                None,
                Some(&view),
            );
            if ordered[0].channel.channel_id == a.channel.channel_id {
                a_first += 1;
            }
        }
        // a:b weights = 100*1 : 100*0.5 = 2:1，期望 a 占 ~66%
        assert!(a_first > 1100, "a should dominate, got {a_first}");
        assert!(a_first < 1500, "but not all-or-nothing, got {a_first}");
    }

    // ---- round_robin ----

    #[test]
    fn round_robin_skips_cooldown() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Cooldown, 0.0),
        ]);
        let compatible = vec![&a, &b];
        let rr_c = rr();
        // 连续 4 次，全部应该是 a（b 被 filter 掉了）
        for _ in 0..4 {
            let ordered = order_channels_by_strategy(
                "round_robin",
                &compatible,
                &rr_c,
                &InflightTracker::new(),
                None,
                None,
                Some(&view),
            );
            assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);
        }
    }

    // ---- least_conn ----

    #[test]
    fn least_conn_score_breaks_inflight_tie() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 0.3),
            (b.channel.channel_id, HealthState::Healthy, 0.9),
        ]);
        let compatible = vec![&a, &b];
        let inflight = InflightTracker::new();
        // 二者 inflight 都是 0 → score 高的 b 优先
        let ordered = order_channels_by_strategy(
            "least_conn",
            &compatible,
            &rr(),
            &inflight,
            None,
            None,
            Some(&view),
        );
        assert_eq!(ordered[0].channel.channel_id, b.channel.channel_id);
    }

    // ---- least_latency ----

    #[test]
    fn least_latency_score_inflates_low_score_channels() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        // 假装提供持久化 latency: a=500ms, b=100ms。
        // 不带 score：b 胜（100 < 500）
        // 带 score: a 健康 score=1.0 → effective_latency = 500*(2-1) = 500
        //          b 低分 score=0.2 → effective_latency = 100*(2-0.2) = 180
        // b 仍胜，但差距收窄。换一个更极端：a=200, b=100，b score=0.05
        // a effective = 200*1 = 200；b effective = 100*1.95 = 195 → b 仅微胜
        // 再极端 a=200, b=100, b score=0.0 → 100*2 = 200 → 同分；
        // a=200, b=120, b score=0.0 → 120*2 = 240 > 200 → a 胜
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Recovering, 0.0),
        ]);
        let compatible = vec![&a, &b];
        let latencies: HashMap<ChannelId, u64> =
            [(a.channel.channel_id, 200), (b.channel.channel_id, 120)]
                .into_iter()
                .collect();
        let ordered = order_channels_by_strategy(
            "least_latency",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            Some(&latencies),
            Some(&view),
        );
        // 带 health：a (raw 200, score 1, eff 200) vs b (raw 120, score 0, eff 240)
        // → a 胜
        assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);

        // 不带 health：b 胜（120 < 200）
        let ordered_no_health = order_channels_by_strategy(
            "least_latency",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            Some(&latencies),
            None,
        );
        assert_eq!(
            ordered_no_health[0].channel.channel_id,
            b.channel.channel_id
        );
    }

    // ---- 5 状态全覆盖 ----

    #[test]
    fn all_states_routing_classification() {
        let a = make_channel("a", 1, 1);
        let b = make_channel("b", 1, 1);
        let c = make_channel("c", 1, 1);
        let d = make_channel("d", 1, 1);
        let e = make_channel("e", 1, 1);
        let view = view_of([
            (a.channel.channel_id, HealthState::Healthy, 1.0),
            (b.channel.channel_id, HealthState::Degraded, 0.5),
            (c.channel.channel_id, HealthState::Cooldown, 0.1),
            (d.channel.channel_id, HealthState::Recovering, 0.6),
            (e.channel.channel_id, HealthState::Banned, 0.0),
        ]);
        let compatible = vec![&a, &b, &c, &d, &e];
        let ordered = order_channels_by_strategy(
            "priority",
            &compatible,
            &rr(),
            &InflightTracker::new(),
            None,
            None,
            Some(&view),
        );
        // 应剩 3 条：Healthy + Degraded + Recovering；Cooldown + Banned 跳过
        assert_eq!(ordered.len(), 3);
        let ids: Vec<ChannelId> = ordered.iter().map(|c| c.channel.channel_id).collect();
        assert!(ids.contains(&a.channel.channel_id));
        assert!(ids.contains(&b.channel.channel_id));
        assert!(ids.contains(&d.channel.channel_id));
        assert!(!ids.contains(&c.channel.channel_id));
        assert!(!ids.contains(&e.channel.channel_id));
        // score 降序排：Healthy(1.0) > Recovering(0.6) > Degraded(0.5)
        assert_eq!(ordered[0].channel.channel_id, a.channel.channel_id);
        assert_eq!(ordered[1].channel.channel_id, d.channel.channel_id);
        assert_eq!(ordered[2].channel.channel_id, b.channel.channel_id);
    }
}
