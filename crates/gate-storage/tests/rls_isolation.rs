//! RLS isolation integration test.
//!
//! Verifies that when `SET LOCAL app.current_org_id` is applied within a
//! transaction, cross-org reads are blocked by PostgreSQL RLS policies.
//!
//! NOTE: These tests run against the superuser connection (postgres/postgres),
//! which normally BYPASSES RLS as a table owner. To exercise the policies we
//! explicitly `SET LOCAL ROLE gate_app` within transactions. The `gate_app` role
//! is created by migration `20260514000001_rls_roles.sql`.

use gate_core::id::{OrgId, ProjectId};
use gate_storage::rls::RlsContext;
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

/// Seed an org + user so FK constraints are satisfied, then insert a project
/// belonging to that org. Returns (org_id, project_id).
async fn seed_org_with_project(pool: &sqlx::PgPool) -> (OrgId, ProjectId, Uuid) {
    let org_id = OrgId::new();
    let user_id = Uuid::now_v7();
    let proj_id = ProjectId::new();

    // user
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash, status)
         VALUES ($1, $2, 'test', 'hash', 'active')",
    )
    .bind(user_id)
    .bind(format!("test-{}@example.com", user_id))
    .execute(pool)
    .await
    .unwrap();

    // org
    sqlx::query("INSERT INTO organizations (id, name, slug, owner_user_id, status) VALUES ($1, $2, $3, $4, 'active')")
        .bind(org_id.as_uuid())
        .bind(format!("org-{}", org_id.as_uuid()))
        .bind(format!("slug-{}", org_id.as_uuid()))
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

    // membership
    sqlx::query("INSERT INTO org_memberships (org_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(org_id.as_uuid())
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

    // project
    sqlx::query(
        "INSERT INTO projects (id, org_id, name, slug, status)
         VALUES ($1, $2, $3, $4, 'active')",
    )
    .bind(proj_id.as_uuid())
    .bind(org_id.as_uuid())
    .bind(format!("proj-{}", proj_id.as_uuid()))
    .bind(format!("p-{}", proj_id.as_uuid()))
    .execute(pool)
    .await
    .unwrap();

    // api_key
    let api_key_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO api_keys (id, project_id, name, key_hash, key_prefix, key_last4, created_by)
         VALUES ($1, $2, 'rls-test', $3, 'sk-kg-rls', 'rlst', $4)",
    )
    .bind(api_key_id)
    .bind(proj_id.as_uuid())
    .bind(format!("hash_rls_{}", api_key_id))
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    (org_id, proj_id, api_key_id)
}

/// When scoped to org_a, projects belonging to org_b must be invisible.
#[tokio::test]
async fn rls_isolates_projects_across_orgs() {
    let (_c, pool) = start_pg().await;

    let (org_a, proj_a, _key_a) = seed_org_with_project(&pool).await;
    let (org_b, proj_b, _key_b) = seed_org_with_project(&pool).await;

    // Verify both projects exist (superuser sees all)
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(total.0 >= 2, "superuser should see all projects");

    // Now query as gate_app with org_a context — should only see org_a's project
    let ctx_a = RlsContext::for_org(org_a);
    let mut tx = ctx_a.begin(&pool).await.unwrap();
    // Switch to gate_app role so RLS policies apply
    sqlx::query("SET LOCAL ROLE gate_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM projects")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let visible: Vec<Uuid> = rows.into_iter().map(|r| r.0).collect();

    assert!(
        visible.contains(proj_a.as_uuid()),
        "org_a's project must be visible"
    );
    assert!(
        !visible.contains(proj_b.as_uuid()),
        "org_b's project must NOT be visible under org_a context"
    );

    // Reverse: query as gate_app with org_b context
    let ctx_b = RlsContext::for_org(org_b);
    let mut tx = ctx_b.begin(&pool).await.unwrap();
    sqlx::query("SET LOCAL ROLE gate_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM projects")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let visible_b: Vec<Uuid> = rows.into_iter().map(|r| r.0).collect();

    assert!(visible_b.contains(proj_b.as_uuid()));
    assert!(!visible_b.contains(proj_a.as_uuid()));
}

/// Platform admin (is_platform_admin = true) should see all projects regardless of org.
#[tokio::test]
async fn rls_platform_admin_sees_all() {
    let (_c, pool) = start_pg().await;

    let (_org_a, proj_a, _key_a) = seed_org_with_project(&pool).await;
    let (_org_b, proj_b, _key_b) = seed_org_with_project(&pool).await;

    let ctx = RlsContext::platform_admin();
    let mut tx = ctx.begin(&pool).await.unwrap();
    sqlx::query("SET LOCAL ROLE gate_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM projects")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let visible: Vec<Uuid> = rows.into_iter().map(|r| r.0).collect();

    assert!(
        visible.contains(proj_a.as_uuid()),
        "admin should see org_a project"
    );
    assert!(
        visible.contains(proj_b.as_uuid()),
        "admin should see org_b project"
    );
}

/// Without any SET LOCAL context, gate_app sees nothing (RLS default-deny).
#[tokio::test]
async fn rls_no_context_sees_nothing() {
    let (_c, pool) = start_pg().await;

    let (_org, _proj, _key) = seed_org_with_project(&pool).await;

    // No RLS context — empty org_id, not platform admin
    let ctx = RlsContext {
        org_id: None,
        project_id: None,
        is_platform_admin: false,
    };
    let mut tx = ctx.begin(&pool).await.unwrap();
    sqlx::query("SET LOCAL ROLE gate_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM projects")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let visible: Vec<Uuid> = rows.into_iter().map(|r| r.0).collect();

    assert!(
        visible.is_empty(),
        "without org context, gate_app should see no projects"
    );
}

/// Usage records are also isolated by org_id via RLS.
#[tokio::test]
async fn rls_isolates_usage_records() {
    let (_c, pool) = start_pg().await;

    let (org_a, proj_a, key_a) = seed_org_with_project(&pool).await;
    let (org_b, proj_b, key_b) = seed_org_with_project(&pool).await;

    // Insert usage records for both orgs
    for (org, proj, key) in [(&org_a, &proj_a, &key_a), (&org_b, &proj_b, &key_b)] {
        sqlx::query(
            "INSERT INTO usage_records (ts, request_id, org_id, project_id, api_key_id, channel_id, model_requested, model_actual, tokens_in, tokens_out, cost_usd, status)
             VALUES (NOW(), gen_random_uuid(), $1, $2, $3, NULL, 'gpt-4', 'gpt-4', 100, 50, 0.0015, 200)",
        )
        .bind(org.as_uuid())
        .bind(proj.as_uuid())
        .bind(key)
        .execute(&pool)
        .await
        .unwrap();
    }

    // org_a should only see its own usage
    let ctx_a = RlsContext::for_org(org_a);
    let mut tx = ctx_a.begin(&pool).await.unwrap();
    sqlx::query("SET LOCAL ROLE gate_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_records")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(count, 1, "org_a should see only its own usage record");
}
