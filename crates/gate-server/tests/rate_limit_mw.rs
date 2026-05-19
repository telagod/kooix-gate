//! 限流 middleware E2E：起 Redis 容器，前 N 个请求过、第 N+1 个 429。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::UserId;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::{RateLimitCfg, Repos};
use gate_server::{AppState, build_router};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;
use tower::ServiceExt;
use uuid::Uuid;

async fn start_redis() -> (
    testcontainers::ContainerAsync<Redis>,
    fred::clients::RedisPool,
) {
    let tag = std::env::var("KOOIX_TEST_REDIS_TAG").unwrap_or_else(|_| "7-alpine".into());
    let container = Redis::default().with_tag(&tag).start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{host}:{port}");
    let pool = gate_cache::connect(&url, 2).await.unwrap();
    (container, pool)
}

#[tokio::test]
async fn user_hits_429_after_quota_exhausted() {
    let (_c, redis_pool) = start_redis().await;

    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let loader = Arc::new(InMemoryLoader::new());
    let user = UserId::new();
    loader.add_user(
        user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: None,
        },
    );

    let state = AppState::new(jwt, loader, Repos::in_memory())
        .with_rate_limiter(gate_cache::RateLimiter::new(redis_pool))
        .with_rate_limit_cfg(RateLimitCfg {
            window_ms: 60_000,
            capacity: 3,
        });
    let jwt = state.jwt.clone();
    let router = build_router(state);

    let (tok, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), None, false)
        .unwrap();

    let mut allowed = 0;
    let mut denied = 0;
    let mut retry_after: u64 = 0;
    for _ in 0..6 {
        let req = Request::builder()
            .method("GET")
            .uri("/v1/me")
            .header("authorization", format!("Bearer {tok}"))
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        if resp.status() == StatusCode::OK {
            allowed += 1;
        } else if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            denied += 1;
            // Retry-After header 存在
            if let Some(v) = resp.headers().get("retry-after") {
                let s = v.to_str().unwrap();
                retry_after = s.parse().unwrap_or(0);
            }
            // body 含 retry_after_ms
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"]["code"], "rate_limited");
            assert_eq!(body["error"]["type"], "rate_limit_error");
        }
    }
    assert_eq!(allowed, 3, "first 3 should pass");
    assert_eq!(denied, 3, "next 3 should 429");
    assert!(retry_after > 0, "Retry-After header must be set");
}

#[tokio::test]
async fn anonymous_buckets_by_ip() {
    // 不带 token 的请求会走 /health → 健康检查不进 /v1，所以无限流
    // /v1/me 没 auth 会 401，但 middleware 在前置——所以应当先被限流卡
    // 这里验证：同一个 X-Forwarded-For 的不同请求共享 bucket
    let (_c, redis_pool) = start_redis().await;

    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();

    let state = AppState::new(jwt, Arc::new(InMemoryLoader::new()), Repos::in_memory())
        .with_rate_limiter(gate_cache::RateLimiter::new(redis_pool))
        .with_rate_limit_cfg(RateLimitCfg {
            window_ms: 60_000,
            capacity: 2,
        });
    let router = build_router(state);

    let mut counts = [0u32; 2]; // [allowed/401, denied]
    for _ in 0..4 {
        let req = Request::builder()
            .method("GET")
            .uri("/v1/me")
            .header("x-forwarded-for", "10.1.2.3")
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        match resp.status() {
            StatusCode::UNAUTHORIZED => counts[0] += 1,
            StatusCode::TOO_MANY_REQUESTS => counts[1] += 1,
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!(counts[0], 2, "first 2 IP requests pass middleware → 401");
    assert_eq!(counts[1], 2, "next 2 same-IP rate limited");
}

#[tokio::test]
async fn health_endpoint_not_rate_limited() {
    // /health 不在 /v1 下，应当永不限流（k8s probe 友好）
    let (_c, redis_pool) = start_redis().await;

    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();
    let state = AppState::new(jwt, Arc::new(InMemoryLoader::new()), Repos::in_memory())
        .with_rate_limiter(gate_cache::RateLimiter::new(redis_pool))
        .with_rate_limit_cfg(RateLimitCfg {
            window_ms: 60_000,
            capacity: 1,
        });
    let router = build_router(state);

    for _ in 0..10 {
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
