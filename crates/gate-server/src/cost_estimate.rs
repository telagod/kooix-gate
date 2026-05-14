//! 请求前的粗略费用估算。
//!
//! 在 pre-debit 场景下，请求尚未发到上游 provider，真实 usage 未知。
//! 这里用启发式规则估算一个保守上界，事后再通过 [`InflightGuard::settle`] 修正。
//!
//! 估算策略：
//! - prompt_tokens ≈ total_chars_in_messages / 4（粗糙 tokenizer 近似）
//!   注意：chars/4 对英文/拉丁语系合理，CJK 字符通常 1 char ≈ 1-2 tokens，
//!   此处会低估 CJK-heavy 请求的 prompt token 数。可接受——pre-debit 偏低只是
//!   暂时少扣，settle 时会用真实 usage 修正。
//! - completion_tokens ≈ max_tokens 请求字段（默认 1024）
//! - cost_micros = total_tokens × DEFAULT_RATE_PER_TOKEN_MICROS，上限 MAX_ESTIMATE_MICROS

use gate_providers::ChatRequest;

/// 粗略单 token 单价（micros）：约 $3 / 1M tokens → 3 micros/token。
/// 偏保守，避免预扣不足导致超支。
pub const DEFAULT_RATE_PER_TOKEN_MICROS: i64 = 3;

/// 预估上限：5_000_000 micros = $5.00。
/// 这是 pre-debit 保护上限，不代表实际计费——settle 时以真实 usage 为准。
/// 设得足够高以覆盖长 context + 长输出的合法请求（如 200k ctx window）。
pub const MAX_ESTIMATE_MICROS: i64 = 5_000_000;

/// 估算一次 chat 请求的费用（micros），用于 pre-debit。
pub fn estimate_cost_micros(req: &ChatRequest, rate_per_token_micros: i64) -> i64 {
    let prompt_chars: usize = req.messages.iter().map(|m| m.content_text().len()).sum();
    let prompt_tokens = (prompt_chars / 4) as i64;
    // 未设 max_tokens 时用 1024 做估算——大多数请求不会跑满 4096，
    // 偏低估可接受，settle 时真实 usage 会修正。
    let completion_tokens = req.max_tokens.unwrap_or(1024) as i64;
    let total_tokens = prompt_tokens + completion_tokens;
    (total_tokens * rate_per_token_micros).min(MAX_ESTIMATE_MICROS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_providers::{ChatMessage, ChatRequest, Role};

    fn make_req(messages: &[&str], max_tokens: Option<u32>) -> ChatRequest {
        ChatRequest {
            model: "gpt-4o".into(),
            messages: messages
                .iter()
                .map(|c| ChatMessage::text(Role::User, *c))
                .collect(),
            temperature: None,
            top_p: None,
            max_tokens,
            stream: false,
            tools: None,
            tool_choice: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn short_message_default_max_tokens() {
        // 12 chars / 4 = 3 prompt tokens + 1024 completion = 1027 tokens × 3 = 3081
        let req = make_req(&["Hello world!"], None);
        assert_eq!(estimate_cost_micros(&req, DEFAULT_RATE_PER_TOKEN_MICROS), 3_081);
    }

    #[test]
    fn explicit_max_tokens() {
        // 8 chars / 4 = 2 + 100 = 102 × 3 = 306
        let req = make_req(&["hi there"], Some(100));
        assert_eq!(estimate_cost_micros(&req, DEFAULT_RATE_PER_TOKEN_MICROS), 306);
    }

    #[test]
    fn caps_at_max_estimate() {
        // 很长的 message + 大 max_tokens → 超限 cap
        let long_msg = "x".repeat(200_000); // 200k chars / 4 = 50k tokens
        let req = make_req(&[&long_msg], Some(50_000));
        // (50000 + 50000) × 3 = 300_000 → still under 5_000_000
        assert_eq!(estimate_cost_micros(&req, DEFAULT_RATE_PER_TOKEN_MICROS), 300_000);
    }

    #[test]
    fn caps_at_max_estimate_extreme() {
        // 极端大请求 → 命中 5_000_000 cap
        let long_msg = "x".repeat(4_000_000); // 4M chars / 4 = 1M tokens
        let req = make_req(&[&long_msg], Some(1_000_000));
        // (1_000_000 + 1_000_000) × 3 = 6_000_000 → capped at 5_000_000
        assert_eq!(estimate_cost_micros(&req, DEFAULT_RATE_PER_TOKEN_MICROS), MAX_ESTIMATE_MICROS);
    }

    #[test]
    fn empty_messages() {
        let req = make_req(&[], Some(512));
        // 0 + 512 = 512 × 3 = 1536
        assert_eq!(estimate_cost_micros(&req, DEFAULT_RATE_PER_TOKEN_MICROS), 1_536);
    }
}
