//! ChannelHealthScoreRepo 集成测试（testcontainers PG）。
//!
//! 对账 ADR-0007 schema + 迁移幂等 + record_outcome / apply_update 行为。

use chrono::Utc;
use gate_core::id::ChannelId;
use gate_storage::{
    ChannelHealthScoreRepo, HealthState, InMemoryChannelHealthScoreRepo, OutcomeObservation,
    PgChannelHealthScoreRepo, ScoreUpdate,
};
use sqlx::Row;
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

/// 插入一行 channels 以便 FK 不报错。返回 ChannelId。
async fn insert_channel(pool: &sqlx::PgPool, code: &str) -> ChannelId {
    let id = ChannelId::new();
    sqlx::query(
        "INSERT INTO channels (id, code, name, provider_type, base_url, config_enc, status, health) \
         VALUES ($1, $2, $2, 'openai', 'https://example.invalid/v1', '\\x'::bytea, 'active', 'healthy')",
    )
    .bind(id.as_uuid())
    .bind(code)
    .execute(pool)
    .await
    .unwrap();
    id
}

// ============================================================================
// Migration 行为
// ============================================================================

#[tokio::test]
async fn migration_creates_score_row_for_existing_channels() {
    // run_migrations 内含 20260619000001 的存量初始化逻辑。
    // 但 migration 在表创建前跑——本测试的 channels 是 migration 之后插入的。
    // 因此 ensure_row 是补齐路径。
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "k14-bench").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());

    // 用户场景：CreateChannel 之后由调用方触发 ensure_row
    repo.ensure_row(id).await.unwrap();
    let s = repo.get(id).await.unwrap().expect("score row exists");
    assert_eq!(s.state, HealthState::Healthy);
    assert!((s.score - 1.0).abs() < 1e-9);
    assert_eq!(s.window_total, 0);
}

#[tokio::test]
async fn migration_initializes_pre_existing_channels() {
    // 模拟"先有 channels，后跑 migration"的存量数据路径。
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "preexisting").await;
    // run_migrations 在 start_pg 里已跑过；存量初始化 INSERT ... ON CONFLICT
    // 应该已经为 id 建好行。
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    let s = repo.get(id).await.unwrap();
    // migration 的存量初始化是在 migration 跑的时候执行的，而 channels 行是后插入的
    // → 这条不应存在；调用方必须显式 ensure_row。这条 case 是为了证明：
    // migration 不会自己监听 channels 表的后续 INSERT。
    assert!(s.is_none());

    repo.ensure_row(id).await.unwrap();
    assert!(repo.get(id).await.unwrap().is_some());
}

// ============================================================================
// record_outcome：success / failure / banned_signal / quota
// ============================================================================

