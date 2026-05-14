//! Parameter adaptation for cross-provider routing.
//!
//! When a ChatRequest with OpenAI-format params is routed to a non-OpenAI
//! provider, some parameters are unsupported and must be dropped or filled in.

use crate::types::ChatRequest;

/// Adapt a `ChatRequest` for a specific provider type.
///
/// - Ensures required params are present (e.g. Anthropic requires `max_tokens`).
/// - Drops OpenAI-specific params that the target provider does not accept.
///
/// OpenAI-compatible providers (`"openai"`, `"azure"`, `"deepseek"`, `"ollama"`,
/// `"mistral"`, and unknown types) are left unchanged.
pub fn adapt_for_provider(req: &mut ChatRequest, provider_type: &str) {
    match provider_type {
        "anthropic" => adapt_anthropic(req),
        "bedrock" => adapt_bedrock(req),
        "gemini" => adapt_gemini(req),
        "cohere" => adapt_cohere(req),
        // OpenAI-compatible providers: no adaptation needed
        _ => {}
    }
}

fn adapt_anthropic(req: &mut ChatRequest) {
    // Anthropic Messages API requires max_tokens
    if req.max_tokens.is_none() {
        req.max_tokens = Some(4096);
    }
    // Drop OpenAI-specific params not supported by Anthropic
    // NOTE: response_format is NOT dropped — Anthropic supports JSON mode via
    // {"type": "json_object"} since 2024-10.
    drop_keys(
        req,
        &["logprobs", "top_logprobs", "logit_bias", "n", "seed", "user"],
    );
}

fn adapt_bedrock(req: &mut ChatRequest) {
    // Bedrock Converse API also requires max_tokens
    if req.max_tokens.is_none() {
        req.max_tokens = Some(4096);
    }

    // Bedrock uses camelCase: top_p → topP, top_k → topK
    if let Some(top_p) = req.top_p.take() {
        req.extra
            .insert("topP".to_string(), serde_json::Value::from(top_p));
    }
    if let Some(top_k) = req.extra.remove("top_k") {
        req.extra.insert("topK".to_string(), top_k);
    }

    drop_keys(
        req,
        &[
            "logprobs",
            "top_logprobs",
            "logit_bias",
            "n",
            "seed",
            "response_format",
            "user",
        ],
    );
}

fn adapt_gemini(req: &mut ChatRequest) {
    // Gemini via OpenAI-compat shim: most params pass through, but a few don't
    drop_keys(req, &["logit_bias", "n"]);
}

fn adapt_cohere(req: &mut ChatRequest) {
    // Cohere OpenAI-compat endpoint: drop unsupported params
    drop_keys(req, &["logprobs", "top_logprobs", "logit_bias", "n", "seed"]);
}

