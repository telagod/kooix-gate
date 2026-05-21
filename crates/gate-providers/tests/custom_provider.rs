//! Custom HTTP provider plugin integration tests.

use futures::StreamExt;
use gate_providers::{ChatMessage, ChatRequest, CustomHttpProvider, Provider, ProviderError, Role};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_req(stream: bool) -> ChatRequest {
    ChatRequest {
        model: "odd-model".into(),
        messages: vec![ChatMessage::text(Role::User, "hello private")],
        max_tokens: Some(32),
        stream,
        ..Default::default()
    }
}

#[tokio::test]
async fn plugin_maps_private_non_stream_request_and_response() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .and(header("x-api-key", "secret-key"))
        .and(body_json(json!({
            "modelName": "odd-model",
            "prompt": "hello private",
            "stream": false,
            "limit": 32
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "rid": "resp-1",
            "model_name": "odd-model-native",
            "result": { "text": "mapped ok", "finish": "done" },
            "usage": { "input": 7, "output": 2 }
        })))
        .mount(&upstream)
        .await;

    let manifest = json!({
        "plugin": {
            "request": {
                "chat_path": "/private/chat",
                "headers": { "X-Api-Key": "{{api_key}}" },
                "body": {
                    "modelName": "{{model}}",
                    "prompt": "{{last_user_message}}",
                    "stream": "{{stream}}",
                    "limit": "{{max_tokens}}"
                }
            },
            "response": {
                "openai_compatible": false,
                "id_path": "rid",
                "model_path": "model_name",
                "content_path": "result.text",
                "finish_reason_path": "result.finish",
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            }
        }
    });

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        manifest,
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.id, "resp-1");
    assert_eq!(resp.model, "odd-model-native");
    assert_eq!(resp.choices[0].message.content_text(), "mapped ok");
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(gate_providers::FinishReason::Stop)
    );
    assert_eq!(resp.usage.total_tokens, 9);
}

#[tokio::test]
async fn plugin_maps_response_paths_fallback_tool_calls_metadata_and_usage_units() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "trace": { "request_id": "up-req-42" },
            "result": {
                "alternatives": [
                    {
                        "model_name": null,
                        "native_model": "private-native",
                        "answer": {
                            "reasoning": "思路",
                            "text": "结果"
                        },
                        "finish": "tool_use",
                        "toolCalls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "lookup",
                                    "arguments": "{\"q\":\"x\"}"
                                }
                            }
                        ]
                    }
                ]
            },
            "stats": {
                "input": "11",
                "output": 3,
                "cached": 2,
                "reasoning": 5,
                "images": 1,
                "audio_seconds": "1.25"
            },
            "vendor": {
                "region": "moon-1",
                "usage": { "opaque": true }
            }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        json!({
            "plugin": {
                "request": { "chat_path": "/private/chat" },
                "response": {
                    "openai_compatible": false,
                    "id_path": "missing.id|trace.request_id|default:\"fallback-id\"",
                    "model_path": "result.alternatives.0.model_name|result.alternatives.0.native_model",
                    "content_path": "result.alternatives.0.answer.text",
                    "reasoning_content_path": "result.alternatives.0.answer.reasoning",
                    "tool_calls_path": "result.alternatives.0.toolCalls",
                    "finish_reason_path": "result.alternatives.0.finish",
                    "request_id_path": "trace.request_id",
                    "metadata_path": "vendor",
                    "usage": {
                        "prompt_tokens_path": "stats.input",
                        "completion_tokens_path": "stats.output",
                        "total_tokens_path": "stats.total|default:14",
                        "cached_tokens_path": "stats.cached",
                        "reasoning_tokens_path": "stats.reasoning",
                        "image_units_path": "stats.images",
                        "audio_seconds_path": "stats.audio_seconds",
                        "raw_path": "stats"
                    }
                }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.id, "up-req-42");
    assert_eq!(resp.model, "private-native");
    assert_eq!(resp.request_id.as_deref(), Some("up-req-42"));
    assert_eq!(resp.upstream_metadata.as_ref().unwrap()["region"], "moon-1");
    assert_eq!(resp.choices[0].message.content_text(), "思路\n结果");
    assert_eq!(
        resp.choices[0].message.tool_calls.as_ref().unwrap()[0].id,
        "call_1"
    );
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(gate_providers::FinishReason::ToolCalls)
    );
    assert_eq!(resp.usage.prompt_tokens, 11);
    assert_eq!(resp.usage.completion_tokens, 3);
    assert_eq!(resp.usage.total_tokens, 14);
    assert_eq!(resp.usage.cached_tokens, 2);
    assert_eq!(resp.usage.reasoning_tokens, Some(5));
    assert_eq!(resp.usage.image_units, Some(1));
    assert_eq!(resp.usage.audio_seconds, Some(1.25));
    assert_eq!(resp.usage.raw.as_ref().unwrap()["reasoning"], 5);
}

