//! Moderation 关键词 + 类别检测规则集合 + 引擎。
//!
//! `Moderator` 是规则集的纯 Rust 包装，可在 host-side 单测中使用——只有
//! `lib.rs` 那一层带 wit-bindgen import。
//!
//! ## 内部结构
//!
//! 每个 `category` 持有两套关键词：
//!
//! - `en` —— 英文关键词，编译为一条 `\b(?:k1|k2|...)\b` case-insensitive 正则；
//!   走单词边界，避免误伤 `phate` / `chateau` / `assassin` 之类的合法词；
//! - `zh` —— 中文关键词，走子串匹配（`str::find`），符合中文无 word boundary
//!   的事实。允许 5 ms 内对 4KB payload 全量扫描。
//!
//! 所有正则在 `once_cell::Lazy` 中编译一次；每次 transform 仅 N 次 `captures_iter`
//! + 中文 `find` 循环，无额外分配。

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeMap;

// ============================================================================
// 规则定义
// ============================================================================

/// 一条 moderation 类别规则。
pub struct CategoryRule {
    pub category: &'static str,
    /// 英文关键词正则：`(?i)\b(?:k1|k2|...)\b`
    pub en_regex: Regex,
    /// 中文关键词字面值列表
    pub zh_keywords: &'static [&'static str],
}

/// 每条命中的替换字符串：`<moderation:{category}>`。
#[inline]
fn placeholder_for(category: &str) -> String {
    format!("<moderation:{category}>")
}

// 内置关键词集合。
// 同一 raw 文本可能命中多个类别；引擎按顺序应用，先命中的占位符把 raw
// 吃掉，后续类别就不会再扫到原始关键词，避免双重计数。
//
// 顺序原则：更敏感 / 更需要前置的类别在前。先 self_harm，再 hate / harassment /
// sexual / violence / illegal —— 避免 violence 的 "kill" 把 self_harm 的
// "kill myself" 上下文吞掉。
fn build_rules() -> Vec<CategoryRule> {
    vec![
        CategoryRule {
            category: "self_harm",
            en_regex: build_en_regex(&[
                "suicide",
                "kill myself",
                "end my life",
                "self harm",
                "self-harm",
                "cut myself",
                "overdose",
            ]),
            zh_keywords: &["自杀", "自残", "自伤", "轻生", "想死"],
        },
        CategoryRule {
            category: "hate",
            en_regex: build_en_regex(&[
                "nigger",
                "faggot",
                "kike",
                "chink",
                "spic",
                "wetback",
                "tranny",
            ]),
            zh_keywords: &["种族歧视", "纳粹", "支那", "黑鬼", "贱种"],
        },
        CategoryRule {
            category: "harassment",
            en_regex: build_en_regex(&[
                "stalk you",
                "doxx",
                "doxxing",
                "harass",
                "harassing",
                "bully",
                "bullying",
            ]),
            zh_keywords: &["人肉搜索", "网暴", "霸凌", "骚扰你", "肉搜"],
        },
        CategoryRule {
            category: "sexual",
            en_regex: build_en_regex(&[
                "porn",
                "pornography",
                "blowjob",
                "handjob",
                "child porn",
                "rape",
                "incest",
            ]),
            zh_keywords: &["色情", "强奸", "口交", "乱伦", "卖淫"],
        },
        CategoryRule {
            category: "violence",
            en_regex: build_en_regex(&[
                "kill you",
                "murder",
                "behead",
                "torture",
                "massacre",
                "lynch",
                "assassinate",
            ]),
            zh_keywords: &["杀了你", "谋杀", "斩首", "屠杀", "酷刑"],
        },
        CategoryRule {
            category: "illegal",
            en_regex: build_en_regex(&[
                "buy heroin",
                "sell cocaine",
                "meth lab",
                "gun smuggling",
                "burglary",
                "pirate software",
                "fentanyl",
            ]),
            zh_keywords: &["买毒品", "贩毒", "走私枪", "盗窃团伙", "制毒"],
        },
    ]
}

