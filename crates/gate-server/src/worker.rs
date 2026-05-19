//! Background worker runtime.
//!
//! Worker mode owns async jobs that must not run once per HTTP replica:
//! billing outbox consumer, pricing sync, channel health check, and inflight
//! quota recovery. `all` mode starts the same jobs for small deployments.

use crate::state::AppState;
use gate_billing::{Consumer, PgOutboxRepo, PgPricingRepo};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const PRICING_SYNC_LOCK: &str = "kooix_pricing_sync";
const INFLIGHT_SWEEP_LOCK: &str = "kooix_inflight_sweep";

pub fn spawn_workers(state: &mut AppState, shutdown: CancellationToken) -> JoinSet<()> {
    let mut tasks = JoinSet::new();

    attach_billing_repos(state);

    if let Some(pool) = state.repos.pool().cloned() {
        let outbox = Arc::new(PgOutboxRepo::new(pool.clone()));

        spawn_outbox_consumer(&mut tasks, pool.clone(), outbox, shutdown.clone());
        spawn_pricing_sync(&mut tasks, pool.clone(), shutdown.clone());
        spawn_inflight_sweeper(state, &mut tasks, pool, shutdown.clone());
    } else {
        tracing::warn!("worker: no PgPool available; DB-backed jobs disabled");
    }

    if let Some(handle) = crate::health_check::spawn_with_shutdown(state, shutdown) {
        tasks.spawn(async move {
            if let Err(e) = handle.await {
                tracing::warn!(error = %e, "health_check task join failed");
            }
        });
    }

    tasks
}

pub fn attach_billing_repos(state: &mut AppState) {
    let Some(pool) = state.repos.pool().cloned() else {
        return;
    };
    if state.outbox.is_none() {
        state.outbox = Some(Arc::new(PgOutboxRepo::new(pool.clone())));
    }
    if state.pricing.is_none() {
        state.pricing = Some(Arc::new(PgPricingRepo::new(pool)));
    }
}

fn spawn_outbox_consumer(
    tasks: &mut JoinSet<()>,
    pool: sqlx::PgPool,
    outbox: Arc<PgOutboxRepo>,
    shutdown: CancellationToken,
) {
    let batch_size = env_i64("KOOIX_OUTBOX_BATCH_SIZE", 100).max(1);
    let interval = Duration::from_millis(env_u64("KOOIX_OUTBOX_INTERVAL_MS", 1000).max(100));
    tasks.spawn(async move {
        tracing::info!(
            batch_size,
            interval_ms = interval.as_millis(),
            "worker: billing outbox consumer started"
        );
        Consumer::new(outbox, pool, batch_size, interval)
            .run_until(shutdown)
            .await;
    });
}

fn spawn_pricing_sync(tasks: &mut JoinSet<()>, pool: sqlx::PgPool, shutdown: CancellationToken) {
    let interval_secs = env_u64("KOOIX_PRICING_SYNC_INTERVAL_SECS", 86_400);
    if interval_secs == 0 {
        tracing::info!("worker: pricing sync disabled via KOOIX_PRICING_SYNC_INTERVAL_SECS=0");
        return;
    }
    tasks.spawn(async move {
        tracing::info!(interval_secs, "worker: pricing sync started");
        run_with_pg_lock(&pool, PRICING_SYNC_LOCK, || async {
            match gate_billing::pricing_sync::sync_from_litellm_upsert(&pool).await {
                Ok((upserted, skipped)) => {
                    metrics::counter!("worker_pricing_sync_total", "outcome" => "success").increment(1);
                    tracing::info!(upserted, skipped, "worker: initial pricing sync done");
                }
                Err(e) => {
                    metrics::counter!("worker_pricing_sync_total", "outcome" => "failure").increment(1);
                    tracing::warn!(error = %e, "worker: initial pricing sync failed");
                }
            }
        })
        .await;

        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!("worker: pricing sync stopped");
                    break;
                }
                _ = ticker.tick() => {
                    run_with_pg_lock(&pool, PRICING_SYNC_LOCK, || async {
                        match gate_billing::pricing_sync::sync_from_litellm_upsert(&pool).await {
                            Ok((upserted, skipped)) => {
                                metrics::counter!("worker_pricing_sync_total", "outcome" => "success").increment(1);
                                tracing::info!(upserted, skipped, "worker: pricing sync done");
                            }
                            Err(e) => {
                                metrics::counter!("worker_pricing_sync_total", "outcome" => "failure").increment(1);
                                tracing::warn!(error = %e, "worker: pricing sync failed");
                            }
                        }
                    }).await;
                }
            }
        }
    });
}

fn spawn_inflight_sweeper(
    state: &AppState,
    tasks: &mut JoinSet<()>,
    pool: sqlx::PgPool,
    shutdown: CancellationToken,
) {
    let inflight_repo = state.repos.inflight.clone();
    let quota_counter = state.quota_counter.clone();
    let interval_secs = env_u64("KOOIX_INFLIGHT_SWEEP_INTERVAL_SECS", 60);
    if interval_secs == 0 {
        tracing::info!("worker: inflight sweep disabled via KOOIX_INFLIGHT_SWEEP_INTERVAL_SECS=0");
        return;
    }
    tasks.spawn(async move {
        tracing::info!(interval_secs, "worker: inflight sweeper started");
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!("worker: inflight sweeper stopped");
                    break;
                }
                _ = ticker.tick() => {
                    let Some(ref qc) = quota_counter else { continue; };
                    run_with_pg_lock(&pool, INFLIGHT_SWEEP_LOCK, || async {
                        match inflight_repo.sweep_expired().await {
                            Ok(expired) => {
                                let count = expired.len();
                                for row in expired {
                                    for (key, micros) in row.quota_keys.iter().zip(row.estimated_micros.iter()) {
                                        let _ = qc.refund(key, *micros).await;
                                    }
                                }
                                metrics::counter!("worker_inflight_swept_total").increment(count as u64);
                                if count > 0 {
                                    tracing::info!(count, "worker: refunded expired pre-debits");
                                }
                            }
                            Err(e) => {
                                metrics::counter!("worker_inflight_sweep_failures_total").increment(1);
                                tracing::warn!(error = %e, "worker: inflight sweep failed");
                            }
                        }
                    }).await;
                }
            }
        }
    });
}

async fn run_with_pg_lock<F, Fut>(pool: &sqlx::PgPool, name: &'static str, f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    match sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock(hashtext($1))")
        .bind(name)
        .fetch_one(pool)
        .await
    {
        Ok(true) => {
            metrics::gauge!("worker_lease_owner", "job" => name).set(1.0);
            f().await;
            let _ = sqlx::query("SELECT pg_advisory_unlock(hashtext($1))")
                .bind(name)
                .execute(pool)
                .await;
            metrics::gauge!("worker_lease_owner", "job" => name).set(0.0);
        }
        Ok(false) => {
            metrics::gauge!("worker_lease_owner", "job" => name).set(0.0);
            tracing::debug!(job = name, "worker: another instance owns lease");
        }
        Err(e) => tracing::warn!(job = name, error = %e, "worker: advisory lock failed"),
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
