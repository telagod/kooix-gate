//! gate-billing 集成测试：enqueue → consumer → usage_records

use chrono::Utc;
use gate_billing::{
    BillingLedgerEvent, Consumer, LedgerDirection, OutboxRepo, UsageEvent, consumer::commit_usage,
    insert_ledger_event, outbox::PgOutboxRepo, reconcile_usage_ledger,
};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

struct Fixture {
    org_id: Uuid,
    project_id: Uuid,
    api_key_id: Uuid,
}

async fn start_pg() -> (testcontainers::ContainerAsync<Postgres>, PgPool, Fixture) {
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

    let fix = seed_fixture(&pool).await;
    (container, pool, fix)
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let user_id = Uuid::now_v7();
    let org_id = Uuid::now_v7();
    let project_id = Uuid::now_v7();
    let api_key_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind("billing-test@test.dev")
    .bind("Test User")
    .bind("$argon2id$v=19$m=19456,t=2,p=1$placeholder$placeholder")
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO organizations (id, name, slug, owner_user_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(org_id)
    .bind("billing-test-org")
    .bind("billing-test-org")
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO projects (id, org_id, name, slug) VALUES ($1, $2, $3, $4)")
        .bind(project_id)
        .bind(org_id)
        .bind("billing-test-project")
        .bind("billing-test-project")
        .execute(pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO api_keys (id, project_id, name, key_hash, key_prefix, key_last4, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(api_key_id)
    .bind(project_id)
    .bind("test-key")
    .bind("fakehash_for_test_000000000000000000000000000000000000")
    .bind("sk-kg-test")
    .bind("test")
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    Fixture {
        org_id,
        project_id,
        api_key_id,
    }
}

fn make_event(fix: &Fixture) -> UsageEvent {
    UsageEvent {
        request_id: Uuid::now_v7(),
        idempotency_key: None,
        api_key_id: fix.api_key_id,
        project_id: fix.project_id,
        org_id: fix.org_id,
        channel_id: None,
        group_id: None,
        model: "gpt-4o-mini".to_string(),
        prompt_tokens: 10,
        completion_tokens: 5,
        cached_tokens: 0,
        reasoning_tokens: 0,
        image_units: 0,
        audio_seconds: 0.0,
        raw_usage: None,
        cost_micros: 150,
        occurred_at: Utc::now(),
        status: 200,
    }
}

#[tokio::test]
async fn enqueue_and_consume_three() {
    let (_c, pool, fix) = start_pg().await;
    let outbox = Arc::new(PgOutboxRepo::new(pool.clone()));

    for _ in 0..3 {
        outbox.enqueue(&make_event(&fix)).await.unwrap();
    }

    let consumer = Consumer::new(outbox.clone(), pool.clone(), 10, Duration::from_secs(9999));
    consumer.tick().await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3, "expected 3 usage_records");

    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE processed_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, 0, "expected 0 pending outbox events");
}

#[tokio::test]
async fn one_failure_doesnt_affect_others() {
    let (_c, pool, fix) = start_pg().await;

    let ev1 = make_event(&fix);
    let ev2 = make_event(&fix);
    commit_usage(&pool, &ev1).await.unwrap();
    commit_usage(&pool, &ev2).await.unwrap();

    let real_outbox = PgOutboxRepo::new(pool.clone());
    let ev3 = make_event(&fix);
    let ev4 = make_event(&fix);
    real_outbox.enqueue(&ev3).await.unwrap();
    real_outbox.enqueue(&ev4).await.unwrap();

    sqlx::query(
        "INSERT INTO outbox_events (topic, payload) \
         VALUES ('usage', '{\"bad\": true}'::jsonb)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "UPDATE outbox_events SET retry_count = 3 WHERE payload = '{\"bad\": true}'::jsonb",
    )
    .execute(&pool)
    .await
    .unwrap();

    let consumer = Consumer::new(
        Arc::new(PgOutboxRepo::new(pool.clone())),
        pool.clone(),
        10,
        Duration::from_secs(9999),
    );
    consumer.tick().await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        count, 4,
        "expected 4 usage_records (2 direct + 2 from consumer)"
    );
}

#[tokio::test]
async fn concurrent_consumers_do_not_double_consume() {
    let (_c, pool, fix) = start_pg().await;
    let outbox = Arc::new(PgOutboxRepo::new(pool.clone()));

    for _ in 0..100 {
        outbox.enqueue(&make_event(&fix)).await.unwrap();
    }

    let c1 = Consumer::new(outbox.clone(), pool.clone(), 10, Duration::from_secs(9999));
    let c2 = Consumer::new(outbox.clone(), pool.clone(), 10, Duration::from_secs(9999));

    let (r1, r2) = tokio::join!(
        async {
            for _ in 0..10 {
                c1.tick().await.unwrap();
            }
        },
        async {
            for _ in 0..10 {
                c2.tick().await.unwrap();
            }
        }
    );
    let _ = (r1, r2);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 100, "expected exactly 100 usage_records");

    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE processed_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, 0, "expected 0 pending outbox events");
}

