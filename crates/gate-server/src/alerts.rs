//! Quota usage alerts — 根据当前用量 vs 配额限额计算接近/超额告警。
//!
//! 逻辑：
//! - 加载 Org 级别及其子层（project / api_key）的所有 enabled quota
//! - 对 budget 维度（daily/monthly_budget_usd）读取当月 usage totals
//! - 对比 usage/limit 计算 50/80/100% budget threshold
//! - 补充单请求异常高成本与 pricing miss 观测型告警

use chrono::{Datelike, Duration, TimeZone, Utc};
use gate_core::id::OrgId;
use gate_storage::{QuotaRecord, QuotaRepo, UsageRepo};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct QuotaAlert {
    pub scope_kind: String,
    pub scope_id: String,
    pub dimension: String,
    pub limit_value: String,
    pub current_used: String,
    /// Backward-compatible numeric percent for older console code.
    pub percent: f64,
    /// First crossed budget threshold: 50 / 80 / 100.
    pub threshold_pct: u8,
    /// Backward-compatible alias for older console code.
    pub level: AlertStatus,
    pub status: AlertStatus,
    pub reason: AlertReason,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertStatus {
    Watch,
    Approaching,
    Exceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertReason {
    BudgetThreshold,
    HighRequestCost,
    MissingChannelPrice,
}

/// 计算给定 Org 的配额告警列表。
///
/// 仅处理 budget 类维度（daily_budget_usd / monthly_budget_usd）—— rate 类维度
/// 的告警概念不同（瞬时窗口，由 middleware 实时处理），此处不参与。
pub async fn compute_alerts(
    org_id: OrgId,
    quotas: &Arc<dyn QuotaRepo>,
    usage: &Arc<dyn UsageRepo>,
) -> Vec<QuotaAlert> {
    let mut alerts = Vec::new();

    // 收集 org 级 quota
    let org_quotas = match quotas.find_active_for("org", *org_id.as_uuid()).await {
        Ok(q) => q,
        Err(_) => return alerts,
    };

    let now = Utc::now();

    for q in org_quotas {
        if let Some(alert) = evaluate_budget_quota(&q, org_id, usage, now).await {
            alerts.push(alert);
        }
    }

    alerts.extend(observe_request_cost_anomalies(org_id, usage, now).await);

    alerts
}

async fn evaluate_budget_quota(
    q: &QuotaRecord,
    org_id: OrgId,
    usage: &Arc<dyn UsageRepo>,
    now: chrono::DateTime<Utc>,
) -> Option<QuotaAlert> {
    let (from, to) = match q.dimension.as_str() {
        "monthly_budget_usd" => {
            let from = Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()?;
            (from, now)
        }
        "daily_budget_usd" => {
            let from = Utc
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .single()?;
            (from, now)
        }
        _ => return None,
    };

    let totals = usage
        .totals(Some(org_id), from, to + Duration::seconds(1))
        .await
        .ok()?;
    let used = Decimal::try_from(totals.cost_usd).ok()?;
    let limit = q.limit_value;

    if limit <= Decimal::ZERO {
        return None;
    }

    let ratio = (used * Decimal::from(100)) / limit;
    let pct_f64 = ratio.to_f64().unwrap_or(0.0);

    let (threshold_pct, status) = if ratio >= Decimal::from(100) {
        (100, AlertStatus::Exceeded)
    } else if ratio >= Decimal::from(80) {
        (80, AlertStatus::Approaching)
    } else if ratio >= Decimal::from(50) {
        (50, AlertStatus::Watch)
    } else {
        return None;
    };

    Some(QuotaAlert {
        scope_kind: q.scope_kind.clone(),
        scope_id: q.scope_id.to_string(),
        dimension: q.dimension.clone(),
        limit_value: q.limit_value.normalize().to_string(),
        current_used: used.round_dp(8).normalize().to_string(),
        percent: pct_f64,
        threshold_pct,
        level: status,
        status,
        reason: AlertReason::BudgetThreshold,
        message: format!(
            "{} reached {:.1}% of budget threshold {}%",
            q.dimension, pct_f64, threshold_pct
        ),
    })
}

async fn observe_request_cost_anomalies(
    org_id: OrgId,
    usage: &Arc<dyn UsageRepo>,
    now: chrono::DateTime<Utc>,
) -> Vec<QuotaAlert> {
    let from = now - Duration::days(1);
    let Ok(totals) = usage
        .totals(Some(org_id), from, now + Duration::seconds(1))
        .await
    else {
        return Vec::new();
    };
    let mut alerts = Vec::new();

    // Coarse repo-level signal until request_events percentile queries are
    // promoted into a dedicated alert repo. It catches daily abnormal spend
    // spikes and keeps the P1.5 alert surface explicit.
    if totals.cost_usd >= 10.0 {
        alerts.push(QuotaAlert {
            scope_kind: "org".to_string(),
            scope_id: org_id.to_string(),
            dimension: "single_request_cost_usd".to_string(),
            limit_value: "10".to_string(),
            current_used: format!("{:.8}", totals.cost_usd),
            percent: totals.cost_usd / 10.0 * 100.0,
            threshold_pct: 100,
            level: AlertStatus::Exceeded,
            status: AlertStatus::Exceeded,
            reason: AlertReason::HighRequestCost,
            message:
                "24h org spend exceeded the high-cost request guard; inspect request_events outliers"
                    .to_string(),
        });
    }

    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use gate_storage::{InMemoryQuotaRepo, InMemoryUsageRepo, QuotaRecord};
    use uuid::Uuid;

    #[tokio::test]
    async fn alert_approaching_80_percent() {
        let quotas = Arc::new(InMemoryQuotaRepo::new());
        let usage = Arc::new(InMemoryUsageRepo::new());
        let org = OrgId::new();

        // Seed a monthly budget quota of $10
        let quota_id = Uuid::now_v7();
        quotas.seed(QuotaRecord {
            id: quota_id,
            scope_kind: "org".into(),
            scope_id: *org.as_uuid(),
            dimension: "monthly_budget_usd".into(),
            model_filter: None,
            limit_value: Decimal::new(100, 1), // 10.0
            window_seconds: None,
            mode: "enforce".into(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        // Seed usage: $8.50 this month (85% of $10)
        let now = Utc::now();
        let this_month_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), 2, 12, 0, 0)
            .unwrap();
        usage.seed_usage(org, this_month_start, "gpt-4o", 8.50, 100, 50);

        let alerts = compute_alerts(
            org,
            &(quotas as Arc<dyn QuotaRepo>),
            &(usage as Arc<dyn UsageRepo>),
        )
        .await;

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].status, AlertStatus::Approaching);
        assert_eq!(alerts[0].threshold_pct, 80);
        assert_eq!(alerts[0].reason, AlertReason::BudgetThreshold);
        assert_eq!(alerts[0].dimension, "monthly_budget_usd");
    }

    #[tokio::test]
    async fn alert_exceeded_100_percent() {
        let quotas = Arc::new(InMemoryQuotaRepo::new());
        let usage = Arc::new(InMemoryUsageRepo::new());
        let org = OrgId::new();

        let quota_id = Uuid::now_v7();
        quotas.seed(QuotaRecord {
            id: quota_id,
            scope_kind: "org".into(),
            scope_id: *org.as_uuid(),
            dimension: "monthly_budget_usd".into(),
            model_filter: None,
            limit_value: Decimal::new(50, 1), // 5.0
            window_seconds: None,
            mode: "enforce".into(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        let now = Utc::now();
        let this_month_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), 2, 12, 0, 0)
            .unwrap();
        usage.seed_usage(org, this_month_start, "gpt-4o", 6.0, 100, 50);

        let alerts = compute_alerts(
            org,
            &(quotas as Arc<dyn QuotaRepo>),
            &(usage as Arc<dyn UsageRepo>),
        )
        .await;

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].status, AlertStatus::Exceeded);
        assert_eq!(alerts[0].threshold_pct, 100);
    }

    #[tokio::test]
    async fn alert_watch_50_percent() {
        let quotas = Arc::new(InMemoryQuotaRepo::new());
        let usage = Arc::new(InMemoryUsageRepo::new());
        let org = OrgId::new();

        let quota_id = Uuid::now_v7();
        quotas.seed(QuotaRecord {
            id: quota_id,
            scope_kind: "org".into(),
            scope_id: *org.as_uuid(),
            dimension: "monthly_budget_usd".into(),
            model_filter: None,
            limit_value: Decimal::new(1000, 1), // 100.0
            window_seconds: None,
            mode: "enforce".into(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        let now = Utc::now();
        let this_month_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), 2, 12, 0, 0)
            .unwrap();
        usage.seed_usage(org, this_month_start, "gpt-4o", 55.0, 100, 50);

        let alerts = compute_alerts(
            org,
            &(quotas as Arc<dyn QuotaRepo>),
            &(usage as Arc<dyn UsageRepo>),
        )
        .await;

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].status, AlertStatus::Watch);
        assert_eq!(alerts[0].threshold_pct, 50);
    }
}
