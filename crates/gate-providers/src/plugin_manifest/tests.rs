//! plugin_manifest unit tests — 从 mod.rs 拆出（M1.3 T3.3 收尾）。
#![cfg(test)]

use super::*;

#[test]
fn parses_v1_manifest_and_keeps_fixed_sections() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "metadata": { "name": "odd", "vendor": "acme", "tags": ["private"] },
                "capabilities": { "chat": true, "streaming": true, "tools": true },
                "auth": { "strategy": "api_key_header", "header_name": "X-Api-Key" },
                "request": {
                    "method": "POST",
                    "path": "/v1/messages/{{model}}",
                    "query": { "stream": "{{stream}}" },
                    "headers": { "X-Model": "{{model}}" },
                    "body": { "messages": "{{messages}}" }
                },
                "embedding_response": {
                    "openai_compatible": false,
                    "data_path": "embeddings.float",
                    "embedding_path": ".",
                    "model_path": "model",
                    "usage": { "prompt_tokens_path": "usage.input", "total_tokens_path": "usage.total" }
                },
                "response": { "openai_compatible": false, "content_path": "answer" },
                "stream": { "openai_compatible": false, "content_path": "token" },
                "usage": { "prompt_tokens_path": "usage.prompt" },
                "error": { "message_path": "error.message" },
                "probe": { "model": "tiny", "success_status": [200] },
                "security": {
                    "max_request_bytes": 4096,
                    "header_redaction": ["authorization"],
                    "outbound_allowlist": ["https://upstream.example"],
                    "permissions": {
                        "secret_slots": ["primary"]
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.metadata.name.as_deref(), Some("odd"));
    assert!(manifest.capabilities.tools);
    assert_eq!(
        manifest.request.path.as_deref(),
        Some("/v1/messages/{{model}}")
    );
    assert_eq!(
        manifest.embedding_response.data_path.as_deref(),
        Some("embeddings.float")
    );
    assert_eq!(manifest.auth.strategy, AuthStrategy::ApiKeyHeader);
    assert_eq!(
        manifest.security.outbound_allowlist,
        vec!["https://upstream.example".to_string()]
    );
    assert!(manifest.security.permissions.outbound_http);
    assert!(!manifest.security.permissions.absolute_urls);
}

#[test]
fn sandbox_permissions_require_explicit_oauth_and_secret_slots() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": "https://idp.example.com/oauth/token"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("permissions.oauth_client_credentials"),
        "err={err}"
    );

    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "api_key_header",
                    "header_name": "X-Api-Key",
                    "secret_slot": "private_key"
                },
                "security": {
                    "permissions": {
                        "secret_slots": ["primary"]
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("secret slot \"private_key\""),
        "err={err}"
    );
}

