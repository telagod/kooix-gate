//! `kgctl doctor` — 部署体检
//!
//! 检查项：
//! 1. KOOIX_MASTER_KEY 存在 + base64 解码后正好 32B
//! 2. KOOIX_JWT_SECRET 存在 + base64 解码后 ≥ 32B
//! 3. KOOIX_DATABASE_URL 存在 + 实际能 SELECT 1
//! 4. KOOIX_REDIS_URL    存在 + 实际能 PING
//!
//! 任一失败 → exit 1（main 把 anyhow::Err 着红色打印）

use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

const OK: &str = "\x1b[32m✓\x1b[0m";
const FAIL: &str = "\x1b[31m✗\x1b[0m";

pub async fn run() -> Result<()> {
    let mut all_ok = true;

    all_ok &= report("KOOIX_MASTER_KEY", check_master_key());
    all_ok &= report("KOOIX_JWT_SECRET", check_jwt_secret());

    all_ok &= report("KOOIX_DATABASE_URL", check_database().await);
    all_ok &= report("KOOIX_REDIS_URL", check_redis().await);

    if !all_ok {
        anyhow::bail!("doctor 发现 1 项以上失败");
    }
    println!("\n所有检查通过。");
    Ok(())
}

fn report(label: &str, result: Result<String, String>) -> bool {
    match result {
        Ok(detail) => {
            println!("{OK} {label:<28} {detail}");
            true
        }
        Err(reason) => {
            println!("{FAIL} {label:<28} {reason}");
            false
        }
    }
}

fn check_master_key() -> Result<String, String> {
    let v = std::env::var("KOOIX_MASTER_KEY").map_err(|_| "未设置".to_string())?;
    let raw = B64
        .decode(v.trim())
        .map_err(|e| format!("不是合法 base64: {e}"))?;
    if raw.len() != 32 {
        return Err(format!("解码后 {} 字节（应为 32）", raw.len()));
    }
    Ok("32B 已就绪".into())
}

fn check_jwt_secret() -> Result<String, String> {
    let v = std::env::var("KOOIX_JWT_SECRET").map_err(|_| "未设置".to_string())?;
    let raw = B64
        .decode(v.trim())
        .map_err(|e| format!("不是合法 base64: {e}"))?;
    if raw.len() < 32 {
        return Err(format!("解码后 {} 字节 < 32（HS256 安全下限）", raw.len()));
    }
    Ok(format!("{}B 已就绪", raw.len()))
}

async fn check_database() -> Result<String, String> {
    let url = std::env::var("KOOIX_DATABASE_URL").map_err(|_| "未设置".to_string())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("SELECT 1 失败: {e}"))?;
    if one != 1 {
        return Err(format!("SELECT 1 返回 {one}"));
    }
    Ok("已连通".into())
}

async fn check_redis() -> Result<String, String> {
    let url = std::env::var("KOOIX_REDIS_URL").map_err(|_| "未设置".to_string())?;
    // 用 fred 简单建一个客户端 ping 一下
    use fred::clients::RedisClient;
    use fred::interfaces::ClientLike;
    use fred::types::{ConnectionConfig, PerformanceConfig, ReconnectPolicy, RedisConfig};
    let config = RedisConfig::from_url(&url).map_err(|e| format!("URL 解析失败: {e}"))?;
    let conn = ConnectionConfig {
        connection_timeout: Duration::from_secs(5),
        ..Default::default()
    };
    // 重连策略：最多 1 次常量延迟 100ms；doctor 是一次性体检，不应阻塞等重连。
    let client = RedisClient::new(
        config,
        Some(PerformanceConfig::default()),
        Some(conn),
        Some(ReconnectPolicy::new_constant(1, 100)),
    );

    // connect 返回 join handle 跑后台事件循环；我们只关心首次连接是否成功。
    let connect_task = client.connect();
    // 5 秒超时，避免 URL 指向 blackhole 时卡死
    match tokio::time::timeout(Duration::from_secs(5), client.wait_for_connect()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("连接失败: {e}")),
        Err(_) => return Err("连接超时 (5s)".into()),
    }

    let ping_result = tokio::time::timeout(Duration::from_secs(3), client.ping::<()>()).await;
    let _ = client.quit().await;
    drop(connect_task); // 让后台任务随 client drop 退出
    match ping_result {
        Ok(Ok(())) => Ok("PONG".into()),
        Ok(Err(e)) => Err(format!("PING 失败: {e}")),
        Err(_) => Err("PING 超时 (3s)".into()),
    }
}
