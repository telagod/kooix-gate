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
use gate_server::state::Repos;
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

    let (loader, repos): (Arc<dyn AuthContextLoader>, Repos) =
        if std::env::var("KOOIX_DEV_INMEMORY").as_deref() == Ok("1") {
            tracing::warn!("KOOIX_DEV_INMEMORY=1 — using InMemoryLoader (NOT for production)");
            (Arc::new(InMemoryLoader::new()), Repos::in_memory())
        } else {
            tracing::info!(url = %redact_db_url(&cfg.database_url), "connecting postgres");
            let pool = gate_storage::connect(&cfg.database_url, 16)
                .await
                .context("connect postgres")?;
            gate_storage::run_migrations(&pool)
                .await
                .context("run migrations")?;
            tracing::info!("migrations applied");
            (
                Arc::new(PgLoader::new(pool.clone())),
                Repos::from_pg(pool),
            )
        };

    let mut state = AppState::new(jwt, loader, repos);

    // 限流：Redis 配置可选，未提供则跳过（middleware fail-open）
    if !cfg.redis_url.is_empty() {
        tracing::info!(url = %redact_redis_url(&cfg.redis_url), "connecting redis");
        match gate_cache::connect(&cfg.redis_url, 4).await {
            Ok(pool) => {
                let rl = gate_cache::RateLimiter::new(pool);
                state = state.with_rate_limiter(rl);
                tracing::info!("rate limiter active");
            }
            Err(e) => {
                tracing::warn!(error = %e, "redis connect failed; running without rate limiter");
            }
        }
    } else {
        tracing::warn!("KOOIX_REDIS_URL not set; rate limiter disabled");
    }

    // Provider: 现阶段简单接 OpenAI 兼容上游
    if let (Ok(base), Ok(key)) = (
        std::env::var("KOOIX_OPENAI_BASE_URL"),
        std::env::var("KOOIX_OPENAI_API_KEY"),
    ) {
        match gate_providers::openai::OpenAiProvider::new(base, key) {
            Ok(p) => {
                state = state.with_provider(p);
                tracing::info!("openai provider active");
            }
            Err(e) => tracing::warn!(error = %e, "openai provider init failed"),
        }
    } else {
        tracing::warn!(
            "KOOIX_OPENAI_BASE_URL / KOOIX_OPENAI_API_KEY not set; /v1/chat/completions will 400"
        );
    }

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

/// 同 [`redact_db_url`]，复用——Redis URL 可能也带 password。
fn redact_redis_url(url: &str) -> String {
    redact_db_url(url)
}
