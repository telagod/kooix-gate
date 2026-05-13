//! /v1/admin/* — 平台运维接口
//!
//! 全部需 Platform 作用域权限。SuperAdmin 短路通过。

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::{routing::get, Json, Router};
use gate_auth::{require, require_user};
use gate_core::rbac::{Permission, Scope};
use serde::Serialize;

#[derive(Serialize)]
pub struct ChannelSummary {
    pub id: String,
    pub code: String,
    pub provider_type: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/channels", get(list_channels))
}

async fn list_channels(Authed(ctx): Authed) -> AppResult<Json<Vec<ChannelSummary>>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);
    Ok(Json(vec![]))
}
