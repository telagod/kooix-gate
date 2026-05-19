//! kgctl 集成测试 — 起真 PG/Redis 容器，跑二进制端到端验证。
//!
//! ⚠ 需要 Docker daemon 可用。
//!
//! 覆盖：
//! - migrate            （空库 → 跑完，输出最新版本号）
//! - migrate --dry-run  （已迁移库上 → "no pending"）
//! - admin create       （新建用户能查到；同 email 二次报错）
//! - doctor             （全 env + migration + Redis Lua 正确 → exit 0；缺 DB → exit 1）
//! - smoke              （mock gate-server HTTP API：login → channel → api key → chat → usage）
//! - seed-pricing       （首次插入 + 二次幂等不报错）

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;
use testcontainers_modules::redis::Redis as RedisImage;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
async fn doctor_json_passes_and_reports_all_checks() {
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

    let output = tokio::task::spawn_blocking(move || {
        kg().args(["doctor", "--json"])
            .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
            .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
            .env("KOOIX_PUBLIC_URL", "http://localhost:8000")
            .env("KOOIX_DATABASE_URL", &db_url)
            .env("KOOIX_REDIS_URL", &redis_url)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    })
    .await
    .unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("doctor --json stdout must be valid JSON");
    assert_eq!(value["ok"], true);
    let checks = value["checks"].as_array().expect("checks array");
    assert_eq!(checks.len(), 5);
    for name in [
        "KOOIX_MASTER_KEY",
        "KOOIX_JWT_SECRET",
        "KOOIX_PUBLIC_URL",
        "KOOIX_DATABASE_URL",
        "KOOIX_REDIS_URL",
    ] {
        let check = checks
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("missing check {name}"));
        assert_eq!(check["ok"], true);
        assert!(check["detail"].as_str().unwrap_or_default().len() > 2);
    }
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

#[test]
fn doctor_json_failure_is_machine_readable() {
    let output = kg()
        .args(["doctor", "--json"])
        .env("KOOIX_MASTER_KEY", TEST_MASTER_KEY)
        .env("KOOIX_JWT_SECRET", TEST_JWT_SECRET)
        .env_remove("KOOIX_PUBLIC_URL")
        .env_remove("KOOIX_DATABASE_URL")
        .env_remove("KOOIX_REDIS_URL")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("doctor --json failure stdout must be valid JSON");
    assert_eq!(value["ok"], false);
    let checks = value["checks"].as_array().expect("checks array");
    let public_url = checks
        .iter()
        .find(|c| c["name"] == "KOOIX_PUBLIC_URL")
        .expect("public url check");
    assert_eq!(public_url["ok"], false);
    assert_eq!(public_url["detail"], "未设置");
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

#[test]
fn plugin_schema_and_lint_work_without_services() {
    kg().args(["plugin", "schema"])
        .assert()
        .success()
        .stdout(predicate::str::contains("plugin"))
        .stdout(predicate::str::contains("capabilities"))
        .stdout(predicate::str::contains("auth"));

    let mut lint = kg();
    lint.args(["plugin", "lint", "--base-url", "https://api.example.com/v1"])
        .write_stdin(
            r#"{
              "plugin": {
                "version": 1,
                "capabilities": { "chat": true, "streaming": true },
                "auth": { "strategy": "bearer", "secret_slot": "primary" },
                "preset": { "provider": "openai_compatible" }
              }
            }"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("plugin manifest ok"));

    let mut bad = kg();
    bad.args(["plugin", "lint", "--base-url", "https://api.example.com/v1"])
        .write_stdin(
            r#"{
              "plugin": {
                "version": 1,
                "auth": { "strategy": "api_key_header" }
              }
            }"#,
        )
        .assert()
        .failure()
        .stderr(predicate::str::contains("/plugin/auth/header_name"));
}

