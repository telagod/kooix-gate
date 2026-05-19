//! `kgctl doctor` — 部署体检
//!
//! 检查项：
//! 1. KOOIX_MASTER_KEY 存在 + base64 解码后正好 32B
//! 2. KOOIX_JWT_SECRET 存在 + base64 解码后 ≥ 32B
//! 3. KOOIX_JWT_PREVIOUS_SECRETS 可选；设置时逗号分隔 base64，每项 ≥ 32B
//! 4. KOOIX_PUBLIC_URL 存在 + URL 形态正确
//! 5. KOOIX_DATABASE_URL 存在 + 实际能 SELECT 1 + migration 已到最新
//! 6. KOOIX_REDIS_URL    存在 + 实际能 PING + Lua 脚本可执行
//!
//! 任一失败 → exit 1（main 把 anyhow::Err 着红色打印）。
//! `--json` 输出机器可读报告，stderr 仍保留失败摘要，方便 CI / deploy pipeline 同时解析 stdout。

use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use gate_cache::{QuotaCounter, RateLimiter};
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

const OK: &str = "\x1b[32m✓\x1b[0m";
const FAIL: &str = "\x1b[31m✗\x1b[0m";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DoctorOutput {
    Human,
    Json,
}

impl DoctorOutput {
    pub fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    ok: bool,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

pub async fn run(output: DoctorOutput) -> Result<()> {
    let report = collect_report().await;
    match output {
        DoctorOutput::Human => print_human(&report),
        DoctorOutput::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }

    if !report.ok {
        anyhow::bail!("doctor 发现 1 项以上失败");
    }
    Ok(())
}

async fn collect_report() -> DoctorReport {
    let checks = vec![
        check("KOOIX_MASTER_KEY", check_master_key()),
        check("KOOIX_JWT_SECRET", check_jwt_secret()),
        check("KOOIX_JWT_PREVIOUS_SECRETS", check_jwt_previous_secrets()),
        check("KOOIX_PUBLIC_URL", check_public_url()),
        check("KOOIX_DATABASE_URL", check_database().await),
        check("KOOIX_REDIS_URL", check_redis().await),
    ];
    let ok = checks.iter().all(|c| c.ok);
    DoctorReport { ok, checks }
}

fn check(name: &'static str, result: Result<String, String>) -> DoctorCheck {
    match result {
        Ok(detail) => DoctorCheck {
            name,
            ok: true,
            detail,
        },
        Err(detail) => DoctorCheck {
            name,
            ok: false,
            detail,
        },
    }
}

fn print_human(report: &DoctorReport) {
    for check in &report.checks {
        if check.ok {
            println!("{OK} {:<28} {}", check.name, check.detail);
        } else {
            println!("{FAIL} {:<28} {}", check.name, check.detail);
        }
    }
    if report.ok {
        println!("\n所有检查通过。");
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
    let raw = decode_jwt_secret(v.trim()).map_err(|e| format!("不是合法 base64: {e}"))?;
    ensure_jwt_secret_len(raw.len())?;
    Ok(format!("{}B 已就绪", raw.len()))
}

fn check_jwt_previous_secrets() -> Result<String, String> {
    let Ok(v) = std::env::var("KOOIX_JWT_PREVIOUS_SECRETS") else {
        return Ok("未配置（正常）".into());
    };
    let mut count = 0usize;
    for (idx, raw) in v
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
    {
        let bytes =
            decode_jwt_secret(raw).map_err(|e| format!("第 {idx} 项不是合法 base64: {e}"))?;
        ensure_jwt_secret_len(bytes.len()).map_err(|e| format!("第 {idx} 项 {e}"))?;
        count += 1;
    }
    if count == 0 {
        Ok("未配置（正常）".into())
    } else {
        Ok(format!("{count} 个旧 secret 已就绪（仅验签窗口）"))
    }
}

fn decode_jwt_secret(raw: &str) -> Result<Vec<u8>, base64::DecodeError> {
    B64.decode(raw)
}

fn ensure_jwt_secret_len(len: usize) -> Result<(), String> {
    if len < 32 {
        return Err(format!("解码后 {len} 字节 < 32（HS256 安全下限）"));
    }
    Ok(())
}

fn check_public_url() -> Result<String, String> {
    let v = std::env::var("KOOIX_PUBLIC_URL").map_err(|_| "未设置".to_string())?;
    let trimmed = v.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("不能为空".into());
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| format!("URL 解析失败: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme 必须是 http/https，当前是 {other}")),
    }
    if parsed.host_str().is_none() {
        return Err("缺少 host".into());
    }
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("必须是根 URL，不应带 path/query/fragment".into());
    }
    Ok(trimmed.to_string())
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

    let migrator = gate_storage::migrator();
    let expected = migrator.iter().map(|m| m.version).max().unwrap_or(0);
    let applied = latest_migration_version(&pool).await?;
    if applied < expected {
        return Err(format!(
            "migration 未到最新：当前 {applied}，期望 {expected}；先运行 kgctl migrate"
        ));
    }

    Ok(format!("已连通，migration {applied}/{expected}"))
}

async fn latest_migration_version(pool: &sqlx::PgPool) -> Result<i64, String> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = current_schema() AND table_name = '_sqlx_migrations'
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("读取 migration 表失败: {e}"))?;
    if !exists {
        return Ok(0);
    }
    let latest: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("读取 latest migration 失败: {e}"))?;
    Ok(latest.unwrap_or(0))
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

    match tokio::time::timeout(Duration::from_secs(3), client.ping::<()>()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = client.quit().await;
            drop(connect_task);
            return Err(format!("PING 失败: {e}"));
        }
        Err(_) => {
            let _ = client.quit().await;
            drop(connect_task);
            return Err("PING 超时 (3s)".into());
        }
    }

    let pool = gate_cache::connect(&url, 1)
        .await
        .map_err(|e| format!("连接池初始化失败: {e}"))?;
    let suffix = Uuid::now_v7().simple().to_string();
    let rl_key = format!("kgctl:doctor:rl:{suffix}");
    let quota_key = format!("kgctl:doctor:quota:{suffix}");

    let limiter = RateLimiter::new(pool.clone());
    let decision = limiter
        .check(&rl_key, 60_000, 1)
        .await
        .map_err(|e| format!("Lua sliding_window 执行失败: {e}"))?;
    if !decision.allowed {
        let _ = client.quit().await;
        drop(connect_task);
        return Err("Lua sliding_window 未允许首次请求".into());
    }

    let quota = QuotaCounter::new(pool);
    let debit = quota
        .debit(&quota_key, 1, 10, 60)
        .await
        .map_err(|e| format!("Lua quota_debit 执行失败: {e}"))?;
    if !debit.ok || debit.current_used != 1 {
        let _ = client.quit().await;
        drop(connect_task);
        return Err(format!("Lua quota_debit 返回异常: {debit:?}"));
    }
    let refund = quota
        .refund(&quota_key, 1)
        .await
        .map_err(|e| format!("Lua quota_refund 执行失败: {e}"))?;
    if refund.current_used != 0 {
        let _ = client.quit().await;
        drop(connect_task);
        return Err(format!("Lua quota_refund 返回异常: {refund:?}"));
    }

    let _ = client.quit().await;
    drop(connect_task); // 让后台任务随 client drop 退出
    Ok("PONG，Lua OK".into())
}
