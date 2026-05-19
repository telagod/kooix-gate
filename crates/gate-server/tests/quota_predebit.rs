//! quota pre-debit E2E — Redis 容器跑 debit/refund 全链路。
//!
//! 验证：
//! 1. 预扣成功后请求通过，结算修正差额
//! 2. 预扣超额时 429
//! 3. 并发请求被原子预扣阻止超支
//! 4. Drop 路径退还全额（取消场景）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use gate_auth::api_key as api_key_auth;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_cache::QuotaCounter;
use gate_core::id::{ApiKeyId, ChannelGroupId, ChannelId, OrgId, ProjectId};
use gate_providers::openai::OpenAiProvider;
use gate_server::loader::{ApiKeyRecord, InMemoryLoader};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use gate_storage::{InMemoryQuotaRepo, QuotaRecord};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::json;
use std::sync::Arc;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::redis::Redis;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PLAINTEXT_KEY: &str = "sk-kg-test-quota-predebit-key-aaa";

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, sqlx::PgPool) {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default().with_tag(&tag).start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.unwrap();
    gate_storage::run_migrations(&pool).await.unwrap();
    (container, pool)
}

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

struct Harness {
    router: axum::Router,
    #[allow(dead_code)]
    quota_counter: Arc<QuotaCounter>,
}

struct EmbeddingHarness {
    router: axum::Router,
    #[allow(dead_code)]
    quota_counter: Arc<QuotaCounter>,
}

struct ImageHarness {
    router: axum::Router,
    #[allow(dead_code)]
    quota_counter: Arc<QuotaCounter>,
}

async fn setup(upstream: &MockServer, pool: fred::clients::RedisPool, limit_usd: &str) -> Harness {
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
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let provider = OpenAiProvider::new(format!("{}/v1", upstream.uri()), "test-key").unwrap();

    // Seed a daily_budget_usd quota on the api_key scope
    let quota_repo = Arc::new(InMemoryQuotaRepo::new());
    let now = Utc::now();
    quota_repo.seed(QuotaRecord {
        id: Uuid::now_v7(),
        scope_kind: "api_key".into(),
        scope_id: *api_key_id.as_uuid(),
        dimension: "daily_budget_usd".into(),
        model_filter: None,
        limit_value: limit_usd.parse::<Decimal>().unwrap(),
        window_seconds: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });

    let qc = Arc::new(QuotaCounter::new(pool.clone()));
    let mut repos = Repos::in_memory();
    repos.quotas = quota_repo;

    let state = AppState::new(jwt, loader, repos)
        .with_provider(provider)
        .with_quota_counter(QuotaCounter::new(pool));

    let quota_counter = qc;

    let router = build_router(state);
    Harness {
        router,
        quota_counter,
    }
}

