//! Banned signal detection (ADR-0007 §3 / M5.1 N1.3)。
//!
//! 把上游响应转成 `Option<String>` 封号原因，喂给 [`gate_storage::OutcomeObservation::banned_signal`]。
//! 命中即触发 [`crate::health_score::ScoreEngine`] 状态机转入 `Banned`（终态）。
//!
//! ## 三层规则
//!
//! 1. **协议层**（默认实现）：401/403 streak + 429 长 Retry-After
//! 2. **语义层**（按 preset）：vendor-specific 响应 body 正则
//!    - OpenAI: `account_deactivated` / `insufficient_quota`
//!    - Anthropic: `permission_error` / `invalid_api_key` 持续
//!    - Azure: `DeploymentNotFound` / `Quota exceeded` 终态
//!    - Bedrock: `AccessDeniedException` / `ServiceQuotaExceeded`
//! 3. **量化层**（balance）：留 N2.2，不在 N1.3 范围
//!
//! ## 使用
//!
//! ```ignore
//! let registry = BannedMatcherRegistry::default();
//! let matcher = registry.for_provider("openai");
//! if let Some(reason) = matcher.detect(BannedDetectionContext {
//!     status: 401,
//!     body: "...account_deactivated...",
//!     consecutive_auth_failures: 3,
//!     retry_after_secs: None,
//! }) {
//!     // 落到 OutcomeObservation.banned_signal = Some(reason)
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// 输入上下文
// ============================================================================

/// 单条响应判定输入。
///
/// 调用方（request handler / probe）收到上游响应后填充这个，调一次 matcher。
#[derive(Debug, Clone)]
pub struct BannedDetectionContext<'a> {
    /// HTTP 状态码。
    pub status: u16,
    /// 响应 body（裁剪后；建议 ≤ 2KiB，避免大字符串 regex 扫描）。
    pub body: &'a str,
    /// 该 channel 当前连续 401/403 次数（来自 `ChannelHealthScore.consecutive_failures`
    /// 或 routing 内 auth-error streak counter）。
    pub consecutive_auth_failures: i32,
    /// 上游 `Retry-After` 头解析后的秒数。
    pub retry_after_secs: Option<i64>,
}

// ============================================================================
// trait
// ============================================================================

/// Vendor 特定的封号判定。所有实现必须是 `Send + Sync`，可由 Arc 共享。
///
/// 命中返 `Some(reason)`；`reason` 落 `ChannelHealthScore.banned_reason`。
pub trait BannedPatternMatcher: Send + Sync + 'static {
    fn detect(&self, ctx: BannedDetectionContext<'_>) -> Option<String>;
}

// ============================================================================
// 协议层默认实现
// ============================================================================

/// 协议层默认规则——所有 provider 都适用。
///
/// 命中条件（任一）：
/// - status ∈ {401, 403} 且 consecutive_auth_failures ≥ `auth_streak_threshold`
/// - status == 429 且 retry_after_secs > `long_retry_after_secs`
///
/// 默认值跟 ADR-0007 §3 一致：`auth_streak=3`，`long_retry_after=3600s` (1h)。
#[derive(Debug, Clone)]
pub struct DefaultProtocolMatcher {
    pub auth_streak_threshold: i32,
    pub long_retry_after_secs: i64,
}

impl Default for DefaultProtocolMatcher {
    fn default() -> Self {
        Self {
            auth_streak_threshold: 3,
            long_retry_after_secs: 3600,
        }
    }
}

impl BannedPatternMatcher for DefaultProtocolMatcher {
    fn detect(&self, ctx: BannedDetectionContext<'_>) -> Option<String> {
        if matches!(ctx.status, 401 | 403)
            && ctx.consecutive_auth_failures >= self.auth_streak_threshold
        {
            return Some(format!(
                "protocol:auth_streak_{}_x{}",
                ctx.status, ctx.consecutive_auth_failures
            ));
        }
        if ctx.status == 429
            && ctx
                .retry_after_secs
                .is_some_and(|s| s > self.long_retry_after_secs)
        {
            return Some(format!(
                "protocol:retry_after_{}s",
                ctx.retry_after_secs.unwrap()
            ));
        }
        None
    }
}

// ============================================================================
// 语义层（4 fast-path preset）
// ============================================================================

/// 复合 matcher：协议层 OR 语义层任一命中即返。
///
/// 这是所有 vendor matcher 的标准包装。
#[derive(Clone)]
pub struct CompositeMatcher {
    protocol: DefaultProtocolMatcher,
    semantic_patterns: Arc<Vec<(String, String)>>, // (pattern_substr, reason_tag)
    label: &'static str,
}

impl CompositeMatcher {
    fn new(label: &'static str, patterns: Vec<(&str, &str)>) -> Self {
        Self {
            protocol: DefaultProtocolMatcher::default(),
            semantic_patterns: Arc::new(
                patterns
                    .into_iter()
                    .map(|(p, r)| (p.to_string(), r.to_string()))
                    .collect(),
            ),
            label,
        }
    }
}

