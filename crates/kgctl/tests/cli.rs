//! kgctl 集成测试 — 起真 PG/Redis 容器，跑二进制端到端验证。
//!
//! ⚠ 需要 Docker daemon 可用。
//!
//! 覆盖：
//! - migrate            （空库 → 跑完，输出最新版本号）
//! - migrate --dry-run  （已迁移库上 → "no pending"）
//! - admin create       （新建用户能查到；同 email 二次报错）
//! - doctor             （全 env + migration + Redis Lua 正确 → exit 0；缺 DB → exit 1）
//! - seed-pricing       （首次插入 + 二次幂等不报错）

use assert_cmd::Command;
use predicates::prelude::*;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;
use testcontainers_modules::redis::Redis as RedisImage;

/// 启动一个 PG 容器，返回 (container guard, postgres URL)
async fn start_pg() -> (testcontainers::ContainerAsync<PgImage>, String) {
    let tag = std::env::var("KOOIX_TEST_PG_TAG").unwrap_or_else(|_| "17-alpine".into());
    let container = PgImage::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    (container, url)
}

async fn start_redis() -> (testcontainers::ContainerAsync<RedisImage>, String) {
    let tag = std::env::var("KOOIX_TEST_REDIS_TAG").unwrap_or_else(|_| "7-alpine".into());
    let container = RedisImage::default()
        .with_tag(&tag)
        .start()
        .await
        .expect("start redis");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{host}:{port}");
    (container, url)
}

fn kg() -> Command {
    Command::cargo_bin("kgctl").expect("kgctl binary built")
}

// 32B base64 master key（确定性，仅供测试）
const TEST_MASTER_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
// 64B base64 jwt secret
const TEST_JWT_SECRET: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

// ────────────────────────────────────────────────────────────────────────────
// 1. migrate
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn migrate_runs_on_empty_db_and_prints_latest_version() {
    let (_c, url) = start_pg().await;

    // 跑 migrate 子命令
    let assert = tokio::task::spawn_blocking(move || {
        kg().arg("migrate")
            .env("KOOIX_DATABASE_URL", &url)
            .assert()
            .success()
            .stdout(predicate::str::contains("latest migration version"))
    })
    .await
    .unwrap();
    drop(assert);
}

#[tokio::test(flavor = "multi_thread")]
async fn migrate_dry_run_after_apply_says_no_pending() {
    let (_c, url) = start_pg().await;

    // 先实际跑一次
    let url_clone = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().arg("migrate")
            .env("KOOIX_DATABASE_URL", &url_clone)
            .assert()
            .success();
    })
    .await
    .unwrap();

    // 再 dry-run
    tokio::task::spawn_blocking(move || {
        kg().args(["migrate", "--dry-run"])
            .env("KOOIX_DATABASE_URL", &url)
            .assert()
            .success()
            .stdout(predicate::str::contains("no pending"));
    })
    .await
    .unwrap();
}

// ────────────────────────────────────────────────────────────────────────────
// 2. admin create
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn admin_create_persists_user_and_super_admin_role() {
    let (_c, url) = start_pg().await;

    // migrate first
    {
        let url = url.clone();
        tokio::task::spawn_blocking(move || {
            kg().arg("migrate")
                .env("KOOIX_DATABASE_URL", &url)
                .assert()
                .success();
        })
        .await
        .unwrap();
    }

    // create admin
    let url_create = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().args([
            "admin",
            "create",
            "--email",
            "root@example.com",
            "--password",
            "supersecret-12345",
        ])
        .env("KOOIX_DATABASE_URL", &url_create)
        .assert()
        .success()
        .stdout(predicate::str::contains("super_admin"))
        .stdout(predicate::str::contains("root@example.com"));
    })
    .await
    .unwrap();

    // 直接连库验证记录存在
    let pool = gate_storage::connect(&url, 2).await.unwrap();
    let row: (String, String) = sqlx::query_as(
        "SELECT u.email::text, pa.role
         FROM users u JOIN platform_admins pa ON pa.user_id = u.id
         WHERE u.email = $1",
    )
    .bind("root@example.com")
    .fetch_one(&pool)
    .await
    .expect("user exists");
    assert_eq!(row.0, "root@example.com");
    assert_eq!(row.1, "super_admin");

    // 第二次同 email 必须失败
    let url_dup = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().args([
            "admin",
            "create",
            "--email",
            "root@example.com",
            "--password",
            "another-pwd-1234",
        ])
        .env("KOOIX_DATABASE_URL", &url_dup)
        .assert()
        .failure()
        .stderr(predicate::str::contains("已存在"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_create_auto_generates_password_when_missing() {
    let (_c, url) = start_pg().await;

    {
        let url = url.clone();
        tokio::task::spawn_blocking(move || {
            kg().arg("migrate")
                .env("KOOIX_DATABASE_URL", &url)
                .assert()
                .success();
        })
        .await
        .unwrap();
    }

    let url_clone = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().args(["admin", "create", "--email", "auto@example.com"])
            .env("KOOIX_DATABASE_URL", &url_clone)
            .assert()
            .success()
            .stdout(predicate::str::contains("initial_password:"));
    })
    .await
    .unwrap();
}

