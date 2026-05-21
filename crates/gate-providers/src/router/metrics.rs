//! Channel-level runtime metrics — 滑动窗口成功率/延迟、per-channel RPM/TPM 限速、inflight 计数器。
//!
//! 这些都是 `ProviderRouter` 的运行时观察器：
//! - `ChannelMetrics`：滑动窗口成功率与延迟，least_latency 用它的 `avg_latency`。
//! - `ChannelRateCheck` / `InMemoryChannelRateLimiter`：per-channel RPM/TPM 限速。
//! - `InflightTracker`：least_conn 用它的 `current`。

use gate_core::id::ChannelId;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

// ============================================================================
// ChannelMetrics — 滑动窗口成功率追踪（auto-disable 机制）
// ============================================================================

/// 轻量级内存滑动窗口，按 channel 追踪成功率和响应延迟。
///
/// 窗口满且成功率低于阈值时，`should_disable` 返回 true。
pub struct ChannelMetrics {
    windows: Mutex<HashMap<ChannelId, VecDeque<bool>>>,
    latencies: Mutex<HashMap<ChannelId, VecDeque<u64>>>, // ms
    window_size: usize,
    threshold: f64,
}

impl ChannelMetrics {
    pub fn new(window_size: usize, threshold: f64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            latencies: Mutex::new(HashMap::new()),
            window_size,
            threshold,
        }
    }

    /// 记录一次请求结果（true = 成功，false = 失败）。
    pub fn record(&self, channel_id: ChannelId, success: bool) {
        let mut windows = self.windows.lock();
        let window = windows.entry(channel_id).or_default();
        if window.len() >= self.window_size {
            window.pop_front();
        }
        window.push_back(success);
    }

    /// 窗口满且成功率低于阈值时返回 true，触发 auto-disable。
    pub fn should_disable(&self, channel_id: ChannelId) -> bool {
        let windows = self.windows.lock();
        let Some(window) = windows.get(&channel_id) else {
            return false;
        };
        if window.len() < self.window_size {
            return false;
        }
        let successes = window.iter().filter(|&&s| s).count();
        let rate = successes as f64 / window.len() as f64;
        rate < self.threshold
    }

    /// 记录响应延迟（毫秒）。
    pub fn record_latency(&self, channel_id: ChannelId, latency_ms: u64) {
        let mut latencies = self.latencies.lock();
        let window = latencies.entry(channel_id).or_default();
        if window.len() >= self.window_size {
            window.pop_front();
        }
        window.push_back(latency_ms);
    }

    /// 获取 channel 的平均延迟（ms）；无数据时返回 u64::MAX。
    pub fn avg_latency(&self, channel_id: ChannelId) -> u64 {
        let latencies = self.latencies.lock();
        let Some(window) = latencies.get(&channel_id) else {
            return u64::MAX;
        };
        if window.is_empty() {
            return u64::MAX;
        }
        let sum: u64 = window.iter().sum();
        sum / window.len() as u64
    }

    /// 清除 channel 的历史记录（re-enable 后调用）。
    pub fn clear(&self, channel_id: ChannelId) {
        let mut windows = self.windows.lock();
        windows.remove(&channel_id);
        let mut latencies = self.latencies.lock();
        latencies.remove(&channel_id);
    }
}

// ============================================================================
// ChannelRateCheck — per-channel RPM/TPM 限速 trait
// ============================================================================

/// Per-channel RPM/TPM 限速抽象。
///
/// 实现方式：
/// - [`InMemoryChannelRateLimiter`]：纯内存固定窗口（dev / 单实例）
/// - Redis 实现在 `gate-cache` crate（生产多实例 sliding window）
///
/// 路由器通过 `Arc<dyn ChannelRateCheck>` 持有，支持运行时注入。
#[async_trait::async_trait]
pub trait ChannelRateCheck: Send + Sync + 'static {
    /// 检查并消耗一次 RPM 额度。
    ///
    /// - `rpm_limit = None` → 无限制，返回 `true`
    /// - 通过 → `true`（计数 +1）
    /// - 超限 → `false`（不消耗）
    async fn check_rpm(&self, channel_id: ChannelId, rpm_limit: Option<i32>) -> bool;

    /// 检查 TPM 限额（pre-flight）。
    ///
    /// 不消耗额度，仅检查当前窗口内 token 总量是否 < limit。
    /// 实际 token 消耗由 `record_tokens` 在 response 后记录。
    ///
    /// - `tpm_limit = None` → 无限制，返回 `true`
    /// - 窗口内 token < limit → `true`
    /// - 超限 → `false`
    async fn check_tpm(&self, channel_id: ChannelId, tpm_limit: Option<i32>) -> bool;

    /// 记录实际 token 消耗（response 后调用）。
    ///
    /// 更新 TPM 滑动窗口计数。
    async fn record_tokens(&self, channel_id: ChannelId, tokens: u32);
}

