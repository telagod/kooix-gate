//! `build_router(state)` — 组装完整 Router，供 bin 与测试共用。

use crate::middleware::{rate_limit_by_subject, request_id_layers, trace_layer};
use crate::routes;
use crate::state::AppState;
use axum::Router;
use axum::middleware::from_fn_with_state;
use tower_http::cors::CorsLayer;

pub fn build_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();

    // 限流只挂在 /v1/* 上 — health/metrics 类不限流，让 k8s probe 始终通过
    let v1 = routes::v1_router().layer(from_fn_with_state(state.clone(), rate_limit_by_subject));

    Router::new()
        .merge(routes::health::router())
        .nest("/v1", v1)
        .with_state(state)
        .layer(propagate_id)
        .layer(trace_layer())
        .layer(set_id)
        .layer(CorsLayer::permissive()) // 生产按域名收紧
}