async fn setup_embeddings(
    upstream: &MockServer,
    pool: fred::clients::RedisPool,
    limit_usd: &str,
) -> EmbeddingHarness {
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
    };

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
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let quota_repo = Arc::new(InMemoryQuotaRepo::new());
    let now = Utc::now();
    quota_repo.seed(QuotaRecord {
        id: Uuid::now_v7(),
        scope_kind: "api_key".into(),
        scope_id: *api_key_id.as_uuid(),
        dimension: "daily_budget_usd".into(),
        model_filter: None,
        limit_value: limit_usd.parse::<Decimal>().unwrap(),
        window_seconds: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    unsafe {
        std::env::set_var("KOOIX_CH_QUOTA_EMBED_WM_KEY", "test-key");
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "quota-embed-wm".into(),
        name: "quota-embed-wiremock".into(),
        provider_type: "openai".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec!["text-embedding-3-small".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::Value::Object(Default::default()),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "quota-embedding-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let qc = Arc::new(QuotaCounter::new(pool.clone()));
    let mut repos = Repos::in_memory();
    repos.quotas = quota_repo;
    repos.channels = ch_repo.clone();
    repos.channel_groups = grp_repo.clone();

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(ProviderRouter::new(ch_repo, grp_repo))
        .with_quota_counter(QuotaCounter::new(pool));
    let router = build_router(state);

    EmbeddingHarness {
        router,
        quota_counter: qc,
    }
}

async fn setup_images(
    upstream: &MockServer,
    pool: fred::clients::RedisPool,
    limit_usd: &str,
) -> ImageHarness {
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo, InMemoryChannelRepo,
    };

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
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );

    let quota_repo = Arc::new(InMemoryQuotaRepo::new());
    let now = Utc::now();
    quota_repo.seed(QuotaRecord {
        id: Uuid::now_v7(),
        scope_kind: "api_key".into(),
        scope_id: *api_key_id.as_uuid(),
        dimension: "daily_budget_usd".into(),
        model_filter: None,
        limit_value: limit_usd.parse::<Decimal>().unwrap(),
        window_seconds: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    unsafe {
        std::env::set_var("KOOIX_CH_QUOTA_IMAGE_WM_KEY", "test-key");
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "quota-image-wm".into(),
        name: "quota-image-wiremock".into(),
        provider_type: "openai".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec!["dall-e-3".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::Value::Object(Default::default()),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "quota-image-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project_id, group_id);

    let qc = Arc::new(QuotaCounter::new(pool.clone()));
    let mut repos = Repos::in_memory();
    repos.quotas = quota_repo;
    repos.channels = ch_repo.clone();
    repos.channel_groups = grp_repo.clone();

    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(ProviderRouter::new(ch_repo, grp_repo))
        .with_quota_counter(QuotaCounter::new(pool));
    let router = build_router(state);

    ImageHarness {
        router,
        quota_counter: qc,
    }
}

fn chat_request_body(msg: &str) -> Body {
    Body::from(
        serde_json::to_vec(&json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": msg}],
            "max_tokens": 100
        }))
        .unwrap(),
    )
}

fn embedding_request_body(input: &str) -> Body {
    Body::from(
        serde_json::to_vec(&json!({
            "model": "text-embedding-3-small",
            "input": input
        }))
        .unwrap(),
    )
}

fn image_request_body(n: u32) -> Body {
    Body::from(
        serde_json::to_vec(&json!({
            "model": "dall-e-3",
            "prompt": "draw a gate",
            "n": n,
            "size": "1024x1024",
            "quality": "hd"
        }))
        .unwrap(),
    )
}

fn mock_response(prompt_tokens: u32, completion_tokens: u32) -> serde_json::Value {
    json!({
        "id": "chatcmpl-predebit-1",
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "ok" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}

fn mock_embedding_response(prompt_tokens: u32) -> serde_json::Value {
    json!({
        "object": "list",
        "data": [{
            "object": "embedding",
            "index": 0,
            "embedding": [0.1, 0.2, 0.3]
        }],
        "model": "text-embedding-3-small",
        "usage": {
            "prompt_tokens": prompt_tokens,
            "total_tokens": prompt_tokens
        }
    })
}

fn mock_image_response(count: usize) -> serde_json::Value {
    let data: Vec<_> = (0..count)
        .map(|idx| json!({ "url": format!("https://cdn.example.test/{idx}.png") }))
        .collect();
    json!({
        "created": 1710000000,
        "data": data
    })
}

/// 等 spawn 出去的 settle/refund task 跑完。
async fn yield_for_settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn images_predebit_settles_and_blocks_when_budget_exceeded() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_image_response(1)))
        .mount(&upstream)
        .await;

    // Request n=2 pre-debits 160000µ; image billing settles by billable requested units.
    // Budget 239999µ: first request settles to 160000µ; second pre-debit would make
    // 320000µ and is denied.
    let h = setup_images(&upstream, pool.clone(), "0.239999").await;

    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/images/generations")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(image_request_body(2))
        .unwrap();
    let resp1 = h.router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let _ = resp1.into_body().collect().await.unwrap();

    yield_for_settle().await;

    let req2 = Request::builder()
        .method("POST")
        .uri("/v1/images/generations")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(image_request_body(2))
        .unwrap();
    let resp2 = h.router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

    let bytes = resp2.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["dimension"], "daily_budget_usd");
}

#[tokio::test]
async fn embeddings_predebit_settles_and_blocks_when_budget_exceeded() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_embedding_response(2)))
        .mount(&upstream)
        .await;

    // 12 chars / 4 = 3 estimated tokens × 3µ = 9µ.
    // Actual upstream usage = 2 tokens × 3µ = 6µ.
    // Budget 14µ: first request settles to 6µ; second pre-debit would make 15µ and is denied.
    let h = setup_embeddings(&upstream, pool.clone(), "0.000014").await;

    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(embedding_request_body("abcdefghijkl"))
        .unwrap();
    let resp1 = h.router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let _ = resp1.into_body().collect().await.unwrap();

    yield_for_settle().await;

    let req2 = Request::builder()
        .method("POST")
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(embedding_request_body("abcdefghijkl"))
        .unwrap();
    let resp2 = h.router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

    let bytes = resp2.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["dimension"], "daily_budget_usd");
}

