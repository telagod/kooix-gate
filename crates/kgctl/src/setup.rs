//! `kgctl setup` — 交互式初次启动引导
//!
//! 步骤：
//! 1. 生成 .env（密钥 + 连接串）
//! 2. doctor 体检
//! 3. migrate 建表
//! 4. 创建 super_admin
//! 5. 创建默认 Org + Project
//! 6. seed-pricing
//! 7. 打印总结

use anyhow::{Context, Result};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use std::io::{self, Write};
use std::time::Duration;
use uuid::Uuid;

const ENV_FILE: &str = ".env";

fn prompt(label: &str, default: &str) -> String {
    if default.is_empty() {
        eprint!("  {label}: ");
    } else {
        eprint!("  {label} [{default}]: ");
    }
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn prompt_password(label: &str) -> String {
    eprint!("  {label} (留空自动生成): ");
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        let pw = generate_password();
        eprintln!("  → 已生成: {pw}");
        pw
    } else {
        trimmed.to_string()
    }
}

fn generate_password() -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64URL};
    use rand::RngCore;
    let mut buf = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut buf);
    B64URL.encode(buf)
}

pub async fn run() -> Result<()> {
    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║           Kooix Gate · 初次启动引导                     ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // ── Step 1: .env ──────────────────────────────────────────
    let env_exists = std::path::Path::new(ENV_FILE).exists();
    let (db_url, redis_url);

    if env_exists {
        println!("[ 1/6 ] 检测到 .env 已存在，从中读取配置。");
        load_dotenv();
        db_url = std::env::var("KOOIX_DATABASE_URL")
            .context("KOOIX_DATABASE_URL 未在 .env 中设置")?;
        redis_url = std::env::var("KOOIX_REDIS_URL")
            .context("KOOIX_REDIS_URL 未在 .env 中设置")?;
    } else {
        println!("[ 1/6 ] 生成 .env 配置文件");
        println!();

        let master_key = gate_crypto::kms::generate_master_key_b64();
        let jwt_secret = gate_auth::jwt::generate_secret_b64();

        db_url = prompt(
            "PostgreSQL URL",
            "postgres://gate:gate_dev@localhost:5432/gate",
        );
        redis_url = prompt("Redis URL", "redis://localhost:6379/0");
        let listen_addr = prompt("监听地址", "0.0.0.0:8000");
        let public_url = prompt("公开 URL", "http://localhost:8000");

        let env_content = format!(
            "\
# ── Kooix Gate · 自动生成 ──
# 密钥丢失 = 所有加密数据失效，务必备份！

KOOIX_MASTER_KEY={master_key}
KOOIX_JWT_SECRET={jwt_secret}
KOOIX_DATABASE_URL={db_url}
KOOIX_REDIS_URL={redis_url}
KOOIX_LISTEN_ADDR={listen_addr}
KOOIX_PUBLIC_URL={public_url}
RUST_LOG=info,gate_server=debug
"
        );

        std::fs::write(ENV_FILE, &env_content)
            .with_context(|| format!("写 {ENV_FILE} 失败"))?;
        println!("  ✓ .env 已写入（密钥已自动生成）");
        println!();

        // SAFETY: setup 是单线程 CLI，此时无其他线程读 env
        unsafe {
            std::env::set_var("KOOIX_DATABASE_URL", &db_url);
            std::env::set_var("KOOIX_REDIS_URL", &redis_url);
            std::env::set_var("KOOIX_MASTER_KEY", &master_key);
            std::env::set_var("KOOIX_JWT_SECRET", &jwt_secret);
        }
    }

    // ── Step 2: Doctor ────────────────────────────────────────
    println!("[ 2/6 ] 连通性检查");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&db_url)
        .await
        .with_context(|| format!("PostgreSQL 连接失败：{}", redact(&db_url)))?;
    println!("  ✓ PostgreSQL 连通");

    if !redis_url.is_empty() {
        match gate_cache::connect(&redis_url, 2).await {
            Ok(_) => println!("  ✓ Redis 连通"),
            Err(e) => println!("  ⚠ Redis 连接失败（非致命）: {e}"),
        }
    }
    println!();

    // ── Step 3: Migrate ───────────────────────────────────────
    println!("[ 3/6 ] 数据库迁移");
    gate_storage::run_migrations(&pool)
        .await
        .context("migration 执行失败")?;
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(version) FROM _sqlx_migrations",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(None);
    println!("  ✓ 迁移完成 (latest: {})", version.unwrap_or(0));
    println!();

    // ── Step 4: Super Admin ───────────────────────────────────
    println!("[ 4/6 ] 创建平台管理员");
    let admin_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM platform_admins)",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    let admin_email;
    let admin_user_id: Uuid;

    if admin_exists {
        let row = sqlx::query(
            "SELECT u.id, u.email FROM users u
             JOIN platform_admins pa ON pa.user_id = u.id
             LIMIT 1",
        )
        .fetch_one(&pool)
        .await?;
        admin_user_id = row.try_get("id")?;
        admin_email = row.try_get::<String, _>("email")?;
        println!("  → 已有管理员: {admin_email} (跳过)");
    } else {
        admin_email = prompt("管理员邮箱", "admin@kooix.local");
        let admin_password = prompt_password("管理员密码");

        if admin_password.len() < 8 {
            anyhow::bail!("密码至少 8 个字符");
        }

        let phash = gate_auth::password::hash(&admin_password)
            .context("密码哈希失败")?;

        admin_user_id = Uuid::now_v7();
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, status)
             VALUES ($1, $2, $3, 'active')",
        )
        .bind(admin_user_id)
        .bind(&admin_email)
        .bind(&phash)
        .execute(&mut *tx)
        .await
        .context("创建用户失败")?;

        sqlx::query(
            "INSERT INTO platform_admins (user_id, role) VALUES ($1, 'super_admin')",
        )
        .bind(admin_user_id)
        .execute(&mut *tx)
        .await
        .context("授予管理员失败")?;

        tx.commit().await?;
        println!("  ✓ 管理员 {admin_email} 已创建");
    }
    println!();

    // ── Step 5: Default Org + Project ─────────────────────────
    println!("[ 5/6 ] 创建默认组织和项目");
    let org_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM organizations WHERE owner_user_id = $1)",
    )
    .bind(admin_user_id)
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    let org_id: Uuid;
    if org_exists {
        let row = sqlx::query(
            "SELECT id, name FROM organizations WHERE owner_user_id = $1 LIMIT 1",
        )
        .bind(admin_user_id)
        .fetch_one(&pool)
        .await?;
        org_id = row.try_get("id")?;
        let org_name: String = row.try_get("name")?;
        println!("  → 已有组织: {org_name} (跳过)");
    } else {
        let org_name = prompt("组织名称", "default");
        let org_slug = prompt("组织 slug", "default");

        org_id = Uuid::now_v7();
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO organizations (id, name, slug, owner_user_id, status)
             VALUES ($1, $2, $3, $4, 'active')",
        )
        .bind(org_id)
        .bind(&org_name)
        .bind(&org_slug)
        .bind(admin_user_id)
        .execute(&mut *tx)
        .await
        .context("创建组织失败")?;

        sqlx::query(
            "INSERT INTO org_memberships (org_id, user_id, role) VALUES ($1, $2, 'owner')",
        )
        .bind(org_id)
        .bind(admin_user_id)
        .execute(&mut *tx)
        .await
        .context("创建组织成员关系失败")?;

        tx.commit().await?;
        println!("  ✓ 组织 \"{org_name}\" 已创建");
    }

    let proj_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM projects WHERE org_id = $1)",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    if proj_exists {
        println!("  → 已有项目 (跳过)");
    } else {
        let proj_name = prompt("项目名称", "default");
        let proj_slug = prompt("项目 slug", "default");
        let proj_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO projects (id, org_id, name, slug, status)
             VALUES ($1, $2, $3, $4, 'active')",
        )
        .bind(proj_id)
        .bind(org_id)
        .bind(&proj_name)
        .bind(&proj_slug)
        .execute(&pool)
        .await
        .context("创建项目失败")?;
        println!("  ✓ 项目 \"{proj_name}\" 已创建");
    }
    println!();

    // ── Step 6: Seed Pricing ──────────────────────────────────
    println!("[ 6/6 ] 写入默认模型定价");
    crate::pricing::seed().await?;
    println!();

    // ── Summary ───────────────────────────────────────────────
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║                   初始化完成                            ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("  管理员     : {admin_email}");
    println!("  数据库     : {}", redact(&db_url));
    println!("  .env       : {ENV_FILE}");
    println!();
    println!("  启动后端:");
    println!("    cargo run -p gate-server");
    println!();
    println!("  启动前端:");
    println!("    cd web && npm run dev");
    println!();
    println!("  Docker 一键启动:");
    println!("    docker compose up");
    println!();
    println!("  浏览器打开 http://localhost:8000 (后端)");
    println!("         或 http://localhost:5173 (前端 dev)");
    println!();

    Ok(())
}

fn redact(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((auth, host)) = rest.split_once('@')
        && let Some((user, _pw)) = auth.split_once(':')
    {
        return format!("{scheme}://{user}:****@{host}");
    }
    url.to_string()
}

fn load_dotenv() {
    let Ok(content) = std::fs::read_to_string(ENV_FILE) else {
        return;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            if std::env::var(k).is_err() {
                // SAFETY: setup 是单线程 CLI
                unsafe { std::env::set_var(k, v) };
            }
        }
    }
}
