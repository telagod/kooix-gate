//! Quota & Rate Limit 领域模型

use crate::id::*;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 配额挂在哪个主体上 — 多维叠加时各自取最严
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaScope {
    /// 全局兜底（平台运营）
    Platform,
    Org,
    Project,
    User,
    Membership,
    ApiKey,
}

/// 配额维度 — 不同维度走不同的存储/算法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaDimension {
    /// 每分钟请求数（滑动窗口）
    Rpm,
    /// 每分钟 token 数（滑动窗口）
    Tpm,
    /// 并发连接数（计数器）
    Concurrent,
    /// 每日预算（USD，重置周期）
    DailyBudgetUsd,
    /// 每月预算（USD，重置周期）
    MonthlyBudgetUsd,
    /// 终身预算（USD，不重置）
    LifetimeBudgetUsd,
    /// 终身配额（不重置）
    LifetimeTokens,
}

impl QuotaDimension {
    pub fn is_rate(&self) -> bool {
        matches!(self, Self::Rpm | Self::Tpm | Self::Concurrent)
    }
    pub fn is_budget(&self) -> bool {
        matches!(
            self,
            Self::DailyBudgetUsd
                | Self::MonthlyBudgetUsd
                | Self::LifetimeBudgetUsd
                | Self::LifetimeTokens
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub id: QuotaId,
    pub scope: QuotaScope,
    /// 通用 ID — 解释依赖 scope（OrgId / ProjectId / UserId / ApiKeyId 的 UUID）
    pub scope_id: uuid::Uuid,
    pub dimension: QuotaDimension,
    /// 限定模型范围（glob，None = 所有模型）
    pub model_filter: Option<String>,
    /// 数值上限（rate 是 N，budget 是 USD）
    pub limit_value: Decimal,
    /// 时间窗口（秒，仅对 rate 类有意义）
    pub window_seconds: Option<i32>,
    /// enforce = 实际拦截；dry_run = 只记录 would-deny。
    pub mode: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 请求级实时用量（用于扣减/检查）
#[derive(Debug, Clone)]
pub struct UsageDelta {
    pub project_id: ProjectId,
    pub api_key_id: ApiKeyId,
    pub model: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub estimated_cost_usd: Decimal,
}

/// 配额检查结果
#[derive(Debug, Clone)]
pub enum QuotaCheck {
    Allowed,
    Denied {
        dimension: QuotaDimension,
        scope: QuotaScope,
        current: Decimal,
        limit: Decimal,
    },
}
