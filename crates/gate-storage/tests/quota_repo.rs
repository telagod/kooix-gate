//! QuotaRepo 集成测试（testcontainers PG）。
//!
//! 覆盖：
//! - upsert 插入 + 同主体同维度 UPSERT 改 limit
//! - find_active_for 只返回 enabled=TRUE
//! - delete 行 + 删不存在的 ID 返回 NotFound
//! - list_by_scope 含 enabled + disabled
//! - 不同 scope_kind 同 scope_id 互不串扰
//! - model_filter NULL vs Some 视为不同行
//! - 多 dimension 同 scope 可共存

use gate_storage::{DbError, PgQuotaRepo, QuotaRepo, QuotaUpsert};
use rust_decimal::Decimal;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

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
async fn quota_upsert_inserts_then_updates_same_row() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool);

    let org_id = Uuid::new_v4();
    let q1 = repo
        .upsert(QuotaUpsert {
            scope_kind: "org".into(),
            scope_id: org_id,
            dimension: "rpm".into(),
            model_filter: None,
            limit_value: Decimal::from(100),
            window_seconds: Some(60),
        })
        .await
        .unwrap();
    assert_eq!(q1.limit_value, Decimal::from(100));

    // UPSERT 同主体+同维度+同 model_filter → 命中现有行
    let q2 = repo
        .upsert(QuotaUpsert {
            scope_kind: "org".into(),
            scope_id: org_id,
            dimension: "rpm".into(),
            model_filter: None,
            limit_value: Decimal::from(500),
            window_seconds: Some(60),
        })
        .await
        .unwrap();
    assert_eq!(q1.id, q2.id, "UPSERT should hit same row");
    assert_eq!(q2.limit_value, Decimal::from(500));

    // active 只有 1 条
    let active = repo.find_active_for("org", org_id).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].limit_value, Decimal::from(500));
}

#[tokio::test]
async fn quota_find_active_filters_disabled() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool.clone());

    let scope_id = Uuid::new_v4();
    let q = repo
        .upsert(QuotaUpsert {
            scope_kind: "project".into(),
            scope_id,
            dimension: "tpm".into(),
            model_filter: None,
            limit_value: Decimal::from(1000),
            window_seconds: Some(60),
        })
        .await
        .unwrap();

    // 默认 enabled=TRUE
    assert_eq!(
        repo.find_active_for("project", scope_id)
            .await
            .unwrap()
            .len(),
        1
    );

    // 手动 disable
    sqlx::query("UPDATE quotas SET enabled = FALSE WHERE id = $1")
        .bind(q.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        repo.find_active_for("project", scope_id)
            .await
            .unwrap()
            .len(),
        0
    );
    // list_by_scope 仍能看到
    assert_eq!(
        repo.list_by_scope("project", scope_id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn quota_delete_removes_row_or_not_found() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool);
    let scope_id = Uuid::new_v4();
    let q = repo
        .upsert(QuotaUpsert {
            scope_kind: "api_key".into(),
            scope_id,
            dimension: "rpm".into(),
            model_filter: Some("gpt-4o*".into()),
            limit_value: Decimal::from(20),
            window_seconds: Some(60),
        })
        .await
        .unwrap();

    repo.delete(q.id).await.unwrap();
    assert_eq!(
        repo.list_by_scope("api_key", scope_id).await.unwrap().len(),
        0
    );

    // 再删一次 → NotFound
    let err = repo.delete(q.id).await.unwrap_err();
    assert!(matches!(err, DbError::NotFound));
}

#[tokio::test]
async fn quota_scope_kind_isolated() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool);

    // 同一个 UUID 当 scope_id，但 scope_kind 不同 → 两条独立行
    let id = Uuid::new_v4();
    repo.upsert(QuotaUpsert {
        scope_kind: "org".into(),
        scope_id: id,
        dimension: "rpm".into(),
        model_filter: None,
        limit_value: Decimal::from(100),
        window_seconds: Some(60),
    })
    .await
    .unwrap();
    repo.upsert(QuotaUpsert {
        scope_kind: "project".into(),
        scope_id: id,
        dimension: "rpm".into(),
        model_filter: None,
        limit_value: Decimal::from(50),
        window_seconds: Some(60),
    })
    .await
    .unwrap();

    let org_q = repo.find_active_for("org", id).await.unwrap();
    let proj_q = repo.find_active_for("project", id).await.unwrap();
    assert_eq!(org_q.len(), 1);
    assert_eq!(proj_q.len(), 1);
    assert_eq!(org_q[0].limit_value, Decimal::from(100));
    assert_eq!(proj_q[0].limit_value, Decimal::from(50));
}

#[tokio::test]
async fn quota_model_filter_creates_separate_rows() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool);
    let scope_id = Uuid::new_v4();

    // 一条 None（所有模型），一条 Some("gpt-4o")
    repo.upsert(QuotaUpsert {
        scope_kind: "project".into(),
        scope_id,
        dimension: "rpm".into(),
        model_filter: None,
        limit_value: Decimal::from(100),
        window_seconds: Some(60),
    })
    .await
    .unwrap();
    repo.upsert(QuotaUpsert {
        scope_kind: "project".into(),
        scope_id,
        dimension: "rpm".into(),
        model_filter: Some("gpt-4o".into()),
        limit_value: Decimal::from(10),
        window_seconds: Some(60),
    })
    .await
    .unwrap();

    let rows = repo.find_active_for("project", scope_id).await.unwrap();
    assert_eq!(
        rows.len(),
        2,
        "model_filter NULL vs 'gpt-4o' should be 2 rows"
    );

    // 再 UPSERT 已存在的 Some("gpt-4o") → 更新而不是新增
    repo.upsert(QuotaUpsert {
        scope_kind: "project".into(),
        scope_id,
        dimension: "rpm".into(),
        model_filter: Some("gpt-4o".into()),
        limit_value: Decimal::from(20),
        window_seconds: Some(60),
    })
    .await
    .unwrap();
    let rows = repo.find_active_for("project", scope_id).await.unwrap();
    assert_eq!(rows.len(), 2);
    let gpt = rows
        .iter()
        .find(|r| r.model_filter.as_deref() == Some("gpt-4o"))
        .unwrap();
    assert_eq!(gpt.limit_value, Decimal::from(20));
}

#[tokio::test]
async fn quota_multiple_dimensions_coexist() {
    let (_c, pool) = start_pg().await;
    let repo = PgQuotaRepo::new(pool);
    let org_id = Uuid::new_v4();

    for (dim, val) in [
        ("rpm", Decimal::from(60)),
        ("tpm", Decimal::from(60_000)),
        ("daily_budget_usd", Decimal::from(100)),
    ] {
        repo.upsert(QuotaUpsert {
            scope_kind: "org".into(),
            scope_id: org_id,
            dimension: dim.into(),
            model_filter: None,
            limit_value: val,
            window_seconds: if dim == "daily_budget_usd" {
                None
            } else {
                Some(60)
            },
        })
        .await
        .unwrap();
    }

    let active = repo.find_active_for("org", org_id).await.unwrap();
    assert_eq!(active.len(), 3);
    let dims: std::collections::HashSet<_> = active.iter().map(|r| r.dimension.clone()).collect();
    assert!(dims.contains("rpm"));
    assert!(dims.contains("tpm"));
    assert!(dims.contains("daily_budget_usd"));
}