impl BannedPatternMatcher for CompositeMatcher {
    fn detect(&self, ctx: BannedDetectionContext<'_>) -> Option<String> {
        if let Some(r) = self.protocol.detect(ctx.clone()) {
            return Some(format!("{}:{}", self.label, r));
        }
        let body_lc = ctx.body.to_ascii_lowercase();
        for (substr, reason) in self.semantic_patterns.iter() {
            if body_lc.contains(&substr.to_ascii_lowercase()) {
                return Some(format!("{}:{}", self.label, reason));
            }
        }
        None
    }
}

/// OpenAI fast-path matcher。
pub fn openai_matcher() -> CompositeMatcher {
    CompositeMatcher::new(
        "openai",
        vec![
            ("account_deactivated", "account_deactivated"),
            ("insufficient_quota", "insufficient_quota"),
            ("billing_hard_limit_reached", "billing_hard_limit"),
            ("organization has been disabled", "org_disabled"),
        ],
    )
}

/// Anthropic fast-path matcher。
pub fn anthropic_matcher() -> CompositeMatcher {
    CompositeMatcher::new(
        "anthropic",
        vec![
            ("permission_error", "permission_error"),
            ("invalid_api_key", "invalid_api_key"),
            ("account is not active", "account_inactive"),
            ("credit balance is too low", "low_credit"),
        ],
    )
}

/// Azure fast-path matcher。
pub fn azure_matcher() -> CompositeMatcher {
    CompositeMatcher::new(
        "azure",
        vec![
            ("DeploymentNotFound", "deployment_not_found"),
            ("Subscription does not have access", "no_access"),
            ("Quota exceeded for this subscription", "quota_exceeded"),
            ("SubscriptionDisabled", "subscription_disabled"),
        ],
    )
}

/// Bedrock fast-path matcher。
pub fn bedrock_matcher() -> CompositeMatcher {
    CompositeMatcher::new(
        "bedrock",
        vec![
            ("AccessDeniedException", "access_denied"),
            ("ServiceQuotaExceededException", "service_quota_exceeded"),
            ("UnauthorizedOperation", "unauthorized_operation"),
            ("AccountTerminated", "account_terminated"),
        ],
    )
}

// ============================================================================
// Registry
// ============================================================================

/// `provider_type` → matcher 路由。fallback 到 [`DefaultProtocolMatcher`]。
#[derive(Clone)]
pub struct BannedMatcherRegistry {
    entries: HashMap<String, Arc<dyn BannedPatternMatcher>>,
    fallback: Arc<dyn BannedPatternMatcher>,
}

impl Default for BannedMatcherRegistry {
    fn default() -> Self {
        let mut reg = Self {
            entries: HashMap::new(),
            fallback: Arc::new(DefaultProtocolMatcher::default()),
        };
        reg.register("openai", Arc::new(openai_matcher()));
        reg.register("anthropic", Arc::new(anthropic_matcher()));
        reg.register("azure", Arc::new(azure_matcher()));
        reg.register("bedrock", Arc::new(bedrock_matcher()));
        reg
    }
}

