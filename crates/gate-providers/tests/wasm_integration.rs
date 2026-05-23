//! 0.4.46: gate-providers + gate-wasm 端到端集成测试。
//!
//! 验证 CustomHttpProvider chat() 完整链路：
//! - manifest 载入 wasm 字段
//! - 请求 body 经 wasm chat_request_transform
//! - 响应 body 经 wasm chat_response_transform

use bytes::Bytes;
use gate_providers::CustomHttpProvider;
use gate_providers::Provider;
use gate_providers::types::{ChatMessage, ChatRequest, MessageContent, Role};
use gate_wasm::{HookContext, HookKind, WasmHost, WasmHostConfig, WasmtimeHost};
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Identity transform wasm 模块（同 0.4.24 测试使用的 WAT）。
const IDENTITY_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "gate_alloc") (param i32) (result i32) i32.const 4096)
  (func $copy (param $ptr i32) (param $len i32) (result i64)
    (local $i i32)
    (block $done
      (loop $copy
        (br_if $done (i32.ge_s (local.get $i) (local.get $len)))
        (i32.store8
          (i32.add (i32.const 4096) (local.get $i))
          (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $copy)))
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const 4096)) (i64.const 32))
      (i64.extend_i32_u (local.get $len))))
  (func (export "chat_request_transform") (param i32 i32) (result i64)
    (call $copy (local.get 0) (local.get 1)))
  (func (export "chat_response_transform") (param i32 i32) (result i64)
    (call $copy (local.get 0) (local.get 1)))
)
"#;

#[tokio::test]
async fn custom_provider_with_wasm_host_round_trips_chat() {
    // 1. mock OpenAI-compat endpoint
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "wasm e2e ok"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4, "total_tokens": 14}
        })))
        .mount(&server)
        .await;

    // 2. wasm host + identity 模块
    let host: Arc<dyn WasmHost> =
        Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
    let module_bytes = wat::parse_str(IDENTITY_WAT).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&module_bytes);
        hex::encode(h.finalize())
    };
    host.load_module("ch-e2e", &module_bytes, &sha).await.unwrap();

    // 3. CustomHttpProvider with manifest.security.wasm 字段 + with_wasm_host()
    let manifest = json!({
        "plugin": {
            "version": 1,
            "preset": { "provider": "openai_compatible" },
            "security": {
                "wasm": {
                    "module": "test.wasm",
                    "module_sha256": sha,
                    "max_memory_bytes": 16777216,
                    "max_cpu_ms": 50,
                    "hooks": ["chat_request_transform", "chat_response_transform"]
                }
            }
        }
    });
    let provider = CustomHttpProvider::new_with_opts(
        server.uri(),
        "sk-test",
        manifest,
        gate_providers::ProviderOpts {
            timeout_ms: 5_000,
        },
    )
    .unwrap()
    .with_wasm_host(host.clone(), "ch-e2e");

    // 4. chat()
    let req = ChatRequest {
        model: "test-model".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("hello".into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        ..Default::default()
    };
    let resp = provider.chat(req).await.unwrap();

    // 5. 验证：identity transform → 响应原样回来
    assert_eq!(resp.choices.len(), 1);
    let content = match &resp.choices[0].message.content {
        Some(MessageContent::Text(s)) => s.clone(),
        _ => panic!("expected text content"),
    };
    assert_eq!(content, "wasm e2e ok");
    assert_eq!(resp.usage.total_tokens, 14);
}

#[tokio::test]
async fn custom_provider_without_wasm_skips_transform() {
    // wasm 字段未配置时不应触发 wasm 路径
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-2",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "no wasm"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
        })))
        .mount(&server)
        .await;

    let manifest = json!({
        "plugin": {
            "version": 1,
            "preset": { "provider": "openai_compatible" }
        }
    });
    let provider = CustomHttpProvider::new_with_opts(
        server.uri(),
        "sk-test",
        manifest,
        gate_providers::ProviderOpts {
            timeout_ms: 5_000,
        },
    )
    .unwrap();

    let req = ChatRequest {
        model: "test-model".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("hi".into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        ..Default::default()
    };
    let resp = provider.chat(req).await.unwrap();
    assert_eq!(resp.usage.total_tokens, 7);
}

