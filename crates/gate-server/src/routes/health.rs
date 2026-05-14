//! GET /health — 无 auth 探活
//! GET /health/status — 系统初始化状态（无 auth）
//! GET /metrics — Prometheus exposition format

use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
pub struct SystemStatus {
    pub initialized: bool,
    pub version: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/health/status", get(system_status))
        .route("/metrics", get(crate::metrics::metrics_handler))
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn system_status(State(app): State<AppState>) -> Json<SystemStatus> {
    let initialized = app
        .repos
        .users
        .has_any_admin()
        .await
        .unwrap_or(false);
    Json(SystemStatus {
        initialized,
        version: env!("CARGO_PKG_VERSION"),
    })
}
