//! gate-server: HTTP 入口
//!
//! 启动流程：
//! 1. 加载配置 (KOOIX_* env + kooix-gate.toml 可选)
//! 2. 初始化 JwtIssuer (from KOOIX_JWT_SECRET base64)
//! 3. 初始化 AuthContextLoader (待 storage Repo 就绪后替换为 PG 实现)
//! 4. 启动 Axum

use anyhow::Context;
use chrono::Duration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::{build_router, AppState, Config};
use gate_server::loader::InMemoryLoader;
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

    // TODO: 替换为 PgLoader 用真实 DB
    let loader = Arc::new(InMemoryLoader::new());

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
