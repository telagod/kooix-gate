//! HTTP Plugin manifest builder/debugger E2E.
//!
//! 这里覆盖 P1.1.7 的控制面闭环：manifest replay → channel create → group binding。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Duration as ChronoDuration;
use gate_auth::jwt::{JwtIssuer, TokenLifetimes};
use gate_core::id::UserId;
use gate_core::identity::PlatformRole;
use gate_server::loader::{InMemoryLoader, UserRecord};
use gate_server::state::Repos;
use gate_server::{AppState, build_router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Harness {
    router: axum::Router,
    token: String,
}

fn setup() -> Harness {
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
    let loader = Arc::new(InMemoryLoader::new());
    let user = UserId::new();
    loader.add_user(
        user,
        UserRecord {
            orgs: HashMap::new(),
            projects: HashMap::new(),
            platform: Some(PlatformRole::SuperAdmin),
        },
    );
    let state = AppState::new(jwt, loader, Repos::in_memory());
    let jwt = state.jwt.clone();
    let router = build_router(state);
    let (token, _) = jwt
        .issue_access(*user.as_uuid(), Uuid::now_v7(), None, true)
        .unwrap();
    Harness { router, token }
}

async fn call(h: &Harness, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", h.token));
    let body = match body {
        Some(value) => {
            req = req.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&value).unwrap())
        }
        None => Body::empty(),
    };
    let resp = h
        .router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn plugin_manifest_builder_flow_creates_fixture_channel_and_group_binding() {
    let h = setup();

    let (status, group) = call(
        &h,
        "POST",
        "/v1/admin/groups",
        Some(json!({
            "name": "Plugin Builder Group",
            "strategy": "priority"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "group={group}");
    let group_id = group["id"].as_str().unwrap().to_string();

    let manifest = json!({
        "plugin": {
            "version": 1,
            "capabilities": { "chat": true, "streaming": true },
            "auth": { "strategy": "api_key_header", "header_name": "X-Api-Key", "secret_slot": "primary" },
            "request": {
                "path": "/private/chat",
                "body": {
                    "modelName": "{{model}}",
                    "prompt": "{{last_user_message}}",
                    "stream": "{{stream}}",
                    "limit": "{{max_tokens}}"
                }
            },
            "response": {
                "openai_compatible": false,
                "content_path": "result.text",
                "finish_reason_path": "result.finish",
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            },
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "content_path": "token",
                "finish_reason_path": "finish",
                "done_path": "type",
                "done_values": ["message_stop"],
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            },
            "probe": {
                "path": "/health/chat",
                "model": "tiny-model",
                "success_status": [200, 204],
                "max_cost_micros": 100
            }
        }
    });

    let raw_sse = concat!(
        "event: token\n",
        "data: {\"payload\":{\"token\":\"he\"}}\n\n",
        "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
        "data: {\"payload\":{\"finish\":\"done\",\"usage\":{\"input\":3,\"output\":2}}}\n\n",
        "data: {\"payload\":{\"type\":\"message_stop\"}}\n\n",
    );
    let (status, replay) = call(
        &h,
        "POST",
        "/v1/admin/plugin-manifest/replay",
        Some(json!({
            "manifest": manifest,
            "base_url": "https://private.example/v1",
            "model": "tiny-model",
            "raw_sse": raw_sse
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "replay={replay}");
    assert_eq!(replay["chunks"].as_array().unwrap().len(), 3);

    let (status, channel) = call(
        &h,
        "POST",
        "/v1/admin/channels",
        Some(json!({
            "code": "plugin-builder-private",
            "provider_type": "plugin",
            "base_url": "https://private.example/v1",
            "supported_models": ["tiny-model"],
            "model_mapping": manifest
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "channel={channel}");
    assert_eq!(
        channel["model_mapping"]["plugin"]["probe"]["model"],
        "tiny-model"
    );
    assert_eq!(
        channel["model_mapping"]["plugin"]["probe"]["max_cost_micros"],
        100
    );
    assert_eq!(channel["capabilities"]["chat"], true);
    assert_eq!(channel["capabilities"]["streaming"], true);

    let channel_uuid = channel["id"].as_str().unwrap().split_once('_').unwrap().1;
    let (status, binding) = call(
        &h,
        "POST",
        &format!("/v1/admin/groups/{group_id}/bindings"),
        Some(json!({
            "channel_id": channel_uuid,
            "priority": 1,
            "weight": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "binding={binding}");
}
