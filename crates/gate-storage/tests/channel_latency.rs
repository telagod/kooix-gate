//! ChannelLatencyRepo 集成测试（testcontainers PG）。

use gate_core::id::ChannelId;
use gate_storage::{ChannelLatencyRepo, InMemoryChannelLatencyRepo, PgChannelLatencyRepo};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, sqlx::PgPool) {
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

#[tokio::test]
async fn pg_avg_latency_uses_recent_successful_samples() {
    let (_c, pool) = start_pg().await;
    let ch_a = ChannelId::new();
    let ch_b = ChannelId::new();

    for (id, code) in [(ch_a, "lat-a"), (ch_b, "lat-b")] {
        sqlx::query(
            "INSERT INTO channels (id, code, name, provider_type, base_url, config_enc, status, health) \
             VALUES ($1, $2, $2, 'openai', 'https://example.invalid/v1', '\\x'::bytea, 'active', 'healthy')",
        )
        .bind(id.as_uuid())
        .bind(code)
        .execute(&pool)
        .await
        .unwrap();
    }

    let repo = PgChannelLatencyRepo::new(pool.clone());
    repo.record_sample(ch_a, 200, true, "request")
        .await
        .unwrap();
    repo.record_sample(ch_a, 201, true, "request")
        .await
        .unwrap();
    repo.record_sample(ch_a, 300, false, "request")
        .await
        .unwrap();
    repo.record_sample(ch_b, 50, true, "health_probe")
        .await
        .unwrap();

    let avg = repo.avg_latency_ms(&[ch_a, ch_b], 300).await.unwrap();
    assert_eq!(avg.get(&ch_a), Some(&200));
    assert_eq!(avg.get(&ch_b), Some(&50));
}

#[tokio::test]
async fn in_memory_avg_latency_filters_failed_samples() {
    let repo = InMemoryChannelLatencyRepo::new();
    let ch = ChannelId::new();
    repo.record_sample(ch, 100, true, "request").await.unwrap();
    repo.record_sample(ch, 900, false, "request").await.unwrap();

    let avg = repo.avg_latency_ms(&[ch], 300).await.unwrap();
    assert_eq!(avg.get(&ch), Some(&100));
}
