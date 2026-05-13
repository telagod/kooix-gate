//! gate-cache 集成测试：起 redis 容器跑 Lua 脚本。

use gate_cache::{QuotaCounter, RateLimiter};
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::redis::Redis;

async fn start() -> (
    testcontainers::ContainerAsync<Redis>,
    fred::clients::RedisPool,
) {
    let tag = std::env::var("KOOIX_TEST_REDIS_TAG").unwrap_or_else(|_| "7-alpine".into());
    let container = Redis::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start redis");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{host}:{port}");
    let pool = gate_cache::connect(&url, 2).await.expect("connect");
    (container, pool)
}

#[tokio::test]
async fn rate_limit_allows_under_limit_and_denies_over() {
    let (_c, pool) = start().await;
    let rl = RateLimiter::new(pool);
    let key = "rl:test:1m";

    // 前 3 个全过
    for i in 1..=3u64 {
        let d = rl.check(key, 60_000, 3).await.unwrap();
        assert!(d.allowed, "i={i}");
        assert_eq!(d.current, i);
    }

    // 第 4 个拒
    let d = rl.check(key, 60_000, 3).await.unwrap();
    assert!(!d.allowed);
    assert_eq!(d.current, 3);
    assert_eq!(d.remaining, 0);
    assert!(d.retry_after_ms > 0);
}

#[tokio::test]
async fn rate_limit_isolated_per_key() {
    let (_c, pool) = start().await;
    let rl = RateLimiter::new(pool);

    rl.check("rl:a", 60_000, 1).await.unwrap();
    let d = rl.check("rl:a", 60_000, 1).await.unwrap();
    assert!(!d.allowed);

    // 另一个 key 应该独立
    let d2 = rl.check("rl:b", 60_000, 1).await.unwrap();
    assert!(d2.allowed);
}

#[tokio::test]
async fn rate_limit_recovers_after_window() {
    let (_c, pool) = start().await;
    let rl = RateLimiter::new(pool);
    let key = "rl:recover";

    // 窗口 200ms，limit 2
    rl.check(key, 200, 2).await.unwrap();
    rl.check(key, 200, 2).await.unwrap();
    let denied = rl.check(key, 200, 2).await.unwrap();
    assert!(!denied.allowed);

    // 等过窗口
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let recovered = rl.check(key, 200, 2).await.unwrap();
    assert!(recovered.allowed, "should recover after window");
}

#[tokio::test]
async fn quota_debit_atomic_and_blocks_overdraw() {
    let (_c, pool) = start().await;
    let q = QuotaCounter::new(pool);
    let key = "q:test:tokens";

    let r = q.debit(key, 100, 250, 60).await.unwrap();
    assert!(r.ok);
    assert_eq!(r.current_used, 100);
    assert_eq!(r.remaining, 150);

    let r = q.debit(key, 100, 250, 60).await.unwrap();
    assert!(r.ok);
    assert_eq!(r.current_used, 200);

    // 第三笔超额：不应增加 current_used
    let r = q.debit(key, 100, 250, 60).await.unwrap();
    assert!(!r.ok);
    assert_eq!(r.current_used, 200);
    assert_eq!(r.remaining, 50);
}

#[tokio::test]
async fn quota_refund_decrements() {
    let (_c, pool) = start().await;
    let q = QuotaCounter::new(pool);
    let key = "q:refund";

    q.debit(key, 500, 1000, 60).await.unwrap();
    let r = q.refund(key, 200).await.unwrap();
    assert_eq!(r.current_used, 300);

    // 再扣应该能放进 700 (limit - 300 = 700)
    let r = q.debit(key, 700, 1000, 60).await.unwrap();
    assert!(r.ok);
}

#[tokio::test]
async fn quota_refund_clamps_at_zero() {
    let (_c, pool) = start().await;
    let q = QuotaCounter::new(pool);
    let key = "q:clamp";

    q.debit(key, 50, 100, 60).await.unwrap();
    let r = q.refund(key, 9999).await.unwrap();
    assert_eq!(r.current_used, 0, "refund should clamp at 0");
}

#[tokio::test]
async fn quota_concurrent_debit_atomic() {
    // 模拟 50 个并发请求扣 quota，limit 30 — 最多有 30 个成功
    let (_c, pool) = start().await;
    let q = std::sync::Arc::new(QuotaCounter::new(pool));
    let key = "q:concurrent";

    let mut handles = Vec::new();
    for _ in 0..50 {
        let q = q.clone();
        let key = key.to_string();
        handles.push(tokio::spawn(async move {
            q.debit(&key, 1, 30, 60).await.unwrap().ok
        }));
    }
    let mut ok_count = 0;
    for h in handles {
        if h.await.unwrap() {
            ok_count += 1;
        }
    }
    assert_eq!(ok_count, 30, "exactly limit count should succeed");
}
