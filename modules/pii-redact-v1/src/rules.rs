//! PII 检测规则集合 + 引擎。
//!
//! `Redactor` 是规则集的纯 Rust 包装，可在 host-side 单测中使用——只有
//! `lib.rs` 那一层带 wit-bindgen import。

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeMap;

// ============================================================================
// 规则定义
// ============================================================================

/// 一条 PII 规则。
#[derive(Debug, Clone)]
pub struct Rule {
    pub kind: &'static str,
    pub pattern: Regex,
}

/// 每条 PII 规则的命中替换字符串：`<redacted:{kind}>`。
#[inline]
fn placeholder_for(kind: &str) -> String {
    format!("<redacted:{kind}>")
}

// 内置规则集合。修改顺序需要保持「更长更特化的模式优先」，避免短模式吞掉长模式。
fn build_rules() -> Vec<Rule> {
    vec![
        // 1. Anthropic key（必须先于 openai_key —— `sk-ant-` 是 `sk-` 的扩展）
        Rule {
            kind: "anthropic_key",
            pattern: Regex::new(r"sk-ant-[A-Za-z0-9_-]{20,}").unwrap(),
        },
        // 2. OpenAI key
        Rule {
            kind: "openai_key",
            pattern: Regex::new(r"sk-[A-Za-z0-9_-]{20,}").unwrap(),
        },
        // 3. Bearer token (Authorization: Bearer xxx)
        Rule {
            kind: "bearer_token",
            pattern: Regex::new(r"(?i)Bearer\s+[A-Za-z0-9._\-]+").unwrap(),
        },
        // 4. 邮箱
        Rule {
            kind: "email",
            pattern: Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[A-Za-z]{2,}").unwrap(),
        },
        // 5. 中国身份证（含末位 X）—— 仅做形态匹配，不验校验位
        Rule {
            kind: "china_id_card",
            pattern: Regex::new(
                r"[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dX]",
            )
            .unwrap(),
        },
        // 6. 中国大陆手机号（11 位）
        Rule {
            kind: "phone_cn",
            pattern: Regex::new(r"(?:^|[^\d])(1[3-9]\d{9})(?:[^\d]|$)").unwrap(),
        },
        // 7. 银行卡号（13-19 位连续数字）
        Rule {
            kind: "bank_card",
            pattern: Regex::new(r"(?:^|[^\d])(\d{13,19})(?:[^\d]|$)").unwrap(),
        },
        // 8. IPv4
        Rule {
            kind: "ipv4",
            pattern: Regex::new(
                r"(?:[0-9]{1,3}\.){3}[0-9]{1,3}",
            )
            .unwrap(),
        },
    ]
}

static DEFAULT_RULES: Lazy<Vec<Rule>> = Lazy::new(build_rules);

// ============================================================================
// 引擎
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct RedactionStats {
    pub by_kind: BTreeMap<String, usize>,
}

impl serde::Serialize for RedactionStats {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut m = ser.serialize_map(Some(self.by_kind.len() + 1))?;
        m.serialize_entry("total", &self.total())?;
        for (k, v) in &self.by_kind {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl RedactionStats {
    pub fn total(&self) -> usize {
        self.by_kind.values().copied().sum()
    }

    fn incr(&mut self, kind: &str) {
        *self.by_kind.entry(kind.to_string()).or_insert(0) += 1;
    }
}

pub struct Redactor<'a> {
    rules: &'a [Rule],
}

impl<'a> Redactor<'a> {
    pub fn new(rules: &'a [Rule]) -> Self {
        Self { rules }
    }

    /// 对裸文本（任意 chunk / SSE data 行）做 PII redaction。
    /// 多规则按顺序应用；命中数累计到 stats。
    pub fn redact_text(
        &self,
        input: &str,
        allowlist: Option<&[String]>,
    ) -> (String, RedactionStats) {
        let mut stats = RedactionStats::default();
        let mut current = input.to_string();
        for rule in self.rules {
            let placeholder = placeholder_for(rule.kind);
            let mut next = String::with_capacity(current.len());
            let mut last_end = 0usize;

            // Some rules wrap the match group in `(?:^|[^\d])(...)(?:[^\d]|$)` —
            // we want to grab the inner group when present.
            let inner_group = matches!(rule.kind, "phone_cn" | "bank_card");

            for m in rule.pattern.captures_iter(&current) {
                let full = m.get(0).unwrap();
                let target = if inner_group {
                    m.get(1).unwrap()
                } else {
                    full
                };
                let raw = &current[target.start()..target.end()];
                if is_allowlisted(raw, allowlist) {
                    continue;
                }
                next.push_str(&current[last_end..target.start()]);
                next.push_str(&placeholder);
                last_end = target.end();
                stats.incr(rule.kind);
            }
            next.push_str(&current[last_end..]);
            current = next;
        }
        (current, stats)
    }

