//! gate-server: HTTP 入口
//!
//! 启动流程：
//! 1. 加载配置 (KOOIX_* env + kooix-gate.toml 可选)
//! 2. 连接 PostgreSQL + 跑迁移
//! 3. 初始化 JwtIssuer (from KOOIX_JWT_SECRET base64)
//! 4. 装配 PgLoader（生产）/ InMemoryLoader（仅当 KOOIX_DEV_INMEMORY=1）
//! 5. 初始化 Prometheus metrics + 可选 OpenTelemetry OTLP
//! 6. 启动 Axum

use anyhow::Context;
use chrono::Duration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::loader::{AuthContextLoader, InMemoryLoader};
use gate_server::state::Repos;
use gate_server::{AppState, Config, PgLoader, build_router};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Phase 1: basic tracing (env filter + fmt)
    // Phase 2: if OTEL_EXPORTER_OTLP_ENDPOINT is set, add OpenTelemetry layer
    let otel_provider = gate_server::telemetry::init_telemetry("gate-server");

    init_tracing(&otel_provider);

    // Install Prometheus metrics recorder (must be before any metrics::* calls)
    gate_server::metrics::install_recorder();

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
    let kms_arc: Option<Arc<gate_crypto::EnvelopeKms>> = match gate_crypto::kms::EnvKms::from_env("KOOIX_MASTER_KEY") {
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
        let key_repo = state.repos.channel_keys.clone();
        let alias_repo = state.repos.model_aliases.clone();

        let mut router = gate_providers::ProviderRouter::new(channel_repo, group_repo)
            .with_channel_key_repo(key_repo)
            .with_model_alias_repo(alias_repo);

        if let Some(ref kms) = kms_arc {
            router = router.with_crypto(kms.clone());
        }
        if let Some(ref rl) = redis_rate_limiter {
            let channel_rl = gate_server::channel_rate_limit::RedisChannelRateLimiter::new(rl.clone());
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
        tracing::info!(
            "KOOIX_OPENAI_BASE_URL not set; requests without channel routing will 400"
        );
    }

    let app = build_router(state.clone());

    gate_server::health_probe::spawn(state.clone());

    // 带 auth + advisory lock + 分级处理的健康巡检
    gate_server::health_check::spawn(&state);

    // 定价自动同步（每 24h 从 LiteLLM 拉取）
    if let Some(pool) = state.repos.pool() {
        let pool = pool.clone();
        tokio::spawn(async move {
            // 启动时立即同步一次
            match gate_billing::pricing_sync::sync_from_litellm_upsert(&pool).await {
                Ok((n, s)) => tracing::info!(upserted = n, skipped = s, "initial pricing sync done"),
                Err(e) => tracing::warn!(error = %e, "initial pricing sync failed"),
            }
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                match gate_billing::pricing_sync::sync_from_litellm_upsert(&pool).await {
                    Ok((n, s)) => tracing::info!(upserted = n, skipped = s, "pricing sync done"),
                    Err(e) => tracing::warn!(error = %e, "pricing sync failed"),
                }
            }
        });
    }

    // Inflight 清扫任务（每 60s 扫 expired 行退还 quota）
    {
        let inflight_repo = state.repos.inflight.clone();
        let quota_counter = state.quota_counter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Some(ref qc) = quota_counter {
                    match inflight_repo.sweep_expired().await {
                        Ok(expired) if !expired.is_empty() => {
                            let count = expired.len();
                            for row in expired {
                                for (key, micros) in row.quota_keys.iter().zip(row.estimated_micros.iter()) {
                                    let _ = qc.refund(key, *micros).await;
                                }
                            }
                            tracing::info!(count, "inflight sweep: refunded expired pre-debits");
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(error = %e, "inflight sweep failed"),
                    }
                }
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(cfg.listen_addr)
        .await
        .context("binding listener")?;
    tracing::info!("listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serve loop")?;

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

async fn shutdown_signal() {
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
