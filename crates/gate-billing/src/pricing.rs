//! PricingRule-based 多维度定价引擎。
//!
//! 支持维度：input_tokens, output_tokens, cached_input_tokens, cache_write_tokens,
//! reasoning_tokens, audio_input_tokens, audio_output_tokens, image_input_tokens,
//! image_output_tokens, per_image, per_minute_audio, per_character_tts,
//! per_second_video, per_search, per_request, batch_multiplier, priority_multiplier,
//! region_multiplier.

use crate::BillingResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sqlx::types::Decimal;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingRule {
    pub id: Uuid,
    pub channel_id: Option<Uuid>,
    pub model: String,
    pub dimension: String,
    pub unit: String,
    pub rate: f64,
    pub conditions: serde_json::Value,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub priority: i32,
    pub description: Option<String>,
}

/// Legacy compat wrapper
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    pub channel_id: Option<Uuid>,
    pub model: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cached_input_per_million: Option<f64>,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
}

/// 计费上下文：一次请求的全部用量维度
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostContext {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub reasoning_tokens: u32,
    pub audio_input_tokens: u32,
    pub audio_output_tokens: u32,
    pub image_input_tokens: u32,
    pub image_output_tokens: u32,
    pub images_generated: u32,
    pub audio_minutes: f64,
    pub tts_characters: u32,
    pub video_seconds: f64,
    pub search_count: u32,
    pub is_batch: bool,
    pub context_length: u32,
    pub image_quality: Option<String>,
    pub image_size: Option<String>,
    pub cache_ttl: Option<String>,
    pub region: Option<String>,
    pub deployment_type: Option<String>,
}

impl CostContext {
    pub fn from_tokens(prompt: u32, completion: u32, cached: u32) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            cached_tokens: cached,
            ..Default::default()
        }
    }
}

#[async_trait]
pub trait PricingRepo: Send + Sync + 'static {
    async fn find_rules(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Vec<PricingRule>>;

    async fn find_for(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Option<ModelPricing>> {
        let rules = self.find_rules(channel_id, model, at).await?;
        let input = rules
            .iter()
            .find(|r| r.dimension == "input_tokens")
            .map(|r| r.rate)
            .unwrap_or(0.0);
        let output = rules
            .iter()
            .find(|r| r.dimension == "output_tokens")
            .map(|r| r.rate)
            .unwrap_or(0.0);
        let cached = rules
            .iter()
            .find(|r| r.dimension == "cached_input_tokens")
            .map(|r| r.rate);
        if input == 0.0 && output == 0.0 {
            return Ok(None);
        }
        let first = rules.first().unwrap();
        Ok(Some(ModelPricing {
            channel_id: first.channel_id,
            model: first.model.clone(),
            input_per_million: input,
            output_per_million: output,
            cached_input_per_million: cached,
            effective_from: first.effective_from,
            effective_until: first.effective_until,
        }))
    }

    async fn list_rules(
        &self,
        channel_id: Option<Uuid>,
        model: Option<&str>,
    ) -> BillingResult<Vec<PricingRule>>;
    async fn upsert_rule(&self, rule: &PricingRule) -> BillingResult<PricingRule>;
    async fn delete_rule(&self, id: Uuid) -> BillingResult<bool>;
}

// ─── Pg Implementation ───────────────────────────────────────────────────────

pub struct PgPricingRepo {
    pool: PgPool,
}

impl PgPricingRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    use std::str::FromStr;
    f64::from_str(&d.to_string()).unwrap_or(0.0)
}

fn row_to_rule(row: &sqlx::postgres::PgRow) -> BillingResult<PricingRule> {
    let rate_dec: Decimal = row.try_get("rate")?;
    Ok(PricingRule {
        id: row.try_get("id")?,
        channel_id: row.try_get("channel_id")?,
        model: row.try_get("model")?,
        dimension: row.try_get("dimension")?,
        unit: row.try_get("unit")?,
        rate: decimal_to_f64(rate_dec),
        conditions: row.try_get("conditions")?,
        effective_from: row.try_get("effective_from")?,
        effective_until: row.try_get("effective_until")?,
        priority: row.try_get("priority")?,
        description: row.try_get("description")?,
    })
}