#[tokio::test]
async fn plugin_normalizes_private_sse_stream() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        ": keepalive\r\n",
        "event: token\r\n",
        "data: {\"payload\":{\"rid\":\"s1\",\"model_name\":\"native\",\"speaker\":\"assistant\"}}\r\n\r\n",
        "data: {\"payload\":{\"token\":\"he\"}}\n\n",
        "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
        "data: {\"payload\":{\"finish\":\"done\",\"usage\":{\"input\":4,\"output\":2}}}\n\n",
        "data: EOF\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let manifest = json!({
        "plugin": {
            "request": { "chat_path": "/private/chat" },
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "id_path": "rid",
                "model_path": "model_name",
                "role_path": "speaker",
                "content_path": "token",
                "finish_reason_path": "finish",
                "done": ["EOF"],
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "completion_tokens_path": "usage.output"
                }
            }
        }
    });

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        manifest,
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    let mut finish = None;
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
        if chunk.choices[0].finish_reason.is_some() {
            finish = chunk.choices[0].finish_reason;
        }
        if chunk.usage.is_some() {
            usage = chunk.usage.clone();
        }
    }

    assert_eq!(content, "hello");
    assert_eq!(finish, Some(gate_providers::FinishReason::Stop));
    assert_eq!(usage.unwrap().total_tokens, 6);
}

#[tokio::test]
async fn plugin_normalizes_event_split_tool_delta_usage_and_vendor_done() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "event: ping\n",
        "data: {\"payload\":{\"token\":\"ignored\"}}\n\n",
        "event: meta\n",
        "data: {\"payload\":{\"rid\":\"s2\",\"model_name\":\"native-v2\",\"speaker\":\"assistant\"}}\n\n",
        "event: token\n",
        "data: {\"payload\":{\"token\":\"use \"}}\n\n",
        "event: tool_delta\n",
        "data: {\"payload\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\\\"x\\\"}\"}}]}}\n\n",
        "event: usage\n",
        "data: {\"payload\":{\"usage\":{\"input\":8,\"cached\":3,\"reasoning\":2,\"raw_vendor\":\"meter\"}}}\n\n",
        "event: done\n",
        "data: {\"payload\":{\"finish\":\"tool_use\",\"type\":\"message_stop\"}}\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let manifest = json!({
        "plugin": {
            "request": { "chat_path": "/private/chat" },
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "ignore_events": ["ping"],
                "done_path": "type",
                "done_values": ["message_stop"],
                "id_path": "rid",
                "model_path": "model_name",
                "role_path": "speaker",
                "content_path": "token",
                "tool_calls_path": "tool_calls",
                "finish_reason_path": "finish",
                "usage": {
                    "prompt_tokens_path": "usage.input",
                    "cached_tokens_path": "usage.cached",
                    "reasoning_tokens_path": "usage.reasoning",
                    "raw_path": "usage"
                }
            }
        }
    });

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        manifest,
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.unwrap());
    }

    assert_eq!(chunks.len(), 5);
    assert_eq!(chunks[0].id, "s2");
    assert_eq!(chunks[0].model, "native-v2");
    assert_eq!(
        chunks[0].choices[0].delta.role,
        Some(gate_providers::Role::Assistant)
    );
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("use "));
    assert_eq!(
        chunks[2].choices[0].delta.tool_calls.as_ref().unwrap()[0]
            .id
            .as_deref(),
        Some("call_1")
    );
    let usage = chunks[3].usage.as_ref().unwrap();
    assert_eq!(usage.prompt_tokens, 8);
    assert_eq!(usage.cached_tokens, 3);
    assert_eq!(usage.reasoning_tokens, Some(2));
    assert_eq!(usage.raw.as_ref().unwrap()["raw_vendor"], "meter");
    assert_eq!(
        chunks[4].choices[0].finish_reason,
        Some(gate_providers::FinishReason::ToolCalls)
    );
}

