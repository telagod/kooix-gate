//! 路由组装
//!
//! 分三组：
//! - `/health` 探活，无 auth
//! - `/v1/*`   控制台 + 业务，Authed 抽取器强制
//! - `/v1/admin/*` 平台运营，依赖 PlatformAdmin 权限

pub mod health;
pub mod me;
pub mod projects;
pub mod api_keys;
pub mod admin;

use crate::state::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .nest("/v1", v1_router())
}

fn v1_router() -> Router<AppState> {
    Router::new()
        .merge(me::router())
        .merge(projects::router())
        .merge(api_keys::router())
        .nest("/admin", admin::router())
}
