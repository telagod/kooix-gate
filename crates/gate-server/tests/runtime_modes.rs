//! Runtime mode route manifest smoke tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_server::loader::InMemoryLoader;
use gate_server::modes::{RuntimeMode, build_router_for_mode};
use gate_server::state::{AppState, Repos};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

fn state() -> AppState {
    let jwt = JwtIssuer::new(
        b"test-secret-32-bytes-minimum-ok!",
        "kg-test",
        "console",
        TokenLifetimes {
            access: ChronoDuration::minutes(15),
            refresh: ChronoDuration::days(1),
        },
    )
    .unwrap();
    AppState::new(jwt, Arc::new(InMemoryLoader::new()), Repos::in_memory())
}

async fn status(mode: RuntimeMode, uri: &str) -> StatusCode {
    let router = build_router_for_mode(mode, state()).expect("http mode");
    let resp = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let _ = resp.into_body().collect().await;
    status
}

#[tokio::test]
async fn gateway_mode_mounts_data_plane_not_admin() {
    assert_ne!(
        status(RuntimeMode::Gateway, "/v1/models").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(RuntimeMode::Gateway, "/v1/admin/channels").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn controlplane_mode_mounts_admin_not_chat() {
    assert_ne!(
        status(RuntimeMode::ControlPlane, "/v1/admin/channels").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(RuntimeMode::ControlPlane, "/v1/chat/completions").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(RuntimeMode::ControlPlane, "/v1/responses").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn route_manifest_export_is_public_in_gateway_mode() {
    let router = build_router_for_mode(RuntimeMode::Gateway, state()).expect("http mode");
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/route-manifest.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["schema_version"], 1);
    assert!(
        body["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route["path"] == "/v1/chat/completions")
    );
    assert!(
        body["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route["path"] == "/v1/responses")
    );
}

#[test]
fn worker_mode_has_no_http_router() {
    assert!(build_router_for_mode(RuntimeMode::Worker, state()).is_none());
}

#[test]
fn invalid_mode_is_rejected() {
    assert!("gateway".parse::<RuntimeMode>().is_ok());
    let err = "admin".parse::<RuntimeMode>().unwrap_err();
    assert!(err.contains("invalid KOOIX_MODE"), "err={err}");
}

#[test]
fn gateway_manifest_contains_no_control_plane_routes() {
    let forbidden = [
        "/v1/admin/",
        "/v1/auth/",
        "/v1/settings",
        "/v1/projects",
        "/v1/api-keys",
        "/v1/channels",
        "/v1/quotas",
        "/v1/billing",
        "/v1/usage",
    ];
    for path in gate_server::route_manifest::gateway_paths() {
        assert!(
            !forbidden.iter().any(|prefix| path.starts_with(prefix)),
            "gateway route leaked control-plane path: {path}"
        );
    }
}
