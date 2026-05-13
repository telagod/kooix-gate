//! `build_router(state)` — 组装完整 Router，供 bin 与测试共用。

use crate::middleware::{request_id_layers, trace_layer};
use crate::routes;
use crate::state::AppState;
use axum::Router;
use tower_http::cors::CorsLayer;

pub fn build_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();

    routes::router()
        .with_state(state)
        .layer(propagate_id)
        .layer(trace_layer())
        .layer(set_id)
        .layer(CorsLayer::permissive()) // 生产按域名收紧
}
