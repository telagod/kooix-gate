//! gate-server: HTTP 入口
//!
//! 启动流程：
//! 1. 加载配置 (KOOIX_* env + kooix-gate.toml 可选)
//! 2. 连接 PostgreSQL + 跑迁移
//! 3. 初始化 JwtIssuer / JwtRing (from KOOIX_JWT_SECRET + optional KOOIX_JWT_PREVIOUS_SECRETS)
//! 4. 装配 PgLoader（生产）/ InMemoryLoader（仅当 KOOIX_DEV_INMEMORY=1）
//! 5. 初始化 Prometheus metrics + 可选 OpenTelemetry OTLP
//! 6. 启动 Axum

use anyhow::Context;
use chrono::Duration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::loader::{AuthContextLoader, InMemoryLoader};
use gate_server::modes::RuntimeMode;
use gate_server::state::Repos;
use gate_server::{AppState, Config, PgLoader, build_router_for_mode};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Phase 1: basic tracing (env filter + fmt)
    // Phase 2: if OTEL_EXPORTER_OTLP_ENDPOINT is set, add OpenTelemetry layer
    let otel_provider = gate_server::telemetry::init_telemetry("gate-server");

    init_tracing(&otel_provider);

    // Install Prometheus metrics recorder (must be before any metrics::* calls)
    gate_server::metrics::install_recorder();

    let cfg = Config::load().context("loading config (set KOOIX_* env vars)")?;
    let mode = RuntimeMode::from_env().map_err(anyhow::Error::msg)?;
    tracing::info!(addr = %cfg.listen_addr, mode = ?mode, "starting");

    let jwt = JwtIssuer::from_env_with_previous(
        "KOOIX_JWT_SECRET",
        "KOOIX_JWT_PREVIOUS_SECRETS",
        cfg.jwt_issuer.clone(),
        cfg.jwt_audience.clone(),
        TokenLifetimes {
            access: Duration::minutes(cfg.token_access_ttl_min),
            refresh: Duration::days(cfg.token_refresh_ttl_day),
        },
    )?;
    if jwt.previous_secret_count() > 0 {
        tracing::info!(
            previous_secrets = jwt.previous_secret_count(),
            "jwt rotation verification window active"
        );
    }

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
            (Arc::new(PgLoader::new(pool.clone())), Repos::from_pg(pool))
        };

    let mut state = AppState::new(jwt, loader, repos);

    // 限流：Redis 配置可选，未提供则跳过（middleware fail-open）
    let mut redis_rate_limiter: Option<Arc<gate_cache::RateLimiter>> = None;
    if !cfg.redis_url.is_empty() {
        tracing::info!(url = %redact_redis_url(&cfg.redis_url), "connecting redis");
        match gate_cache::connect(&cfg.redis_url, 4).await {
            Ok(pool) => {
                let rl = Arc::new(gate_cache::RateLimiter::new(pool));
                redis_rate_limiter = Some(rl.clone());
                state = state.with_rate_limiter_arc(rl.clone());
                tracing::info!("rate limiter active");
            }
            Err(e) => {
                tracing::warn!(error = %e, "redis connect failed; running without rate limiter");
            }
        }
    } else {
        tracing::warn!("KOOIX_REDIS_URL not set; rate limiter disabled");
    }

    // Envelope KMS: 用于解密 channel key 和 SSO client_secret
    let kms_arc: Option<Arc<gate_crypto::EnvelopeKms>> = match gate_crypto::kms::EnvKms::from_env(
        "KOOIX_MASTER_KEY",
    ) {
        Ok(k) => {
            let kms = gate_crypto::EnvelopeKms::new(k);
            let arc = Arc::new(kms);
            state = state.with_crypto_arc(arc.clone());
            tracing::info!("envelope KMS active");
            Some(arc)
        }
        Err(e) => {
            tracing::warn!(error = %e, "KOOIX_MASTER_KEY not set; channel key encryption unavailable");
            None
        }
    };

    // ProviderRouter: 多渠道路由（channel group → channel 选路 → key 解密 → provider 构造）
    {
        let channel_repo = state.repos.channels.clone();
        let group_repo = state.repos.channel_groups.clone();
        let latency_repo = state.repos.channel_latency.clone();
        let key_repo = state.repos.channel_keys.clone();
        let alias_repo = state.repos.model_aliases.clone();

        let mut router = gate_providers::ProviderRouter::new(channel_repo, group_repo)
            .with_channel_latency_repo(latency_repo)
            .with_channel_key_repo(key_repo)
            .with_model_alias_repo(alias_repo);

        if let Some(ref kms) = kms_arc {
            router = router.with_crypto(kms.clone());
        }
        if let Some(ref rl) = redis_rate_limiter {
            let channel_rl =
                gate_server::channel_rate_limit::RedisChannelRateLimiter::new(rl.clone());
            router = router.with_rate_limiter(Arc::new(channel_rl));
            tracing::info!("channel rate limiter (Redis) injected into ProviderRouter");
        }

        state = state.with_provider_router(router);
        tracing::info!("provider router active");
    }

    // Fallback Provider: 单一 OpenAI 兼容上游（当 ProviderRouter 选路失败时兜底）
    if let (Ok(base), Ok(key)) = (
        std::env::var("KOOIX_OPENAI_BASE_URL"),
        std::env::var("KOOIX_OPENAI_API_KEY"),
    ) {
        match gate_providers::openai::OpenAiProvider::new(base, key) {
            Ok(p) => {
                state = state
                    .with_image_provider(p.clone())
                    .with_audio_provider(p.clone())
                    .with_provider(p);
                tracing::info!("fallback openai provider active (chat + images + audio)");
            }
            Err(e) => tracing::warn!(error = %e, "fallback openai provider init failed"),
        }
    } else {
        tracing::info!("KOOIX_OPENAI_BASE_URL not set; requests without channel routing will 400");
    }

    if state.repos.pool().is_some() {
        gate_server::worker::attach_billing_repos(&mut state);
    }

    let shutdown = CancellationToken::new();
    let worker_shutdown = shutdown.clone();
    let mut workers = if mode.runs_workers() {
        gate_server::worker::spawn_workers(&mut state, worker_shutdown)
    } else {
        Default::default()
    };

    if let Some(app) = build_router_for_mode(mode, state.clone()) {
        let listener = tokio::net::TcpListener::bind(cfg.listen_addr)
            .await
            .context("binding listener")?;
        tracing::info!("listening");

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal(shutdown.clone()))
        .await
        .context("serve loop")?;
    } else {
        tracing::info!("worker mode active; waiting for shutdown signal");
        shutdown_signal(shutdown.clone()).await;
    }

    shutdown.cancel();
    while let Some(joined) = workers.join_next().await {
        if let Err(e) = joined {
            tracing::warn!(error = %e, "worker task failed during shutdown");
        }
    }

    // Graceful shutdown: flush OTLP spans
    if let Some(provider) = otel_provider {
        tracing::info!("shutting down OpenTelemetry tracer");
        if let Err(e) = provider.shutdown() {
            tracing::warn!(error = %e, "OpenTelemetry shutdown error");
        }
    }

    Ok(())
}

fn init_tracing(otel_provider: &Option<opentelemetry_sdk::trace::TracerProvider>) {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,gate=debug"));

    let otel_layer = otel_provider
        .as_ref()
        .map(gate_server::telemetry::otel_layer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer())
        .with(otel_layer)
        .init();
}

async fn shutdown_signal(shutdown: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
    shutdown.cancel();
}

/// 日志里脱敏 password 段：`postgres://user:****@host/db`
fn redact_db_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((auth, host)) = rest.split_once('@')
        && let Some((user, _pw)) = auth.split_once(':')
    {
        return format!("{scheme}://{user}:****@{host}");
    }
    url.to_string()
}

/// 同 [`redact_db_url`]，复用——Redis URL 可能也带 password。
fn redact_redis_url(url: &str) -> String {
    redact_db_url(url)
}
