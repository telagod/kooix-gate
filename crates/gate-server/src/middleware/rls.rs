//! RLS context injection middleware.
//!
//! After auth resolution, extracts org_id / project_id / is_platform_admin from
//! [`AuthContext`] and stores an [`RlsContext`] in request extensions.
//!
//! Downstream handlers and repo methods can retrieve `RlsContext` from extensions
//! to call [`gate_storage::with_rls`] for defense-in-depth DB isolation.
//!
//! This middleware does NOT modify the database connection itself — that is the
//! responsibility of the repo layer (opt-in today, mandatory in Phase 2 when the
//! pool connects as `gate_app`).

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use gate_auth::Subject;
use gate_storage::RlsContext;

/// Middleware that injects [`RlsContext`] into request extensions.
///
/// Must be mounted AFTER rate_limit (which may insert AuthContext into extensions)
/// or at least after the auth header is available for resolution.
pub async fn rls_inject(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();

    let ctx = match crate::auth::resolve_for_state(&mut parts, &state).await {
        Ok(c) => c,
        Err(_) => {
            // Auth failed — let downstream handler deal with 401/403.
            let req = Request::from_parts(parts, body);
            return next.run(req).await;
        }
    };

    let rls = match ctx.subject() {
        Some(Subject::ApiKey {
            org_id, project_id, ..
        }) => RlsContext {
            org_id: Some(*org_id),
            project_id: Some(*project_id),
            is_platform_admin: false,
        },
        Some(Subject::User { .. }) => RlsContext {
            org_id: ctx.current_org(),
            project_id: None, // project determined per-request by route params
            is_platform_admin: ctx.is_super_admin(),
        },
        Some(Subject::System) => RlsContext::platform_admin(),
        None => RlsContext {
            org_id: None,
            project_id: None,
            is_platform_admin: false,
        },
    };

    parts.extensions.insert(rls);
    // Also store AuthContext so downstream extractors can skip re-resolution
    parts.extensions.insert(ctx);

    let req = Request::from_parts(parts, body);
    next.run(req).await
}