#[test]
fn plugin_replay_export_import_fixture_roundtrip() {
    let dir = std::env::temp_dir().join(format!("kgctl-plugin-fixture-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("manifest.json");
    let sse_path = dir.join("sample.sse");
    let fixture_path = dir.join("fixture.json");

    std::fs::write(
        &manifest_path,
        r#"{
          "plugin": {
            "version": 1,
            "capabilities": { "chat": true, "streaming": true },
            "auth": { "strategy": "bearer", "secret_slot": "primary" },
            "request": {
              "path": "/private/chat",
              "body": {
                "modelName": "{{model}}",
                "prompt": "{{last_user_message}}",
                "stream": "{{stream}}"
              }
            },
            "response": {
              "openai_compatible": false,
              "content_path": "result.text",
              "usage": {
                "prompt_tokens_path": "usage.input",
                "completion_tokens_path": "usage.output"
              }
            },
            "stream": {
              "openai_compatible": false,
              "event_path": "payload",
              "content_path": "token",
              "finish_reason_path": "finish",
              "done_path": "type",
              "done_values": ["message_stop"],
              "usage": {
                "prompt_tokens_path": "usage.input",
                "completion_tokens_path": "usage.output"
              }
            }
          }
        }"#,
    )
    .unwrap();
    std::fs::write(
        &sse_path,
        r#"event: token
data: {"payload":{"rid":"r1","model_name":"native","speaker":"assistant"}}

data: {"payload":{"token":"he"}}

data: {"payload":{"token":"llo"}}

data: {"payload":{"finish":"done","usage":{"input":3,"output":2}}}

data: {"payload":{"type":"message_stop"}}

"#,
    )
    .unwrap();

    kg().args([
        "plugin",
        "lint",
        manifest_path.to_str().unwrap(),
        "--base-url",
        "https://api.example.com/v1",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("plugin manifest ok"));

    kg().args([
        "plugin",
        "replay",
        manifest_path.to_str().unwrap(),
        "--sse",
        sse_path.to_str().unwrap(),
        "--base-url",
        "https://api.example.com/v1",
        "--model",
        "native",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"content\": \"he\""))
    .stdout(predicate::str::contains("\"content\": \"llo\""))
    .stdout(predicate::str::contains("total_tokens"));

    kg().args([
        "plugin",
        "export",
        manifest_path.to_str().unwrap(),
        "--sse",
        sse_path.to_str().unwrap(),
        "--output",
        fixture_path.to_str().unwrap(),
        "--base-url",
        "https://api.example.com/v1",
        "--model",
        "native",
    ])
    .assert()
    .success();

    kg().args([
        "plugin",
        "import",
        fixture_path.to_str().unwrap(),
        "--verify",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("plugin fixture ok"));

    let fixture = std::fs::read_to_string(&fixture_path).unwrap();
    assert!(fixture.contains("expected_chunks"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_test_posts_to_mock_upstream() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_json(json!({
            "modelName": "odd-model",
            "prompt": "ping",
            "stream": false,
            "limit": 1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": { "text": "mapped ok", "finish": "stop" },
            "usage": { "input": 2, "output": 3 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = std::env::temp_dir().join(format!("kgctl-plugin-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("manifest.json");
    std::fs::write(
        &manifest_path,
        r#"{
          "plugin": {
            "version": 1,
            "capabilities": { "chat": true },
            "auth": { "strategy": "bearer", "secret_slot": "primary" },
            "request": {
              "path": "/private/chat",
              "body": {
                "modelName": "{{model}}",
                "prompt": "{{last_user_message}}",
                "stream": "{{stream}}",
                "limit": "{{max_tokens}}"
              }
            },
            "response": {
              "openai_compatible": false,
              "content_path": "result.text",
              "finish_reason_path": "result.finish",
              "usage": {
                "prompt_tokens_path": "usage.input",
                "completion_tokens_path": "usage.output"
              }
            }
          }
        }"#,
    )
    .unwrap();

    kg().args([
        "plugin",
        "test",
        manifest_path.to_str().unwrap(),
        "--base-url",
        &server.uri(),
        "--api-key",
        "test-key",
        "--model",
        "odd-model",
        "--prompt",
        "ping",
        "--max-tokens",
        "1",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("mapped ok"))
    .stdout(predicate::str::contains("total_tokens"));

    let _ = std::fs::remove_dir_all(dir);
}

// ────────────────────────────────────────────────────────────────────────────
// 6. smoke
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn smoke_walks_login_channel_apikey_chat_usage_flow() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/auth/login"))
        .and(body_json(json!({
            "email": "root@example.com",
            "password": "supersecret-12345"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "user-token",
            "refresh_token": "refresh-token",
            "expires_at": "2026-05-19T00:00:00Z",
            "user": { "id": "usr_019e2c1ba7d17162842207e4b24f5f98", "email": "root@example.com", "display_name": null }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/me"))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "subject": { "kind": "user", "user_id": "usr_019e2c1ba7d17162842207e4b24f5f98", "session_id": "019e2c1b-a7d1-7162-8422-07e4b24f5f98" },
            "current_org": "org_019e2c1ba7d17162842207e4b24f5f98",
            "is_platform_admin": true,
            "orgs": ["org_019e2c1ba7d17162842207e4b24f5f98"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/v1/orgs/019e2c1b-a7d1-7162-8422-07e4b24f5f98/projects",
        ))
        .and(header("authorization", "Bearer user-token"))
        .and(header(
            "x-kooix-org",
            "org_019e2c1ba7d17162842207e4b24f5f98",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "proj_019e2c1ba7d17162842207e4b24f5f99",
            "name": "Smoke",
            "slug": "smoke",
            "status": "active"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/admin/channels"))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ch_019e2c1ba7d17162842207e4b24f5f90",
            "code": "smoke",
            "name": "Smoke",
            "provider_type": "openai",
            "base_url": "http://upstream.test/v1",
            "status": "active",
            "health": "healthy",
            "supported_models": ["gpt-4o-mini"],
            "rpm_limit": null,
            "tpm_limit": null,
            "timeout_ms": 10000,
            "max_retries": 0,
            "tags": ["kgctl-smoke"],
            "model_mapping": null,
            "balance": null,
            "balance_updated_at": null,
            "last_error": null,
            "last_error_at": null,
            "created_at": "2026-05-19T00:00:00Z",
            "updated_at": "2026-05-19T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f90/keys",
        ))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chk_019e2c1ba7d17162842207e4b24f5f91",
            "channel_id": "ch_019e2c1ba7d17162842207e4b24f5f90",
            "label": "kgctl-smoke",
            "fingerprint": "abc123",
            "weight": 1,
            "health": "healthy",
            "total_requests": 0,
            "total_errors": 0,
            "consecutive_errors": 0,
            "last_error_code": null,
            "last_error_at": null,
            "cooldown_until": null,
            "created_at": "2026-05-19T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/admin/groups"))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "grp_019e2c1ba7d17162842207e4b24f5f92",
            "name": "Smoke",
            "description": "",
            "strategy": "priority",
            "enabled": true,
            "fallback_group_id": null,
            "channel_count": 0,
            "created_at": "2026-05-19T00:00:00Z",
            "updated_at": "2026-05-19T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/v1/admin/groups/019e2c1b-a7d1-7162-8422-07e4b24f5f92/bindings",
        ))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path(
            "/v1/admin/projects/019e2c1b-a7d1-7162-8422-07e4b24f5f99/default-group",
        ))
        .and(header("authorization", "Bearer user-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/orgs/019e2c1b-a7d1-7162-8422-07e4b24f5f98/projects/019e2c1b-a7d1-7162-8422-07e4b24f5f99/api-keys"))
        .and(header("authorization", "Bearer user-token"))
        .and(header("x-kooix-org", "org_019e2c1ba7d17162842207e4b24f5f98"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "key_019e2c1ba7d17162842207e4b24f5f93",
            "name": "kgctl-smoke",
            "plaintext": "sk-kg-smoke-plaintext",
            "prefix": "sk-kg",
            "last4": "text"
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-kg-smoke-plaintext"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-smoke",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/usage"))
        .and(header("authorization", "Bearer user-token"))
        .and(header(
            "x-kooix-org",
            "org_019e2c1ba7d17162842207e4b24f5f98",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "range": "7d",
            "group_by": "day",
            "from": "2026-05-12T00:00:00Z",
            "to": "2026-05-19T00:00:00Z",
            "total_cost_usd": 0.0,
            "total_tokens_in": 0,
            "total_tokens_out": 0,
            "series": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let base = server.uri();
    tokio::task::spawn_blocking(move || {
        kg().args([
            "smoke",
            "--base-url",
            &base,
            "--email",
            "root@example.com",
            "--password",
            "supersecret-12345",
            "--upstream-base-url",
            "http://upstream.test/v1",
            "--upstream-api-key",
            "sk-upstream",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("smoke ok"))
        .stdout(predicate::str::contains("create channel/group binding"))
        .stdout(predicate::str::contains("chat completions"));
    })
    .await
    .unwrap();
}
