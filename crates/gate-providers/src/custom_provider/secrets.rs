//! Plugin secret slot resolution — manifest secret_slot → channel_keys / env fallback。
//!
//! 命名规则：
//! - "api_key" / 空 → "primary"
//! - 其他 slot → 小写
//! - env fallback：`KOOIX_CH_<CODE>_KEY` → primary；`AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`；
//!   `KOOIX_PLUGIN_SECRET_<SLOT_UPPER>` 通配。

use std::collections::HashMap;

pub(super) fn normalize_secret_slots(secrets: HashMap<String, String>) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = secrets
        .into_iter()
        .map(|(slot, value)| (normalize_secret_slot(&slot), value))
        .collect();
    if !out.contains_key("primary")
        && let Some(value) = out.get("api_key").cloned()
    {
        out.insert("primary".to_string(), value);
    }
    out
}

pub(super) fn normalize_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

pub(super) fn env_key_for_secret_slot(slot: &str) -> String {
    let normalized = normalize_secret_slot(slot);
    match normalized.as_str() {
        "primary" => "KOOIX_PLUGIN_SECRET_PRIMARY".to_string(),
        "aws_secret_key" => "AWS_SECRET_ACCESS_KEY".to_string(),
        "aws_session_token" => "AWS_SESSION_TOKEN".to_string(),
        other => format!(
            "KOOIX_PLUGIN_SECRET_{}",
            other
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        ),
    }
}

pub fn env_secret_slots(channel_code: &str) -> HashMap<String, String> {
    let mut secrets = HashMap::new();
    if let Some(primary) = env_primary_secret(channel_code) {
        secrets.insert("primary".to_string(), primary);
    }
    for (slot, env_key) in [
        ("aws_secret_key", "AWS_SECRET_ACCESS_KEY"),
        ("aws_session_token", "AWS_SESSION_TOKEN"),
    ] {
        if let Ok(value) = std::env::var(env_key) {
            secrets.entry(slot.to_string()).or_insert(value);
        }
    }
    for (key, value) in std::env::vars() {
        let Some(slot) = key.strip_prefix("KOOIX_PLUGIN_SECRET_") else {
            continue;
        };
        if slot.is_empty() {
            continue;
        }
        let slot = normalize_secret_slot(slot);
        secrets.entry(slot).or_insert(value);
    }
    secrets
}

pub(super) fn env_primary_secret(channel_code: &str) -> Option<String> {
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        channel_code
            .to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .or_else(|_| std::env::var("KOOIX_PLUGIN_SECRET_PRIMARY"))
        .ok()
}
