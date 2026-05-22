//! custom_provider integration tests — 从 mod.rs 拆出（M1.3 T3.2 收尾）。
#![cfg(test)]

use super::*;
use crate::plugin_manifest::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_MAX_SSE_EVENT_BYTES};
use futures::StreamExt;

fn make_req(stream: bool) -> ChatRequest {
    ChatRequest {
        model: "odd-model".into(),
        messages: vec![ChatMessage::text(Role::User, "Hi plugin")],
        max_tokens: Some(16),
        stream,
        ..Default::default()
    }
}

#[test]
fn request_template_preserves_native_json_values() {
    let manifest = json!({
        "request": {
            "body": {
                "m": "{{model}}",
                "prompt": "{{last_user_message}}",
                "streaming": "{{stream}}",
                "limit": "{{max_tokens}}"
            }
        }
    });
    let provider = CustomHttpProvider::new_with_opts(
        "http://x",
        "k",
        manifest,
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let body = provider.build_body(&make_req(true)).unwrap();
    assert_eq!(body["m"], "odd-model");
    assert_eq!(body["prompt"], "Hi plugin");
    assert_eq!(body["streaming"], true);
    assert_eq!(body["limit"], 16);
}

#[test]
fn request_template_supports_tools_tool_choice_metadata_and_prunes_empty_fields() {
    let manifest = json!({
        "plugin": {
            "version": 1,
            "request": {
                "path": "/deployments/{{metadata.deployment}}/chat",
                "query": {
                    "tenant": "{{metadata.tenant}}",
                    "missing": "{{metadata.missing}}"
                },
                "headers": {
                    "X-Tenant": "{{metadata.tenant}}",
                    "X-Missing": "{{metadata.missing}}"
                },
                "body": {
                    "model": "{{model}}",
                    "messages": "{{messages}}",
                    "tools": "{{tools}}",
                    "tool_choice": "{{tool_choice}}",
                    "metadata": "{{metadata}}",
                    "drop_null": "{{metadata.missing}}",
                    "drop_empty_array": "{{request.parallel_tool_calls}}",
                    "nested": {
                        "keep": "literal",
                        "drop": "{{metadata.missing}}"
                    }
                }
            }
        }
    });
    let provider = CustomHttpProvider::new_with_opts(
        "https://private.example/v1",
        "k",
        manifest,
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let mut req = make_req(false);
    req.tools = Some(vec![ToolDef {
        r#type: "function".into(),
        function: FunctionDef {
            name: "lookup".into(),
            description: Some("Lookup docs".into()),
            parameters: Some(json!({"type": "object"})),
        },
    }]);
    req.tool_choice = Some(json!({"type": "function", "function": {"name": "lookup"}}));
    req.extra.insert(
        "metadata".into(),
        json!({ "tenant": "acme", "deployment": "private-deploy" }),
    );

    assert_eq!(
        provider.endpoint_url_for(&req).unwrap(),
        "https://private.example/v1/deployments/private-deploy/chat?tenant=acme"
    );
    let headers = provider.request_headers_for(&req).unwrap();
    assert_eq!(headers.get("x-tenant").unwrap(), "acme");
    assert!(headers.get("x-missing").is_none());

    let body = provider.build_body(&req).unwrap();
    assert_eq!(body["model"], "odd-model");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["tools"][0]["function"]["name"], "lookup");
    assert_eq!(body["tool_choice"]["function"]["name"], "lookup");
    assert_eq!(body["metadata"]["tenant"], "acme");
    assert!(body.get("drop_null").is_none());
    assert!(body.get("drop_empty_array").is_none());
    assert_eq!(body["nested"], json!({ "keep": "literal" }));
}

#[test]
fn openai_compatible_preset_expands_defaults_and_usage_stream_options() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.deepseek.com/v1",
        "sk-test",
        json!({ "plugin": { "preset": { "provider": "deepseek" } } }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    assert_eq!(
        provider.endpoint_url_for(&make_req(true)).unwrap(),
        "https://api.deepseek.com/v1/chat/completions"
    );
    let body = provider.build_body(&make_req(true)).unwrap();
    assert_eq!(body["model"], "odd-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert!(provider.manifest.response.is_openai_compatible());
    assert!(provider.manifest.stream.is_openai_compatible());
}

#[test]
fn vertex_openai_preset_targets_openai_compatible_vertex_endpoint() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi",
        "vertex-token",
        json!({ "plugin": { "preset": { "provider": "vertex_openai" } } }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let req = make_req(false);
    assert_eq!(
        provider.endpoint_url_for(&req).unwrap(),
        "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi/chat/completions"
    );
    let headers = provider.request_headers_for(&req).unwrap();
    assert_eq!(headers.get("authorization").unwrap(), "Bearer vertex-token");
    assert_eq!(
        provider
            .embedding_endpoint_url_for(&EmbeddingRequest {
                model: "text-embedding-3-small".into(),
                input: EmbeddingInput::Single("hello".into()),
                encoding_format: None,
                dimensions: None,
            })
            .unwrap(),
        "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi/embeddings"
    );
    assert!(provider.manifest.response.is_openai_compatible());
    assert!(provider.manifest.stream.is_openai_compatible());
}

#[test]
fn azure_preset_templates_deployment_path_and_api_key_header() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://example.openai.azure.com",
        "azure-key",
        json!({
            "plugin": {
                "preset": { "provider": "azure_openai", "api_version": "2024-02-15-preview" }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let req = make_req(false);
    let body = provider.build_body(&req).unwrap();
    assert_eq!(body["model"], "odd-model");
    assert_eq!(
        provider.endpoint_url_for(&req).unwrap(),
        "https://example.openai.azure.com/openai/deployments/odd-model/chat/completions?api-version=2024-02-15-preview"
    );
    assert_eq!(
        provider
            .embedding_endpoint_url_for(&EmbeddingRequest {
                model: "odd-model".into(),
                input: EmbeddingInput::Single("hello".into()),
                encoding_format: None,
                dimensions: None,
            })
            .unwrap(),
        "https://example.openai.azure.com/openai/deployments/odd-model/embeddings?api-version=2024-02-15-preview"
    );
    let headers = provider.request_headers_for(&req).unwrap();
    assert_eq!(headers.get("api-key").unwrap(), "azure-key");
    assert!(headers.get("authorization").is_none());
}

#[test]
fn anthropic_preset_adapts_openai_request_to_messages_api() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.anthropic.com",
        "anthropic-key",
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let req = ChatRequest {
        model: "claude-sonnet".into(),
        messages: vec![
            ChatMessage::text(Role::System, "You are terse"),
            ChatMessage::text(Role::User, "Hi"),
        ],
        max_tokens: Some(32),
        stream: true,
        ..Default::default()
    };

    assert_eq!(
        provider.endpoint_url_for(&req).unwrap(),
        "https://api.anthropic.com/v1/messages"
    );
    let headers = provider.request_headers_for(&req).unwrap();
    assert_eq!(headers.get("x-api-key").unwrap(), "anthropic-key");
    assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert!(headers.get("authorization").is_none());
    let body = provider.build_body(&req).unwrap();
    assert_eq!(body["model"], "claude-sonnet");
    assert_eq!(body["max_tokens"], 32);
    assert_eq!(body["system"], "You are terse");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "Hi");
    assert_eq!(body["stream"], true);
}

#[test]
fn plugin_auth_api_key_query_appends_secret_to_url() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com/v1",
        "query-key",
        json!({
            "plugin": {
                "version": 1,
                "auth": { "strategy": "api_key_query", "query_name": "key" },
                "request": {
                    "path": "/private/chat",
                    "query": { "model": "{{model}}" }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let url = provider.endpoint_url_for(&make_req(false)).unwrap();
    assert_eq!(
        url,
        "https://api.example.com/v1/private/chat?model=odd-model&key=query-key"
    );
    let headers = provider.request_headers_for(&make_req(false)).unwrap();
    assert!(headers.get("authorization").is_none());
}

#[test]
fn embedding_request_template_uses_embedding_path_body_and_auth() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com/v1",
        "embed-key",
        json!({
            "plugin": {
                "version": 1,
                "capabilities": { "embeddings": true },
                "auth": { "strategy": "api_key_header", "header_name": "X-Embed-Key" },
                "request": {
                    "embedding_path": "/private/embed/{{model}}",
                    "query": { "dims": "{{dimensions}}" },
                    "embedding_body": {
                        "modelName": "{{model}}",
                        "texts": "{{input_texts}}",
                        "format": "{{encoding_format}}",
                        "dimensions": "{{dimensions}}"
                    }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let req = EmbeddingRequest {
        model: "embed-model".into(),
        input: EmbeddingInput::Multiple(vec!["hello".into(), "world".into()]),
        encoding_format: Some("float".into()),
        dimensions: Some(3),
    };

    assert_eq!(
        provider.embedding_endpoint_url_for(&req).unwrap(),
        "https://api.example.com/v1/private/embed/embed-model?dims=3"
    );
    let body = provider.build_embedding_body(&req).unwrap();
    assert_eq!(body["modelName"], "embed-model");
    assert_eq!(body["texts"], json!(["hello", "world"]));
    assert_eq!(body["format"], "float");
    assert_eq!(body["dimensions"], 3);
    let headers = provider.embedding_request_headers_for(&req).unwrap();
    assert_eq!(headers.get("x-embed-key").unwrap(), "embed-key");
}

#[test]
fn custom_embedding_response_mapper_normalizes_vendor_vectors() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com/v1",
        "embed-key",
        json!({
            "plugin": {
                "version": 1,
                "embedding_response": {
                    "openai_compatible": false,
                    "data_path": "result.vectors",
                    "embedding_path": "values",
                    "index_path": "position",
                    "model_path": "result.model",
                    "usage": {
                        "prompt_tokens_path": "usage.input_tokens",
                        "total_tokens_path": "usage.total_tokens"
                    }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let response = provider
        .parse_embedding_response(
            json!({
                "result": {
                    "model": "vendor-embed",
                    "vectors": [
                        { "position": 1, "values": [0.1, "0.2"] },
                        { "position": 2, "values": [0.3, 0.4] }
                    ]
                },
                "usage": { "input_tokens": 7, "total_tokens": 7 }
            }),
            "fallback-model",
        )
        .unwrap();

    assert_eq!(response.object, "list");
    assert_eq!(response.model, "vendor-embed");
    assert_eq!(response.data[0].index, 1);
    assert_eq!(response.data[0].embedding, vec![0.1, 0.2]);
    assert_eq!(response.usage.total_tokens, 7);
}

#[test]
fn plugin_auth_basic_and_custom_headers_use_secret_slots() {
    // SAFETY: unit test only needs process env for a synthetic plugin secret.
    unsafe {
        std::env::set_var("KOOIX_PLUGIN_SECRET_USER", "basic-user");
    }

    let basic = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "basic-pass",
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "basic",
                    "username_slot": "user",
                    "password_slot": "primary"
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let headers = basic.request_headers_for(&make_req(false)).unwrap();
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Basic YmFzaWMtdXNlcjpiYXNpYy1wYXNz"
    );

    let custom = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "primary-key",
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "custom_headers",
                    "headers": {
                        "X-Api-Key": "{{api_key}}",
                        "X-Model": "{{model}}"
                    }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let headers = custom.request_headers_for(&make_req(false)).unwrap();
    assert_eq!(headers.get("x-api-key").unwrap(), "primary-key");
    assert_eq!(headers.get("x-model").unwrap(), "odd-model");
    assert!(headers.get("authorization").is_none());
}

#[test]
fn plugin_auth_uses_explicit_secret_slot_map() {
    let provider = CustomHttpProvider::new_with_secret_slots(
        "https://api.example.com",
        HashMap::from([
            ("primary".to_string(), "primary-key".to_string()),
            ("Alt-Key".to_string(), "alt-key".to_string()),
        ]),
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "api_key_header",
                    "header_name": "X-Alt-Key",
                    "secret_slot": "alt-key"
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let headers = provider.request_headers_for(&make_req(false)).unwrap();
    assert_eq!(headers.get("x-alt-key").unwrap(), "alt-key");
    assert!(
        headers
            .get("x-alt-key")
            .is_some_and(|value| value != "primary-key")
    );
}

#[test]
fn plugin_env_secret_slots_include_named_plugin_secrets() {
    // SAFETY: unit test owns synthetic plugin env names.
    unsafe {
        std::env::set_var("KOOIX_PLUGIN_SECRET_CLIENT_ID", "env-client");
        std::env::set_var("KOOIX_PLUGIN_SECRET_CLIENT_SECRET", "env-secret");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "env-aws-secret");
    }

    let secrets = CustomHttpProvider::env_secret_slots("missing-env-channel");
    assert_eq!(
        secrets.get("client_id").map(String::as_str),
        Some("env-client")
    );
    assert_eq!(
        secrets.get("client_secret").map(String::as_str),
        Some("env-secret")
    );
    assert_eq!(
        secrets.get("aws_secret_key").map(String::as_str),
        Some("env-aws-secret")
    );

    let provider = CustomHttpProvider::new_with_secret_slots(
        "https://api.example.com",
        secrets,
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "basic",
                    "username_slot": "client_id",
                    "password_slot": "client_secret"
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let headers = provider.request_headers_for(&make_req(false)).unwrap();
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Basic ZW52LWNsaWVudDplbnYtc2VjcmV0"
    );
}

#[test]
fn plugin_auth_hmac_signs_method_path_body_timestamp_nonce() {
    let provider = CustomHttpProvider::new_with_secret_slots(
        "https://api.example.com",
        HashMap::from([("signing".to_string(), "hmac-secret".to_string())]),
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "hmac",
                    "secret_slot": "signing",
                    "hmac": {
                        "signature_header": "X-Kooix-Signature",
                        "timestamp_header": "X-Kooix-Timestamp",
                        "nonce_header": "X-Kooix-Nonce",
                        "signed_payload": "{{method}}\n{{path}}\n{{query}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}",
                        "signature_encoding": "hex"
                    }
                },
                "request": {
                    "path": "/private/chat/{{model}}",
                    "query": { "stream": "{{stream}}" },
                    "body": { "prompt": "{{last_user_message}}", "stream": "{{stream}}" }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let req = make_req(false);
    let body = provider.request_json_body(&req).unwrap();
    let endpoint = provider.endpoint_url_for(&req).unwrap();
    let ctx = provider.request_context_for(&req).unwrap();
    let signature = provider
        .hmac_signature(&endpoint, &body, "POST", "1700000000", "nonce-1", &ctx)
        .unwrap();

    assert_eq!(
        provider
            .hmac_signed_payload(&endpoint, &body, "POST", "1700000000", "nonce-1", &ctx)
            .unwrap(),
        format!(
            "POST\n/private/chat/odd-model\nstream=false\n{}\n1700000000\nnonce-1",
            sha256_hex(&body)
        )
    );
    assert_eq!(
        signature,
        "d7304b247aa7c8ddc7618cca688b5f2f1de8dd13cc5169739655a9348510e854"
    );

    let headers = provider
        .request_headers_for_parts(&req, &endpoint, &body, "POST")
        .unwrap();
    assert!(headers.get("x-kooix-timestamp").is_some());
    assert!(headers.get("x-kooix-nonce").is_some());
    assert_eq!(
        headers
            .get("x-kooix-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .len(),
        64
    );
    assert!(headers.get("authorization").is_none());
}

#[test]
fn plugin_auth_aws_sigv4_signs_bedrock_request() {
    let provider = CustomHttpProvider::new_with_secret_slots(
        "https://bedrock-runtime.us-east-1.amazonaws.com",
        HashMap::from([
            ("primary".to_string(), "AKIDEXAMPLE".to_string()),
            (
                "aws_secret_key".to_string(),
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            ),
            ("aws_session_token".to_string(), "session-token".to_string()),
        ]),
        json!({
            "plugin": {
                "version": 1,
                "preset": { "provider": "bedrock_converse" },
                "auth": {
                    "strategy": "aws_sigv4",
                    "aws_sigv4": {
                        "service": "bedrock",
                        "region": "us-east-1"
                    }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let req = make_req(false);
    let body = provider.request_json_body(&req).unwrap();
    let endpoint = provider.endpoint_url_for(&req).unwrap();
    let signature = provider
        .aws_sigv4_signature(&endpoint, &body, "POST", "20260519T092000Z", "20260519")
        .unwrap();

    assert!(signature.canonical_request.starts_with(concat!(
        "POST\n",
        "/model/odd-model/converse\n\n",
        "host:bedrock-runtime.us-east-1.amazonaws.com\n",
        "x-amz-content-sha256:"
    )));
    assert!(signature.string_to_sign.starts_with(
        "AWS4-HMAC-SHA256\n20260519T092000Z\n20260519/us-east-1/bedrock/aws4_request\n"
    ));
    assert_eq!(
        signature.authorization,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260519/us-east-1/bedrock/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=ceffee8ab945dd52eba6a21f6f61d5fd27c7b138ff1c6403c1815c1adebf3f9e"
    );

    let mut headers = HeaderMap::new();
    provider
        .apply_aws_sigv4_auth_headers_at(
            &mut headers,
            &endpoint,
            &body,
            "POST",
            "20260519T092000Z",
            "20260519",
        )
        .unwrap();
    assert!(headers.get("authorization").is_some());
    assert_eq!(
        headers.get("x-amz-date").unwrap().to_str().unwrap(),
        "20260519T092000Z"
    );
    assert_eq!(
        headers
            .get("x-amz-security-token")
            .unwrap()
            .to_str()
            .unwrap(),
        "session-token"
    );
    assert!(headers.get("x-amz-access-key").is_none());
    assert!(headers.get("x-amz-secret-key").is_none());
}

#[tokio::test]
async fn plugin_auth_oauth_client_credentials_caches_until_expiry() {
    let token_server = wiremock::MockServer::start().await;
    let chat_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .and(wiremock::matchers::body_string_contains(
            "grant_type=client_credentials",
        ))
        .and(wiremock::matchers::body_string_contains(
            "client_id=client-1",
        ))
        .and(wiremock::matchers::body_string_contains(
            "client_secret=secret-1",
        ))
        .and(wiremock::matchers::body_string_contains(
            "scope=chat%3Awrite",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "oauth-token-1",
            "token_type": "Bearer",
            "expires_in": 120
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/private/chat"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer oauth-token-1",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-oauth",
            "model": "odd-model",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .expect(2)
        .mount(&chat_server)
        .await;

    let provider = CustomHttpProvider::new_with_secret_slots(
        chat_server.uri(),
        HashMap::from([
            ("client_id".to_string(), "client-1".to_string()),
            ("client_secret".to_string(), "secret-1".to_string()),
        ]),
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": format!("{}/oauth/token", token_server.uri()),
                        "scope": "chat:write"
                    }
                },
                "security": {
                    "permissions": { "oauth_client_credentials": true }
                },
                "request": { "path": "/private/chat" }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let first = provider.chat(make_req(false)).await.unwrap();
    let second = provider.chat(make_req(false)).await.unwrap();
    assert_eq!(first.choices[0].message.content_text(), "ok");
    assert_eq!(second.usage.total_tokens, 2);
}

#[tokio::test]
async fn plugin_embedding_posts_openai_compatible_body_to_embeddings_path() {
    let server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/embeddings"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer embed-key",
        ))
        .and(wiremock::matchers::body_json(json!({
            "model": "text-embedding-3-small",
            "input": ["hello", "world"],
            "encoding_format": "float",
            "dimensions": 3
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                { "object": "embedding", "index": 0, "embedding": [0.1, 0.2, 0.3] },
                { "object": "embedding", "index": 1, "embedding": [0.4, 0.5, 0.6] }
            ],
            "model": "text-embedding-3-small",
            "usage": { "prompt_tokens": 4, "total_tokens": 4 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = CustomHttpProvider::new_with_opts(
        server.uri(),
        "embed-key",
        json!({
            "plugin": {
                "version": 1,
                "preset": { "provider": "openai_compatible" }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let response = provider
        .embed(EmbeddingRequest {
            model: "text-embedding-3-small".into(),
            input: EmbeddingInput::Multiple(vec!["hello".into(), "world".into()]),
            encoding_format: Some("float".into()),
            dimensions: Some(3),
        })
        .await
        .unwrap();

    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
    assert_eq!(response.usage.total_tokens, 4);
}

#[tokio::test]
async fn plugin_auth_oauth_client_credentials_refreshes_expired_token() {
    let token_server = wiremock::MockServer::start().await;
    let chat_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "short-token",
            "expires_in": 1
        })))
        .expect(2)
        .mount(&token_server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/private/chat"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer short-token",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-oauth",
            "model": "odd-model",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .expect(2)
        .mount(&chat_server)
        .await;

    let provider = CustomHttpProvider::new_with_secret_slots(
        chat_server.uri(),
        HashMap::from([
            ("client_id".to_string(), "client-1".to_string()),
            ("client_secret".to_string(), "secret-1".to_string()),
        ]),
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": format!("{}/oauth/token", token_server.uri()),
                        "expiry_skew_seconds": 0
                    }
                },
                "security": {
                    "permissions": { "oauth_client_credentials": true }
                },
                "request": { "path": "/private/chat" }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    provider.chat(make_req(false)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    provider.chat(make_req(false)).await.unwrap();
}

#[tokio::test]
async fn plugin_auth_oauth_client_credentials_rejects_invalid_token_response() {
    let token_server = wiremock::MockServer::start().await;
    let chat_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    let provider = CustomHttpProvider::new_with_secret_slots(
        chat_server.uri(),
        HashMap::from([
            ("client_id".to_string(), "client-1".to_string()),
            ("client_secret".to_string(), "secret-1".to_string()),
        ]),
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": format!("{}/oauth/token", token_server.uri())
                    }
                },
                "security": {
                    "permissions": { "oauth_client_credentials": true }
                },
                "request": { "path": "/private/chat" }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider.chat(make_req(false)).await.unwrap_err();
    assert!(err.to_string().contains("access_token"), "err={err}");
}

#[test]
fn preset_allows_request_overrides_without_losing_response_defaults() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://proxy.internal",
        "sk-test",
        json!({
            "plugin": {
                "preset": { "provider": "openai_compatible" },
                "request": {
                    "chat_path": "/custom/chat",
                    "headers": { "X-Proxy-Key": "{{api_key}}" }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    assert_eq!(
        provider.endpoint_url_for(&make_req(false)).unwrap(),
        "https://proxy.internal/custom/chat"
    );
    assert!(provider.manifest.response.is_openai_compatible());
    assert_eq!(
        provider
            .request_headers_for(&make_req(false))
            .unwrap()
            .get("x-proxy-key")
            .unwrap(),
        "sk-test"
    );
}

#[test]
fn plugin_manifest_blocks_absolute_chat_path_by_default() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "request": {
                    "chat_path": "http://169.254.169.254/latest/meta-data"
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
    assert!(
        err.to_string().contains("absolute URLs are disabled"),
        "err={err}"
    );
}

#[test]
fn plugin_sandbox_validate_endpoint_blocks_absolute_url_without_permission() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({ "plugin": {} }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider
        .sandbox
        .validate_endpoint("https://api.other.example/v1", EndpointKind::AbsolutePath)
        .unwrap_err();
    assert!(
        err.to_string().contains("absolute URLs are disabled"),
        "err={err}"
    );
}

#[test]
fn plugin_manifest_rejects_internal_absolute_url_even_when_enabled() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "request": {
                    "chat_path": "http://localhost/admin"
                },
                "security": {
                    "allow_absolute_chat_path": true,
                    "permissions": { "absolute_urls": true }
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider.endpoint_url_for(&make_req(false)).unwrap_err();
    assert!(
        err.to_string().contains("forbidden host localhost"),
        "err={err}"
    );
}

#[test]
fn plugin_manifest_requires_permission_for_absolute_urls() {
    let err = match CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "request": {
                    "chat_path": "https://api.other.example/v1/chat"
                },
                "security": {
                    "allow_absolute_chat_path": true
                }
            }
        }),
        crate::ProviderOpts::default(),
    ) {
        Ok(_) => panic!("manifest should require absolute_urls permission"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("permissions.absolute_urls"),
        "err={err}"
    );
}

#[test]
fn plugin_manifest_enforces_outbound_allowlist() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "security": {
                    "outbound_allowlist": ["https://api.example.com"]
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider
        .sandbox
        .validate_endpoint("https://blocked.example/chat", EndpointKind::BaseUrl)
        .unwrap_err();
    assert!(err.to_string().contains("outbound_allowlist"), "err={err}");
}

#[test]
fn plugin_manifest_redacts_headers_and_query_secrets_for_probe_debug() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "auth": { "strategy": "api_key_query", "query_name": "api_key" },
                "request": {
                    "path": "/chat",
                    "headers": { "X-Trace-Secret": "{{api_key}}" }
                },
                "security": {
                    "header_redaction": ["x-trace-secret"]
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let mut endpoint = provider.endpoint_url_for(&make_req(false)).unwrap();
    endpoint.push_str("?api_key=sk-test");
    let redacted_url = provider.sandbox.redact_url(&endpoint);
    assert!(redacted_url.contains("api_key="));
    assert!(!redacted_url.contains("sk-test"), "url={redacted_url}");

    let headers = provider.request_headers_for(&make_req(false)).unwrap();
    let redacted_headers = provider.sandbox.redact_headers(&headers);
    assert_eq!(
        redacted_headers
            .get("x-trace-secret")
            .unwrap()
            .to_str()
            .unwrap(),
        "[REDACTED]"
    );
}

#[test]
fn plugin_dns_rebind_guard_rejects_private_resolved_addresses() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({ "plugin": {} }),
        crate::ProviderOpts::default(),
    )
    .unwrap();
    let err = provider
        .sandbox
        .validate_resolved_addrs(
            "evil.example",
            &[SocketAddr::from(([169, 254, 169, 254], 80))],
        )
        .unwrap_err();

    assert!(err.to_string().contains("DNS rebind guard"), "err={err}");
}

#[test]
fn plugin_manifest_rejects_unknown_header_template_variable() {
    let err = match CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "request": {
                    "headers": { "X-Leak": "{{request.messages}}" }
                }
            }
        }),
        crate::ProviderOpts::default(),
    ) {
        Ok(_) => panic!("manifest should reject unsupported header template variable"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("unsupported template variable"),
        "err={err}"
    );
}

#[test]
fn plugin_request_body_size_limit_is_enforced() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.example.com",
        "sk-test",
        json!({
            "plugin": {
                "request": {
                    "body": { "payload": "{{last_user_message}}" }
                },
                "security": {
                    "max_request_bytes": 32
                }
            }
        }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let err = provider.request_json_body(&make_req(false)).unwrap_err();
    assert!(
        err.to_string().contains("plugin request body too large"),
        "err={err}"
    );
}

#[tokio::test]
async fn maps_weird_sse_frames_to_openai_chunks() {
    let manifest = PluginManifest::from_value(
        json!({
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "id_path": "rid",
                "model_path": "model_name",
                "role_path": "speaker",
                "content_path": "token",
                "finish_reason_path": "reason",
                "done": ["EOF"],
                "usage": {
                    "prompt_tokens_path": "usage.in",
                    "completion_tokens_path": "usage.out"
                }
            }
        }),
        "http://x",
    )
    .unwrap();
    let mapper = StreamMapper {
        stream: manifest.stream,
        fallback_id: "fallback".into(),
        fallback_model: "odd-model".into(),
        max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
    };
    let sse = concat!(
        "event: token\n",
        "data: {\"payload\":{\"rid\":\"r1\",\"model_name\":\"m1\",\"speaker\":\"assistant\"}}\n\n",
        "data: {\"payload\":{\"token\":\"he\"}}\n\n",
        "data: {\"payload\":{\"token\":\"llo\"}}\n\n",
        "data: {\"payload\":{\"reason\":\"done\",\"usage\":{\"in\":3,\"out\":2}}}\n\n",
        "data: EOF\n\n"
    );
    let stream =
        futures::stream::once(async move { Ok(bytes::Bytes::from_static(sse.as_bytes())) });
    let chunks: Vec<_> = normalize_plugin_sse(stream, mapper)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("he"));
    assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("llo"));
    assert_eq!(chunks[3].choices[0].finish_reason, Some(FinishReason::Stop));
    assert_eq!(chunks[3].usage.as_ref().unwrap().total_tokens, 5);
}

#[test]
fn replays_manifest_driven_sse_events_tool_calls_usage_and_done_object() {
    let manifest = json!({
        "plugin": {
            "stream": {
                "openai_compatible": false,
                "event_path": "payload",
                "ignore_events": ["ping"],
                "done_events": ["close"],
                "done": ["EOF"],
                "done_path": "type",
                "done_values": ["message_stop", { "kind": "done" }],
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
    let sse = concat!(
        "event: ping\n",
        "data: {\"payload\":{\"token\":\"ignored\"}}\n\n",
        "event: token\n",
        "data: {\"payload\":{\"rid\":\"r1\",\"model_name\":\"native\",\"speaker\":\"assistant\"}}\n\n",
        "event: token\n",
        "data: {\"payload\":{\"token\":\"he\"}}\n\n",
        "event: tool_delta\n",
        "data: {\"payload\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\"}}]}}\n\n",
        "event: usage\n",
        "data: {\"payload\":{\"usage\":{\"input\":5,\"cached\":2,\"reasoning\":3}}}\n\n",
        "event: token\n",
        "data: {\"payload\":{\"finish\":\"tool_use\"}}\n\n",
        "event: vendor\n",
        "data: {\"payload\":{\"type\":\"message_stop\"}}\n\n",
        "event: close\n",
        "data: {\"payload\":{\"token\":\"ignored-too\"}}\n\n"
    );

    let chunks = replay_plugin_sse(manifest, "http://x", sse, "fallback").unwrap();
    assert_eq!(chunks.len(), 5);
    assert_eq!(chunks[0].id, "r1");
    assert_eq!(chunks[0].model, "native");
    assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
    assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("he"));
    assert_eq!(
        chunks[2].choices[0].delta.tool_calls.as_ref().unwrap()[0]
            .function
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("lookup")
    );
    let usage = chunks[3].usage.as_ref().unwrap();
    assert_eq!(usage.prompt_tokens, 5);
    assert_eq!(usage.cached_tokens, 2);
    assert_eq!(usage.reasoning_tokens, Some(3));
    assert_eq!(usage.raw.as_ref().unwrap()["input"], 5);
    assert_eq!(
        chunks[4].choices[0].finish_reason,
        Some(FinishReason::ToolCalls)
    );
}

// ─── ADR-0002 catch_unwind fallback tests ─────────────────────────────

#[tokio::test]
async fn run_fastpath_returns_some_for_ok_future() {
    // 用最简单的 manifest 拿一个 CustomHttpProvider 实例（不需要真的发请求）
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.openai.com".to_string(),
        "sk-test".to_string(),
        json!({ "plugin": { "preset": { "provider": "openai" } } }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    let result = provider
        .run_fastpath(ProviderPresetKind::Openai, "test_op", async {
            Ok::<u32, ProviderError>(42)
        })
        .await;

    match result {
        Some(Ok(v)) => assert_eq!(v, 42),
        other => panic!("expected Some(Ok(42)), got {other:?}"),
    }
}

#[tokio::test]
async fn run_fastpath_returns_none_for_panicking_future() {
    let provider = CustomHttpProvider::new_with_opts(
        "https://api.openai.com".to_string(),
        "sk-test".to_string(),
        json!({ "plugin": { "preset": { "provider": "openai" } } }),
        crate::ProviderOpts::default(),
    )
    .unwrap();

    // panic 应该被 catch_unwind 抓住，函数返回 None
    let result = provider
        .run_fastpath::<u32>(ProviderPresetKind::Openai, "test_op", async {
            panic!("simulated fast-path panic");
            #[allow(unreachable_code)]
            Ok(0)
        })
        .await;

    assert!(
        result.is_none(),
        "panicking future must return None to trigger manifest runtime fallback"
    );
}

#[test]
fn panic_message_extracts_string_payload() {
    // 模拟 catch_unwind 返回的 Box<dyn Any + Send>
    let payload: Box<dyn std::any::Any + Send> = Box::new("static str panic");
    assert_eq!(panic_message(&payload), "static str panic");

    let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("owned panic"));
    assert_eq!(panic_message(&payload), "owned panic");

    let payload: Box<dyn std::any::Any + Send> = Box::new(42_u32);
    assert_eq!(panic_message(&payload), "<non-string panic>");
}