/// 把一组英文关键词编译成 `(?i)\b(?:k1|k2|...)\b` 正则。
/// 调用方需保证关键词不含正则元字符（内置表均为纯字面）。
fn build_en_regex(keywords: &[&str]) -> Regex {
    // 长关键词优先（避免短的吞掉长的），编译期一次性确定
    let mut sorted: Vec<&str> = keywords.iter().copied().collect();
    sorted.sort_by_key(|k| std::cmp::Reverse(k.len()));
    let alternation = sorted.join("|");
    // \b 在英文 ASCII 边界上工作良好；空格/连字符 in keywords 需用 (?:...)
    // 包住整体匹配。"kill myself" / "self-harm" 这类含空格 / 连字符的项，
    // \b 仍能锚在 "kill" 起点和 "myself" 末尾，足够避免误伤。
    Regex::new(&format!(r"(?i)\b(?:{alternation})\b"))
        .expect("moderation en keyword regex compile")
}

static DEFAULT_RULES: Lazy<Vec<CategoryRule>> = Lazy::new(build_rules);

// ============================================================================
// 引擎
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct ModerationStats {
    pub by_category: BTreeMap<String, usize>,
}

impl serde::Serialize for ModerationStats {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        // 输出形如 {"total":N,"by_category":{"hate":2,"violence":1}}
        let mut m = ser.serialize_map(Some(2))?;
        m.serialize_entry("total", &self.total())?;
        m.serialize_entry("by_category", &self.by_category)?;
        m.end()
    }
}

impl ModerationStats {
    pub fn total(&self) -> usize {
        self.by_category.values().copied().sum()
    }

    fn incr(&mut self, category: &str) {
        *self.by_category.entry(category.to_string()).or_insert(0) += 1;
    }
}

pub struct Moderator<'a> {
    rules: &'a [CategoryRule],
}

impl<'a> Moderator<'a> {
    pub fn new(rules: &'a [CategoryRule]) -> Self {
        Self { rules }
    }

    /// 对裸文本做关键词审核。
    /// 多规则按顺序应用；命中数累计到 stats。
    pub fn moderate_text(
        &self,
        input: &str,
        allowlist: Option<&[String]>,
    ) -> (String, ModerationStats) {
        let mut stats = ModerationStats::default();
        let mut current = input.to_string();
        for rule in self.rules {
            let placeholder = placeholder_for(rule.category);

            // ---- 英文：单一 alternation regex，单词边界 ----
            current = apply_en(&rule.en_regex, &current, &placeholder, allowlist, |raw| {
                stats.incr(rule.category);
                let _ = raw;
            });

            // ---- 中文：逐 keyword 子串扫描 ----
            for kw in rule.zh_keywords {
                current = apply_zh(kw, &current, &placeholder, allowlist, |_| {
                    stats.incr(rule.category);
                });
            }
        }
        (current, stats)
    }