impl BannedMatcherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册（或覆盖）一个 provider_type 的 matcher。
    pub fn register(&mut self, provider_type: &str, matcher: Arc<dyn BannedPatternMatcher>) {
        self.entries.insert(provider_type.to_string(), matcher);
    }

    /// 按 provider_type 取 matcher，未注册返回 fallback。
    pub fn for_provider(&self, provider_type: &str) -> Arc<dyn BannedPatternMatcher> {
        self.entries
            .get(provider_type)
            .cloned()
            .unwrap_or_else(|| self.fallback.clone())
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(status: u16, body: &str) -> BannedDetectionContext<'_> {
        BannedDetectionContext {
            status,
            body,
            consecutive_auth_failures: 0,
            retry_after_secs: None,
        }
    }

    // ---- DefaultProtocolMatcher ----

    #[test]
    fn protocol_auth_streak_triggers_at_threshold() {
        let m = DefaultProtocolMatcher::default();
        let mut c = ctx(401, "");
        c.consecutive_auth_failures = 2;
        assert!(m.detect(c.clone()).is_none());
        c.consecutive_auth_failures = 3;
        assert!(m.detect(c).is_some());
    }

    #[test]
    fn protocol_only_401_403_count_as_auth() {
        let m = DefaultProtocolMatcher::default();
        let mut c = ctx(500, "");
        c.consecutive_auth_failures = 10;
        assert!(m.detect(c).is_none());
    }

    #[test]
    fn protocol_long_retry_after_triggers() {
        let m = DefaultProtocolMatcher::default();
        let mut c = ctx(429, "");
        c.retry_after_secs = Some(3500);
        assert!(m.detect(c.clone()).is_none(), "below threshold");
        c.retry_after_secs = Some(3601);
        let r = m.detect(c).unwrap();
        assert!(r.contains("3601"));
    }

    #[test]
    fn protocol_429_without_retry_after_no_signal() {
        let m = DefaultProtocolMatcher::default();
        let c = ctx(429, "");
        assert!(m.detect(c).is_none());
    }

    // ---- OpenAI ----

    #[test]
    fn openai_detects_account_deactivated_in_body() {
        let m = openai_matcher();
        let r = m.detect(ctx(403, r#"{"error":{"code":"account_deactivated"}}"#));
        assert!(r.is_some());
        assert!(r.unwrap().contains("account_deactivated"));
    }

    #[test]
    fn openai_detects_insufficient_quota_case_insensitive() {
        let m = openai_matcher();
        assert!(m.detect(ctx(429, "Insufficient_Quota")).is_some());
    }

    #[test]
    fn openai_passes_through_normal_responses() {
        let m = openai_matcher();
        assert!(m.detect(ctx(200, r#"{"choices":[]}"#)).is_none());
    }

    // ---- Anthropic ----

    #[test]
    fn anthropic_detects_permission_error() {
        let m = anthropic_matcher();
        assert!(
            m.detect(ctx(403, r#"{"type":"permission_error"}"#))
                .is_some()
        );
    }

    #[test]
    fn anthropic_detects_invalid_api_key() {
        let m = anthropic_matcher();
        let mut c = ctx(401, r#"{"error":{"type":"invalid_api_key"}}"#);
        c.consecutive_auth_failures = 1;
        // 语义层直接命中，不需要协议层 streak
        assert!(m.detect(c).is_some());
    }

    #[test]
    fn anthropic_detects_low_credit_balance() {
        let m = anthropic_matcher();
        assert!(
            m.detect(ctx(400, "Your credit balance is too low."))
                .is_some()
        );
    }

    // ---- Azure ----

    #[test]
    fn azure_detects_deployment_not_found() {
        let m = azure_matcher();
        let r = m.detect(ctx(404, r#"{"error":{"code":"DeploymentNotFound"}}"#));
        assert!(r.unwrap().contains("deployment_not_found"));
    }

    #[test]
    fn azure_detects_quota_exceeded() {
        let m = azure_matcher();
        assert!(
            m.detect(ctx(429, "Quota exceeded for this subscription"))
                .is_some()
        );
    }

    // ---- Bedrock ----

    #[test]
    fn bedrock_detects_access_denied() {
        let m = bedrock_matcher();
        let r = m.detect(ctx(403, r#"{"__type":"AccessDeniedException"}"#));
        assert!(r.unwrap().contains("access_denied"));
    }

    #[test]
    fn bedrock_detects_service_quota_exceeded() {
        let m = bedrock_matcher();
        assert!(
            m.detect(ctx(400, "ServiceQuotaExceededException"))
                .is_some()
        );
    }

    // ---- Composite priority: protocol layer wins first ----

    #[test]
    fn composite_protocol_layer_wins_when_both_match() {
        let m = openai_matcher();
        let mut c = ctx(401, "account_deactivated");
        c.consecutive_auth_failures = 5;
        let r = m.detect(c).unwrap();
        // 协议层先命中
        assert!(r.contains("auth_streak"));
    }

    // ---- Registry ----

    #[test]
    fn registry_routes_to_vendor_matcher() {
        let reg = BannedMatcherRegistry::default();
        let openai = reg.for_provider("openai");
        assert!(
            openai
                .detect(ctx(403, "account_deactivated"))
                .unwrap()
                .contains("openai")
        );
    }

    #[test]
    fn registry_unknown_falls_back_to_protocol_only() {
        let reg = BannedMatcherRegistry::default();
        let unknown = reg.for_provider("zzz_custom_unregistered");
        let mut c = ctx(401, "account_deactivated"); // vendor 关键字
        c.consecutive_auth_failures = 1;
        // 协议层不满足（streak < 3），vendor 不识别 → None
        assert!(unknown.detect(c).is_none());
    }

    #[test]
    fn registry_allows_overriding_existing() {
        let mut reg = BannedMatcherRegistry::default();
        struct AlwaysHit;
        impl BannedPatternMatcher for AlwaysHit {
            fn detect(&self, _ctx: BannedDetectionContext<'_>) -> Option<String> {
                Some("always".to_string())
            }
        }
        reg.register("openai", Arc::new(AlwaysHit));
        let m = reg.for_provider("openai");
        assert_eq!(m.detect(ctx(200, "")).as_deref(), Some("always"));
    }

    // ---- Custom protocol thresholds ----

    #[test]
    fn protocol_custom_threshold() {
        let m = DefaultProtocolMatcher {
            auth_streak_threshold: 1,
            long_retry_after_secs: 60,
        };
        let mut c = ctx(403, "");
        c.consecutive_auth_failures = 1;
        assert!(m.detect(c).is_some());
        let mut c = ctx(429, "");
        c.retry_after_secs = Some(120);
        assert!(m.detect(c).is_some());
    }
}