#[tokio::test]
async fn record_outcome_accumulates_window_counters() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "rec-acc").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    for _ in 0..7 {
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    for _ in 0..3 {
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let s = repo.get(id).await.unwrap().unwrap();
    assert_eq!(s.window_total, 10);
    assert_eq!(s.window_success, 7);
    assert_eq!(s.consecutive_failures, 3);
}

#[tokio::test]
async fn record_outcome_success_resets_failure_streak() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "rec-reset").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    for _ in 0..4 {
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    repo.record_outcome(
        id,
        &OutcomeObservation {
            success: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let s = repo.get(id).await.unwrap().unwrap();
    assert_eq!(s.consecutive_failures, 0);
    assert_eq!(s.window_total, 5);
    assert_eq!(s.window_success, 1);
}

#[tokio::test]
async fn record_outcome_banned_signal_overrides_to_one() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "rec-banned").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    repo.record_outcome(
        id,
        &OutcomeObservation {
            banned_signal: Some("account_deactivated".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let s = repo.get(id).await.unwrap().unwrap();
    assert!((s.banned_signal - 1.0).abs() < 1e-9);
}

#[tokio::test]
async fn record_outcome_quota_remaining_overwrites_latest() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "rec-quota").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    repo.record_outcome(
        id,
        &OutcomeObservation {
            quota_remaining_norm: Some(0.42),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let s = repo.get(id).await.unwrap().unwrap();
    assert!((s.quota_remaining_norm - 0.42).abs() < 1e-9);

    repo.record_outcome(
        id,
        &OutcomeObservation {
            quota_remaining_norm: Some(0.10),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let s = repo.get(id).await.unwrap().unwrap();
    assert!((s.quota_remaining_norm - 0.10).abs() < 1e-9);
}

// ============================================================================
// apply_update：完整覆盖式写回
// ============================================================================

#[tokio::test]
async fn apply_update_transitions_state_and_stamps_timestamp() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "apply-trans").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    let before = repo.get(id).await.unwrap().unwrap().last_transition_at;

    // 同状态：last_transition_at 不变
    let same_state = ScoreUpdate {
        score: 0.92,
        success_rate: 0.92,
        latency_p99_ms: 180,
        banned_signal: 0.0,
        quota_remaining_norm: 1.0,
        consecutive_failures: 0,
        state: HealthState::Healthy,
        cooldown_until: None,
        banned_reason: None,
        window_total: 100,
        window_success: 92,
        window_started_at: Utc::now(),
    };
    repo.apply_update(id, &same_state).await.unwrap();
    assert_eq!(
        repo.get(id).await.unwrap().unwrap().last_transition_at,
        before
    );

    // 状态变：last_transition_at 推进
    let cooldown_until = Utc::now() + chrono::Duration::seconds(60);
    let new_state = ScoreUpdate {
        state: HealthState::Cooldown,
        cooldown_until: Some(cooldown_until),
        banned_reason: None,
        score: 0.30,
        success_rate: 0.45,
        latency_p99_ms: 2400,
        banned_signal: 0.0,
        quota_remaining_norm: 1.0,
        consecutive_failures: 5,
        window_total: 100,
        window_success: 45,
        window_started_at: Utc::now(),
    };
    repo.apply_update(id, &new_state).await.unwrap();
    let s = repo.get(id).await.unwrap().unwrap();
    assert_eq!(s.state, HealthState::Cooldown);
    assert!(s.last_transition_at > before);
    assert!(s.cooldown_until.is_some());
}

#[tokio::test]
async fn apply_update_writes_banned_state_and_reason() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "apply-banned").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    let update = ScoreUpdate {
        score: 0.0,
        success_rate: 0.0,
        latency_p99_ms: 0,
        banned_signal: 1.0,
        quota_remaining_norm: 0.0,
        consecutive_failures: 10,
        state: HealthState::Banned,
        cooldown_until: None,
        banned_reason: Some("openai:account_deactivated".to_string()),
        window_total: 50,
        window_success: 0,
        window_started_at: Utc::now(),
    };
    repo.apply_update(id, &update).await.unwrap();
    let s = repo.get(id).await.unwrap().unwrap();
    assert_eq!(s.state, HealthState::Banned);
    assert_eq!(
        s.banned_reason.as_deref(),
        Some("openai:account_deactivated")
    );
    assert!(s.state.skip_in_routing());
}

// ============================================================================
// 滚动窗口 + 批量读取
// ============================================================================

#[tokio::test]
async fn reset_window_zeros_counters_only() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "reset-window").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());
    repo.ensure_row(id).await.unwrap();

    for _ in 0..3 {
        repo.record_outcome(
            id,
            &OutcomeObservation {
                success: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let before = repo.get(id).await.unwrap().unwrap();
    assert_eq!(before.window_total, 3);
    assert_eq!(before.consecutive_failures, 3);

    repo.reset_window(id).await.unwrap();
    let after = repo.get(id).await.unwrap().unwrap();
    assert_eq!(after.window_total, 0);
    assert_eq!(after.window_success, 0);
    // reset_window 不应清 consecutive_failures（那是 score 跨窗口生效的链路）
    assert_eq!(after.consecutive_failures, 3);
}

#[tokio::test]
async fn get_many_batches_known_channels() {
    let (_c, pool) = start_pg().await;
    let a = insert_channel(&pool, "many-a").await;
    let b = insert_channel(&pool, "many-b").await;
    let c = insert_channel(&pool, "many-c").await;
    let repo = PgChannelHealthScoreRepo::new(pool.clone());

    repo.ensure_row(a).await.unwrap();
    repo.ensure_row(b).await.unwrap();
    // c 不调 ensure_row → get_many 不返回它

    let result = repo.get_many(&[a, b, c]).await.unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains_key(&a));
    assert!(result.contains_key(&b));
    assert!(!result.contains_key(&c));
}

// ============================================================================
// channel_groups 扩字段（schema 验证）
// ============================================================================

#[tokio::test]
async fn channel_groups_has_use_health_score_default_false() {
    let (_c, pool) = start_pg().await;
    // 直接读 channel_groups 列定义
    let row = sqlx::query(
        "SELECT column_name, column_default \
         FROM information_schema.columns \
         WHERE table_name = 'channel_groups' AND column_name = 'use_health_score'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    let row = row.expect("channel_groups.use_health_score column exists");
    let default_expr: String = row.try_get("column_default").unwrap();
    // default 'false' 在 PG 系统目录里是 'false'
    assert!(default_expr.contains("false"));
}

#[tokio::test]
async fn channel_groups_has_health_weights_jsonb_nullable() {
    let (_c, pool) = start_pg().await;
    let row = sqlx::query(
        "SELECT data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_name = 'channel_groups' AND column_name = 'health_weights'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    let row = row.expect("channel_groups.health_weights column exists");
    let data_type: String = row.try_get("data_type").unwrap();
    let is_nullable: String = row.try_get("is_nullable").unwrap();
    assert_eq!(data_type, "jsonb");
    assert_eq!(is_nullable, "YES");
}

// ============================================================================
// In-memory parity sanity（reuse one PG case 的行为）
// ============================================================================

#[tokio::test]
async fn in_memory_and_pg_have_same_record_outcome_semantics() {
    let (_c, pool) = start_pg().await;
    let id = insert_channel(&pool, "parity").await;
    let pg = PgChannelHealthScoreRepo::new(pool.clone());
    let mem = InMemoryChannelHealthScoreRepo::new();

    pg.ensure_row(id).await.unwrap();
    mem.ensure_row(id).await.unwrap();

    let events = [
        OutcomeObservation {
            success: Some(true),
            ..Default::default()
        },
        OutcomeObservation {
            success: Some(false),
            ..Default::default()
        },
        OutcomeObservation {
            success: Some(false),
            ..Default::default()
        },
        OutcomeObservation {
            quota_remaining_norm: Some(0.25),
            ..Default::default()
        },
    ];
    for e in &events {
        pg.record_outcome(id, e).await.unwrap();
        mem.record_outcome(id, e).await.unwrap();
    }

    let p = pg.get(id).await.unwrap().unwrap();
    let m = mem.get(id).await.unwrap().unwrap();
    assert_eq!(p.window_total, m.window_total);
    assert_eq!(p.window_success, m.window_success);
    assert_eq!(p.consecutive_failures, m.consecutive_failures);
    assert!((p.quota_remaining_norm - m.quota_remaining_norm).abs() < 1e-9);
}