#[test]
fn request_mapping_accepts_tool_choice_and_metadata_templates() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "request": {
                    "path": "/deployments/{{metadata.deployment}}/chat",
                    "embedding_path": "/deployments/{{model}}/embeddings",
                    "query": {
                        "tenant": "{{metadata.tenant}}",
                        "tool": "{{tool_choice}}"
                    },
                    "headers": {
                        "X-Tenant": "{{metadata.tenant}}"
                    },
                    "body": {
                        "messages": "{{messages}}",
                        "tools": "{{tools}}",
                        "toolChoice": "{{tool_choice}}",
                        "metadata": "{{metadata}}"
                    },
                    "embedding_body": {
                        "texts": "{{input_texts}}",
                        "format": "{{encoding_format}}",
                        "dimensions": "{{dimensions}}"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(
        manifest.request.path.as_deref(),
        Some("/deployments/{{metadata.deployment}}/chat")
    );
    assert_eq!(
        manifest.request.embedding_path.as_deref(),
        Some("/deployments/{{model}}/embeddings")
    );
    assert!(manifest.request.embedding_body.is_some());
}

#[test]
fn request_mapping_rejects_header_messages_template_variable() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "request": {
                    "headers": {
                        "X-Leak": "{{messages}}"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("unsupported template variable {{messages}}"),
        "err={err}"
    );
}

#[test]
fn response_mapping_accepts_fallback_defaults_and_multimodal_usage_paths() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "response": {
                    "openai_compatible": false,
                    "id_path": "missing.id|trace.request_id|default:\"local\"",
                    "model_path": "result.0.model",
                    "content_path": "result.0.text",
                    "reasoning_content_path": "result.0.reasoning",
                    "tool_calls_path": "result.0.tool_calls",
                    "finish_reason_path": "result.0.finish",
                    "request_id_path": "trace.request_id",
                    "metadata_path": "vendor",
                    "usage": {
                        "prompt_tokens_path": "usage.input",
                        "completion_tokens_path": "usage.output",
                        "total_tokens_path": "usage.total|default:0",
                        "cached_tokens_path": "usage.cached",
                        "reasoning_tokens_path": "usage.reasoning",
                        "image_units_path": "usage.images",
                        "audio_seconds_path": "usage.audio_seconds",
                        "raw_path": "usage"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(
        manifest.response.request_id_path.as_deref(),
        Some("trace.request_id")
    );
    assert_eq!(
        manifest.response.usage.image_units_path.as_deref(),
        Some("usage.images")
    );
}

#[test]
fn response_mapping_rejects_bracket_array_index() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "response": {
                    "openai_compatible": false,
                    "content_path": "choices[0].message.content"
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("use dot array indexes"),
        "err={err}"
    );
}

#[test]
fn parses_hmac_auth_manifest_defaults_and_payload_template() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "hmac",
                    "secret_slot": "signing-key",
                    "hmac": {
                        "signature_header": "X-Kooix-Signature",
                        "signed_payload": "{{method}}\n{{path}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}",
                        "signature_encoding": "base64"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(manifest.auth.strategy, AuthStrategy::Hmac);
    assert_eq!(manifest.auth.secret_slot(), "signing-key");
    assert_eq!(manifest.auth.hmac.signature_header, "X-Kooix-Signature");
    assert_eq!(
        manifest.auth.hmac.signature_encoding,
        SignatureEncoding::Base64
    );
    assert_eq!(manifest.auth.hmac.timestamp_header, "X-Timestamp");
    assert_eq!(manifest.auth.hmac.nonce_header, "X-Nonce");
}

#[test]
fn parses_aws_sigv4_auth_manifest_defaults() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "aws_sigv4",
                    "aws_sigv4": {
                        "region": "us-east-1"
                    }
                }
            }
        }),
        "https://bedrock-runtime.us-east-1.amazonaws.com",
    )
    .unwrap();

    assert_eq!(manifest.auth.strategy, AuthStrategy::AwsSigv4);
    assert_eq!(manifest.auth.aws_sigv4.service, "bedrock");
    assert_eq!(manifest.auth.aws_sigv4.secret_key_slot, "aws_secret_key");
    assert_eq!(
        manifest.auth.aws_sigv4.session_token_slot.as_deref(),
        Some("aws_session_token")
    );
}

#[test]
fn parses_oauth_client_credentials_manifest_defaults() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": "https://idp.example.com/oauth/token",
                        "scope": "chat:write"
                    }
                },
                "security": {
                    "permissions": { "oauth_client_credentials": true }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(manifest.auth.strategy, AuthStrategy::OauthClientCredentials);
    assert_eq!(
        manifest.auth.oauth.token_url,
        "https://idp.example.com/oauth/token"
    );
    assert_eq!(manifest.auth.oauth.client_id_slot, "client_id");
    assert_eq!(manifest.auth.oauth.client_secret_slot, "client_secret");
    assert_eq!(manifest.auth.oauth.scope.as_deref(), Some("chat:write"));
    assert_eq!(manifest.auth.oauth.token_type, "Bearer");
    assert_eq!(manifest.auth.oauth.expiry_skew_seconds, 60);
}

#[test]
fn oauth_rejects_plain_http_token_url() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "oauth_client_credentials",
                    "oauth": {
                        "token_url": "http://idp.example.com/oauth/token"
                    }
                },
                "security": {
                    "permissions": { "oauth_client_credentials": true }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("oauth token_url must use https"),
        "err={err}"
    );
}

#[test]
fn bedrock_preset_defaults_to_aws_sigv4_without_fake_secret_headers() {
    let manifest = PluginManifest::from_value(
        json!({ "plugin": { "preset": { "provider": "bedrock_converse" } } }),
        "https://bedrock-runtime.us-east-1.amazonaws.com",
    )
    .unwrap();

    assert_eq!(manifest.auth.strategy, AuthStrategy::AwsSigv4);
    assert!(!manifest.request.headers.contains_key("X-Amz-Access-Key"));
    assert!(!manifest.request.headers.contains_key("X-Amz-Secret-Key"));
    assert_eq!(
        manifest.request.path.as_deref(),
        Some("/model/{{model}}/converse")
    );
}

