//! Toxiproxy 注入器骨架（Phase 2 准备，按 docs/chaos-testing.md）。
//!
//! 0.4.147（按 0.4.146 接口）：实装 ToxiproxyInjector + with_latency helper，
//! 但**不真启 toxiproxy 容器**（依赖 testcontainers / docker，留 v0.5.x）。
//! 本步先定 builder API + 与 ChaosInjector trait 集成形状，让真实 chaos test
//! 能直接套上。

#![allow(dead_code)]

mod chaos_common;

use chaos_common::ChaosInjector;
use std::sync::atomic::{AtomicU64, Ordering};

/// Toxiproxy-style 注入器（v0：仅 latency + failure_rate，不真启 toxiproxy 容器）。
pub struct ToxiproxyInjector {
    latency: AtomicU64,
    failure_bps: AtomicU64, // basis points (0..10000)
    counter: std::sync::atomic::AtomicUsize,
}

impl ToxiproxyInjector {
    pub fn new() -> Self {
        Self {
            latency: AtomicU64::new(0),
            failure_bps: AtomicU64::new(0),
            counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn with_latency(self, ms: u64) -> Self {
        self.latency.store(ms, Ordering::Relaxed);
        self
    }

    /// 失败率：0..=10000 basis points（10000=100%）
    pub fn with_failure_bps(self, bps: u64) -> Self {
        self.failure_bps.store(bps.min(10_000), Ordering::Relaxed);
        self
    }
}

impl Default for ToxiproxyInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl ChaosInjector for ToxiproxyInjector {
    fn latency_ms(&self) -> u64 {
        self.counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.latency.load(Ordering::Relaxed)
    }

    fn failure_rate(&self) -> f64 {
        (self.failure_bps.load(Ordering::Relaxed) as f64) / 10_000.0
    }

    fn injected_count(&self) -> usize {
        self.counter.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[test]
fn toxiproxy_builder_chains_latency_and_failure() {
    let inj = ToxiproxyInjector::new()
        .with_latency(500)
        .with_failure_bps(2_500);
    assert_eq!(inj.latency_ms(), 500);
    assert!((inj.failure_rate() - 0.25).abs() < 0.0001);
    assert_eq!(inj.injected_count(), 1);
}

#[test]
fn toxiproxy_failure_bps_clamped_to_10000() {
    let inj = ToxiproxyInjector::new().with_failure_bps(99_999);
    assert!((inj.failure_rate() - 1.0).abs() < 0.0001);
}

#[test]
fn toxiproxy_default_no_injection() {
    let inj = ToxiproxyInjector::default();
    assert_eq!(inj.latency_ms(), 0);
    assert_eq!(inj.failure_rate(), 0.0);
}
