//! `build_router(state)` — 组装完整 Router，供 bin 与测试共用。

use crate::middleware::{
    metrics_layer, quota_enforce, rate_limit_by_subject, request_id_extension, request_id_layers,
    rls_inject, trace_layer,
};
use crate::routes;
use crate::state::AppState;
use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use tower_http::cors::CorsLayer;

pub fn build_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();

    // /v1 layer 顺序：rate_limit 在外层（更早拦截，廉价），quota 在内层（拿到更多 context）。
    // axum 中 `.layer(A).layer(B)` 的执行顺序是 B 先于 A —— 后挂载的更靠外。
    // 我们要 rate_limit 先执行 → 把它放到 .layer 链最后一个。
    // rls_inject 最内层：在 quota 之后注入 RlsContext 到 extensions。
    let v1 = routes::v1_router()
        .layer(from_fn_with_state(state.clone(), rls_inject))
        .layer(from_fn_with_state(state.clone(), quota_enforce))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_subject));

    Router::new()
        .merge(routes::health::router())
        .nest("/v1", routes::setup::router())
        .nest("/v1", v1)
        .with_state(state)
        .layer(propagate_id)
        .layer(from_fn(metrics_layer)) // metrics: outermost business layer, captures all requests
        .layer(trace_layer())
        .layer(from_fn(request_id_extension))
        .layer(set_id)
        .layer(CorsLayer::permissive()) // 生产按域名收紧
}

pub fn build_gateway_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();
    let v1 = Router::new()
        .merge(routes::models::router())
        .merge(routes::chat::router())
        .merge(routes::embeddings::router())
        .merge(routes::images::router())
        .merge(routes::audio::router())
        .layer(from_fn_with_state(state.clone(), rls_inject))
        .layer(from_fn_with_state(state.clone(), quota_enforce))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_subject));

    Router::new()
        .merge(routes::health::public_router())
        .nest("/v1", v1)
        .with_state(state)
        .layer(propagate_id)
        .layer(from_fn(metrics_layer))
        .layer(trace_layer())
        .layer(from_fn(request_id_extension))
        .layer(set_id)
        .layer(CorsLayer::permissive())
}

pub fn build_controlplane_router(state: AppState) -> Router {
    let (set_id, propagate_id) = request_id_layers();
    let v1 = Router::new()
        .merge(routes::me::router())
        .merge(routes::settings::router())
        .merge(routes::model_aliases::router())
        .merge(routes::projects::router())
        .merge(routes::api_keys::router())
        .merge(routes::auth::router())
        .merge(routes::sso::router())
        .merge(routes::usage::router())
        .merge(routes::channels::router())
        .merge(routes::quotas::router())
        .merge(routes::billing::router())
        .nest(
            "/admin",
            routes::admin::router().merge(routes::request_logs::router()),
        )
        .layer(from_fn_with_state(state.clone(), rls_inject))
        .layer(from_fn_with_state(state.clone(), quota_enforce))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_subject));

    Router::new()
        .merge(routes::health::router())
        .nest("/v1", routes::setup::router())
        .nest("/v1", v1)
        .with_state(state)
        .layer(propagate_id)
        .layer(from_fn(metrics_layer))
        .layer(trace_layer())
        .layer(from_fn(request_id_extension))
        .layer(set_id)
        .layer(CorsLayer::permissive())
}