    /// 对 JSON 字符串做 redaction。
    ///
    /// 策略：解析→ 遍历所有 String 叶子值 → 应用 [`Self::redact_text`]
    /// → 重新序列化。非 JSON 输入退到 [`Self::redact_text`] 文本路径。
    pub fn redact_json(
        &self,
        input: &str,
        allowlist: Option<&[String]>,
    ) -> (String, RedactionStats) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(input);
        let mut value = match parsed {
            Ok(v) => v,
            Err(_) => return self.redact_text(input, allowlist),
        };
        let mut stats = RedactionStats::default();
        self.walk(&mut value, allowlist, &mut stats);
        let serialized =
            serde_json::to_string(&value).unwrap_or_else(|_| input.to_string());
        (serialized, stats)
    }

    fn walk(
        &self,
        value: &mut serde_json::Value,
        allowlist: Option<&[String]>,
        stats: &mut RedactionStats,
    ) {
        match value {
            serde_json::Value::String(s) => {
                let (replaced, mut chunk_stats) = self.redact_text(s, allowlist);
                *s = replaced;
                for (k, v) in std::mem::take(&mut chunk_stats.by_kind) {
                    *stats.by_kind.entry(k).or_insert(0) += v;
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    self.walk(v, allowlist, stats);
                }
            }
            serde_json::Value::Object(obj) => {
                for (_, v) in obj.iter_mut() {
                    self.walk(v, allowlist, stats);
                }
            }
            _ => {}
        }
    }
}

fn is_allowlisted(value: &str, allowlist: Option<&[String]>) -> bool {
    allowlist
        .map(|al| al.iter().any(|literal| literal == value))
        .unwrap_or(false)
}

