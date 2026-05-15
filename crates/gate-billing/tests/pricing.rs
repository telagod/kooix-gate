//! gate-billing pricing 测试 — PgPricingRepo + InMemoryPricingRepo + compute_cost_micros。
//!
//! 覆盖：
//! 1. channel_id 优先匹配（命中 → 不走 NULL 回退）
//! 2. 无 channel pricing → 回退到 channel_id IS NULL 全局默认
//! 3. effective_from / effective_until 时间窗口边界
//! 4. compute_cost_micros 数值（gpt-4o-mini: $0.15/1M in, $0.60/1M out → 1k in + 500 out = 0.00045 USD = 450 micros）
//! 5. InMemory 与 Pg 在同样数据下行为一致

use chrono::{DateTime, Duration, TimeZone, Utc};
use gate_billing::{
    InMemoryPricingRepo, ModelPricing, PgPricingRepo, PricingRepo, compute_cost_micros,
};
use gate_providers::Usage;
use sqlx::PgPool;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, PgPool) {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = Postgres::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = gate_storage::connect(&url, 4).await.expect("connect");
    gate_storage::run_migrations(&pool).await.expect("migrate");
    (container, pool)
}

/// 直接 SQL 插 pricing_rules（每次插 input + output 两条）。
async fn insert_pricing(
    pool: &PgPool,
    channel_id: Option<Uuid>,
    model: &str,
    input: f64,
    output: f64,
    eff_from: DateTime<Utc>,
    eff_until: Option<DateTime<Utc>>,
) {
    sqlx::query(
        "INSERT INTO pricing_rules \
         (channel_id, model, dimension, unit, rate, conditions, effective_from, effective_until, priority) \
         VALUES ($1, $2, 'input_tokens', 'per_million_tokens', $3::numeric, '{}'::jsonb, $5, $6, 0), \
                ($1, $2, 'output_tokens', 'per_million_tokens', $4::numeric, '{}'::jsonb, $5, $6, 0)",
    )
    .bind(channel_id)
    .bind(model)
    .bind(input)
    .bind(output)
    .bind(eff_from)
    .bind(eff_until)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_min_channel(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, code, name, provider_type, base_url, config_enc) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(format!("ch-{}", &id.simple().to_string()[..8]))
    .bind("test channel")
    .bind("openai")
    .bind("https://api.example.com/v1")
    .bind(b"".as_slice()) // config_enc NOT NULL，测试里塞空 bytea
    .execute(pool)
    .await
    .unwrap();
    id
}

// ============================================================================
// compute_cost_micros — 纯逻辑
// ============================================================================

#[test]
fn compute_cost_micros_gpt4o_mini() {
    let pricing = ModelPricing {
        channel_id: None,
        model: "gpt-4o-mini".into(),
        input_per_million: 0.15,
        output_per_million: 0.60,
        cached_input_per_million: None,
        effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
    };
    let usage = Usage {
        prompt_tokens: 1000,
        completion_tokens: 500,
        total_tokens: 1500,
        ..Default::default()
    };
    // 1000 * 0.15 / 1M + 500 * 0.60 / 1M = 0.00015 + 0.00030 = 0.00045 USD
    // = 450 micros
    assert_eq!(compute_cost_micros(&usage, &pricing), 450);
}

#[test]
fn compute_cost_micros_zero_usage_zero_cost() {
    let pricing = ModelPricing {
        channel_id: None,
        model: "gpt-4o".into(),
        input_per_million: 2.5,
        output_per_million: 10.0,
        cached_input_per_million: None,
        effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
    };
    let usage = Usage::default();
    assert_eq!(compute_cost_micros(&usage, &pricing), 0);
}

