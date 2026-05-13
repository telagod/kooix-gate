//! `build_router(state)` — 组装完整 Router，供 bin 与测试共用。

use crate::middleware::{quota_enforce, rate_limit_by_subject, request_id_layers, trace_layer};
use crate::routes;
use crate::state::AppState;
use axum::Router;
use axum::middleware::from_fn_with_state;
use tower_http::cors::CorsLayer;

pub fn build_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();

    // /v1 layer 顺序：rate_limit 在外层（更早拦截，廉价），quota 在内层（拿到更多 context）。
    // axum 中 `.layer(A).layer(B)` 的执行顺序是 B 先于 A —— 后挂载的更靠外。
    // 我们要 rate_limit 先执行 → 把它放到 .layer 链最后一个。
    let v1 = routes::v1_router()
        .layer(from_fn_with_state(state.clone(), quota_enforce))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_subject));

    Router::new()
        .merge(routes::health::router())
        .nest("/v1", v1)
        .with_state(state)
        .layer(propagate_id)
        .layer(trace_layer())
        .layer(set_id)
        .layer(CorsLayer::permissive()) // 生产按域名收紧
}
