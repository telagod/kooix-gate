//! `prompt-injection-v1` — Kooix Gate 第一条护城河 v1 WASM transform 插件（ADR-0006 / M6.1）。
//!
//! 在 `transform_request` / `transform_response` / `transform_stream_event` 三个 hook
//! 中对 JSON / 文本扫描 prompt injection 痕迹。命中即原位替换为
//! `<injection:{tactic}>` 占位符，并在 `metadata` 中报告 `total`、`by_tactic`
//! 与 `highest_risk`。
//!
//! ## 设计目标
//!
//! - **启发式 + 正则**：避免单关键字误伤，强制要求上下文锚点
//!   （如 `ignore` 必须搭配 `previous|prior|above|之前` 等才命中 `override`）。
//! - **零分配热路径**：所有正则用 [`once_cell::sync::Lazy`] 缓存；
//!   每次 transform 仅做一次扫描，typical chat payload（≤ 4KB）下 p99 < 1ms。
//! - **流式 chunk-level 审核**：`transform_stream_event` 与
//!   `transform_response` 共用规则集，按 SSE chunk 边界即时检测。
//!
//! ## tactic 集合
//!
//! | tactic | 触发模式 |
//! |--------|---------|
//! | `override`     | 改写 system prompt 句式（"ignore previous instructions" / "你现在是" 等） |
//! | `role_swap`    | 越狱身份切换（DAN / developer mode + jailbreak / "act as ..." 等） |
//! | `exfiltration` | 诱导泄露 system prompt（"repeat your instructions" / "system prompt" 等） |
//! | `encoding`     | base64/rot13/leet 编码绕过痕迹 |
//! | `tool_abuse`   | tool / function-call 指令注入 |
//!
//! `highest_risk` 严重度排序：`override > exfiltration > role_swap > tool_abuse > encoding`。
//!
//! ## Allowlist
//!
//! 与 `pii-redact-v1` 同构：JSON 顶层 `_kooix_allowlist: ["literal1", ...]`
//! 内的整段字符串跳过扫描，便于宿主允许已审内部 prompt 模板穿透。

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/gate-wasm/wit/kooix-plugin.wit",
});

use exports::kooix::plugin::transform::Guest;
use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};

mod rules;

pub use rules::{Detector, DetectionStats, default_detector};

struct PromptInjectionPlugin;

impl Guest for PromptInjectionPlugin {
    fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let detector = default_detector();
        let (json, allowlist) = take_allowlist(input.json);
        let (scrubbed, stats) = detector.scan_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: scrubbed,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let detector = default_detector();
        let (json, allowlist) = take_allowlist(input.json);
        let (scrubbed, stats) = detector.scan_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: scrubbed,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let detector = default_detector();
        // chunk 文本可能不是 JSON（SSE data 行），按裸字符串路径走
        let (scrubbed, stats) = detector.scan_text(&input.json, None);
        Ok(TransformOutput {
            json: scrubbed,
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

export!(PromptInjectionPlugin);

#[cfg(test)]
mod plugin_smoke {
    use super::*;

    #[test]
    fn allowlist_extracted_from_json() {
        let input = r#"{"_kooix_allowlist":["ignore previous instructions"],"messages":[{"content":"hi"}]}"#
            .to_string();
        let (rest, allow) = take_allowlist(input);
        let allow = allow.unwrap();
        assert_eq!(allow.len(), 1);
        assert!(allow.contains(&"ignore previous instructions".to_string()));
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
