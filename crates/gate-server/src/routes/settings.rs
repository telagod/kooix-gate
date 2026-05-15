//! /v1/me/* — 个人设置

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::put};
use gate_auth::Subject;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Serialize)]
pub struct ChangePasswordResponse {
    pub ok: bool,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/me/password", put(change_password))
}

async fn change_password(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<Json<ChangePasswordResponse>> {
    let user_id = match ctx.subject().unwrap() {
        Subject::User { user_id, .. } => user_id,
        _ => return Err(AppError::Forbidden("only user subjects".into())),
    };

    let (_, existing_hash) = app
        .repos
        .users
        .find_credentials(&app.repos.users.find_by_id(*user_id).await?.email)
        .await?;

    let hash = existing_hash
        .ok_or_else(|| AppError::BadRequest("SSO users cannot change password".into()))?;

    gate_auth::password::verify(&req.current_password, &hash)
        .map_err(|_| AppError::BadRequest("current password is incorrect".into()))?;

    let new_hash = gate_auth::password::hash(&req.new_password)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    app.repos.users.update_password(*user_id, &new_hash).await?;

    app.audit.emit(
        &ctx,
        "user.change_password",
        "user",
        Some(*user_id.as_uuid()),
        None,
    );

    Ok(Json(ChangePasswordResponse { ok: true }))
}
