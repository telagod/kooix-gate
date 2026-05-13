//! GET /v1/me — 返回当前身份画像
//!
//! 任何已认证 subject 都能访问（无特殊权限门槛）。

use crate::auth::Authed;
use crate::error::AppResult;
use crate::state::AppState;
use axum::{routing::get, Json, Router};
use gate_auth::Subject;
use gate_core::id::OrgId;
use serde::Serialize;

#[derive(Serialize)]
pub struct MeResponse {
    pub subject: SubjectView,
    pub current_org: Option<String>,
    pub is_platform_admin: bool,
    pub orgs: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectView {
    User { user_id: String, session_id: String },
    ApiKey { api_key_id: String, project_id: String, org_id: String },
    System,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(me))
}

async fn me(Authed(ctx): Authed) -> AppResult<Json<MeResponse>> {
    let subject = match ctx.subject().expect("Authed guarantees subject") {
        Subject::User { user_id, session_id } => SubjectView::User {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
        },
        Subject::ApiKey { api_key_id, project_id, org_id } => SubjectView::ApiKey {
            api_key_id: api_key_id.to_string(),
            project_id: project_id.to_string(),
            org_id: org_id.to_string(),
        },
        Subject::System => SubjectView::System,
    };

    Ok(Json(MeResponse {
        subject,
        current_org: ctx.current_org().map(|o: OrgId| o.to_string()),
        is_platform_admin: ctx.is_super_admin(),
        orgs: ctx.accessible_orgs().iter().map(ToString::to_string).collect(),
    }))
}
