//! `kgctl admin create` — 创建 platform super_admin
//!
//! 步骤：
//! 1. 连 DB
//! 2. 检查 email 是否已存在（存在即硬报错，绝不静默更新）
//! 3. 生成 uuid_v7 作 user_id
//! 4. 哈希密码（未提供则生成 24B base64url 随机密码并打印一次）
//! 5. INSERT INTO users + INSERT INTO platform_admins

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64URL};
use rand::RngCore;
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

const ENV_DB: &str = "KOOIX_DATABASE_URL";
const GEN_PASSWORD_BYTES: usize = 24;

pub async fn create(email: String, password: Option<String>) -> Result<()> {
    let url = std::env::var(ENV_DB)
        .with_context(|| format!("环境变量 {ENV_DB} 未设置；先 export 一个 postgres URL"))?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| format!("连库失败：{url}"))?;

    // 已存在即拒绝（保护：避免把既存用户静默提权为 super_admin）
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&pool)
        .await
        .with_context(|| "查 users 失败")?;
    if existing.is_some() {
        anyhow::bail!("email {email} 已存在；如需提权为 super_admin 请手工 INSERT platform_admins");
    }

    // 密码：给了就用，没给就生成
    let (plaintext, auto_generated) = match password {
        Some(p) => (p, false),
        None => (generate_password(), true),
    };

    let phash = gate_auth::password::hash(&plaintext)
        .with_context(|| "argon2 哈希密码失败（密码过短？至少 8 字符）")?;

    let user_id = Uuid::now_v7();

    // 事务里两张表一起写
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, status)
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&phash)
    .execute(&mut *tx)
    .await
    .with_context(|| "INSERT users 失败")?;

    sqlx::query(
        "INSERT INTO platform_admins (user_id, role)
         VALUES ($1, 'super_admin')",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .with_context(|| "INSERT platform_admins 失败")?;

    tx.commit().await?;

    // 读回一遍验证
    let check_row = sqlx::query(
        "SELECT u.id, u.email, pa.role
         FROM users u JOIN platform_admins pa ON pa.user_id = u.id
         WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .with_context(|| "回查创建结果失败")?;
    let role: String = check_row.try_get("role")?;
    debug_assert_eq!(role, "super_admin");

    println!("ok · super_admin 创建成功");
    println!("  user_id : {user_id}");
    println!("  email   : {email}");
    println!("  role    : {role}");
    if auto_generated {
        println!("  initial_password: {plaintext}   (仅本次显示，立即保存!)");
    }
    println!();
    println!("登录入口：POST {{PUBLIC_URL}}/v1/auth/login");

    Ok(())
}

fn generate_password() -> String {
    let mut buf = [0u8; GEN_PASSWORD_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    B64URL.encode(buf)
}