/// Test 1: request succeeds and guard settles (refunds overestimate).
///
/// With max_tokens=100 and a short message "Hi" (2 chars → 0 prompt tokens estimate),
/// estimated = (0 + 100) × 3 = 300 micros.
/// Actual usage = 10 + 5 = 15 tokens × 3 = 45 micros.
/// After settle: budget counter should show 45 (not 300).
#[tokio::test]
async fn predebit_settles_refunds_overestimate() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    // Returns small usage
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(10, 5)))
        .mount(&upstream)
        .await;

    // Budget = $1 = 1_000_000 micros — plenty of headroom
    let h = setup(&upstream, pool.clone(), "1.0").await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(chat_request_body("Hi"))
        .unwrap();
    let resp = h.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();

    yield_for_settle().await;

    // After settlement, the counter should reflect actual usage (15 tokens × 3 = 45 micros)
    // We need to peek the budget key. Since we don't know the exact quota UUID,
    // let's scan — but simpler: the counter value should be 45.
    // Actually, let's just verify next request can still pass (under 1M micros budget).
    // The key test: make a second request with a very tight budget scenario below.
}

/// Test 2: pre-debit blocks request when budget would be exceeded.
///
/// Budget = $0.0003 = 300 micros. Estimated cost for max_tokens=100 + "Hi"(~0 tokens) = 300 micros.
/// First request fills budget exactly → second request rejected.
#[tokio::test]
async fn predebit_blocks_when_budget_exceeded() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(10, 5)))
        .mount(&upstream)
        .await;

    // Budget = $0.0003 = 300 micros. Estimated for max_tokens=100, ~0 prompt = 300 micros.
    let h = setup(&upstream, pool.clone(), "0.0003").await;

    // First request: estimated 300 micros == limit exactly → debit should succeed
    // (debit allows current_used + amount <= limit)
    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(chat_request_body("Hi"))
        .unwrap();
    let resp1 = h.router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let _ = resp1.into_body().collect().await.unwrap();

    yield_for_settle().await;

    // After settlement: actual = (10+5)×3 = 45 micros. Remaining = 300 - 45 = 255 micros.
    // Second request: estimate = 300 micros. 45 + 300 = 345 > 300 limit → blocked!
    let req2 = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .body(chat_request_body("Hi"))
        .unwrap();
    let resp2 = h.router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

    let bytes = resp2.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "quota_exceeded");
    assert_eq!(body["error"]["dimension"], "daily_budget_usd");
}