// ────────────────────────────────────────────────────────────────────────────
// 3. doctor
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn doctor_passes_when_all_env_correct() {
    let (_pg, db_url) = start_pg().await;
    let (_redis, redis_url) = start_redis().await;

    {
        let db_url = db_url.clone();
        tokio::task::spawn_blocking(move || {
            kg().arg("migrate")
                .env("KOOIX_DATABASE_URL", &db_url)
                .assert()
                .success();
        })
        .await
        .unwrap();
    }

    tokio::task::spawn_blocking(move || {
        kg().arg("doctor")
            .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
            .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
            .env("KOOIX_PUBLIC_URL", "http://localhost:8000")
            .env("KOOIX_DATABASE_URL", &db_url)
            .env("KOOIX_REDIS_URL", &redis_url)
            .assert()
            .success()
            .stdout(predicate::str::contains("migration"))
            .stdout(predicate::str::contains("Lua OK"))
            .stdout(predicate::str::contains("所有检查通过"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_fails_without_database_url() {
    // 即使其他 env 都对，缺 DB 必须 exit 1
    tokio::task::spawn_blocking(|| {
        kg().arg("doctor")
            .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
            .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
            .env("KOOIX_PUBLIC_URL", "http://localhost:8000")
            .env_remove("KOOIX_DATABASE_URL")
            .env_remove("KOOIX_REDIS_URL")
            .assert()
            .failure()
            .stdout(predicate::str::contains("KOOIX_DATABASE_URL"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_fails_when_migrations_are_pending() {
    let (_pg, db_url) = start_pg().await;
    let (_redis, redis_url) = start_redis().await;

    tokio::task::spawn_blocking(move || {
        kg().arg("doctor")
            .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
            .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
            .env("KOOIX_PUBLIC_URL", "http://localhost:8000")
            .env("KOOIX_DATABASE_URL", &db_url)
            .env("KOOIX_REDIS_URL", &redis_url)
            .assert()
            .failure()
            .stdout(predicate::str::contains("migration 未到最新"));
    })
    .await
    .unwrap();
}

#[test]
fn doctor_fails_without_public_url() {
    kg().arg("doctor")
        .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
        .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
        .env_remove("KOOIX_PUBLIC_URL")
        .env_remove("KOOIX_DATABASE_URL")
        .env_remove("KOOIX_REDIS_URL")
        .assert()
        .failure()
        .stdout(predicate::str::contains("KOOIX_PUBLIC_URL"));
}

// ────────────────────────────────────────────────────────────────────────────
// 4. seed-pricing
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn seed_pricing_is_idempotent() {
    let (_c, url) = start_pg().await;

    {
        let url = url.clone();
        tokio::task::spawn_blocking(move || {
            kg().arg("migrate")
                .env("KOOIX_DATABASE_URL", &url)
                .assert()
                .success();
        })
        .await
        .unwrap();
    }

    let first_url = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().arg("seed-pricing")
            .env("KOOIX_DATABASE_URL", &first_url)
            .assert()
            .success()
            .stdout(predicate::str::contains("inserted 5"));
    })
    .await
    .unwrap();

    // 二次跑：全部 skip，但仍 exit 0
    let second_url = url.clone();
    tokio::task::spawn_blocking(move || {
        kg().arg("seed-pricing")
            .env("KOOIX_DATABASE_URL", &second_url)
            .assert()
            .success()
            .stdout(predicate::str::contains("inserted 0"))
            .stdout(predicate::str::contains("skipped 5"));
    })
    .await
    .unwrap();

    // 校验 DB：永久生效记录正好 5 条（非 NULL channel_id 不参与计数）
    let pool = gate_storage::connect(&url, 2).await.unwrap();
    let cnt: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM model_pricing
         WHERE channel_id IS NULL AND effective_until IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cnt, 5);
}

// ────────────────────────────────────────────────────────────────────────────
// 5. env / version 烟雾测试（不需要容器）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn env_lists_all_required_vars_including_oidc_redirect() {
    kg().arg("env")
        .assert()
        .success()
        .stdout(predicate::str::contains("KOOIX_MASTER_KEY"))
        .stdout(predicate::str::contains("KOOIX_DATABASE_URL"))
        .stdout(predicate::str::contains("KOOIX_REDIS_URL"))
        .stdout(predicate::str::contains("KOOIX_OIDC_DEFAULT_REDIRECT"));
}