// ============================================================================
// InMemoryChannelRateLimiter — 纯内存固定窗口实现
// ============================================================================

struct RateWindow {
    rpm_count: u32,
    tpm_count: u32,
    window_start: Instant,
}

/// 纯内存固定窗口限速器。每个 channel 一个 60s 窗口，过期自动重置。
///
/// 适用于 dev / 单实例部署。多实例部署应使用 Redis 实现。
pub struct InMemoryChannelRateLimiter {
    counters: Mutex<HashMap<ChannelId, RateWindow>>,
}

impl Default for InMemoryChannelRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryChannelRateLimiter {
    pub fn new() -> Self {
        Self {
            counters: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl ChannelRateCheck for InMemoryChannelRateLimiter {
    async fn check_rpm(&self, channel_id: ChannelId, rpm_limit: Option<i32>) -> bool {
        let Some(limit) = rpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u32;
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        if window.rpm_count >= limit {
            return false;
        }
        window.rpm_count += 1;
        true
    }

    async fn check_tpm(&self, channel_id: ChannelId, tpm_limit: Option<i32>) -> bool {
        let Some(limit) = tpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u32;
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        window.tpm_count < limit
    }

    async fn record_tokens(&self, channel_id: ChannelId, tokens: u32) {
        let mut counters = self.counters.lock();
        let window = counters.entry(channel_id).or_insert_with(|| RateWindow {
            rpm_count: 0,
            tpm_count: 0,
            window_start: Instant::now(),
        });
        if window.window_start.elapsed().as_secs() >= 60 {
            window.rpm_count = 0;
            window.tpm_count = 0;
            window.window_start = Instant::now();
        }
        window.tpm_count = window.tpm_count.saturating_add(tokens);
    }
}

/// Backward-compat alias.
pub type ChannelRateLimiter = InMemoryChannelRateLimiter;

pub(super) const DEFAULT_CHANNEL_LATENCY_WINDOW_SECS: i64 = 300;

// ============================================================================
// InflightTracker — least_conn 策略用的 inflight 计数器
// ============================================================================

/// 轻量级 inflight 请求计数器。
///
/// warmup 之后热路径只做 `RwLock::read` + `AtomicI64` CAS，无分配。
pub struct InflightTracker {
    counts: RwLock<HashMap<ChannelId, Arc<AtomicI64>>>,
}

impl Default for InflightTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl InflightTracker {
    pub fn new() -> Self {
        Self {
            counts: RwLock::new(HashMap::new()),
        }
    }

    /// 标记一个 channel 获取了请求（inflight +1）。
    pub fn acquire(&self, channel_id: ChannelId) {
        // 快路径：read lock
        {
            let counts = self.counts.read();
            if let Some(counter) = counts.get(&channel_id) {
                counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        // 慢路径：write lock，首次见到的 channel
        let mut counts = self.counts.write();
        let counter = counts
            .entry(channel_id)
            .or_insert_with(|| Arc::new(AtomicI64::new(0)));
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// 标记一个 channel 释放了请求（inflight -1）。
    pub fn release(&self, channel_id: ChannelId) {
        let counts = self.counts.read();
        if let Some(counter) = counts.get(&channel_id) {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// 查询 channel 当前 inflight 数。
    pub fn current(&self, channel_id: ChannelId) -> i64 {
        let counts = self.counts.read();
        counts
            .get(&channel_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }
}
