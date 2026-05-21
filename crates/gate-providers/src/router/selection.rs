//! Channel selection strategies — priority / weighted_random / round_robin / least_conn / least_latency。
//!
//! 这些都是纯函数：拿一组候选 channel + strategy + metrics，挑一个或排序整个候选列表返回。
//! 路由决策的入口是 `select_channel`（单选）或 `order_channels_by_strategy`（排序后用于 fallback）。

use super::metrics::{ChannelMetrics, InflightTracker};
use gate_core::id::ChannelId;
use gate_storage::ChannelBinding;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Strategy selection functions
// ============================================================================

/// 根据策略名选择 channel。
///
/// 保留用于未来可能需要「单选」的场景（如 health probe 等）。
#[allow(dead_code)]
pub(super) fn select_channel<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
) -> &'a ChannelBinding {
    match strategy {
        "weighted_random" => select_weighted_random(compatible),
        "round_robin" => select_round_robin(compatible, rr_counter),
        "least_conn" => select_least_conn(compatible, inflight),
        "least_latency" => select_least_latency(compatible, metrics),
        // "priority" + 未知 strategy → 取第一条（已按 priority ASC 排序）
        _ => compatible[0],
    }
}

/// 按 weight 做加权随机选择。
fn select_weighted_random<'a>(channels: &'a [&ChannelBinding]) -> &'a ChannelBinding {
    use rand::Rng;
    let total_weight: u32 = channels.iter().map(|c| c.weight.max(1) as u32).sum();
    let mut rng = rand::thread_rng();
    let mut roll = rng.gen_range(0..total_weight);
    for ch in channels {
        let w = ch.weight.max(1) as u32;
        if roll < w {
            return ch;
        }
        roll -= w;
    }
    // 安全兜底（浮点/溢出不可能，但 defensive）
    channels.last().unwrap()
}

/// 循环轮转：用 AtomicU64 取模。
fn select_round_robin<'a>(
    channels: &'a [&ChannelBinding],
    counter: &AtomicU64,
) -> &'a ChannelBinding {
    let idx = counter.fetch_add(1, Ordering::Relaxed) as usize % channels.len();
    channels[idx]
}

/// 选 inflight 最少的 channel；同分时取第一个（即 priority 最高的）。
#[allow(dead_code)]
fn select_least_conn<'a>(
    channels: &'a [&ChannelBinding],
    inflight: &InflightTracker,
) -> &'a ChannelBinding {
    channels
        .iter()
        .min_by_key(|ch| inflight.current(ch.channel.channel_id))
        .unwrap()
}

/// 选平均延迟最低的 channel；无延迟数据时 fallback 到第一条。
#[allow(dead_code)]
fn select_least_latency<'a>(
    channels: &'a [&ChannelBinding],
    metrics: Option<&ChannelMetrics>,
) -> &'a ChannelBinding {
    let Some(m) = metrics else {
        return channels[0];
    };
    channels
        .iter()
        .min_by_key(|ch| m.avg_latency(ch.channel.channel_id))
        .unwrap()
}

/// 按策略返回所有 compatible channels 的有序列表（首选在前）。
///
/// 与 `select_channel` 不同，这里返回全部候选而非单个。
/// 用于 rate limit fallback：首选被限速时依次尝试后续。
pub(super) fn order_channels_by_strategy<'a>(
    strategy: &str,
    compatible: &'a [&ChannelBinding],
    rr_counter: &AtomicU64,
    inflight: &InflightTracker,
    metrics: Option<&ChannelMetrics>,
    persistent_latencies: Option<&HashMap<ChannelId, u64>>,
) -> Vec<&'a ChannelBinding> {
    if compatible.len() <= 1 {
        return compatible.to_vec();
    }

    match strategy {
        "weighted_random" => {
            // 首选 = weighted random pick，其余按 priority 排
            let first = select_weighted_random(compatible);
            let mut rest: Vec<_> = compatible
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            rest.sort_by_key(|c| c.priority);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "round_robin" => {
            // 首选 = round_robin pick，其余按 priority 排
            let first = select_round_robin(compatible, rr_counter);
            let mut rest: Vec<_> = compatible
                .iter()
                .filter(|c| c.channel.channel_id != first.channel.channel_id)
                .copied()
                .collect();
            rest.sort_by_key(|c| c.priority);
            let mut result = vec![first];
            result.extend(rest);
            result
        }
        "least_conn" => {
            // 按 inflight 升序排
            let mut sorted: Vec<_> = compatible.to_vec();
            sorted.sort_by_key(|c| inflight.current(c.channel.channel_id));
            sorted
        }
        "least_latency" => {
            // 按延迟升序排
            let mut sorted: Vec<_> = compatible.to_vec();
            sorted.sort_by_key(|c| {
                persistent_latencies
                    .and_then(|latencies| latencies.get(&c.channel.channel_id).copied())
                    .or_else(|| metrics.map(|m| m.avg_latency(c.channel.channel_id)))
                    .unwrap_or(u64::MAX)
            });
            sorted
        }
        // "priority" + 未知 → 已按 priority ASC 排序的原始顺序
        _ => compatible.to_vec(),
    }
}
