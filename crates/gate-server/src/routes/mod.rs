//! 路由组装
//!
//! 分三组：
//! - `/health` 探活，无 auth
//! - `/v1/*`   控制台 + 业务，Authed 抽取器强制
//! - `/v1/admin/*` 平台运营，依赖 PlatformAdmin 权限

pub mod admin;
pub mod api_keys;
pub mod auth;
pub mod chat;
pub mod health;
pub mod me;
pub mod projects;

use crate::state::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .nest("/v1", v1_router())
}

/// 公开 v1 router 让 `build_router` 单独给它叠限流 middleware。
pub fn v1_router() -> Router<AppState> {
    Router::new()
        .merge(me::router())
        .merge(projects::router())
        .merge(api_keys::router())
        .merge(chat::router())
        .merge(auth::router())
        .nest("/admin", admin::router())
}