#[tokio::test]
async fn preset_openai_compatible_posts_normalized_request_and_streams_usage() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"preset\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"cmpl-1\",\"model\":\"gpt-compatible\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer preset-key"))
        .and(body_json(json!({
            "model": "odd-model",
            "messages": [{ "role": "user", "content": "hello private" }],
            "max_tokens": 32,
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "preset-key",
        json!({ "plugin": { "preset": { "provider": "openai_compatible" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
        if chunk.usage.is_some() {
            usage = chunk.usage.clone();
        }
    }

    assert_eq!(content, "preset");
    assert_eq!(usage.unwrap().total_tokens, 4);
}

#[tokio::test]
async fn preset_anthropic_messages_posts_native_body_and_normalizes_response() {
    // ADR-0002 fast-path 接通后，preset.provider="anthropic_messages" 走
    // fastpath_anthropic_chat（直接调 to_anthropic_request）而不是 manifest 模板。
    // fast-path body 跟编译期 AnthropicProvider 一致：stream 字段为 None 时 skip。
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "anthropic-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(body_json(json!({
            "model": "odd-model",
            "max_tokens": 32,
            "messages": [{ "role": "user", "content": "hello private" }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg-1",
            "model": "claude-native",
            "content": [{ "type": "text", "text": "anthropic mapped" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 5, "output_tokens": 2, "cache_read_input_tokens": 1 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "anthropic-key",
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.id, "msg-1");
    assert_eq!(resp.model, "claude-native");
    assert_eq!(resp.choices[0].message.content_text(), "anthropic mapped");
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(gate_providers::FinishReason::Stop)
    );
    assert_eq!(resp.usage.prompt_tokens, 5);
    assert_eq!(resp.usage.completion_tokens, 2);
    assert_eq!(resp.usage.total_tokens, 7);
    assert_eq!(resp.usage.cached_tokens, 1);
}

#[tokio::test]
async fn plugin_error_mapper_normalizes_rate_limit_and_retry_after() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/private/chat"))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("retry-after", "9")
                .set_body_json(json!({
                    "vendor_error": {
                        "status": 429,
                        "code": "quota_busy",
                        "message": "vendor asked us to slow down"
                    }
                })),
        )
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        json!({
            "plugin": {
                "request": { "chat_path": "/private/chat" },
                "error": {
                    "status_path": "vendor_error.status",
                    "code_path": "vendor_error.code",
                    "message_path": "vendor_error.message",
                    "rate_limit_status": [429],
                    "retryable_codes": ["quota_busy"],
                    "cooldown_ms": 12000,
                    "circuit_breaker_failures": 2
                }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider.chat(make_req(false)).await.unwrap_err();
    match err {
        ProviderError::Mapped {
            status,
            code,
            metadata,
            ..
        } => {
            assert_eq!(status, Some(429));
            assert_eq!(code.as_deref(), Some("quota_busy"));
            assert_eq!(metadata.retry_after_ms, Some(9_000));
            assert_eq!(metadata.cooldown_ms, Some(12_000));
            assert_eq!(metadata.circuit_breaker_failures, Some(2));
        }
        other => panic!("expected mapped rate limit error, got {other:?}"),
    }
}

#[tokio::test]
async fn plugin_error_mapper_normalizes_model_not_found_and_policy_block() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/missing-model"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "code": "model_missing", "message": "no such model" }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/safety"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "code": "blocked_by_vendor_policy", "message": "safety policy block" }
        })))
        .mount(&upstream)
        .await;

    let missing = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        json!({
            "plugin": {
                "request": { "chat_path": "/missing-model" },
                "error": {
                    "code_path": "error.code",
                    "message_path": "error.message",
                    "model_not_found_status": [404]
                }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap()
    .chat(make_req(false))
    .await
    .unwrap_err();
    assert!(matches!(
        missing,
        ProviderError::Mapped {
            metadata: gate_providers::error::ProviderErrorMetadata {
                kind: gate_providers::error::NormalizedProviderErrorKind::ModelNotFound,
                ..
            },
            ..
        }
    ));

    let safety = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        json!({
            "plugin": {
                "request": { "chat_path": "/safety" },
                "error": {
                    "code_path": "error.code",
                    "message_path": "error.message",
                    "safety_block_codes": ["blocked_by_vendor_policy"]
                }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap()
    .chat(make_req(false))
    .await
    .unwrap_err();
    assert!(matches!(
        safety,
        ProviderError::Mapped {
            metadata: gate_providers::error::ProviderErrorMetadata {
                kind: gate_providers::error::NormalizedProviderErrorKind::Policy,
                ..
            },
            ..
        }
    ));
}

#[tokio::test]
async fn plugin_probe_request_uses_manifest_path_body_status_and_cost() {
    let upstream = MockServer::start().await;
    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "secret-key",
        json!({
            "plugin": {
                "auth": { "strategy": "api_key_header", "header_name": "X-Api-Key" },
                "probe": {
                    "model": "tiny-health",
                    "path": "/probe/{{model}}",
                    "body": {
                        "model": "{{model}}",
                        "messages": "{{messages}}",
                        "max_tokens": "{{max_tokens}}"
                    },
                    "success_status": [200, 204],
                    "max_cost_micros": 25
                }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let probe = provider.build_probe_request().await.unwrap();
    assert_eq!(probe.method, reqwest::Method::POST);
    assert_eq!(probe.url, format!("{}/probe/tiny-health", upstream.uri()));
    assert_eq!(
        probe.headers.get("authorization").unwrap(),
        "Bearer secret-key"
    );
    assert_eq!(probe.model, "tiny-health");
    assert_eq!(probe.success_status, vec![200, 204]);
    assert_eq!(probe.max_cost_micros, Some(25));
    let body: Value = serde_json::from_slice(probe.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["model"], "tiny-health");
    assert_eq!(body["max_tokens"], 1);
    assert_eq!(body["messages"][0]["role"], "user");
}

// ─── ADR-0002 fast-path runtime tests ──────────────────────────────────────

#[tokio::test]
async fn fastpath_openai_chat_routes_through_dispatch() {
    // preset.provider="openai" 触发 builtin_fastpath=true，
    // CustomHttpProvider::chat 走 fastpath_openai_chat 直接 POST，跳过 manifest 解释器。
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer fast-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-fastpath",
            "object": "chat.completion",
            "created": 1730000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "fast" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "fast-key",
        json!({ "plugin": { "preset": { "provider": "openai" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.choices[0].message.content_text(), "fast");
    assert_eq!(resp.usage.total_tokens, 2);
}

#[tokio::test]
async fn fastpath_openai_chat_stream_routes_through_dispatch() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer fast-key"))
        // fast-path 会强制注入 stream_options.include_usage=true（与编译期 OpenAi 一致）
        .and(body_json(json!({
            "model": "odd-model",
            "messages": [{ "role": "user", "content": "hello private" }],
            "max_tokens": 32,
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "fast-key",
        json!({ "plugin": { "preset": { "provider": "openai" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    let mut usage = None;
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
        if chunk.usage.is_some() {
            usage = chunk.usage.clone();
        }
    }

    assert_eq!(content, "hi");
    assert_eq!(usage.unwrap().total_tokens, 2);
}

#[tokio::test]
async fn fastpath_does_not_apply_to_openai_compatible_preset() {
    // preset.provider="openai_compatible" 不在 fast-path 白名单 → 走 manifest 解释器。
    // 这个 test 的目的：确认 fast-path 没误伤其他 OpenAI 兼容协议的渠道。
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer compat-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-compat",
            "object": "chat.completion",
            "created": 1730000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "compat" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "compat-key",
        json!({ "plugin": { "preset": { "provider": "openai_compatible" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.choices[0].message.content_text(), "compat");
}

#[tokio::test]
async fn fastpath_anthropic_chat_routes_through_dispatch() {
    // preset.provider="anthropic_messages" → builtin_fastpath=true → fastpath_anthropic_chat
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "ant-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_fast",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-sonnet-latest",
            "content": [
                { "type": "text", "text": "anthropic fastpath" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 3, "output_tokens": 2, "cache_read_input_tokens": 0 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "ant-key",
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.choices[0].message.content_text(), "anthropic fastpath");
    assert_eq!(resp.usage.prompt_tokens, 3);
    assert_eq!(resp.usage.completion_tokens, 2);
    assert_eq!(resp.usage.total_tokens, 5);
}

#[tokio::test]
async fn fastpath_anthropic_chat_stream_routes_through_dispatch() {
    let upstream = MockServer::start().await;
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_s\",\"model\":\"claude-3-5-sonnet-latest\",\"usage\":{\"input_tokens\":4,\"output_tokens\":0,\"cache_read_input_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"streamed\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "ant-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "ant-key",
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let mut stream = provider.chat_stream(make_req(true)).await.unwrap();
    let mut content = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        if let Some(c) = &chunk.choices[0].delta.content {
            content.push_str(c);
        }
    }
    assert_eq!(content, "streamed");
}

#[tokio::test]
async fn fastpath_azure_chat_uses_deployment_url_and_api_key_header() {
    let upstream = MockServer::start().await;
    // Azure URL 模板：/openai/deployments/{model}/chat/completions?api-version=X
    Mock::given(method("POST"))
        .and(path("/openai/deployments/odd-model/chat/completions"))
        .and(wiremock::matchers::query_param(
            "api-version",
            "2024-08-01-preview",
        ))
        .and(header("api-key", "az-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-azure",
            "object": "chat.completion",
            "created": 1730000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "azure fastpath" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 2, "completion_tokens": 2, "total_tokens": 4 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "az-key",
        json!({ "plugin": { "preset": { "provider": "azure_openai" } } }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.choices[0].message.content_text(), "azure fastpath");
    assert_eq!(resp.usage.total_tokens, 4);
}

#[tokio::test]
async fn fastpath_azure_uses_manifest_api_version_override() {
    // preset.api_version 应该传到 URL query
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/odd-model/chat/completions"))
        .and(wiremock::matchers::query_param(
            "api-version",
            "2024-02-15-preview",
        ))
        .and(header("api-key", "az-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-azure-v2",
            "object": "chat.completion",
            "created": 1730000000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "v2" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&upstream)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        upstream.uri(),
        "az-key",
        json!({
            "plugin": {
                "preset": { "provider": "azure_openai", "api_version": "2024-02-15-preview" }
            }
        }),
        gate_providers::ProviderOpts::default(),
    )
    .unwrap();

    let resp = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(resp.choices[0].message.content_text(), "v2");
}