#[tokio::test]
async fn invoke_with_fallback_wraps_real_module() {
    // 直接验证 fallback wrapper + real module 联动
    let host: Arc<dyn WasmHost> =
        Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
    let module_bytes = wat::parse_str(IDENTITY_WAT).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&module_bytes);
        hex::encode(h.finalize())
    };
    host.load_module("ch-fb", &module_bytes, &sha).await.unwrap();

    let payload = Bytes::from_static(b"identity-fallback-test");
    let result = gate_wasm::invoke_with_fallback(
        host,
        "ch-fb",
        HookKind::ChatRequest,
        payload.clone(),
        HookContext::default(),
    )
    .await;
    assert_eq!(result, payload);
}

#[tokio::test]
async fn custom_provider_with_wasm_host_streams_chunks() {
    use futures::StreamExt;
    // 1. mock SSE endpoint，发 2 个 chunk + [DONE]
    let server = MockServer::start().await;
    let sse_body = "data: {\"id\":\"chatcmpl-s1\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"chatcmpl-s1\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(sse_body, "text/event-stream"),
        )
        .mount(&server)
        .await;

    // 2. wasm host + identity 模块 (含 stream_chunk_transform export)
    let host: Arc<dyn WasmHost> =
        Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
    let stream_wat = r#"
        (module
          (memory (export "memory") 1)
          (func (export "gate_alloc") (param i32) (result i32) i32.const 4096)
          (func $copy (param $ptr i32) (param $len i32) (result i64)
            (local $i i32)
            (block $done
              (loop $copy
                (br_if $done (i32.ge_s (local.get $i) (local.get $len)))
                (i32.store8
                  (i32.add (i32.const 4096) (local.get $i))
                  (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
                (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br $copy)))
            (i64.or
              (i64.shl (i64.extend_i32_u (i32.const 4096)) (i64.const 32))
              (i64.extend_i32_u (local.get $len))))
          (func (export "chat_request_transform") (param i32 i32) (result i64)
            (call $copy (local.get 0) (local.get 1)))
          (func (export "stream_chunk_transform") (param i32 i32) (result i64)
            (call $copy (local.get 0) (local.get 1)))
        )
    "#;
    let module_bytes = wat::parse_str(stream_wat).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&module_bytes);
        hex::encode(h.finalize())
    };
    host.load_module("ch-stream", &module_bytes, &sha).await.unwrap();

    let manifest = json!({
        "plugin": {
            "version": 1,
            "preset": { "provider": "openai_compatible" },
            "security": {
                "wasm": {
                    "module": "stream.wasm",
                    "module_sha256": sha,
                    "max_memory_bytes": 16777216,
                    "max_cpu_ms": 50,
                    "hooks": ["chat_request_transform", "stream_chunk_transform"]
                }
            }
        }
    });
    let provider = CustomHttpProvider::new_with_opts(
        server.uri(),
        "sk-test",
        manifest,
        gate_providers::ProviderOpts {
            timeout_ms: 5_000,
        },
    )
    .unwrap()
    .with_wasm_host(host.clone(), "ch-stream");

    // 3. chat_stream 验证 chunk 全部走过 wasm transform（identity → 原文本一致）
    let req = ChatRequest {
        model: "test-model".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("stream".into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        stream: true,
        ..Default::default()
    };
    let mut stream = provider.chat_stream(req).await.unwrap();
    let mut concat = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        for choice in &chunk.choices {
            if let Some(s) = &choice.delta.content {
                concat.push_str(s);
            }
        }
    }
    assert_eq!(concat, "hello world");
}