/// 全局默认 redactor。WASM 入口和单测都用这个。
pub fn default_redactor() -> Redactor<'static> {
    Redactor::new(&DEFAULT_RULES)
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> (String, RedactionStats) {
        default_redactor().redact_text(text, None)
    }

    #[test]
    fn redacts_email() {
        let (out, stats) = run("contact alice@example.com please");
        assert_eq!(out, "contact <redacted:email> please");
        assert_eq!(stats.total(), 1);
        assert_eq!(stats.by_kind.get("email").copied(), Some(1));
    }

    #[test]
    fn redacts_china_id_card() {
        let (out, stats) = run("身份证号 110105199001011234 已记录");
        assert!(out.contains("<redacted:china_id_card>"));
        assert_eq!(stats.by_kind.get("china_id_card").copied(), Some(1));
    }

    #[test]
    fn redacts_china_id_card_with_trailing_x() {
        let (out, _stats) = run("ID: 11010519900101123X");
        assert!(out.contains("<redacted:china_id_card>"));
    }

    #[test]
    fn redacts_phone_cn() {
        let (out, stats) = run("My phone 13800138000 is here");
        assert!(out.contains("<redacted:phone_cn>"));
        assert_eq!(stats.by_kind.get("phone_cn").copied(), Some(1));
    }

    #[test]
    fn redacts_anthropic_key_before_openai() {
        let (out, stats) = run("anth=sk-ant-abcdefghij1234567890XYZ here");
        assert!(out.contains("<redacted:anthropic_key>"));
        // openai_key 不应再命中（已被替换占位符吃掉）
        assert_eq!(stats.by_kind.get("anthropic_key").copied(), Some(1));
        assert!(stats.by_kind.get("openai_key").copied().unwrap_or(0) == 0);
    }

    #[test]
    fn redacts_openai_key() {
        let (out, stats) = run("key=sk-proj-abcdef1234567890XYZWVU done");
        assert!(out.contains("<redacted:openai_key>"));
        assert_eq!(stats.by_kind.get("openai_key").copied(), Some(1));
    }

    #[test]
    fn redacts_bearer_token() {
        let (out, _stats) =
            run("Authorization: Bearer abc.123.xyz_token go");
        assert!(out.contains("<redacted:bearer_token>"));
    }

    #[test]
    fn redacts_bank_card() {
        let (out, _stats) = run("PAN: 4111111111111111 trailing");
        assert!(out.contains("<redacted:bank_card>"));
    }

    #[test]
    fn redacts_ipv4() {
        let (out, _stats) = run("from 192.168.1.42 to api");
        assert!(out.contains("<redacted:ipv4>"));
    }

    #[test]
    fn no_match_passes_through() {
        let (out, stats) = run("Just a friendly hello, no PII here.");
        assert_eq!(out, "Just a friendly hello, no PII here.");
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn multiple_kinds_in_one_pass() {
        let (out, stats) = run(
            "Hi alice@example.com, my id 110105199001011234 and key sk-proj-abcdef1234567890XYZWVU.",
        );
        assert!(out.contains("<redacted:email>"));
        assert!(out.contains("<redacted:china_id_card>"));
        assert!(out.contains("<redacted:openai_key>"));
        assert_eq!(stats.total(), 3);
    }

    #[test]
    fn allowlist_skips_literal() {
        let allow = vec!["keep@example.com".to_string()];
        let (out, stats) =
            default_redactor().redact_text("ping keep@example.com please", Some(&allow));
        assert_eq!(out, "ping keep@example.com please");
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn redact_json_walks_nested_strings() {
        let input = r#"{
            "messages": [
                {"role": "user", "content": "send to alice@example.com please"},
                {"role": "assistant", "content": "ok"}
            ],
            "metadata": {"phone": "13800138000"}
        }"#;
        let (out, stats) = default_redactor().redact_json(input, None);
        assert!(out.contains("<redacted:email>"));
        assert!(out.contains("<redacted:phone_cn>"));
        assert_eq!(stats.total(), 2);
        // 仍然是合法 JSON
        let _: serde_json::Value = serde_json::from_str(&out).expect("still json");
    }

    #[test]
    fn redact_json_passes_through_non_json() {
        let input = "data: {raw sse fragment with email alice@example.com}\n\n";
        let (out, stats) = default_redactor().redact_json(input, None);
        assert!(out.contains("<redacted:email>"));
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn stats_serializes_with_total() {
        let mut s = RedactionStats::default();
        s.incr("email");
        s.incr("email");
        s.incr("phone_cn");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"total\":3"));
        assert!(json.contains("\"email\":2"));
        assert!(json.contains("\"phone_cn\":1"));
    }

    // ------------------------------------------------------------------
    // Golden tests：fixtures/ + golden/ 对照
    // ------------------------------------------------------------------
    fn read_relative(path: &str) -> String {
        let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        std::fs::read_to_string(&full)
            .unwrap_or_else(|e| panic!("read {}: {e}", full.display()))
    }

    #[test]
    fn golden_chat_request() {
        let input = read_relative("fixtures/chat_request.json");
        let (got, _) = default_redactor().redact_json(&input, None);
        let expected = read_relative("golden/chat_request.json");
        let g: serde_json::Value = serde_json::from_str(&got).expect("got is json");
        let e: serde_json::Value = serde_json::from_str(&expected).expect("expected is json");
        assert_eq!(g, e);
    }

    #[test]
    fn golden_chat_response() {
        let input = read_relative("fixtures/chat_response.json");
        let (got, _) = default_redactor().redact_json(&input, None);
        let expected = read_relative("golden/chat_response.json");
        let g: serde_json::Value = serde_json::from_str(&got).expect("got is json");
        let e: serde_json::Value = serde_json::from_str(&expected).expect("expected is json");
        assert_eq!(g, e);
    }

    #[test]
    fn golden_sse_chunk() {
        let input = read_relative("fixtures/sse_chunk.txt");
        let (got, _) = default_redactor().redact_text(&input, None);
        let expected = read_relative("golden/sse_chunk.txt");
        assert_eq!(got, expected);
    }

    // ------------------------------------------------------------------
    // 性能 baseline: typical 4KB chat payload < 1ms on dev hardware
    // ------------------------------------------------------------------
    #[test]
    fn typical_4kb_payload_under_5ms() {
        let mut body = String::with_capacity(4096);
        // ~67 chars per repeat × 70 ≈ 4690 → 略超 4KB
        for _ in 0..70 {
            body.push_str("Lorem ipsum dolor sit amet, contact alice@example.com or 13800138000. ");
        }
        assert!(body.len() >= 4000, "fixture body too short: {}", body.len());
        let start = std::time::Instant::now();
        for _ in 0..10 {
            let _ = default_redactor().redact_text(&body, None);
        }
        let avg = start.elapsed() / 10;
        // 宽松上限 5ms（debug build；release 通常 < 1ms）
        assert!(
            avg < std::time::Duration::from_millis(5),
            "avg redaction {avg:?} > 5ms"
        );
    }
}
