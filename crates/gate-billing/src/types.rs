//! UsageEvent — 计费用量事件，与 usage_records 表字段对齐。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 一次 LLM 请求的用量快照。
///
/// 字段与 usage_records 表保持兼容，cost_micros 用 i64（避免浮点）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    /// 全局请求 ID（用于幂等写 usage_records）
    pub request_id: Uuid,
    /// Stable idempotency key for exactly-once settlement across projections.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// 发起请求的 API Key ID
    pub api_key_id: Uuid,
    /// Project ID
    pub project_id: Uuid,
    /// Org ID
    pub org_id: Uuid,
    /// 路由到的 Channel ID（可选，fallback provider 时为 None）
    pub channel_id: Option<Uuid>,
    /// 实际使用的模型名
    pub model: String,
    /// 输入 token 数
    pub prompt_tokens: i32,
    /// 输出 token 数
    pub completion_tokens: i32,
    /// 缓存命中 token 数
    #[serde(default)]
    pub cached_tokens: i32,
    /// 费用（微美元，1 USD = 1_000_000 cost_micros）
    pub cost_micros: i64,
    /// 事件发生时间
    pub occurred_at: DateTime<Utc>,
    /// 请求状态（HTTP status code）
    pub status: i16,
}
