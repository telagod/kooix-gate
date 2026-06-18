//! `pii-redact-v1` — Kooix Gate 第一条护城河 v1 WASM transform 插件（ADR-0006 / M6.1）。
//!
//! 在 `transform_request` / `transform_response` / `transform_stream_event` 三个 hook
//! 中对 JSON 体进行 PII 检测与 redaction。规则集合内置 8 类常见 PII，所有匹配
//! 都被替换为 `<redacted:{kind}>` 占位符。
//!
//! ## 设计目标
//!
//! - **正则 + 词典 + 上下文白名单**：每条规则有 vendor 中立的正则，
//!   并允许调用方通过 `metadata` 字段传入白名单字符串（已知不敏感的子串
//!   不被 redact，避免误杀）。
//! - **零分配热路径**：使用 [`once_cell::sync::Lazy`] 缓存所有
//!   编译期正则，每次 transform 仅做一次扫描，使 p99 在 typical chat
//!   payload（≤ 4KB）下 < 1ms。
//! - **流式 chunk-level 审核**：`transform_stream_event` 与
//!   `transform_response` 共用同一规则集，调用方按 SSE chunk 边界
//!   传入即可，无需聚合整段响应。
//!
//! ## 规则集合
//!
//! | kind | 触发模式 |
//! |------|---------|
//! | `china_id_card` | 18 位国内身份证（含末位 X 校验） |
//! | `phone_cn`      | 11 位中国大陆手机号（13/14/15/16/17/18/19 段） |
//! | `email`         | RFC 5322 子集 |
//! | `openai_key`    | `sk-[A-Za-z0-9-_]{20,}` |
//! | `anthropic_key` | `sk-ant-[A-Za-z0-9-_]{20,}` |
//! | `bearer_token`  | `Authorization: Bearer xxxxx` header 风格 |
//! | `bank_card`     | 13-19 位连续数字（疑似卡号） |
//! | `ipv4`          | 标准 IPv4 字符串 |
//!
//! 输出占位符固定为 `<redacted:{kind}>`，便于下游 audit 解析。

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/gate-wasm/wit/kooix-plugin.wit",
});

use exports::kooix::plugin::transform::Guest;
use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};

mod rules;

pub use rules::{Redactor, RedactionStats, default_redactor};

struct PiiRedactPlugin;

impl Guest for PiiRedactPlugin {
    fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let redactor = default_redactor();
        let (json, allowlist) = take_allowlist(input.json);
        let (redacted, stats) = redactor.redact_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: redacted,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let redactor = default_redactor();
        let (json, allowlist) = take_allowlist(input.json);
        let (redacted, stats) = redactor.redact_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: redacted,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let redactor = default_redactor();
        // chunk 文本可能不是 JSON（SSE data 行），按裸字符串路径走
        let (redacted, stats) = redactor.redact_text(&input.json, None);
        Ok(TransformOutput {
            json: redacted,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn finish_stream(input: TransformInput) -> Result<TransformOutput, TransformError> {
        Ok(TransformOutput {
            json: input.json,
            metadata: "{\"stage\":\"finish_stream\"}".to_string(),
        })
    }
}

/// allowlist 约定：在 JSON 顶层放 `_kooix_allowlist: ["literal1", ...]`，
/// 本函数从输入 JSON 里取走该字段并返回剩余 JSON + literal 列表。
/// 非 JSON 输入或字段缺失时返回原 JSON + `None`。
fn take_allowlist(json: String) -> (String, Option<Vec<String>>) {
    let mut parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(_) => return (json, None),
    };
    let allowlist = parsed
        .as_object_mut()
        .and_then(|obj| obj.remove("_kooix_allowlist"));
    let literals = allowlist
        .as_ref()
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
    let serialized = serde_json::to_string(&parsed).unwrap_or(json);
    (serialized, literals)
}

export!(PiiRedactPlugin);

#[cfg(test)]
mod plugin_smoke {
    use super::*;

    #[test]
    fn allowlist_extracted_from_json() {
        let input = r#"{"_kooix_allowlist":["keep@example.com"],"messages":[{"content":"hi"}]}"#
            .to_string();
        let (rest, allow) = take_allowlist(input);
        let allow = allow.unwrap();
        assert_eq!(allow.len(), 1);
        assert!(allow.contains(&"keep@example.com".to_string()));
        assert!(!rest.contains("_kooix_allowlist"));
        assert!(rest.contains("messages"));
    }

    #[test]
    fn no_allowlist_field_returns_none() {
        let input = r#"{"messages":[]}"#.to_string();
        let (rest, allow) = take_allowlist(input);
        assert!(allow.is_none());
        assert!(rest.contains("messages"));
    }

    #[test]
    fn non_json_passes_through() {
        let input = "raw sse fragment".to_string();
        let (rest, allow) = take_allowlist(input.clone());
        assert!(allow.is_none());
        assert_eq!(rest, input);
    }
}
