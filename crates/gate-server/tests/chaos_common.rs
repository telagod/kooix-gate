//! Chaos test fixture skeleton — Phase 1（按 docs/backlog/chaos-testing.md）。
//!
//! 0.4.146（按 0.4.99 设计稿）：先把 fixture trait + 默认 NoopChaos 实装
//! 立起来，让真实 chaos cases（PG 拒绝连接 / Redis 停 / 上游 503 风暴 / pool
//! 耗尽 / toxiproxy latency 等）有统一注入点。
//!
//! Phase 2（v0.5.x）会加 toxiproxy + testcontainers 真实注入 ChaosInjector impl。

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

/// 注入故障的统一 trait。每个 chaos test 用一个 InjectorImpl 注入特定故障，
/// 然后用 with_latency / with_failure_rate 等方法配置。
pub trait ChaosInjector: Send + Sync {
    /// 给操作前注入延迟 ms。
    fn latency_ms(&self) -> u64 {
        0
    }

    /// 0..1 之间的失败率（操作前抛 chaos）。
    fn failure_rate(&self) -> f64 {
        0.0
    }

    /// 注入次数（已注入了多少次延迟 / 故障）。
    fn injected_count(&self) -> usize {
        0
    }
}

/// 默认实现：无任何注入。
#[derive(Debug, Default)]
pub struct NoopChaos;

impl ChaosInjector for NoopChaos {}

/// 计数器型 chaos —— 记录被询问 latency_ms 的次数（用于断言"未被注入"）。
#[derive(Debug, Default)]
pub struct ProbeChaos {
    pub asked: AtomicUsize,
}

impl ChaosInjector for ProbeChaos {
    fn latency_ms(&self) -> u64 {
        self.asked.fetch_add(1, Ordering::Relaxed);
        0
    }

    fn injected_count(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }
}

#[test]
fn noop_chaos_returns_no_injection() {
    let c = NoopChaos;
    assert_eq!(c.latency_ms(), 0);
    assert_eq!(c.failure_rate(), 0.0);
}

#[test]
fn probe_chaos_counts_calls() {
    let c = ProbeChaos::default();
    assert_eq!(c.injected_count(), 0);
    c.latency_ms();
    c.latency_ms();
    c.latency_ms();
    assert_eq!(c.injected_count(), 3);
}
