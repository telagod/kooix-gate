//! Toxiproxy 注入器骨架 + 真实容器 launcher。
//!
//! 0.4.147（v0）：仅 builder API + 与 ChaosInjector trait 集成形状。
//! 0.4.164（第四刀 #4 step 1）：加 `ToxiproxyContainer` 真实启动 launcher（testcontainers）。
//!   默认 `#[ignore]` + env `KOOIX_CHAOS_DOCKER=1` opt-in 跑，CI 不阻塞。
//!
//! 用法（local docker）：
//! ```bash
//! KOOIX_CHAOS_DOCKER=1 cargo test -p gate-server --test chaos_toxiproxy -- --ignored
//! ```

#![allow(dead_code)]

mod chaos_common;

use chaos_common::ChaosInjector;
use std::sync::atomic::{AtomicU64, Ordering};

/// Toxiproxy-style 注入器（in-process，仅 latency + failure_rate 模拟）。
/// 真实 toxiproxy 容器走 [`ToxiproxyContainer`]。
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

// ============================================================================
// 0.4.164：真实 toxiproxy 容器 launcher（testcontainers + admin HTTP client）
// ============================================================================

/// 真实 toxiproxy 容器封装。
/// 启动需 docker daemon + `KOOIX_CHAOS_DOCKER=1` opt-in。
///
/// admin API: http://localhost:{admin_port}（默认 toxiproxy 端 8474）
/// proxy 端口动态分配 — 调 `add_proxy` 注册并返回宿主可访问 host:port。
pub struct ToxiproxyContainer {
    _container: testcontainers::ContainerAsync<testcontainers::GenericImage>,
    admin_port: u16,
    /// 宿主可达的 admin URL，用于直接 curl /proxies 调试
    pub admin_url: String,
}

impl ToxiproxyContainer {
    /// 启动 toxiproxy 容器。仅 `KOOIX_CHAOS_DOCKER=1` 且 docker 可用时调用。
    pub async fn start() -> anyhow::Result<Self> {
        use testcontainers::{GenericImage, ImageExt, core::IntoContainerPort, runners::AsyncRunner};

        let image = GenericImage::new("ghcr.io/shopify/toxiproxy", "2.9.0")
            .with_exposed_port(8474u16.tcp())
            .with_exposed_port(8666u16.tcp()); // 预留 1 个常用 proxy 端口
        let container = image.start().await?;
        let admin_port = container.get_host_port_ipv4(8474u16).await?;
        Ok(Self {
            _container: container,
            admin_port,
            admin_url: format!("http://localhost:{admin_port}"),
        })
    }

    pub fn admin_port(&self) -> u16 {
        self.admin_port
    }
}

/// 用环境变量门禁判断本地是否真跑 chaos 容器测试。
/// CI / 默认开发环境返 false（保证 `cargo test` 全绿）。
pub fn chaos_docker_enabled() -> bool {
    std::env::var("KOOIX_CHAOS_DOCKER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
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

#[test]
fn chaos_docker_env_gate_off_by_default() {
    // SAFETY: 测试线程内 unset，避免污染其它测试
    // SAFETY: env mutation 在 single-test scope
    unsafe { std::env::remove_var("KOOIX_CHAOS_DOCKER"); }
    assert!(!chaos_docker_enabled(), "默认不启 chaos docker");
}

/// 真实启动 toxiproxy 容器；仅 KOOIX_CHAOS_DOCKER=1 时跑。
#[tokio::test]
#[ignore = "需要 docker + KOOIX_CHAOS_DOCKER=1 opt-in"]
async fn toxiproxy_container_starts_and_admin_responds() {
    if !chaos_docker_enabled() {
        eprintln!("跳过：未设 KOOIX_CHAOS_DOCKER=1");
        return;
    }
    let tp = ToxiproxyContainer::start().await.expect("toxiproxy 容器启动失败");
    assert!(tp.admin_port() > 0);
    let resp = reqwest::get(format!("{}/version", tp.admin_url))
        .await
        .expect("admin /version 请求失败");
    assert!(resp.status().is_success(), "/version 应返 200");
    let body = resp.text().await.expect("body 解码失败");
    assert!(!body.is_empty(), "/version 应返非空版本");
}