#[test]
fn compute_cost_micros_large_volume_no_overflow() {
    // 100M tokens at $10/M output —— 1000 USD = 1_000_000_000 micros，远小于 i64::MAX
    let pricing = ModelPricing {
        channel_id: None,
        model: "gpt-4".into(),
        input_per_million: 30.0,
        output_per_million: 60.0,
        cached_input_per_million: None,
        effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
    };
    let usage = Usage {
        prompt_tokens: 100_000_000,
        completion_tokens: 100_000_000,
        total_tokens: 200_000_000,
        ..Default::default()
    };
    // input: 100M * 30 / 1M = 3000 USD = 3_000_000_000 micros
    // output: 100M * 60 / 1M = 6000 USD = 6_000_000_000 micros
    // total: 9000 USD = 9_000_000_000 micros
    let cost = compute_cost_micros(&usage, &pricing);
    assert_eq!(cost, 9_000_000_000);
}

// ============================================================================
// InMemoryPricingRepo — 纯内存语义
// ============================================================================

#[tokio::test]
async fn inmemory_global_default_hit() {
    let repo = InMemoryPricingRepo::new();
    repo.seed_global("gpt-4o-mini", 0.15, 0.60);

    let hit = repo
        .find_for(None, "gpt-4o-mini", Utc::now())
        .await
        .unwrap();
    assert!(hit.is_some());
    let p = hit.unwrap();
    assert_eq!(p.channel_id, None);
    assert!((p.input_per_million - 0.15).abs() < 1e-9);
}

#[tokio::test]
async fn inmemory_channel_specific_wins_over_global() {
    let repo = InMemoryPricingRepo::new();
    let ch = Uuid::now_v7();
    // 全局默认 $1/M, channel 特价 $0.5/M
    repo.seed_global("gpt-4o-mini", 1.0, 2.0);
    repo.seed_legacy(ModelPricing {
        channel_id: Some(ch),
        model: "gpt-4o-mini".into(),
        input_per_million: 0.5,
        output_per_million: 1.0,
        cached_input_per_million: None,
        effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        effective_until: None,
    });

    let hit = repo
        .find_for(Some(ch), "gpt-4o-mini", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.channel_id, Some(ch));
    assert!((hit.input_per_million - 0.5).abs() < 1e-9);
}

#[tokio::test]
async fn inmemory_fallback_to_global_when_no_channel_match() {
    let repo = InMemoryPricingRepo::new();
    repo.seed_global("gpt-4o-mini", 0.15, 0.60);

    // channel_id 没对应 pricing → 回退到 global
    let other_ch = Uuid::now_v7();
    let hit = repo
        .find_for(Some(other_ch), "gpt-4o-mini", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.channel_id, None);
}

#[tokio::test]
async fn inmemory_effective_window_filters_correctly() {
    let repo = InMemoryPricingRepo::new();
    let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    // 旧价 2024 → 2025
    repo.seed_legacy(ModelPricing {
        channel_id: None,
        model: "gpt-4o-mini".into(),
        input_per_million: 0.30,
        output_per_million: 1.20,
        cached_input_per_million: None,
        effective_from: t0,
        effective_until: Some(t1),
    });
    // 新价 2025 起
    repo.seed_legacy(ModelPricing {
        channel_id: None,
        model: "gpt-4o-mini".into(),
        input_per_million: 0.15,
        output_per_million: 0.60,
        cached_input_per_million: None,
        effective_from: t1,
        effective_until: None,
    });

    // 2024 中间：取旧价
    let mid_2024 = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
    let p = repo
        .find_for(None, "gpt-4o-mini", mid_2024)
        .await
        .unwrap()
        .unwrap();
    assert!((p.input_per_million - 0.30).abs() < 1e-9);

    // 2025 之后：取新价
    let p = repo
        .find_for(None, "gpt-4o-mini", t2)
        .await
        .unwrap()
        .unwrap();
    assert!((p.input_per_million - 0.15).abs() < 1e-9);

    // 边界：effective_until 是排他的，t1 时刻新价生效（>= effective_from 且 < effective_until）
    let p = repo
        .find_for(None, "gpt-4o-mini", t1)
        .await
        .unwrap()
        .unwrap();
    assert!((p.input_per_million - 0.15).abs() < 1e-9);
}

