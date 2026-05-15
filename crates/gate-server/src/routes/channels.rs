//! GET /v1/orgs/:org_id/channels — Org 视图下的 channel 只读列表。
//!
//! 当前实现简化为「列全部 channels」（admin 级视图），由 `OrgRead` 把住门槛。
//! 后续可改成 JOIN channel_groups → projects 过滤出真正挂在该 Org 的 channels。
//!
//! 安全：
//! - `require_user!`：拒绝 API key 自查（控制台专用）
//! - `Permission::OrgRead` on `Scope::Org`：用户必须能看这个 Org
//! - 不返回 channel 密钥相关任何字段；只是状态/健康度元信息

use crate::auth::Authed;
use crate::error::AppResult;
use crate::flex_uuid::FlexUuid;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{Json, Router, routing::get};
use chrono::{DateTime, Utc};
use gate_auth::{require, require_user};
use gate_core::id::OrgId;
use gate_core::rbac::{Permission, Scope};
use serde::Serialize;

#[derive(Serialize)]
pub struct ChannelView {
    pub id: String,
    pub code: String,
    pub name: String,
    pub provider_type: String,
    pub status: String,
    pub health: String,
    pub updated_at: DateTime<Utc>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/orgs/:org_id/channels", get(list_channels))
}

async fn list_channels(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ChannelView>>> {
    require_user!(ctx);
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::OrgRead, Scope::Org(&org));

    let records = app.repos.channels.list_admin_view().await?;
    Ok(Json(
        records
            .into_iter()
            .map(|r| ChannelView {
                id: r.channel_id.to_string(),
                code: r.code,
                name: r.name,
                provider_type: r.provider_type,
                status: r.status,
                health: r.health,
                updated_at: r.updated_at,
            })
            .collect(),
    ))
}