#[async_trait]
impl PricingRepo for PgPricingRepo {
    async fn find_rules(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Vec<PricingRule>> {
        // Channel-specific first, then global fallback, then wildcard
        let rows = sqlx::query(
            "WITH ranked AS (
                SELECT *, ROW_NUMBER() OVER (PARTITION BY dimension ORDER BY
                    CASE WHEN channel_id = $1 THEN 0 WHEN channel_id IS NULL THEN 1 ELSE 2 END,
                    priority DESC, effective_from DESC
                ) AS rn
                FROM pricing_rules
                WHERE (channel_id = $1 OR channel_id IS NULL)
                  AND (model = $2 OR model = '*')
                  AND effective_from <= $3
                  AND (effective_until IS NULL OR $3 < effective_until)
            )
            SELECT id, channel_id, model, dimension, unit, rate, conditions,
                   effective_from, effective_until, priority, description
            FROM ranked WHERE rn = 1",
        )
        .bind(channel_id)
        .bind(model)
        .bind(at)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_rule).collect()
    }

    async fn list_rules(
        &self,
        channel_id: Option<Uuid>,
        model: Option<&str>,
    ) -> BillingResult<Vec<PricingRule>> {
        let rows = sqlx::query(
            "SELECT id, channel_id, model, dimension, unit, rate, conditions,
                    effective_from, effective_until, priority, description
             FROM pricing_rules
             WHERE ($1::uuid IS NULL OR channel_id = $1 OR channel_id IS NULL)
               AND ($2::text IS NULL OR model = $2)
             ORDER BY model, dimension, priority DESC",
        )
        .bind(channel_id)
        .bind(model)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_rule).collect()
    }

    async fn upsert_rule(&self, rule: &PricingRule) -> BillingResult<PricingRule> {
        let row = sqlx::query(
            "INSERT INTO pricing_rules (id, channel_id, model, dimension, unit, rate, conditions, effective_from, effective_until, priority, description)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                channel_id = EXCLUDED.channel_id, model = EXCLUDED.model,
                dimension = EXCLUDED.dimension, unit = EXCLUDED.unit, rate = EXCLUDED.rate,
                conditions = EXCLUDED.conditions, effective_from = EXCLUDED.effective_from,
                effective_until = EXCLUDED.effective_until, priority = EXCLUDED.priority,
                description = EXCLUDED.description
             RETURNING id, channel_id, model, dimension, unit, rate, conditions, effective_from, effective_until, priority, description"
        )
        .bind(rule.id)
        .bind(rule.channel_id)
        .bind(&rule.model)
        .bind(&rule.dimension)
        .bind(&rule.unit)
        .bind(Decimal::from_str_exact(&format!("{:.8}", rule.rate)).unwrap_or_default())
        .bind(&rule.conditions)
        .bind(rule.effective_from)
        .bind(rule.effective_until)
        .bind(rule.priority)
        .bind(&rule.description)
        .fetch_one(&self.pool)
        .await?;
        row_to_rule(&row)
    }

    async fn delete_rule(&self, id: Uuid) -> BillingResult<bool> {
        let result = sqlx::query("DELETE FROM pricing_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ─── InMemory Implementation ─────────────────────────────────────────────────

#[derive(Default)]
pub struct InMemoryPricingRepo {
    inner: RwLock<Vec<PricingRule>>,
}

impl InMemoryPricingRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, rule: PricingRule) {
        self.inner.write().push(rule);
    }

    pub fn seed_legacy(&self, pricing: ModelPricing) {
        let base_from = pricing.effective_from;
        let base_until = pricing.effective_until;
        self.seed(PricingRule {
            id: Uuid::new_v4(),
            channel_id: pricing.channel_id,
            model: pricing.model.clone(),
            dimension: "input_tokens".into(),
            unit: "per_million_tokens".into(),
            rate: pricing.input_per_million,
            conditions: serde_json::json!({}),
            effective_from: base_from,
            effective_until: base_until,
            priority: 0,
            description: None,
        });
        self.seed(PricingRule {
            id: Uuid::new_v4(),
            channel_id: pricing.channel_id,
            model: pricing.model.clone(),
            dimension: "output_tokens".into(),
            unit: "per_million_tokens".into(),
            rate: pricing.output_per_million,
            conditions: serde_json::json!({}),
            effective_from: base_from,
            effective_until: base_until,
            priority: 0,
            description: None,
        });
        if let Some(cached) = pricing.cached_input_per_million {
            self.seed(PricingRule {
                id: Uuid::new_v4(),
                channel_id: pricing.channel_id,
                model: pricing.model.clone(),
                dimension: "cached_input_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: cached,
                conditions: serde_json::json!({}),
                effective_from: base_from,
                effective_until: base_until,
                priority: 0,
                description: None,
            });
        }
    }

    pub fn seed_global(
        &self,
        model: impl Into<String>,
        input_per_million: f64,
        output_per_million: f64,
    ) {
        self.seed_legacy(ModelPricing {
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
    async fn find_rules(
        &self,
        channel_id: Option<Uuid>,
        model: &str,
        at: DateTime<Utc>,
    ) -> BillingResult<Vec<PricingRule>> {
        let inner = self.inner.read();
        let in_window = |r: &PricingRule| -> bool {
            r.effective_from <= at && r.effective_until.map(|u| at < u).unwrap_or(true)
        };
        let matches_model = |r: &PricingRule| -> bool { r.model == model || r.model == "*" };

        let mut by_dim: HashMap<String, PricingRule> = HashMap::new();
        for r in inner.iter().filter(|r| matches_model(r) && in_window(r)) {
            let matches_channel = match (channel_id, r.channel_id) {
                (Some(cid), Some(rid)) => cid == rid,
                (_, None) => true,
                _ => false,
            };
            if !matches_channel {
                continue;
            }
            let existing = by_dim.get(&r.dimension);
            let better = match existing {
                None => true,
                Some(e) => {
                    // channel-specific (Some) beats global (None): higher score = better
                    let e_score = if e.channel_id.is_some() { 1 } else { 0 };
                    let r_score = if r.channel_id.is_some() { 1 } else { 0 };
                    (r_score, r.priority, r.effective_from)
                        > (e_score, e.priority, e.effective_from)
                }
            };
            if better {
                by_dim.insert(r.dimension.clone(), r.clone());
            }
        }
        Ok(by_dim.into_values().collect())
    }

    async fn list_rules(
        &self,
        channel_id: Option<Uuid>,
        model: Option<&str>,
    ) -> BillingResult<Vec<PricingRule>> {
        let inner = self.inner.read();
        Ok(inner
            .iter()
            .filter(|r| {
                (channel_id.is_none() || r.channel_id == channel_id || r.channel_id.is_none())
                    && (model.is_none() || r.model == model.unwrap())
            })
            .cloned()
            .collect())
    }

    async fn upsert_rule(&self, rule: &PricingRule) -> BillingResult<PricingRule> {
        let mut inner = self.inner.write();
        inner.retain(|r| r.id != rule.id);
        inner.push(rule.clone());
        Ok(rule.clone())
    }

    async fn delete_rule(&self, id: Uuid) -> BillingResult<bool> {
        let mut inner = self.inner.write();
        let before = inner.len();
        inner.retain(|r| r.id != id);
        Ok(inner.len() < before)
    }
}

// ─── Cost Computation ────────────────────────────────────────────────────────

/// 多维度计费：把 CostContext 中各维度的用量 × 对应 rule 的 rate 累加。
pub fn compute_cost(ctx: &CostContext, rules: &[PricingRule]) -> i64 {
    let mut total_usd: f64 = 0.0;

    for rule in rules {
        let qty: f64 = match rule.dimension.as_str() {
            "input_tokens" => {
                let uncached = ctx.prompt_tokens.saturating_sub(ctx.cached_tokens);
                uncached as f64
            }
            "output_tokens" => ctx.completion_tokens as f64,
            "cached_input_tokens" => ctx.cached_tokens as f64,
            "cache_write_tokens" if ctx.cache_ttl.is_some() => ctx.cached_tokens as f64,
            "cache_write_tokens" => 0.0,
            "reasoning_tokens" => ctx.reasoning_tokens as f64,
            "audio_input_tokens" => ctx.audio_input_tokens as f64,
            "audio_output_tokens" => ctx.audio_output_tokens as f64,
            "image_input_tokens" => ctx.image_input_tokens as f64,
            "image_output_tokens" => ctx.image_output_tokens as f64,
            "per_image" if conditions_match(&rule.conditions, ctx) => ctx.images_generated as f64,
            "per_image" => 0.0,
            "per_minute_audio" => ctx.audio_minutes,
            "per_character_tts" => ctx.tts_characters as f64,
            "per_second_video" => ctx.video_seconds,
            "per_search" => ctx.search_count as f64,
            "per_request" => 1.0,
            _ => 0.0,
        };

        if qty == 0.0 {
            continue;
        }

        let cost = match rule.unit.as_str() {
            "per_million_tokens" => qty * rule.rate / 1_000_000.0,
            "per_million_characters" => qty * rule.rate / 1_000_000.0,
            "per_image" | "per_minute" | "per_second" | "per_character" | "per_search"
            | "per_request" => qty * rule.rate,
            _ => qty * rule.rate / 1_000_000.0,
        };
        total_usd += cost;
    }

    // Apply multipliers
    for rule in rules {
        match rule.dimension.as_str() {
            "batch_multiplier" if ctx.is_batch => {
                total_usd *= rule.rate;
            }
            "region_multiplier"
                if ctx.region.is_some() && conditions_match(&rule.conditions, ctx) =>
            {
                total_usd *= rule.rate;
            }
            _ => {}
        }
    }

    let micros = total_usd * 1_000_000.0;
    if !micros.is_finite() {
        return 0;
    }
    micros.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

/// Legacy compat
pub fn compute_cost_micros(usage: &gate_providers::Usage, pricing: &ModelPricing) -> i64 {
    let mut ctx = CostContext::from_tokens(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens,
    );
    ctx.reasoning_tokens = usage.reasoning_tokens.unwrap_or_default();
    ctx.images_generated = usage.image_units.unwrap_or_default();
    ctx.audio_minutes = usage.audio_seconds.unwrap_or_default() / 60.0;
    let mut rules = vec![
        PricingRule {
            id: Uuid::nil(),
            channel_id: pricing.channel_id,
            model: pricing.model.clone(),
            dimension: "input_tokens".into(),
            unit: "per_million_tokens".into(),
            rate: pricing.input_per_million,
            conditions: serde_json::json!({}),
            effective_from: pricing.effective_from,
            effective_until: pricing.effective_until,
            priority: 0,
            description: None,
        },
        PricingRule {
            id: Uuid::nil(),
            channel_id: pricing.channel_id,
            model: pricing.model.clone(),
            dimension: "output_tokens".into(),
            unit: "per_million_tokens".into(),
            rate: pricing.output_per_million,
            conditions: serde_json::json!({}),
            effective_from: pricing.effective_from,
            effective_until: pricing.effective_until,
            priority: 0,
            description: None,
        },
    ];
    if let Some(rate) = pricing.cached_input_per_million {
        rules.push(PricingRule {
            id: Uuid::nil(),
            channel_id: pricing.channel_id,
            model: pricing.model.clone(),
            dimension: "cached_input_tokens".into(),
            unit: "per_million_tokens".into(),
            rate,
            conditions: serde_json::json!({}),
            effective_from: pricing.effective_from,
            effective_until: pricing.effective_until,
            priority: 0,
            description: None,
        });
    }
    compute_cost(&ctx, &rules)
}

fn conditions_match(conditions: &serde_json::Value, ctx: &CostContext) -> bool {
    let obj = match conditions.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => return true,
    };
    for (key, val) in obj {
        let matches = match key.as_str() {
            "quality" => ctx.image_quality.as_deref() == val.as_str(),
            "size" => ctx.image_size.as_deref() == val.as_str(),
            "cache_ttl" => ctx.cache_ttl.as_deref() == val.as_str(),
            "region" => ctx.region.as_deref() == val.as_str(),
            "deployment_type" => ctx.deployment_type.as_deref() == val.as_str(),
            "batch" => ctx.is_batch == val.as_bool().unwrap_or(false),
            "context_above" => ctx.context_length > val.as_u64().unwrap_or(0) as u32,
            _ => true,
        };
        if !matches {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_basic_tokens() {
        let rules = vec![
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o-mini".into(),
                dimension: "input_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 0.15,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o-mini".into(),
                dimension: "output_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 0.60,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
        ];
        let ctx = CostContext::from_tokens(1000, 500, 0);
        assert_eq!(compute_cost(&ctx, &rules), 450);
    }

    #[test]
    fn compute_with_cache() {
        let rules = vec![
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "input_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 2.50,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "output_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 10.00,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "cached_input_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 1.25,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
        ];
        // 1000 prompt, 200 cached, 500 output
        // uncached input: 800 * 2.50 / 1M = 0.002 USD = 2000 micros
        // cached: 200 * 1.25 / 1M = 0.00025 = 250 micros
        // output: 500 * 10.0 / 1M = 0.005 = 5000 micros
        // total = 7250 micros
        let ctx = CostContext::from_tokens(1000, 500, 200);
        assert_eq!(compute_cost(&ctx, &rules), 7250);
    }

    #[test]
    fn compute_per_image() {
        let rules = vec![PricingRule {
            id: Uuid::nil(),
            channel_id: None,
            model: "dall-e-3".into(),
            dimension: "per_image".into(),
            unit: "per_image".into(),
            rate: 0.08,
            conditions: serde_json::json!({"quality":"hd","size":"1024x1024"}),
            effective_from: Utc::now(),
            effective_until: None,
            priority: 0,
            description: None,
        }];
        let ctx = CostContext {
            images_generated: 2,
            image_quality: Some("hd".into()),
            image_size: Some("1024x1024".into()),
            ..Default::default()
        };
        // 2 * $0.08 = $0.16 = 160000 micros
        assert_eq!(compute_cost(&ctx, &rules), 160000);
    }

    #[test]
    fn compute_batch_multiplier() {
        let rules = vec![
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "input_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 2.50,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "output_tokens".into(),
                unit: "per_million_tokens".into(),
                rate: 10.0,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
            PricingRule {
                id: Uuid::nil(),
                channel_id: None,
                model: "gpt-4o".into(),
                dimension: "batch_multiplier".into(),
                unit: "multiplier".into(),
                rate: 0.5,
                conditions: serde_json::json!({}),
                effective_from: Utc::now(),
                effective_until: None,
                priority: 0,
                description: None,
            },
        ];
        let ctx = CostContext {
            prompt_tokens: 1000,
            completion_tokens: 500,
            is_batch: true,
            ..Default::default()
        };
        // normal: 1000*2.5/1M + 500*10/1M = 0.0025 + 0.005 = 0.0075 = 7500 micros
        // batch 0.5x: 3750 micros
        assert_eq!(compute_cost(&ctx, &rules), 3750);
    }
}
