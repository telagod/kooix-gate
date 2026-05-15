//! `kgctl pricing` — pricing_rules 管理
//!
//! 子命令：
//!   kgctl pricing list [--model gpt-4o] [--channel-id UUID]
//!   kgctl pricing set --model gpt-4o --dimension input_tokens --unit per_million --rate 2.5 [--channel-id UUID] [--priority 10]
//!   kgctl pricing delete --id UUID
//!   kgctl pricing seed  (legacy: 写入默认定价到旧 model_pricing 表)

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

const ENV_DB: &str = "KOOIX_DATABASE_URL";

async fn connect_pool() -> Result<sqlx::PgPool> {
    let url = std::env::var(ENV_DB).with_context(|| format!("环境变量 {ENV_DB} 未设置"))?;
    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| format!("连库失败：{url}"))
}

pub async fn list(model: Option<String>, channel_id: Option<String>) -> Result<()> {
    let pool = connect_pool().await?;
    let ch_id = channel_id
        .map(|s| Uuid::parse_str(&s).with_context(|| "invalid channel_id UUID"))
        .transpose()?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        Option<Uuid>,
        String,
        String,
        String,
        Decimal,
        serde_json::Value,
        i32,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, model, dimension, unit, rate, conditions, priority, description
             FROM pricing_rules
             WHERE ($1::uuid IS NULL OR channel_id = $1 OR (channel_id IS NULL AND $1 IS NULL))
               AND ($2::text IS NULL OR model = $2)
             ORDER BY model, dimension, priority DESC",
    )
    .bind(ch_id)
    .bind(model.as_deref())
    .fetch_all(&pool)
    .await?;

    if rows.is_empty() {
        println!("(no pricing rules found)");
        return Ok(());
    }

    println!(
        "{:<36} {:<6} {:<20} {:<16} {:<14} {:>10} {:<4} DESC",
        "ID", "CH", "MODEL", "DIMENSION", "UNIT", "RATE", "PRI"
    );
    println!("{}", "─".repeat(120));
    for (id, ch, model, dim, unit, rate, _cond, pri, desc) in &rows {
        let ch_short = ch
            .map(|c| format!("{}…", &c.to_string()[..8]))
            .unwrap_or_else(|| "global".into());
        let desc_str = desc.as_deref().unwrap_or("");
        println!(
            "{id:<36} {ch_short:<6} {model:<20} {dim:<16} {unit:<14} {rate:>10} {pri:<4} {desc_str}"
        );
    }
    println!("\n{} rules", rows.len());
    Ok(())
}

pub async fn set(
    model: String,
    dimension: String,
    unit: String,
    rate: f64,
    channel_id: Option<String>,
    priority: i32,
    description: Option<String>,
) -> Result<()> {
    let pool = connect_pool().await?;
    let ch_id = channel_id
        .map(|s| Uuid::parse_str(&s).with_context(|| "invalid channel_id UUID"))
        .transpose()?;

    let rate_dec = Decimal::from_str_exact(&format!("{rate:.8}"))
        .with_context(|| format!("invalid rate: {rate}"))?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO pricing_rules (channel_id, model, dimension, unit, rate, priority, description, effective_from)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
         ON CONFLICT (id) DO UPDATE SET
             rate = EXCLUDED.rate,
             priority = EXCLUDED.priority,
             description = EXCLUDED.description,
             updated_at = NOW()
         RETURNING id"
    )
    .bind(ch_id)
    .bind(&model)
    .bind(&dimension)
    .bind(&unit)
    .bind(rate_dec)
    .bind(priority)
    .bind(description.as_deref())
    .fetch_one(&pool)
    .await
    .with_context(|| "upsert pricing rule failed")?;

    let scope = if ch_id.is_some() { "channel" } else { "global" };
    println!("ok · {scope} {model} / {dimension} / {unit} = {rate} (id: {id})");
    Ok(())
}

pub async fn delete(id: String) -> Result<()> {
    let pool = connect_pool().await?;
    let uuid = Uuid::parse_str(&id).with_context(|| "invalid rule ID")?;

    let affected = sqlx::query("DELETE FROM pricing_rules WHERE id = $1")
        .bind(uuid)
        .execute(&pool)
        .await?
        .rows_affected();

    if affected == 0 {
        println!("not found: {id}");
    } else {
        println!("ok · deleted {id}");
    }
    Ok(())
}

// Legacy: seed-pricing (writes to old model_pricing table)
const DEFAULTS: &[(&str, &str, &str, Option<&str>)] = &[
    ("gpt-4o-mini", "0.150", "0.600", Some("0.075")),
    ("gpt-4o", "2.500", "10.000", Some("1.250")),
    ("gpt-4-turbo", "10.000", "30.000", None),
    ("claude-3-5-sonnet", "3.000", "15.000", Some("0.300")),
    ("claude-3-5-haiku", "0.800", "4.000", Some("0.080")),
];

pub async fn seed() -> Result<()> {
    let pool = connect_pool().await?;
    let mut inserted = 0u32;
    let mut skipped = 0u32;

    for (model, input, output, cached) in DEFAULTS {
        let inp = Decimal::from_str(input)?;
        let out = Decimal::from_str(output)?;
        let cached_dec = cached.map(Decimal::from_str).transpose()?;

        let affected = sqlx::query(
            "INSERT INTO model_pricing
                (channel_id, model, input_per_million, output_per_million,
                 cached_input_per_million, effective_from, effective_until)
             SELECT NULL, $1, $2, $3, $4, NOW(), NULL
             WHERE NOT EXISTS (
                 SELECT 1 FROM model_pricing
                 WHERE channel_id IS NULL AND model = $1 AND effective_until IS NULL
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
    println!("\nok · inserted {inserted}, skipped {skipped}");
    Ok(())
}