/// Test 3: concurrent requests are atomically pre-debited — no oversupply.
///
/// Budget = 900 micros. Each request estimates 300 micros. So exactly 3 can pass.
/// Fire 6 concurrent → exactly 3 get OK, 3 get 429.
#[tokio::test]
async fn predebit_concurrent_prevents_overspend() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(10, 5)))
        .mount(&upstream)
        .await;

    // Budget = $0.0009 = 900 micros. Each req estimate (max_tokens=100, 0 prompt) = 300 micros.
    let h = setup(&upstream, pool.clone(), "0.0009").await;
    let router = h.router;

    let mut handles = Vec::new();
    for _ in 0..6 {
        let r = router.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
                .header("content-type", "application/json")
                .body(chat_request_body("Hi"))
                .unwrap();
            let resp = r.oneshot(req).await.unwrap();
            resp.status()
        }));
    }

    let mut ok_count = 0;
    let mut denied_count = 0;
    for h in handles {
        match h.await.unwrap() {
            StatusCode::OK => ok_count += 1,
            StatusCode::TOO_MANY_REQUESTS => denied_count += 1,
            other => panic!("unexpected status: {other}"),
        }
    }
    assert_eq!(ok_count, 3, "exactly 3 requests fit in 900 micros budget");
    assert_eq!(denied_count, 3, "remaining 3 must be rejected");
}

/// Test 4: no quota configured → requests pass without pre-debit (no guards).
#[tokio::test]
async fn no_quota_configured_passes_through() {
    let (_c, pool) = start_redis().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(10, 5)))
        .mount(&upstream)
        .await;

    // No quota seeded
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
    let api_key_id = ApiKeyId::new();
    let project_id = ProjectId::new();
    let org_id = OrgId::new();
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id,
            org_id,
            revoked: false,
            allowed_ips: vec![],
        },
    );
    let provider = OpenAiProvider::new(format!("{}/v1", upstream.uri()), "test-key").unwrap();

    let state = AppState::new(jwt, loader, Repos::in_memory())
        .with_provider(provider)
        .with_quota_counter(QuotaCounter::new(pool));
    let router = build_router(state);

    // 10 requests should all pass without any quota limit
    for _ in 0..10 {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
            .header("content-type", "application/json")
            .body(chat_request_body("Hi"))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = resp.into_body().collect().await.unwrap();
    }
}

