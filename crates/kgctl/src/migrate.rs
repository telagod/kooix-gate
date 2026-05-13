//! `kgctl migrate` — 跑 / 列出 PostgreSQL migrations
//!
//! 设计：
//! - 默认行为：连库 → 跑 `gate_storage::run_migrations` → 打印当前最新版本号
//! - `--dry-run`：列出 pending migration（不会创建 `_sqlx_migrations` 表外的副作用）
//! - 失败：anyhow 抛出，由 main 把消息着红色后 exit 1

use anyhow::{Context, Result};
use sqlx::Row;
use sqlx::migrate::MigrateDatabase;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashSet;
use std::time::Duration;

const ENV_DB: &str = "KOOIX_DATABASE_URL";

pub async fn run(dry_run: bool) -> Result<()> {
    let url = std::env::var(ENV_DB)
        .with_context(|| format!("环境变量 {ENV_DB} 未设置；先 export 一个 postgres URL"))?;

    // 数据库可能完全不存在（首次部署）：sqlx 不会自动建库，我们也保持谨慎不自动建，
    // 但如果 URL 指向的库不存在则给出明确报错。
    if !sqlx::Postgres::database_exists(&url)
        .await
        .with_context(|| "无法连到 PostgreSQL 实例 (database_exists 探测失败)")?
    {
        anyhow::bail!("数据库不存在：{url}\n请先 createdb 或在管理面板里建库");
    }

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| format!("连库失败：{url}"))?;

    let migrator = gate_storage::migrator();

    if dry_run {
        // 列出 pending：embedded migrator 全集 minus 已应用版本
        let applied = applied_versions(&pool).await.unwrap_or_default();
        let pending: Vec<_> = migrator
            .iter()
            .filter(|m| !applied.contains(&m.version))
            .collect();

        if pending.is_empty() {
            println!(
                "no pending migrations (latest version: {})",
                latest_version(&pool).await?.unwrap_or(0)
            );
        } else {
            println!("pending migrations ({}):", pending.len());
            for m in pending {
                println!("  - {} {}", m.version, m.description);
            }
        }
        return Ok(());
    }

    gate_storage::run_migrations(&pool)
        .await
        .with_context(|| "run_migrations 失败")?;

    let latest = latest_version(&pool).await?;
    match latest {
        Some(v) => println!("ok · latest migration version: {v}"),
        None => println!("ok · 无任何 migration 记录（空库）"),
    }
    Ok(())
}

async fn applied_versions(pool: &sqlx::PgPool) -> Result<HashSet<i64>> {
    let rows = sqlx::query("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await
        .with_context(|| "读 _sqlx_migrations 失败")?;
    let mut set = HashSet::new();
    for r in rows {
        set.insert(r.try_get::<i64, _>("version")?);
    }
    Ok(set)
}

async fn latest_version(pool: &sqlx::PgPool) -> Result<Option<i64>> {
    // _sqlx_migrations 在首次 migrate 后即存在；空库情况下表不存在 → 返回 None
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = current_schema() AND table_name = '_sqlx_migrations'
        )",
    )
    .fetch_one(pool)
    .await?;
    if !exists {
        return Ok(None);
    }
    let v: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(v)
}