    /// 对 JSON 字符串做关键词审核。
    ///
    /// 策略：解析 → 遍历所有 String 叶子值 → 应用 [`Self::moderate_text`]
    /// → 重新序列化。非 JSON 输入退到 [`Self::moderate_text`] 文本路径。
    pub fn moderate_json(
        &self,
        input: &str,
        allowlist: Option<&[String]>,
    ) -> (String, ModerationStats) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(input);
        let mut value = match parsed {
            Ok(v) => v,
            Err(_) => return self.moderate_text(input, allowlist),
        };
        let mut stats = ModerationStats::default();
        self.walk(&mut value, allowlist, &mut stats);
        let serialized = serde_json::to_string(&value).unwrap_or_else(|_| input.to_string());
        (serialized, stats)
    }

    fn walk(
        &self,
        value: &mut serde_json::Value,
        allowlist: Option<&[String]>,
        stats: &mut ModerationStats,
    ) {
        match value {
            serde_json::Value::String(s) => {
                let (replaced, mut chunk_stats) = self.moderate_text(s, allowlist);
                *s = replaced;
                for (k, v) in std::mem::take(&mut chunk_stats.by_category) {
                    *stats.by_category.entry(k).or_insert(0) += v;
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

/// 应用英文正则；每命中一处调一次 `on_hit`，返回替换后字符串。
fn apply_en<F: FnMut(&str)>(
    re: &Regex,
    current: &str,
    placeholder: &str,
    allowlist: Option<&[String]>,
    mut on_hit: F,
) -> String {
    let mut next = String::with_capacity(current.len());
    let mut last_end = 0usize;
    for m in re.find_iter(current) {
        let raw = &current[m.start()..m.end()];
        if is_allowlisted(raw, allowlist) {
            continue;
        }
        next.push_str(&current[last_end..m.start()]);
        next.push_str(placeholder);
        last_end = m.end();
        on_hit(raw);
    }
    next.push_str(&current[last_end..]);
    next
}

/// 应用单个中文 keyword 子串扫描。
fn apply_zh<F: FnMut(&str)>(
    keyword: &str,
    current: &str,
    placeholder: &str,
    allowlist: Option<&[String]>,
    mut on_hit: F,
) -> String {
    if keyword.is_empty() {
        return current.to_string();
    }
    let mut next = String::with_capacity(current.len());
    let mut last_end = 0usize;
    let bytes = current.as_bytes();
    let kw_bytes = keyword.as_bytes();
    let mut i = 0usize;
    while i + kw_bytes.len() <= bytes.len() {
        if bytes[i..].starts_with(kw_bytes) {
            // 命中：raw == keyword
            if is_allowlisted(keyword, allowlist) {
                // allowlist 跳过：保留原文，索引前进 keyword 长度
                next.push_str(&current[last_end..i + kw_bytes.len()]);
                last_end = i + kw_bytes.len();
                i = last_end;
                continue;
            }
            next.push_str(&current[last_end..i]);
            next.push_str(placeholder);
            last_end = i + kw_bytes.len();
            i = last_end;
            on_hit(keyword);
        } else {
            i += 1;
        }
    }
    next.push_str(&current[last_end..]);
    next
}

fn is_allowlisted(value: &str, allowlist: Option<&[String]>) -> bool {
    allowlist
        .map(|al| al.iter().any(|literal| literal == value))
        .unwrap_or(false)
}

/// 全局默认 moderator。WASM 入口和单测都用这个。
pub fn default_moderator() -> Moderator<'static> {
    Moderator::new(&DEFAULT_RULES)
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> (String, ModerationStats) {
        default_moderator().moderate_text(text, None)
    }

    // -------- 每类正例 + 反例 --------

    #[test]
    fn hate_en_positive() {
        let (out, stats) = run("you damn faggot leave");
        assert!(out.contains("<moderation:hate>"));
        assert_eq!(stats.by_category.get("hate").copied(), Some(1));
    }

    #[test]
    fn hate_zh_positive() {
        let (out, stats) = run("这是赤裸裸的种族歧视行为");
        assert!(out.contains("<moderation:hate>"));
        assert_eq!(stats.by_category.get("hate").copied(), Some(1));
    }

    #[test]
    fn hate_negative_no_substring_false_positive() {
        // "chateau" 含 "chink" 没关系，但 "phate" / "hate" 这类常见词不应误伤。
        // 注意：我们的 hate 词典里并没有裸的 "hate" 词条，所以正常句子不会命中。
        let (out, stats) = run("I hate Mondays but love the chateau views.");
        assert!(!out.contains("<moderation:hate>"));
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn harassment_en_positive() {
        let (out, stats) = run("I will doxx you tomorrow");
        assert!(out.contains("<moderation:harassment>"));
        assert_eq!(stats.by_category.get("harassment").copied(), Some(1));
    }

    #[test]
    fn harassment_zh_positive() {
        let (out, stats) = run("有人在搞人肉搜索我");
        assert!(out.contains("<moderation:harassment>"));
        assert_eq!(stats.by_category.get("harassment").copied(), Some(1));
    }

    #[test]
    fn harassment_negative_normal_text() {
        let (out, stats) = run("Bulletin board posts are fine.");
        assert!(!out.contains("<moderation:harassment>"));
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn self_harm_en_positive() {
        let (out, stats) = run("I want to kill myself tonight");
        assert!(out.contains("<moderation:self_harm>"));
        assert_eq!(stats.by_category.get("self_harm").copied(), Some(1));
    }

    #[test]
    fn self_harm_zh_positive() {
        let (out, stats) = run("他最近一直说想自杀");
        assert!(out.contains("<moderation:self_harm>"));
        assert_eq!(stats.by_category.get("self_harm").copied(), Some(1));
    }

    #[test]
    fn self_harm_negative_homicide() {
        // "kill the bug" 不是 self_harm 也不是 violence（violence 词典里是 "kill you" 整体）
        let (out, stats) = run("kill the bug in this code please");
        assert!(!out.contains("<moderation:self_harm>"));
        assert!(!out.contains("<moderation:violence>"));
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn sexual_en_positive() {
        let (out, stats) = run("This site hosts porn streams");
        assert!(out.contains("<moderation:sexual>"));
        assert_eq!(stats.by_category.get("sexual").copied(), Some(1));
    }

    #[test]
    fn sexual_zh_positive() {
        let (out, stats) = run("传播色情内容是违法的");
        // 同时含 "色情" (sexual) + "违法" 不在 illegal 词典里 → 仅 sexual
        assert!(out.contains("<moderation:sexual>"));
        assert_eq!(stats.by_category.get("sexual").copied(), Some(1));
    }

    #[test]
    fn sexual_negative_word_boundary() {
        // "pornography" 在词典里所以会命中；但 "popcorn" 之类纯无关词不应误伤
        let (out, stats) = run("Popcorn and movies on Friday.");
        assert!(!out.contains("<moderation:sexual>"));
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn violence_en_positive() {
        let (out, stats) = run("I will kill you slowly");
        assert!(out.contains("<moderation:violence>"));
        assert_eq!(stats.by_category.get("violence").copied(), Some(1));
    }

    #[test]
    fn violence_zh_positive() {
        let (out, stats) = run("他在威胁要杀了你");
        assert!(out.contains("<moderation:violence>"));
        assert_eq!(stats.by_category.get("violence").copied(), Some(1));
    }

    #[test]
    fn violence_negative_assassin_word() {
        // "assassinate" 在词典里所以会命中；但 "assassin" 单独不应被吞
        // （也不该出 violence 占位符）。
        let (out, stats) = run("The assassin character is fictional.");
        assert!(!out.contains("<moderation:violence>"));
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn illegal_en_positive() {
        let (out, stats) = run("Where can I buy heroin online");
        assert!(out.contains("<moderation:illegal>"));
        assert_eq!(stats.by_category.get("illegal").copied(), Some(1));
    }

    #[test]
    fn illegal_zh_positive() {
        let (out, stats) = run("他被指控走私枪");
        assert!(out.contains("<moderation:illegal>"));
        assert_eq!(stats.by_category.get("illegal").copied(), Some(1));
    }

    #[test]
    fn illegal_negative_general_law() {
        let (out, stats) = run("Legal compliance is mandatory.");
        assert!(!out.contains("<moderation:illegal>"));
        assert_eq!(stats.total(), 0);
    }

    // -------- allowlist --------

    #[test]
    fn allowlist_skips_en_literal() {
        let allow = vec!["porn".to_string()];
        let (out, stats) =
            default_moderator().moderate_text("research on porn industry", Some(&allow));
        assert_eq!(out, "research on porn industry");
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn allowlist_skips_zh_literal() {
        let allow = vec!["色情".to_string()];
        let (out, stats) =
            default_moderator().moderate_text("研究色情产业的社会学论文", Some(&allow));
        assert_eq!(out, "研究色情产业的社会学论文");
        assert_eq!(stats.total(), 0);
    }

    // -------- multi-category --------

    #[test]
    fn multi_category_single_pass() {
        let (out, stats) = run(
            "I hate Mondays but if you doxx me I will kill you and buy heroin to end my life.",
        );
        // self_harm("end my life") + harassment("doxx") + violence("kill you") + illegal("buy heroin")
        assert!(out.contains("<moderation:self_harm>"));
        assert!(out.contains("<moderation:harassment>"));
        assert!(out.contains("<moderation:violence>"));
        assert!(out.contains("<moderation:illegal>"));
        // "hate" 不在词典里，不计
        assert_eq!(stats.by_category.get("self_harm").copied(), Some(1));
        assert_eq!(stats.by_category.get("harassment").copied(), Some(1));
        assert_eq!(stats.by_category.get("violence").copied(), Some(1));
        assert_eq!(stats.by_category.get("illegal").copied(), Some(1));
        assert_eq!(stats.total(), 4);
    }

    // -------- JSON 路径 --------

    #[test]
    fn moderate_json_walks_nested_strings() {
        let input = r#"{
            "messages": [
                {"role": "user", "content": "I want to kill myself"},
                {"role": "assistant", "content": "ok"}
            ],
            "metadata": {"note": "种族歧视言论"}
        }"#;
        let (out, stats) = default_moderator().moderate_json(input, None);
        assert!(out.contains("<moderation:self_harm>"));
        assert!(out.contains("<moderation:hate>"));
        assert_eq!(stats.total(), 2);
        // 仍然是合法 JSON
        let _: serde_json::Value = serde_json::from_str(&out).expect("still json");
    }

    #[test]
    fn moderate_json_passes_through_non_json() {
        let input = "data: {sse fragment with kill you marker}\n\n";
        let (out, stats) = default_moderator().moderate_json(input, None);
        assert!(out.contains("<moderation:violence>"));
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn no_match_passes_through() {
        let (out, stats) = run("Just a friendly hello from the team.");
        assert_eq!(out, "Just a friendly hello from the team.");
        assert_eq!(stats.total(), 0);
    }

    // -------- stats serialize --------

    #[test]
    fn stats_serializes_with_total_and_by_category() {
        let mut s = ModerationStats::default();
        s.incr("hate");
        s.incr("hate");
        s.incr("violence");
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"total\":3"),
            "missing total in {json}"
        );
        assert!(
            json.contains("\"by_category\":{") && json.contains("\"hate\":2")
                && json.contains("\"violence\":1"),
            "missing by_category in {json}"
        );
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
    fn golden_abuse_request() {
        let input = read_relative("fixtures/abuse_request.json");
        let (got, _) = default_moderator().moderate_json(&input, None);
        let expected = read_relative("golden/abuse_request.json");
        let g: serde_json::Value = serde_json::from_str(&got).expect("got is json");
        let e: serde_json::Value = serde_json::from_str(&expected).expect("expected is json");
        assert_eq!(g, e);
    }

    #[test]
    fn golden_clean_sse() {
        let input = read_relative("fixtures/clean_sse.txt");
        let (got, _) = default_moderator().moderate_text(&input, None);
        let expected = read_relative("golden/clean_sse.txt");
        assert_eq!(got, expected);
    }

    // ------------------------------------------------------------------
    // 性能 baseline: typical 4KB payload < 5ms（debug build）
    // ------------------------------------------------------------------
    #[test]
    fn typical_4kb_payload_under_5ms() {
        let mut body = String::with_capacity(4096);
        // ~64 chars per repeat × 75 ≈ 4800 → 略超 4KB
        for _ in 0..75 {
            body.push_str("Lorem ipsum dolor sit amet, just a benign sentence repeating. ");
        }
        assert!(body.len() >= 4000, "fixture body too short: {}", body.len());
        let start = std::time::Instant::now();
        for _ in 0..10 {
            let _ = default_moderator().moderate_text(&body, None);
        }
        let avg = start.elapsed() / 10;
        // 宽松上限 5ms（debug build；release 通常 < 1ms）
        assert!(
            avg < std::time::Duration::from_millis(5),
            "avg moderation {avg:?} > 5ms"
        );
    }
}
