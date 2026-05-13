//! GET /health — 无 auth 探活

use crate::state::AppState;
use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