#[test]
fn preset_defaults_fill_capabilities() {
    let manifest = PluginManifest::from_value(
        json!({ "plugin": { "preset": { "provider": "anthropic_messages" } } }),
        "https://api.anthropic.com",
    )
    .unwrap();

    assert!(manifest.capabilities.chat);
    assert!(manifest.capabilities.streaming);
    assert!(manifest.capabilities.tools);
    assert!(manifest.capabilities.vision);
    assert!(manifest.capabilities.json_mode);
    assert!(!manifest.capabilities.embeddings);
}

#[test]
fn openai_compatible_variant_presets_parse() {
    for provider in [
        "vllm",
        "lm_studio",
        "ollama_openai",
        "localai",
        "xinference",
        "vertex_openai",
    ] {
        let manifest = PluginManifest::from_value(
            json!({ "plugin": { "preset": { "provider": provider } } }),
            "http://localhost:8000/v1",
        )
        .unwrap();

        assert_eq!(
            manifest.request.path.as_deref(),
            Some("/chat/completions"),
            "provider={provider}"
        );
        assert_eq!(
            manifest.request.embedding_path.as_deref(),
            Some("/embeddings"),
            "provider={provider}"
        );
        assert!(manifest.capabilities.chat, "provider={provider}");
        assert!(manifest.capabilities.streaming, "provider={provider}");
        assert!(manifest.capabilities.embeddings, "provider={provider}");
    }
}

#[test]
fn vertex_openai_preset_uses_openai_path_and_bearer_auth() {
    let manifest = PluginManifest::from_value(
        json!({ "plugin": { "preset": { "provider": "vertex_openai" } } }),
        "https://aiplatform.googleapis.com/v1/projects/demo/locations/us-central1/endpoints/openapi",
    )
    .unwrap();

    assert_eq!(manifest.auth.strategy, AuthStrategy::Bearer);
    assert_eq!(manifest.auth.secret_slot.as_deref(), Some("primary"));
    assert_eq!(manifest.request.path.as_deref(), Some("/chat/completions"));
    assert_eq!(
        manifest.request.embedding_path.as_deref(),
        Some("/embeddings")
    );
    assert!(manifest.response.is_openai_compatible());
    assert!(manifest.embedding_response.is_openai_compatible());
    assert!(manifest.stream.is_openai_compatible());
    assert!(manifest.capabilities.chat);
    assert!(manifest.capabilities.streaming);
    assert!(manifest.capabilities.tools);
    assert!(manifest.capabilities.embeddings);
    assert!(manifest.capabilities.vision);
    assert!(manifest.capabilities.json_mode);
}

#[test]
fn hmac_rejects_unknown_payload_template_variable() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "auth": {
                    "strategy": "hmac",
                    "hmac": {
                        "signed_payload": "{{api_key}}\n{{body_sha256}}"
                    }
                }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("unsupported template variable {{api_key}}"),
        "err={err}"
    );
}

#[test]
fn upgrades_legacy_v0_preset_manifest() {
    let manifest = PluginManifest::from_value(
        json!({ "plugin": { "preset": { "provider": "azure_openai" } } }),
        "https://example.openai.azure.com",
    )
    .unwrap();

    assert_eq!(manifest.version, 1);
    assert!(
        manifest
            .request
            .path
            .unwrap()
            .contains("/openai/deployments/")
    );
    assert_eq!(manifest.auth.strategy, AuthStrategy::ApiKeyHeader);
    assert_eq!(manifest.auth.header_name(), Some("api-key"));
    assert!(!manifest.request.headers.contains_key("Authorization"));
}

#[test]
fn deserialize_error_reports_json_pointer() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "security": { "max_request_bytes": "large" }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("/plugin/security/max_request_bytes"),
        "err={err}"
    );
}

#[test]
fn validates_probe_manifest_path_body_status_and_cost() {
    let manifest = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "probe": {
                    "model": "tiny-health",
                    "path": "/health/{{model}}",
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
        "https://upstream.example",
    )
    .unwrap();

    assert_eq!(manifest.probe.model.as_deref(), Some("tiny-health"));
    assert_eq!(manifest.probe.path.as_deref(), Some("/health/{{model}}"));
    assert_eq!(manifest.probe.success_status_or_default(), vec![200, 204]);
    assert_eq!(manifest.probe.max_cost_micros, Some(25));
}

