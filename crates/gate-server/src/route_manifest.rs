//! Static route manifest for runtime boundary tests and lightweight contract export.

use crate::modes::RuntimeMode;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthClass {
    Public,
    User,
    ApiKey,
    UserOrApiKey,
    PlatformAdmin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RouteMeta {
    pub method: &'static str,
    pub path: &'static str,
    pub auth: AuthClass,
    pub modes: &'static [RuntimeMode],
}

const ALL_HTTP: &[RuntimeMode] = &[
    RuntimeMode::All,
    RuntimeMode::Gateway,
    RuntimeMode::ControlPlane,
];
const DATA_PLANE: &[RuntimeMode] = &[RuntimeMode::All, RuntimeMode::Gateway];
const CONTROL_PLANE: &[RuntimeMode] = &[RuntimeMode::All, RuntimeMode::ControlPlane];

macro_rules! route {
    ($method:literal, $path:literal, $auth:ident, $modes:ident) => {
        RouteMeta {
            method: $method,
            path: $path,
            auth: AuthClass::$auth,
            modes: $modes,
        }
    };
}

pub const ROUTES: &[RouteMeta] = &[
    route!("GET", "/health", Public, ALL_HTTP),
    route!("GET", "/metrics", Public, ALL_HTTP),
    route!("GET", "/route-manifest.json", Public, ALL_HTTP),
    route!("GET", "/health/status", Public, CONTROL_PLANE),
    route!("GET", "/v1/models", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/chat/completions", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/responses", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/embeddings", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/images/generations", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/audio/speech", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/audio/transcriptions", UserOrApiKey, DATA_PLANE),
    route!("POST", "/v1/setup", Public, CONTROL_PLANE),
    route!("POST", "/v1/auth/login", Public, CONTROL_PLANE),
    route!("POST", "/v1/auth/refresh", Public, CONTROL_PLANE),
    route!("POST", "/v1/auth/logout", User, CONTROL_PLANE),
    route!("GET", "/v1/auth/sso/:slug/start", Public, CONTROL_PLANE),
    route!("GET", "/v1/auth/sso/callback", Public, CONTROL_PLANE),
    route!("GET", "/v1/me", User, CONTROL_PLANE),
    route!("PUT", "/v1/me/password", User, CONTROL_PLANE),
    route!("GET", "/v1/orgs/:org_id/projects", User, CONTROL_PLANE),
    route!("POST", "/v1/orgs/:org_id/projects", User, CONTROL_PLANE),
    route!(
        "GET",
        "/v1/orgs/:org_id/projects/:project_id",
        User,
        CONTROL_PLANE
    ),
    route!(
        "PUT",
        "/v1/orgs/:org_id/projects/:project_id",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/projects/:project_id/api-keys",
        User,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/orgs/:org_id/projects/:project_id/api-keys",
        User,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/orgs/:org_id/projects/:project_id/api-keys/:key_id",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/projects/:project_id/model-aliases",
        User,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/orgs/:org_id/projects/:project_id/model-aliases",
        User,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/orgs/:org_id/projects/:project_id/model-aliases/:alias",
        User,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/usage", User, CONTROL_PLANE),
    route!("GET", "/v1/orgs/:org_id/requests", User, CONTROL_PLANE),
    route!(
        "GET",
        "/v1/orgs/:org_id/requests/filters",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/requests/:request_id",
        User,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/orgs/:org_id/channels", User, CONTROL_PLANE),
    route!("GET", "/v1/orgs/:org_id/quotas", User, CONTROL_PLANE),
    route!("POST", "/v1/orgs/:org_id/quotas", User, CONTROL_PLANE),
    route!(
        "GET",
        "/v1/orgs/:org_id/quotas/explain",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/quotas/reconcile",
        User,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/orgs/:org_id/quotas/:quota_id",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/billing/export",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/billing/export.json",
        User,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/orgs/:org_id/billing/:month",
        User,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/orgs/:org_id/billing/:month/state",
        User,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/orgs/:org_id/quota-alerts", User, CONTROL_PLANE),
    route!(
        "GET",
        "/v1/admin/plugin-manifest/schema",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/plugin-manifest/replay",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/admin/channels", PlatformAdmin, CONTROL_PLANE),
    route!("POST", "/v1/admin/channels", PlatformAdmin, CONTROL_PLANE),
    route!(
        "PUT",
        "/v1/admin/channels/:id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/admin/channels/:id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/batch-enable",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/batch-disable",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/batch-delete",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/channels/:id/keys",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/:id/keys",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/:id/keys/rotate",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/admin/channels/:id/keys/:key_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/channels/:id/stats",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/channels/:id/probe",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/channels/:id/test",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/channels/:id/balance",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/admin/audit-logs", PlatformAdmin, CONTROL_PLANE),
    route!("GET", "/v1/admin/orgs", PlatformAdmin, CONTROL_PLANE),
    route!("POST", "/v1/admin/orgs", PlatformAdmin, CONTROL_PLANE),
    route!(
        "PUT",
        "/v1/admin/orgs/:org_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/admin/users", PlatformAdmin, CONTROL_PLANE),
    route!("POST", "/v1/admin/users", PlatformAdmin, CONTROL_PLANE),
    route!(
        "PUT",
        "/v1/admin/users/:id/status",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "PUT",
        "/v1/admin/users/:id/password",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/admin/groups", PlatformAdmin, CONTROL_PLANE),
    route!("POST", "/v1/admin/groups", PlatformAdmin, CONTROL_PLANE),
    route!("PUT", "/v1/admin/groups/:id", PlatformAdmin, CONTROL_PLANE),
    route!(
        "DELETE",
        "/v1/admin/groups/:id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/groups/:id/bindings",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/groups/:id/bindings",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "PUT",
        "/v1/admin/groups/:id/bindings/:channel_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/admin/groups/:id/bindings/:channel_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/groups/:id/detail",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "PUT",
        "/v1/admin/projects/:id/default-group",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/orgs/:org_id/members",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/orgs/:org_id/members",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/admin/orgs/:org_id/members/:user_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!("GET", "/v1/admin/requests", PlatformAdmin, CONTROL_PLANE),
    route!(
        "GET",
        "/v1/admin/requests/filters",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/requests/:request_id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/dashboard-stats",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "GET",
        "/v1/admin/pricing-rules",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "POST",
        "/v1/admin/pricing-rules",
        PlatformAdmin,
        CONTROL_PLANE
    ),
    route!(
        "DELETE",
        "/v1/admin/pricing-rules/:id",
        PlatformAdmin,
        CONTROL_PLANE
    ),
];

pub fn routes_for_mode(mode: RuntimeMode) -> impl Iterator<Item = &'static RouteMeta> {
    ROUTES
        .iter()
        .filter(move |route| route.modes.contains(&mode))
}

pub fn gateway_paths() -> impl Iterator<Item = &'static str> {
    routes_for_mode(RuntimeMode::Gateway).map(|route| route.path)
}

pub fn all_routes() -> &'static [RouteMeta] {
    ROUTES
}

pub fn manifest_json() -> serde_json::Value {
    json!({
        "schema_version": 1,
        "generated_by": "gate-server::route_manifest",
        "routes": ROUTES,
    })
}
