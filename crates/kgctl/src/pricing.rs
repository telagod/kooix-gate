//! `kgctl seed-pricing` — 写入主流模型默认定价（USD per 1M tokens）
//!
//! 幂等策略：
//! - `model_pricing` 表只有普通索引（不是唯一约束），所以用 `WHERE NOT EXISTS` 而非
//!   `ON CONFLICT`。语义：channel_id IS NULL（全局默认） + 同 model + effective_until IS NULL
//!   已有一条「永久生效」记录就跳过。
//! - 价格取 2025-Q1 官方公开报价；未来调价请走新一条 `effective_from=NOW()` 记录、
//!   把旧的 `effective_until` 闭区间。

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use std::str::FromStr;
use std::time::Duration;

const ENV_DB: &str = "KOOIX_DATABASE_URL";

/// (model, input_per_million_usd, output_per_million_usd, cached_input_per_million_usd)
///
/// TODO 价格随官方调整，需要时手工更新本表后重跑 seed-pricing 不会动旧数据，
/// 必须先 UPDATE effective_until 才能让新一条生效。
const DEFAULTS: &[(&str, &str, &str, Option<&str>)] = &[
    // OpenAI
    ("gpt-4o-mini", "0.150", "0.600", Some("0.075")),
    ("gpt-4o", "2.500", "10.000", Some("1.250")),
    ("gpt-4-turbo", "10.000", "30.000", None),
    // Anthropic
    ("claude-3-5-sonnet", "3.000", "15.000", Some("0.300")),
    ("claude-3-5-haiku", "0.800", "4.000", Some("0.080")),
];

pub async fn seed() -> Result<()> {
    let url = std::env::var(ENV_DB).with_context(|| format!("环境变量 {ENV_DB} 未设置"))?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| format!("连库失败：{url}"))?;

    let mut inserted = 0u32;
    let mut skipped = 0u32;

    for (model, input, output, cached) in DEFAULTS {
        let inp = Decimal::from_str(input).with_context(|| format!("非法 input 价格 {input}"))?;
        let out =
            Decimal::from_str(output).with_context(|| format!("非法 output 价格 {output}"))?;
        let cached_dec = cached
            .map(|c| Decimal::from_str(c).with_context(|| format!("非法 cached 价格 {c}")))
            .transpose()?;

        // 幂等：channel_id IS NULL + 同 model + 永久生效 已存在则跳过
        let affected = sqlx::query(
            "INSERT INTO model_pricing
                (channel_id, model, input_per_million, output_per_million,
                 cached_input_per_million, effective_from, effective_until)
             SELECT NULL, $1, $2, $3, $4, NOW(), NULL
             WHERE NOT EXISTS (
                 SELECT 1 FROM model_pricing
                 WHERE channel_id IS NULL
                   AND model = $1
                   AND effective_until IS NULL
             )",
        )
        .bind(model)
        .bind(inp)
        .bind(out)
        .bind(cached_dec)
        .execute(&pool)
        .await
        .with_context(|| format!("插入 {model} 定价失败"))?;

        if affected.rows_affected() == 1 {
            inserted += 1;
            println!("  + {model:<22} input ${input}/M  output ${output}/M");
        } else {
            skipped += 1;
            println!("  = {model:<22} 已存在（跳过）");
        }
    }

    println!();
    println!("ok · inserted {inserted}, skipped {skipped}");
    Ok(())
}