/// Remove the given keys from `ChatRequest.extra` (the flattened serde map).
fn drop_keys(req: &mut ChatRequest, keys: &[&str]) {
    for key in keys {
        req.extra.remove(*key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req_with_extras(extras: serde_json::Value) -> ChatRequest {
        let mut req = ChatRequest::default();
        if let Some(obj) = extras.as_object() {
            for (k, v) in obj {
                req.extra.insert(k.clone(), v.clone());
            }
        }
        req
    }

    #[test]
    fn anthropic_injects_max_tokens_when_missing() {
        let mut req = ChatRequest::default();
        assert!(req.max_tokens.is_none());
        adapt_for_provider(&mut req, "anthropic");
        assert_eq!(req.max_tokens, Some(4096));
    }

    #[test]
    fn anthropic_preserves_explicit_max_tokens() {
        let mut req = ChatRequest::default();
        req.max_tokens = Some(1024);
        adapt_for_provider(&mut req, "anthropic");
        assert_eq!(req.max_tokens, Some(1024));
    }

    #[test]
    fn anthropic_drops_unsupported_keys() {
        let mut req = req_with_extras(json!({
            "logprobs": true,
            "top_logprobs": 5,
            "logit_bias": {"50256": -100},
            "n": 3,
            "seed": 42,
            "response_format": {"type": "json_object"},
            "user": "user-abc",
            "stream_options": {"include_usage": true}
        }));
        adapt_for_provider(&mut req, "anthropic");
        for key in &["logprobs", "top_logprobs", "logit_bias", "n", "seed", "user"] {
            assert!(!req.extra.contains_key(*key), "key '{key}' should have been dropped");
        }
        // response_format is kept — Anthropic supports JSON mode
        assert!(req.extra.contains_key("response_format"));
        // Keys not in the drop list must survive
        assert!(req.extra.contains_key("stream_options"));
    }

    #[test]
    fn bedrock_injects_max_tokens_and_drops_keys() {
        let mut req = req_with_extras(json!({"n": 2, "seed": 99, "logprobs": false}));
        adapt_for_provider(&mut req, "bedrock");
        assert_eq!(req.max_tokens, Some(4096));
        assert!(!req.extra.contains_key("n"));
        assert!(!req.extra.contains_key("seed"));
        assert!(!req.extra.contains_key("logprobs"));
    }

    #[test]
    fn bedrock_maps_top_p_to_camel_case() {
        let mut req = req_with_extras(json!({"top_k": 40}));
        req.top_p = Some(0.9);
        adapt_for_provider(&mut req, "bedrock");

        // top_p field should be cleared (moved to extra as topP)
        assert!(req.top_p.is_none());
        assert_eq!(req.extra.get("topP"), Some(&json!(0.9_f32)));
        // top_k from extra should be renamed to topK
        assert!(!req.extra.contains_key("top_k"));
        assert_eq!(req.extra.get("topK"), Some(&json!(40)));
    }

    #[test]
    fn bedrock_no_top_p_when_absent() {
        let mut req = ChatRequest::default();
        adapt_for_provider(&mut req, "bedrock");
        assert!(req.top_p.is_none());
        assert!(!req.extra.contains_key("topP"));
        assert!(!req.extra.contains_key("topK"));
    }

    #[test]
    fn gemini_drops_only_logit_bias_and_n() {
        let mut req = req_with_extras(json!({
            "logit_bias": {"50256": -100},
            "n": 2,
            "seed": 42
        }));
        adapt_for_provider(&mut req, "gemini");
        assert!(!req.extra.contains_key("logit_bias"));
        assert!(!req.extra.contains_key("n"));
        // seed is fine for gemini
        assert!(req.extra.contains_key("seed"));
    }

    #[test]
    fn cohere_drops_correct_keys() {
        let mut req = req_with_extras(json!({
            "logprobs": true,
            "top_logprobs": 3,
            "logit_bias": {},
            "n": 1,
            "seed": 7,
            "response_format": {"type": "text"}
        }));
        adapt_for_provider(&mut req, "cohere");
        for key in &["logprobs", "top_logprobs", "logit_bias", "n", "seed"] {
            assert!(!req.extra.contains_key(*key), "key '{key}' should have been dropped");
        }
        // response_format is NOT in cohere drop list
        assert!(req.extra.contains_key("response_format"));
    }

    #[test]
    fn openai_passthrough_unchanged() {
        let mut req = req_with_extras(json!({
            "logprobs": true,
            "n": 2,
            "seed": 42,
            "response_format": {"type": "json_object"}
        }));
        let orig_extra = req.extra.clone();
        adapt_for_provider(&mut req, "openai");
        assert_eq!(req.extra, orig_extra);
        assert!(req.max_tokens.is_none());
    }

    #[test]
    fn unknown_provider_passthrough_unchanged() {
        let mut req = req_with_extras(json!({"logprobs": true, "n": 5}));
        let orig_extra = req.extra.clone();
        adapt_for_provider(&mut req, "some_future_provider");
        assert_eq!(req.extra, orig_extra);
    }
}
