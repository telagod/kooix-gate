//! gate-server: HTTP 入口
//!
//! 启动流程：
//! 1. 加载配置 (KOOIX_* env + kooix-gate.toml 可选)
//! 2. 连接 PostgreSQL + 跑迁移
//! 3. 初始化 JwtIssuer (from KOOIX_JWT_SECRET base64)
//! 4. 装配 PgLoader（生产）/ InMemoryLoader（仅当 KOOIX_DEV_INMEMORY=1）
//! 5. 启动 Axum

use anyhow::Context;
use chrono::Duration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::loader::{AuthContextLoader, InMemoryLoader};
use gate_server::{build_router, AppState, Config, PgLoader};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = Config::load().context("loading config (set KOOIX_* env vars)")?;
    tracing::info!(addr = %cfg.listen_addr, "starting");

    let jwt = JwtIssuer::from_env(
        "KOOIX_JWT_SECRET",
        cfg.jwt_issuer.clone(),
        cfg.jwt_audience.clone(),
        TokenLifetimes {
            access: Duration::minutes(cfg.token_access_ttl_min),
            refresh: Duration::days(cfg.token_refresh_ttl_day),
        },
    )?;

    let loader: Arc<dyn AuthContextLoader> =
        if std::env::var("KOOIX_DEV_INMEMORY").as_deref() == Ok("1") {
            tracing::warn!("KOOIX_DEV_INMEMORY=1 — using InMemoryLoader (NOT for production)");
            Arc::new(InMemoryLoader::new())
        } else {
            tracing::info!(url = %redact_db_url(&cfg.database_url), "connecting postgres");
            let pool = gate_storage::connect(&cfg.database_url, 16)
                .await
                .context("connect postgres")?;
            gate_storage::run_migrations(&pool)
                .await
                .context("run migrations")?;
            tracing::info!("migrations applied");
            Arc::new(PgLoader::new(pool))
        };

    let state = AppState::new(jwt, loader);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(cfg.listen_addr)
        .await
        .context("binding listener")?;
    tracing::info!("listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .context("serve loop")?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,gate=debug")))
        .with(fmt::layer())
        .init();
}

/// 日志里脱敏 password 段：`postgres://user:****@host/db`
fn redact_db_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((auth, host)) = rest.split_once('@') {
            if let Some((user, _pw)) = auth.split_once(':') {
                return format!("{scheme}://{user}:****@{host}");
            }
        }
    }
    url.to_string()
}
