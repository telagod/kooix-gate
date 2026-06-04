//! ADR-0006 e2e: build a v1 component plugin → load via UnifiedWasmHost → invoke transform.

use bytes::Bytes;
use gate_wasm::{
    HookContext, HookKind, UnifiedWasmHost, WasmFormat, WasmHost, WasmHostConfig, detect_format,
};

const COMPONENT_WASM: &[u8] =
    include_bytes!("../../../examples/wasm-transform-v1/plugin.component.wasm");

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn component_detected_as_v1() {
    assert_eq!(detect_format(COMPONENT_WASM).unwrap(), WasmFormat::Component);
}

#[tokio::test(flavor = "multi_thread")]
async fn unified_host_loads_v1_component() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
    let sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1-e2e", COMPONENT_WASM, &sha)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn v1_transform_request_modifies_model() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
    let sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1-transform", COMPONENT_WASM, &sha)
        .await
        .unwrap();

    let payload = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
    let result = host
        .invoke_hook(
            "ch-v1-transform",
            HookKind::ChatRequest,
            Bytes::from(payload),
            HookContext::default(),
        )
        .await
        .unwrap()
        .expect("should return transformed payload");

    let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(
        json["model"].as_str().unwrap(),
        "[transformed] gpt-4o",
        "v1 plugin should prepend [transformed] to model"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn v1_response_passthrough() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
    let sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1-resp", COMPONENT_WASM, &sha)
        .await
        .unwrap();

    let payload = r#"{"id":"resp-1","choices":[]}"#;
    let result = host
        .invoke_hook(
            "ch-v1-resp",
            HookKind::ChatResponse,
            Bytes::from(payload),
            HookContext::default(),
        )
        .await
        .unwrap()
        .expect("should return response");

    let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(json["id"].as_str().unwrap(), "resp-1");
}

#[tokio::test(flavor = "multi_thread")]
async fn v1_stream_chunk_passthrough() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
    let sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1-stream", COMPONENT_WASM, &sha)
        .await
        .unwrap();

    let payload = r#"{"delta":{"content":"hello"}}"#;
    let result = host
        .invoke_hook(
            "ch-v1-stream",
            HookKind::StreamChunk,
            Bytes::from(payload),
            HookContext::default(),
        )
        .await
        .unwrap()
        .expect("should return stream chunk");

    let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(json["delta"]["content"].as_str().unwrap(), "hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn v0_and_v1_coexist_in_unified_host() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();

    // Load v0 core module
    let v0_module = wat::parse_str("(module)").unwrap();
    let v0_sha = sha256(&v0_module);
    host.load_module("ch-v0", &v0_module, &v0_sha)
        .await
        .unwrap();

    // Load v1 component
    let v1_sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1", COMPONENT_WASM, &v1_sha)
        .await
        .unwrap();

    // v0 passthrough (no hook exported → identity)
    let v0_payload = Bytes::from_static(b"v0-data");
    let v0_result = host
        .invoke_hook("ch-v0", HookKind::ChatRequest, v0_payload.clone(), HookContext::default())
        .await
        .unwrap();
    assert_eq!(v0_result, Some(v0_payload));

    // v1 transforms
    let v1_payload = Bytes::from(r#"{"model":"claude-3"}"#);
    let v1_result = host
        .invoke_hook("ch-v1", HookKind::ChatRequest, v1_payload, HookContext::default())
        .await
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&v1_result).unwrap();
    assert_eq!(json["model"].as_str().unwrap(), "[transformed] claude-3");
}

#[tokio::test(flavor = "multi_thread")]
async fn v1_unload_and_reinvoke_returns_none() {
    let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
    let sha = sha256(COMPONENT_WASM);
    host.load_module("ch-v1-unload", COMPONENT_WASM, &sha)
        .await
        .unwrap();
    host.unload_module("ch-v1-unload").await.unwrap();

    let result = host
        .invoke_hook(
            "ch-v1-unload",
            HookKind::ChatRequest,
            Bytes::from(r#"{"model":"x"}"#),
            HookContext::default(),
        )
        .await
        .unwrap();
    assert!(result.is_none());
}
