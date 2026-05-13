//! ModelPricing 查询 + cost_micros 计算。
//!
//! 表 `model_pricing`：
//!   id UUID, channel_id UUID NULL, model TEXT,
//!   input_per_million NUMERIC, output_per_million NUMERIC,
//!   cached_input_per_million NUMERIC NULL,
//!   effective_from TIMESTAMPTZ, effective_until TIMESTAMPTZ NULL,
//!   metadata JSONB, created_at, updated_at
//!
//! 查询语义（`find_for(channel_id, model, at)`）：
//! 1. 优先匹配 `channel_id = $channel_id`（参数为 Some 时），命中即返回
//! 2. 否则取 `channel_id IS NULL` 的全局默认
//! 3. 时间窗口：`effective_from <= at AND (effective_until IS NULL OR at < effective_until)`
//! 4. 同一区间允许多条历史，取 `effective_from DESC` 第一条
//!
//! `compute_cost_micros`：
//!   prompt_usd     = prompt_tokens     * input_per_million  / 1_000_000
//!   completion_usd = completion_tokens * output_per_million / 1_000_000
//!   total_usd → micros (i64, saturating)

use crate::BillingResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_providers::Usage;
use sqlx::types::Decimal;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

/// 一条 pricing 快照。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    pub channel_id: Option<Uuid>,
    pub model: String,
    /// USD per 1M input tokens
    pub input_per_million: f64,
    /// USD per 1M output tokens
    pub output_per_million: f64,
    /// USD per 1M cached input tokens（None = 不打折）
    pub cached_input_per_million: Option<f64>,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait PricingRepo: Send + Sync + 'static {
    /// 查 `(channel_id, model)` 在 `at` 时刻的有效定价。
    ///
    /// channel_id = Some → 先查精确匹配，无果再回退 NULL 全局默认。
    /// channel_id = None → 直接查全局默认。
    async fn find_for(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Option<ModelPricing>>;
}

// ============================================================================
// PgPricingRepo
// ============================================================================

pub struct PgPricingRepo {
    pool: PgPool,
}

impl PgPricingRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_pricing(row: &sqlx::postgres::PgRow) -> BillingResult<ModelPricing> {
    let channel_id: Option<Uuid> = row.try_get("channel_id")?;
    let input_dec: Decimal = row.try_get("input_per_million")?;
    let output_dec: Decimal = row.try_get("output_per_million")?;
    let cached_dec: Option<Decimal> = row.try_get("cached_input_per_million")?;
    Ok(ModelPricing {
        channel_id,
        model: row.try_get("model")?,
        input_per_million: decimal_to_f64(input_dec),
        output_per_million: decimal_to_f64(output_dec),
        cached_input_per_million: cached_dec.map(decimal_to_f64),
        effective_from: row.try_get("effective_from")?,
        effective_until: row.try_get("effective_until")?,
    })
}

fn decimal_to_f64(d: Decimal) -> f64 {
    // Decimal → f64 — 价格区间在 [0, 数百] USD/M tokens，f64 精度足够
    use std::str::FromStr;
    f64::from_str(&d.to_string()).unwrap_or(0.0)
}

#[async_trait]
impl PricingRepo for PgPricingRepo {
    async fn find_for(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Option<ModelPricing>> {
        // 第一优先：精确 channel_id 匹配
        if let Some(cid) = channel_id {
            let row = sqlx::query(
                "SELECT channel_id, model, input_per_million, output_per_million, \
                        cached_input_per_million, effective_from, effective_until \
                 FROM model_pricing \
                 WHERE channel_id = $1 \
                   AND model = $2 \
                   AND effective_from <= $3 \
                   AND (effective_until IS NULL OR $3 < effective_until) \
                 ORDER BY effective_from DESC \
                 LIMIT 1",
            )
            .bind(cid)
            .bind(model)
            .bind(at)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(r) = row {
                return Ok(Some(row_to_pricing(&r)?));
            }
        }

        // 第二优先：channel_id IS NULL 的全局默认
        let row = sqlx::query(
            "SELECT channel_id, model, input_per_million, output_per_million, \
                    cached_input_per_million, effective_from, effective_until \
             FROM model_pricing \
             WHERE channel_id IS NULL \
               AND model = $1 \
               AND effective_from <= $2 \
               AND (effective_until IS NULL OR $2 < effective_until) \
             ORDER BY effective_from DESC \
             LIMIT 1",
        )
        .bind(model)
        .bind(at)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| row_to_pricing(&r)).transpose()?)
    }
}

// ============================================================================
// InMemoryPricingRepo（测试 / dev）
// ============================================================================