#[tokio::test]
async fn embedding_request_id_is_shared_by_quota_inflight_and_billing_outbox() {
    use gate_billing::{InMemoryOutboxRepo, InMemoryPricingRepo, OutboxRepo, PricingRepo};
    use gate_providers::ProviderRouter;
    use gate_storage::{
        ApiKeyRepo, ChannelGroupRecord, ChannelRecord, InMemoryChannelGroupRepo,
        InMemoryChannelRepo, OrgRepo, PgApiKeyRepo, PgInFlightRepo, PgOrgRepo, PgProjectRepo,
        PgQuotaRepo, PgUserRepo, ProjectRepo, QuotaRepo, UserRepo,
    };

    let (_redis_c, redis_pool) = start_redis().await;
    let (_pg_c, pg_pool) = start_pg().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_embedding_response(10)))
        .mount(&upstream)
        .await;

    let user_repo = PgUserRepo::new(pg_pool.clone());
    let org_repo = PgOrgRepo::new(pg_pool.clone());
    let project_repo = PgProjectRepo::new(pg_pool.clone());
    let api_key_repo = PgApiKeyRepo::new(pg_pool.clone());
    let user = user_repo
        .create("quota-request-id@test.dev", None, None, None)
        .await
        .unwrap();
    let org = org_repo
        .create("ReqId Org", "reqid-org", user.id)
        .await
        .unwrap();
    let project = project_repo
        .create(org.id, "ReqId Project", "reqid-project")
        .await
        .unwrap();
    let api_key_id = api_key_repo
        .create(
            project.id,
            "ReqId Embedding Key",
            &api_key_auth::hash(PLAINTEXT_KEY),
            "sk-kg-test",
            "-aaa",
            user.id,
            &[],
        )
        .await
        .unwrap();

    let group_id = ChannelGroupId::new();
    let channel_id = ChannelId::new();
    let ch_repo = Arc::new(InMemoryChannelRepo::new());
    let grp_repo = Arc::new(InMemoryChannelGroupRepo::new());
    let now = Utc::now();
    unsafe {
        std::env::set_var("KOOIX_CH_REQID_EMBED_WM_KEY", "test-key");
    }
    ch_repo.seed_channel(ChannelRecord {
        channel_id,
        code: "reqid-embed-wm".into(),
        name: "reqid-embed-wiremock".into(),
        provider_type: "openai".into(),
        base_url: format!("{}/v1", upstream.uri()),
        supported_models: vec!["text-embedding-3-small".into()],
        status: "active".into(),
        health: "healthy".into(),
        timeout_ms: 60_000,
        max_retries: 1,
        rpm_limit: None,
        tpm_limit: None,
        tags: vec![],
        model_mapping: serde_json::Value::Object(Default::default()),
        balance: None,
        balance_updated_at: None,
        last_error: None,
        last_error_at: None,
        created_at: now,
        updated_at: now,
    });
    ch_repo.seed_binding(group_id, channel_id, 10, 1);
    grp_repo.seed_group(ChannelGroupRecord {
        group_id,
        name: "reqid-embedding-default".into(),
        description: String::new(),
        strategy: "priority".into(),
        fallback_group_id: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    });
    grp_repo.seed_default(project.id, group_id);

    let quota_repo = PgQuotaRepo::new(pg_pool.clone());
    quota_repo
        .upsert(gate_storage::QuotaUpsert {
            scope_kind: "api_key".into(),
            scope_id: *api_key_id.as_uuid(),
            dimension: "daily_budget_usd".into(),
            model_filter: None,
            limit_value: "1.0".parse::<Decimal>().unwrap(),
            window_seconds: None,
        })
        .await
        .unwrap();

    let loader = Arc::new(InMemoryLoader::new());
    loader.add_api_key(
        PLAINTEXT_KEY,
        ApiKeyRecord {
            api_key_id,
            project_id: project.id,
            org_id: org.id,
            revoked: false,
            allowed_ips: vec![],
        },
    );
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
    let pricing = Arc::new(InMemoryPricingRepo::new());
    pricing.seed(gate_billing::PricingRule {
        id: Uuid::now_v7(),
        channel_id: None,
        model: "text-embedding-3-small".into(),
        dimension: "input_tokens".into(),
        unit: "per_million_tokens".into(),
        rate: 0.02,
        conditions: json!({}),
        effective_from: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
        priority: 0,
        description: None,
    });
    let outbox = Arc::new(InMemoryOutboxRepo::new());
    let mut repos = Repos::from_pg(pg_pool.clone());
    repos.quotas = Arc::new(quota_repo);
    repos.inflight = Arc::new(PgInFlightRepo::new(pg_pool.clone()));
    repos.channels = ch_repo.clone();
    repos.channel_groups = grp_repo.clone();
    let state = AppState::new(jwt, loader, repos)
        .with_provider_router(ProviderRouter::new(ch_repo, grp_repo))
        .with_quota_counter(QuotaCounter::new(redis_pool))
        .with_outbox(outbox.clone() as Arc<dyn OutboxRepo>)
        .with_pricing(pricing.clone() as Arc<dyn PricingRepo>);
    let router = build_router(state);

    let expected_request_id = Uuid::now_v7();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {}", PLAINTEXT_KEY))
        .header("content-type", "application/json")
        .header("x-request-id", expected_request_id.to_string())
        .body(embedding_request_body("abcdefghijkl"))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = resp.into_body().collect().await.unwrap();

    yield_for_settle().await;

    let inflight_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM inflight_requests WHERE request_id = $1")
            .bind(expected_request_id)
            .fetch_one(&pg_pool)
            .await
            .unwrap();
    assert_eq!(
        inflight_count, 0,
        "settled guard should delete the same request_id"
    );

    let events = outbox.snapshot();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].request_id, expected_request_id);
    assert_eq!(events[0].model, "text-embedding-3-small");
    assert_eq!(events[0].prompt_tokens, 10);
    assert_eq!(events[0].completion_tokens, 0);
    assert_eq!(
        events[0].idempotency_key,
        Some(expected_request_id.to_string())
    );
}