#[test]
fn rejects_invalid_probe_success_status_and_negative_cost() {
    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "probe": { "success_status": [99] }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();
    assert!(err.to_string().contains("/plugin/probe/success_status"));

    let err = PluginManifest::from_value(
        json!({
            "plugin": {
                "version": 1,
                "probe": { "max_cost_micros": -1 }
            }
        }),
        "https://upstream.example",
    )
    .unwrap_err();
    assert!(err.to_string().contains("/plugin/probe/max_cost_micros"));
}

#[test]
fn schema_contains_v1_sections() {
    let schema = plugin_manifest_schema_json();
    let props = &schema["$defs"]["PluginManifest"]["properties"];
    for section in [
        "version",
        "metadata",
        "capabilities",
        "auth",
        "request",
        "response",
        "stream",
        "usage",
        "error",
        "probe",
        "security",
    ] {
        assert!(props.get(section).is_some(), "missing {section}");
    }
}

#[test]
fn builtin_fastpath_injected_for_fast_path_presets() {
    for preset in [
        "openai",
        "anthropic_messages",
        "azure_openai",
        "bedrock_converse",
    ] {
        let value = json!({ "plugin": { "preset": { "provider": preset } } });
        let manifest = PluginManifest::from_value(value, "https://example.com")
            .unwrap_or_else(|err| panic!("preset '{preset}' failed: {err}"));
        assert!(
            manifest.security.builtin_fastpath,
            "preset '{preset}' should have builtin_fastpath=true after apply_preset"
        );
    }
}

#[test]
fn builtin_fastpath_not_set_for_non_fast_path_presets() {
    for preset in ["deepseek", "mistral", "gemini", "groq", "ollama"] {
        let value = json!({ "plugin": { "preset": { "provider": preset } } });
        let manifest = PluginManifest::from_value(value, "https://example.com")
            .unwrap_or_else(|err| panic!("preset '{preset}' failed: {err}"));
        assert!(
            !manifest.security.builtin_fastpath,
            "preset '{preset}' must NOT have builtin_fastpath; only 4 ADR-0002 \
             fast-path presets are allowed"
        );
    }
}

#[test]
fn user_cannot_override_builtin_fastpath() {
    let value = json!({
        "plugin": {
            "preset": { "provider": "deepseek" },
            "security": { "builtin_fastpath": true }
        }
    });
    let manifest = PluginManifest::from_value(value, "https://example.com").unwrap();
    assert!(
        !manifest.security.builtin_fastpath,
        "user manifest must not be able to turn on builtin_fastpath for non-fast-path preset"
    );
}

#[test]
fn user_cannot_disable_builtin_fastpath_for_fast_path_preset() {
    let value = json!({
        "plugin": {
            "preset": { "provider": "openai" },
            "security": { "builtin_fastpath": false }
        }
    });
    let manifest = PluginManifest::from_value(value, "https://api.openai.com").unwrap();
    assert!(
        manifest.security.builtin_fastpath,
        "user manifest must not be able to disable builtin_fastpath for fast-path preset"
    );
}

#[test]
fn parses_wasm_module_manifest_field() {
    // ADR-0003 v0：wasm 字段可被用户 manifest 设置。
    let value = json!({
        "plugin": {
            "version": 1,
            "security": {
                "wasm": {
                    "module": "modules/transform.wasm",
                    "module_sha256": "abc123",
                    "max_memory_bytes": 8388608,
                    "max_cpu_ms": 25,
                    "hooks": ["chat_request_transform", "chat_response_transform"]
                }
            }
        }
    });
    let manifest = PluginManifest::from_value(value, "https://upstream.example").unwrap();
    let wasm = manifest.security.wasm.expect("wasm field present");
    assert_eq!(wasm.module, "modules/transform.wasm");
    assert_eq!(wasm.module_sha256, "abc123");
    assert_eq!(wasm.max_memory_bytes, Some(8388608));
    assert_eq!(wasm.max_cpu_ms, Some(25));
    assert_eq!(wasm.hooks.len(), 2);
}

#[test]
fn wasm_field_absent_when_not_configured() {
    let value = json!({ "plugin": { "version": 1 } });
    let manifest = PluginManifest::from_value(value, "https://upstream.example").unwrap();
    assert!(manifest.security.wasm.is_none(), "wasm should be None by default");
}