#[tokio::test]
async fn commit_usage_writes_read_models_rollups_and_ledger_once() {
    let (_c, pool, fix) = start_pg().await;
    let mut event = make_event(&fix);
    event.idempotency_key = Some("idem-test-once".to_string());

    commit_usage(&pool, &event).await.unwrap();
    commit_usage(&pool, &event).await.unwrap();

    let usage_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(usage_count, 1);

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_count, 1);

    let hourly_requests: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(request_count), 0)::bigint FROM usage_hourly_rollups",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hourly_requests, 1);

    let daily_requests: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(request_count), 0)::bigint FROM usage_daily_rollups",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(daily_requests, 1);

    let ledger_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM billing_ledger_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ledger_count, 1);

    let event_type: String =
        sqlx::query_scalar("SELECT event_type FROM billing_ledger_events LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_type, "actual_settle");
}

#[tokio::test]
async fn ledger_event_model_accepts_p1_5_event_types() {
    let (_c, pool, fix) = start_pg().await;
    let request_id = Uuid::now_v7();
    let now = Utc::now();

    let events = [
        BillingLedgerEvent::estimated_debit(
            "ledger-estimated".to_string(),
            request_id,
            now,
            fix.org_id,
            fix.project_id,
            fix.api_key_id,
            None,
            900,
            serde_json::json!({"phase":"predebit"}),
        ),
        BillingLedgerEvent::actual_settle(
            &UsageEvent {
                request_id,
                idempotency_key: Some("ledger-settle".to_string()),
                api_key_id: fix.api_key_id,
                project_id: fix.project_id,
                org_id: fix.org_id,
                channel_id: None,
                group_id: None,
                model: "gpt-4o-mini".to_string(),
                prompt_tokens: 10,
                completion_tokens: 5,
                cached_tokens: 0,
                reasoning_tokens: 0,
                image_units: 0,
                audio_seconds: 0.0,
                raw_usage: None,
                cost_micros: 150,
                occurred_at: now,
                status: 200,
            },
            "ledger-settle".to_string(),
        ),
        BillingLedgerEvent::refund(
            "ledger-refund".to_string(),
            Some(request_id),
            now,
            fix.org_id,
            Some(fix.project_id),
            Some(fix.api_key_id),
            750,
            request_id.to_string(),
            serde_json::json!({"reason":"estimate_overrun"}),
        ),
        BillingLedgerEvent::manual_adjustment(
            "ledger-adjustment".to_string(),
            now,
            fix.org_id,
            Some(fix.project_id),
            42,
            LedgerDirection::Credit,
            "support-ticket-1".to_string(),
            serde_json::json!({"actor":"billing-admin"}),
        ),
        BillingLedgerEvent::invoice_close(
            "ledger-invoice-close".to_string(),
            now,
            fix.org_id,
            "2026-05".to_string(),
            150,
            serde_json::json!({"closed_by":"system"}),
        ),
    ];

    for event in events {
        assert!(
            insert_ledger_event(&pool, &event).await.unwrap(),
            "first insert must be effective"
        );
        assert!(
            !insert_ledger_event(&pool, &event).await.unwrap(),
            "idempotency key must dedupe ledger events"
        );
    }

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT event_type, direction FROM billing_ledger_events ORDER BY event_type",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 5);
    assert!(rows.contains(&("estimated_debit".to_string(), "debit".to_string())));
    assert!(rows.contains(&("actual_settle".to_string(), "debit".to_string())));
    assert!(rows.contains(&("refund".to_string(), "credit".to_string())));
    assert!(rows.contains(&("manual_adjustment".to_string(), "credit".to_string())));
    assert!(rows.contains(&("invoice_close".to_string(), "none".to_string())));
}

#[tokio::test]
async fn reconcile_usage_records_with_actual_settle_ledger() {
    let (_c, pool, fix) = start_pg().await;
    let mut event = make_event(&fix);
    event.idempotency_key = Some("ledger-reconcile-balanced".to_string());

    commit_usage(&pool, &event).await.unwrap();
    let from = event.occurred_at - chrono::Duration::minutes(1);
    let to = event.occurred_at + chrono::Duration::minutes(1);
    let balanced = reconcile_usage_ledger(&pool, Some(fix.org_id), from, to)
        .await
        .unwrap();

    assert!(balanced.is_balanced(), "balanced report: {balanced:?}");
    assert_eq!(balanced.usage_total_micros, event.cost_micros);
    assert_eq!(balanced.ledger_total_micros, event.cost_micros);

    sqlx::query(
        "UPDATE billing_ledger_events SET amount_micros = amount_micros + 1 WHERE request_id = $1",
    )
    .bind(event.request_id)
    .execute(&pool)
    .await
    .unwrap();

    let mismatched = reconcile_usage_ledger(&pool, Some(fix.org_id), from, to)
        .await
        .unwrap();
    assert!(!mismatched.is_balanced());
    assert_eq!(mismatched.amount_mismatches.len(), 1);
    assert_eq!(
        mismatched.amount_mismatches[0].usage_micros,
        Some(event.cost_micros)
    );
    assert_eq!(
        mismatched.amount_mismatches[0].ledger_micros,
        Some(event.cost_micros + 1)
    );
}
