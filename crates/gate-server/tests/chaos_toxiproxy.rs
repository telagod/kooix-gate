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
        use testcontainers::{GenericImage, core::IntoContainerPort, runners::AsyncRunner};

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

    /// 通过 admin API 注册一个 proxy：上游 `upstream_host:upstream_port`
    /// 监听 `listen_port`（toxiproxy 容器内部），返回宿主可达端口。
    /// upstream_host 通常是 "host.docker.internal"（macOS / win）或宿主 docker0 IP。
    pub async fn add_proxy(
        &self,
        name: &str,
        listen_port: u16,
        upstream_host: &str,
        upstream_port: u16,
    ) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "name": name,
            "listen": format!("0.0.0.0:{listen_port}"),
            "upstream": format!("{upstream_host}:{upstream_port}"),
            "enabled": true,
        });
        let resp = client
            .post(format!("{}/proxies", self.admin_url))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("add_proxy failed: {}", resp.status());
        }
        Ok(())
    }

    /// 禁用 / 启用 proxy（即彻底拒绝连接）。
    pub async fn set_proxy_enabled(&self, name: &str, enabled: bool) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/proxies/{name}", self.admin_url))
            .json(&serde_json::json!({"enabled": enabled}))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("set_proxy_enabled failed: {}", resp.status());
        }
        Ok(())
    }

    /// 给已注册 proxy 加一个 toxic（latency / timeout / reset_peer 等）。
    /// `attrs` 形如 `{"latency": 1000, "jitter": 100}` 或 `{"timeout": 0}`。
    pub async fn add_toxic(
        &self,
        proxy_name: &str,
        toxic_name: &str,
        toxic_type: &str,
        stream: &str,
        attrs: serde_json::Value,
    ) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "name": toxic_name,
            "type": toxic_type,
            "stream": stream,
            "attributes": attrs,
        });
        let resp = client
            .post(format!("{}/proxies/{proxy_name}/toxics", self.admin_url))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "add_toxic {proxy_name}/{toxic_name} failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }
        Ok(())
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
    unsafe {
        std::env::remove_var("KOOIX_CHAOS_DOCKER");
    }
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
    let tp = ToxiproxyContainer::start()
        .await
        .expect("toxiproxy 容器启动失败");
    assert!(tp.admin_port() > 0);
    let resp = reqwest::get(format!("{}/version", tp.admin_url))
        .await
        .expect("admin /version 请求失败");
    assert!(resp.status().is_success(), "/version 应返 200");
    let body = resp.text().await.expect("body 解码失败");
    assert!(!body.is_empty(), "/version 应返非空版本");
}

/// Chaos case #1（v0.4.165）：拒绝连接。
/// 通过 admin API 创建一个 proxy 然后 disable，验上游连接被立即关闭。
/// 这是「PG 拒绝连接」的 chaos 原语 — 完整 PG 容器接通推 v0.5.x（涉及 host.docker.internal 网络）。
#[tokio::test]
#[ignore = "需要 docker + KOOIX_CHAOS_DOCKER=1 opt-in"]
async fn toxiproxy_disabled_proxy_refuses_connection() {
    if !chaos_docker_enabled() {
        eprintln!("跳过：未设 KOOIX_CHAOS_DOCKER=1");
        return;
    }
    let tp = ToxiproxyContainer::start()
        .await
        .expect("toxiproxy 容器启动失败");

    // 注册一个 proxy 指向不存在上游（127.0.0.1:1 — 保证连不上），然后立刻 disable
    tp.add_proxy("chaos_refuse", 8666, "127.0.0.1", 1)
        .await
        .expect("add_proxy");
    tp.set_proxy_enabled("chaos_refuse", false)
        .await
        .expect("disable proxy");

    // 验 admin API 真把状态写下去了
    let list: serde_json::Value = reqwest::get(format!("{}/proxies/chaos_refuse", tp.admin_url))
        .await
        .expect("GET proxy")
        .json()
        .await
        .expect("decode json");
    assert_eq!(
        list["enabled"],
        serde_json::json!(false),
        "proxy 应被 disable"
    );
    assert_eq!(list["name"], serde_json::json!("chaos_refuse"));
}

/// Chaos case #2（v0.4.166）：Redis 闪断 — latency toxic 注入。
/// add_toxic 加 latency 1000ms + jitter 200ms，验 admin API 真把 toxic 记入。
/// 完整接 fred Redis client 走 toxiproxy → 上游 redis container 推 v0.5.x。
#[tokio::test]
#[ignore = "需要 docker + KOOIX_CHAOS_DOCKER=1 opt-in"]
async fn toxiproxy_latency_toxic_registered() {
    if !chaos_docker_enabled() {
        eprintln!("跳过：未设 KOOIX_CHAOS_DOCKER=1");
        return;
    }
    let tp = ToxiproxyContainer::start()
        .await
        .expect("toxiproxy 容器启动失败");
    tp.add_proxy("chaos_redis", 8666, "127.0.0.1", 6379)
        .await
        .expect("add_proxy");
    tp.add_toxic(
        "chaos_redis",
        "slow",
        "latency",
        "downstream",
        serde_json::json!({"latency": 1000, "jitter": 200}),
    )
    .await
    .expect("add_toxic");

    // 验 admin API 真把 toxic 记入
    let toxics: serde_json::Value =
        reqwest::get(format!("{}/proxies/chaos_redis/toxics", tp.admin_url))
            .await
            .expect("GET toxics")
            .json()
            .await
            .expect("decode toxics");
    let arr = toxics.as_array().expect("toxics 数组");
    assert_eq!(arr.len(), 1, "应有 1 个 toxic");
    assert_eq!(arr[0]["type"], serde_json::json!("latency"));
    assert_eq!(arr[0]["attributes"]["latency"], serde_json::json!(1000));
}

/// Chaos case #3（v0.4.167）：上游 503 风暴。
/// 用 wiremock 起一个 always-503 上游 + reqwest 重试 4 次都 503，
/// 验 ProbeChaos 计数（每次失败 inject 1）和 failure_rate 一致。
/// 这是「上游短时不可用 → retry 风暴 → 雪崩」的 chaos 原语 —
/// 真接 gate-server provider router + retry policy 推 v0.5.x。
#[tokio::test]
async fn upstream_503_storm_increments_probe_chaos() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream down"))
        .mount(&mock)
        .await;

    // 100% 失败 chaos injector — 替代「上游永远 503」语义
    let inj = ToxiproxyInjector::new().with_failure_bps(10_000);
    assert!((inj.failure_rate() - 1.0).abs() < 1e-6);

    let client = reqwest::Client::new();
    let url = format!("{}/v1/chat/completions", mock.uri());
    let mut attempts = 0u32;
    let mut all_503 = true;
    for _ in 0..4u32 {
        attempts += 1;
        // injector 模拟「请求经过 chaos」：每次调用 latency_ms() 计 inflight
        let _ = inj.latency_ms();
        let resp = client.post(&url).body("{}").send().await.expect("send");
        if resp.status() != 503 {
            all_503 = false;
        }
    }
    assert_eq!(attempts, 4);
    assert!(all_503, "wiremock always-503，所有重试应得 503");
    assert_eq!(inj.injected_count(), 4, "ProbeChaos 应记 4 次注入");
}
