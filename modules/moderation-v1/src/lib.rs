//! `moderation-v1` — Kooix Gate v1 WASM transform 插件：关键词 + 类别审核
//! （ADR-0007 §3 banned-signal 三层规则中的语义层补强）。
//!
//! 在 `transform_request` / `transform_response` / `transform_stream_event`
//! 三个 hook 中扫描 6 类常见违禁关键词（英文 + 中文），命中即原位替换为
//! `<moderation:{category}>` 占位符。
//!
//! ## 类别集合
//!
//! | category      | 含义                       |
//! |---------------|----------------------------|
//! | `hate`        | 仇恨 / 歧视                |
//! | `harassment`  | 骚扰 / 欺凌                |
//! | `self_harm`   | 自伤 / 自杀                |
//! | `sexual`      | 色情                       |
//! | `violence`    | 暴力                       |
//! | `illegal`     | 违法（毒品 / 武器 / 盗窃） |
//!
//! ## 设计目标
//!
//! - **关键词 + 类别**：每条规则有一个 `category` 标签 + 一组 keyword（中英文）。
//! - **英文走单词边界**（`\b`），避免 `"hate"` 误伤 `"phate"` / `"chateau"`；
//!   中文走子串匹配（`memchr` 风格 `contains`），符合中文无 word boundary 的事实。
//! - **大小写不敏感**：英文统一对 lowercase 后的输入做匹配；中文不受影响。
//! - **allowlist 复用**：与 [`pii-redact-v1`] 同款 `_kooix_allowlist` 顶层字段，
//!   命中关键词若 raw 值落在 allowlist 内则跳过。
//! - **流式 chunk-level 审核**：`transform_stream_event` 与 `transform_response`
//!   共用同一规则集，调用方按 SSE chunk 边界传入即可。
//!
//! 输出占位符固定为 `<moderation:{category}>`，便于下游 audit 解析。

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/gate-wasm/wit/kooix-plugin.wit",
});

use exports::kooix::plugin::transform::Guest;
use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};

mod rules;

pub use rules::{ModerationStats, Moderator, default_moderator};

struct ModerationPlugin;

impl Guest for ModerationPlugin {
    fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let moderator = default_moderator();
        let (json, allowlist) = take_allowlist(input.json);
        let (moderated, stats) = moderator.moderate_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: moderated,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let moderator = default_moderator();
        let (json, allowlist) = take_allowlist(input.json);
        let (moderated, stats) = moderator.moderate_json(&json, allowlist.as_deref());
        Ok(TransformOutput {
            json: moderated,
            metadata: serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string()),
        })
    }

    fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let moderator = default_moderator();
        // chunk 文本可能不是 JSON（SSE data 行），按裸字符串路径走
        let (moderated, stats) = moderator.moderate_text(&input.json, None);
        Ok(TransformOutput {
            json: moderated,
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

export!(ModerationPlugin);

#[cfg(test)]
mod plugin_smoke {
    use super::*;

    #[test]
    fn allowlist_extracted_from_json() {
        let input = r#"{"_kooix_allowlist":["debate"],"messages":[{"content":"hi"}]}"#
            .to_string();
        let (rest, allow) = take_allowlist(input);
        let allow = allow.unwrap();
        assert_eq!(allow.len(), 1);
        assert!(allow.contains(&"debate".to_string()));
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
