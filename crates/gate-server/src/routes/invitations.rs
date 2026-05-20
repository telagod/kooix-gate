//! /v1/invitations — public invitation accept flow.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router, routing::post};
use chrono::Utc;
use gate_core::identity::{OrgRole, ProjectRole};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/invitations/preview", post(preview_invitation))
        .route("/invitations/accept", post(accept_invitation))
}

#[derive(Deserialize)]
pub struct InvitationTokenRequest {
    pub token: String,
}

#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub email: String,
    pub password: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Serialize)]
pub struct InvitationPreviewResponse {
    pub id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub email: String,
    pub role: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub status: String,
}

#[derive(Serialize)]
pub struct AcceptInvitationResponse {
    pub user_id: String,
    pub email: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub role: String,
    pub accepted_at: chrono::DateTime<Utc>,
}

async fn preview_invitation(
    State(app): State<AppState>,
    Json(req): Json<InvitationTokenRequest>,
) -> AppResult<Json<InvitationPreviewResponse>> {
    let token_hash = invitation_token_hash(&normalize_invitation_token(&req.token)?);
    let invitation = app
        .repos
        .invitations
        .find_by_token_hash(&token_hash)
        .await?;
    let status = invitation.status_at(Utc::now()).to_string();
    Ok(Json(InvitationPreviewResponse {
        id: invitation.id.to_string(),
        scope_kind: invitation.scope_kind,
        scope_id: invitation.scope_id.to_string(),
        email: invitation.email,
        role: invitation.role,
        expires_at: invitation.expires_at,
        status,
    }))
}

async fn accept_invitation(
    State(app): State<AppState>,
    Json(req): Json<AcceptInvitationRequest>,
) -> AppResult<Json<AcceptInvitationResponse>> {
    let token = normalize_invitation_token(&req.token)?;
    let token_hash = invitation_token_hash(&token);
    let invitation = app
        .repos
        .invitations
        .find_by_token_hash(&token_hash)
        .await?;

    if !invitation.is_pending_at(Utc::now()) {
        return Err(AppError::BadRequest("invitation is not pending".into()));
    }

    let email = normalize_email(&req.email)?;
    if email != invitation.email {
        return Err(AppError::Forbidden(
            "invitation email does not match requester".into(),
        ));
    }

    let user = match app.repos.users.find_by_email(&email).await {
        Ok(user) => {
            if !matches!(user.status, gate_core::identity::UserStatus::Active) {
                return Err(AppError::Forbidden("user is not active".into()));
            }
            user
        }
        Err(gate_storage::DbError::NotFound) => {
            let password = req.password.as_deref().ok_or_else(|| {
                AppError::BadRequest("password is required for new invitee".into())
            })?;
            let hash = gate_auth::password::hash(password).map_err(AppError::Auth)?;
            app.repos
                .users
                .create(
                    &email,
                    Some(&hash),
                    req.display_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|v| !v.is_empty()),
                    Some("active"),
                )
                .await?
        }
        Err(other) => return Err(AppError::Db(other)),
    };

    let accepted = app.repos.invitations.accept(&token_hash, user.id).await?;
    match accepted.scope_kind.as_str() {
        "org" => {
            let role = parse_org_invite_role(&accepted.role)?;
            app.repos
                .memberships
                .add_org_member(accepted.scope_id.into(), user.id, role)
                .await?;
        }
        "project" => {
            let role = parse_project_invite_role(&accepted.role)?;
            let project = app
                .repos
                .projects
                .find_by_id(accepted.scope_id.into())
                .await?;
            app.repos
                .memberships
                .add_project_member_in_org(project.org_id, project.id, user.id, role)
                .await?;
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "unknown invite scope: {other}"
            )));
        }
    }

    Ok(Json(AcceptInvitationResponse {
        user_id: user.id.to_string(),
        email: user.email,
        scope_kind: accepted.scope_kind,
        scope_id: accepted.scope_id.to_string(),
        role: accepted.role,
        accepted_at: accepted.accepted_at.unwrap_or_else(Utc::now),
    }))
}

pub(crate) fn invitation_token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

pub(crate) fn normalize_invitation_token(raw: &str) -> AppResult<String> {
    let token = raw.trim();
    if token.len() < 32 || token.len() > 256 {
        return Err(AppError::BadRequest("invalid invitation token".into()));
    }
    if !token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(AppError::BadRequest("invalid invitation token".into()));
    }
    Ok(token.to_string())
}

pub(crate) fn normalize_email(raw: &str) -> AppResult<String> {
    let email = raw.trim().to_ascii_lowercase();
    if email.len() < 3 || email.len() > 320 || !email.contains('@') {
        return Err(AppError::BadRequest("valid email is required".into()));
    }
    Ok(email)
}

pub(crate) fn parse_org_invite_role(raw: &str) -> AppResult<OrgRole> {
    match raw {
        "owner" => Ok(OrgRole::Owner),
        "admin" => Ok(OrgRole::Admin),
        "billing_viewer" => Ok(OrgRole::BillingViewer),
        "member" => Ok(OrgRole::Member),
        _ => Err(AppError::BadRequest(
            "role must be one of: owner, admin, billing_viewer, member".into(),
        )),
    }
}

pub(crate) fn parse_project_invite_role(raw: &str) -> AppResult<ProjectRole> {
    match raw {
        "owner" => Ok(ProjectRole::Owner),
        "admin" => Ok(ProjectRole::Admin),
        "developer" => Ok(ProjectRole::Developer),
        "viewer" => Ok(ProjectRole::Viewer),
        _ => Err(AppError::BadRequest(
            "role must be one of: owner, admin, developer, viewer".into(),
        )),
    }
}