#[derive(Default)]
pub struct InMemoryPricingRepo {
    inner: RwLock<Vec<ModelPricing>>,
    /// 调试用：按 (channel_id, model) 查 hit miss 计数（暂未暴露）
    #[allow(dead_code)]
    counters: RwLock<HashMap<String, u64>>,
}

impl InMemoryPricingRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 往内存里塞一条 pricing。
    pub fn seed(&self, pricing: ModelPricing) {
        self.inner.write().unwrap().push(pricing);
    }

    /// 便捷：seed 一条永久有效的全局默认 pricing。
    pub fn seed_global(
        &self,
        model: impl Into<String>,
        input_per_million: f64,
        output_per_million: f64,
    ) {
        self.seed(ModelPricing {
            channel_id: None,
            model: model.into(),
            input_per_million,
            output_per_million,
            cached_input_per_million: None,
            effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            effective_until: None,
        });
    }
}

#[async_trait]
impl PricingRepo for InMemoryPricingRepo {
    async fn find_for(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Option<ModelPricing>> {
        let inner = self.inner.read().unwrap();

        let in_window = |p: &ModelPricing| -> bool {
            p.effective_from <= at && p.effective_until.map(|until| at < until).unwrap_or(true)
        };

        // 第一优先：精确 channel_id 匹配
        if let Some(cid) = channel_id {
            let hit = inner
                .iter()
                .filter(|p| p.channel_id == Some(cid) && p.model == model && in_window(p))
                .max_by_key(|p| p.effective_from);
            if let Some(p) = hit {
                return Ok(Some(p.clone()));
            }
        }

        // 第二优先：channel_id IS NULL 的全局默认
        let hit = inner
            .iter()
            .filter(|p| p.channel_id.is_none() && p.model == model && in_window(p))
            .max_by_key(|p| p.effective_from);
        Ok(hit.cloned())
    }
}

// ============================================================================
// 计费计算
// ============================================================================

/// 把 (Usage, ModelPricing) 折算成 micro-USD（1 USD = 1_000_000）。
///
/// 计算：
///   prompt_usd     = prompt_tokens     * input_per_million  / 1_000_000
///   completion_usd = completion_tokens * output_per_million / 1_000_000
///   total_usd      = prompt_usd + completion_usd
///   micros         = total_usd * 1_000_000  (i64, saturating)
///
/// 示例：gpt-4o-mini 0.15/M in, 0.60/M out, 1000 in + 500 out
///   prompt_usd  = 1000 * 0.15 / 1_000_000 = 0.00015
///   output_usd  =  500 * 0.60 / 1_000_000 = 0.00030
///   total_usd   = 0.00045
///   micros      = 450
///
/// **注意**：cached_input_per_million 暂未参与计算（C1 阶段未传 cached_tokens）；
/// 后续 Usage 携带 prompt_tokens_details.cached_tokens 时再扣减。
pub fn compute_cost_micros(usage: &Usage, pricing: &ModelPricing) -> i64 {
    let prompt = usage.prompt_tokens as f64;
    let completion = usage.completion_tokens as f64;
    let prompt_usd = prompt * pricing.input_per_million / 1_000_000.0;
    let completion_usd = completion * pricing.output_per_million / 1_000_000.0;
    let total_usd = prompt_usd + completion_usd;
    let micros_f = total_usd * 1_000_000.0;
    // 溢出 / NaN 兜底
    if !micros_f.is_finite() {
        return 0;
    }
    if micros_f >= i64::MAX as f64 {
        return i64::MAX;
    }
    if micros_f <= i64::MIN as f64 {
        return i64::MIN;
    }
    // 舍入到最近整数
    micros_f.round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_basic() {
        let p = ModelPricing {
            channel_id: None,
            model: "gpt-4o-mini".into(),
            input_per_million: 0.15,
            output_per_million: 0.60,
            cached_input_per_million: None,
            effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            effective_until: None,
        };
        let u = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
        };
        // 1000 * 0.15 / 1M = 0.00015 USD = 150 micros
        // 500  * 0.60 / 1M = 0.00030 USD = 300 micros
        // total = 450 micros
        assert_eq!(compute_cost_micros(&u, &p), 450);
    }

    #[test]
    fn compute_zero_usage() {
        let p = ModelPricing {
            channel_id: None,
            model: "gpt-4o-mini".into(),
            input_per_million: 0.15,
            output_per_million: 0.60,
            cached_input_per_million: None,
            effective_from: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            effective_until: None,
        };
        let u = Usage::default();
        assert_eq!(compute_cost_micros(&u, &p), 0);
    }
}