#[tokio::test]
async fn inmemory_no_match_returns_none() {
    let repo = InMemoryPricingRepo::new();
    let hit = repo
        .find_for(None, "gpt-7-fantasy", Utc::now())
        .await
        .unwrap();
    assert!(hit.is_none());
}

#[tokio::test]
async fn inmemory_past_effective_until_excluded() {
    let repo = InMemoryPricingRepo::new();
    let t0 = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let t1 = Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap();
    repo.seed_legacy(ModelPricing {
        channel_id: None,
        model: "legacy".into(),
        input_per_million: 1.0,
        output_per_million: 2.0,
        cached_input_per_million: None,
        effective_from: t0,
        effective_until: Some(t1),
    });
    // 2024 时刻 → 过期了
    let hit = repo
        .find_for(
            None,
            "legacy",
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        )
        .await
        .unwrap();
    assert!(hit.is_none());
}

// ============================================================================
// PgPricingRepo — 接 testcontainers，验证与 InMemory 行为一致
// ============================================================================

#[tokio::test]
async fn pg_channel_specific_wins_over_global() {
    let (_c, pool) = start_pg().await;
    let ch = seed_min_channel(&pool).await;
    let now = Utc::now();

    // 全局默认 + 渠道特价
    insert_pricing(
        &pool,
        None,
        "gpt-4o-mini",
        1.0,
        2.0,
        now - Duration::days(30),
        None,
    )
    .await;
    insert_pricing(
        &pool,
        Some(ch),
        "gpt-4o-mini",
        0.15,
        0.60,
        now - Duration::days(30),
        None,
    )
    .await;

    let repo = PgPricingRepo::new(pool);
    let hit = repo
        .find_for(Some(ch), "gpt-4o-mini", now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.channel_id, Some(ch));
    assert!((hit.input_per_million - 0.15).abs() < 1e-9);
}

#[tokio::test]
async fn pg_fallback_to_global_when_no_channel_pricing() {
    let (_c, pool) = start_pg().await;
    let ch = seed_min_channel(&pool).await;
    let now = Utc::now();

    // 只 seed 全局
    insert_pricing(
        &pool,
        None,
        "gpt-4o-mini",
        0.15,
        0.60,
        now - Duration::days(30),
        None,
    )
    .await;

    let repo = PgPricingRepo::new(pool);
    let hit = repo
        .find_for(Some(ch), "gpt-4o-mini", now)
        .await
        .unwrap()
        .unwrap();
    // 没渠道特价，回退到 NULL
    assert_eq!(hit.channel_id, None);
    assert!((hit.input_per_million - 0.15).abs() < 1e-9);
}

#[tokio::test]
async fn pg_effective_window() {
    let (_c, pool) = start_pg().await;
    let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();

    insert_pricing(&pool, None, "gpt-4o-mini", 0.30, 1.20, t0, Some(t1)).await;
    insert_pricing(&pool, None, "gpt-4o-mini", 0.15, 0.60, t1, None).await;

    let repo = PgPricingRepo::new(pool);
    let p = repo
        .find_for(None, "gpt-4o-mini", now)
        .await
        .unwrap()
        .unwrap();
    assert!((p.input_per_million - 0.30).abs() < 1e-9);

    // 2025 之后取新价
    let future = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let p = repo
        .find_for(None, "gpt-4o-mini", future)
        .await
        .unwrap()
        .unwrap();
    assert!((p.input_per_million - 0.15).abs() < 1e-9);
}

#[tokio::test]
async fn pg_no_pricing_returns_none() {
    let (_c, pool) = start_pg().await;
    let repo = PgPricingRepo::new(pool);
    let hit = repo
        .find_for(None, "no-such-model", Utc::now())
        .await
        .unwrap();
    assert!(hit.is_none());
}
