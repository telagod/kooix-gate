//! Secret redaction helpers shared by HTTP errors, audit payloads and debug views.
//!
//! The rule is intentionally conservative: field names that look credential-like are fully
//! replaced, and free-form strings have common token patterns masked. This keeps audit logs useful
//! for diffing while preventing raw keys from escaping into logs or API responses.

use serde_json::{Map, Value};

const REDACTED: &str = "[REDACTED]";

const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "authorization",
    "cookie",
    "password",
    "secret",
    "token",
    "credential",
    "client_secret",
    "api_key",
    "access_key",
    "secret_key",
    "private_key",
    "x-api-key",
    "set-cookie",
];

const TOKEN_PREFIXES: &[&str] = &[
    "sk-", "sk_", "sk_live_", "sk_test_", "sk-proj-", "sk-kg-", "Bearer ", "bearer ",
];

pub fn redact_json(value: Value) -> Value {
    redact_json_at_key(value, None)
}

fn redact_json_at_key(value: Value, key: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_string());
    }

    match value {
        Value::Object(map) => Value::Object(redact_object(map)),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_json).collect()),
        Value::String(s) => Value::String(redact_text(&s)),
        other => other,
    }
}

fn redact_object(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let redacted = redact_json_at_key(value, Some(&key));
            (key, redacted)
        })
        .collect()
}

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for prefix in TOKEN_PREFIXES {
        out = redact_prefixed_tokens(&out, prefix);
    }
    out = redact_url_query_secrets(&out);
    if looks_like_raw_secret(&out) {
        REDACTED.to_string()
    } else {
        out
    }
}

fn redact_prefixed_tokens(input: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        let (head, tail) = rest.split_at(idx);
        out.push_str(head);
        out.push_str(prefix);
        let token = &tail[prefix.len()..];
        let end = token
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | ')' | '(' | '<' | '>')
            })
            .unwrap_or(token.len());
        if end >= 8 {
            out.push_str(REDACTED);
        } else {
            out.push_str(&token[..end]);
        }
        rest = &token[end..];
    }
    out.push_str(rest);
    out
}

fn redact_url_query_secrets(input: &str) -> String {
    input
        .split('&')
        .map(|part| {
            if let Some((key, _value)) = part.split_once('=')
                && is_sensitive_key(query_key_name(key))
            {
                return format!("{key}={REDACTED}");
            }
            part.to_string()
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn query_key_name(raw: &str) -> &str {
    raw.rsplit(['?', '/', ' ']).next().unwrap_or(raw)
}

fn looks_like_raw_secret(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 24 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if TOKEN_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(&prefix.to_ascii_lowercase()))
    {
        return true;
    }
    let has_alpha = trimmed.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let charset_ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+'));
    has_alpha && has_digit && charset_ok && trimmed.len() >= 40
}

pub fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('_', "-");
    SENSITIVE_KEY_FRAGMENTS
        .iter()
        .any(|fragment| key.contains(&fragment.replace('_', "-")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_nested_json_secrets_but_keeps_non_sensitive_diff() {
        let value = redact_json(json!({
            "name": "prod",
            "auth": {
                "api_key": "demo",
                "client_id": "public-client"
            },
            "url": "https://example.com/chat?api_key=sk-test-1234567890abcdef&model=gpt"
        }));

        assert_eq!(value["name"], "prod");
        assert_eq!(value["auth"]["api_key"], REDACTED);
        assert_eq!(value["auth"]["client_id"], "public-client");
        assert_eq!(
            redact_json(json!({"key_fingerprint": "abc"}))["key_fingerprint"],
            "abc"
        );
        assert!(!value["url"].as_str().unwrap().contains("sk-test"));
        assert!(
            value["url"]
                .as_str()
                .unwrap()
                .contains("api_key=[REDACTED]")
        );
    }

    #[test]
    fn redacts_bearer_and_openai_style_tokens_from_text() {
        let text = redact_text(
            "upstream failed Authorization: Bearer sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
        );
        assert!(text.contains("Bearer [REDACTED]"));
        assert!(!text.contains("abcdefghijklmnopqrstuvwxyz"));
    }
}
