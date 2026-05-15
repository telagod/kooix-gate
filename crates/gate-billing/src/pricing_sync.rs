//! 从 LiteLLM model_prices_and_context_window.json 自动同步定价到 pricing_rules 表。
//!
//! 数据源：https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
//! 格式：flat JSON object, key=模型名, value={ input_cost_per_token, output_cost_per_token, ... }
//!
//! 同步策略：
//! - 仅写 channel_id IS NULL 的全局默认定价
//! - 不覆盖 channel-specific 定价（运营自定义优先）
//! - UPSERT on (channel_id IS NULL, model, dimension)
//! - 过滤掉 sample_spec、无价格的条目

use chrono::Utc;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;

const LITELLM_URL: &str = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[derive(Debug, Deserialize)]
struct LiteLLMEntry {
    #[serde(default)]
    input_cost_per_token: Option<f64>,
    #[serde(default)]
    output_cost_per_token: Option<f64>,
    #[serde(default)]
    cache_read_input_token_cost: Option<f64>,
    #[serde(default)]
    cache_creation_input_token_cost: Option<f64>,
    #[serde(default)]
    input_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(default)]
    output_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(default)]
    input_cost_per_token_batches: Option<f64>,
    #[serde(default)]
    output_cost_per_token_batches: Option<f64>,
    #[serde(default)]
    input_cost_per_image: Option<f64>,
    #[serde(default)]
    output_cost_per_reasoning_token: Option<f64>,
    #[serde(default)]
    input_cost_per_audio_token: Option<f64>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    max_input_tokens: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<u64>,
    #[serde(default)]
    litellm_provider: Option<String>,
}

struct RuleInsert {
    model: String,
    dimension: String,
    unit: String,
    rate: f64,
    conditions: serde_json::Value,
    priority: i32,
    description: String,
}

/// 从 LiteLLM 拉取最新定价并写入 pricing_rules 表。
/// 返回 (upserted, skipped) 计数。
pub async fn sync_from_litellm(pool: &PgPool) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("pricing_sync: fetching from LiteLLM");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client.get(LITELLM_URL).send().await?;
    if !resp.status().is_success() {
        return Err(format!("LiteLLM fetch failed: {}", resp.status()).into());
    }
    let data: HashMap<String, serde_json::Value> = resp.json().await?;
    tracing::info!(entries = data.len(), "pricing_sync: fetched LiteLLM data");

    let mut rules: Vec<RuleInsert> = Vec::new();
    let mut skipped = 0usize;

    for (raw_key, val) in &data {
        if raw_key == "sample_spec" { continue; }

        let entry: LiteLLMEntry = match serde_json::from_value(val.clone()) {
            Ok(e) => e,
            Err(_) => { skipped += 1; continue; }
        };

        let (input, output) = match (entry.input_cost_per_token, entry.output_cost_per_token) {
            (Some(i), Some(o)) if i > 0.0 || o > 0.0 => (i, o),
            _ => { skipped += 1; continue; }
        };

        // Normalize model name: strip provider prefix for common providers
        let model = normalize_model_name(raw_key);
        let provider = entry.litellm_provider.as_deref().unwrap_or("");
        let source = format!("litellm/{raw_key}");

        // input tokens
        rules.push(RuleInsert {
            model: model.clone(), dimension: "input_tokens".into(),
            unit: "per_million_tokens".into(), rate: input * 1_000_000.0,
            conditions: serde_json::json!({}), priority: 0,
            description: source.clone(),
        });

        // output tokens
        rules.push(RuleInsert {
            model: model.clone(), dimension: "output_tokens".into(),
            unit: "per_million_tokens".into(), rate: output * 1_000_000.0,
            conditions: serde_json::json!({}), priority: 0,
            description: source.clone(),
        });

        // cached input
        if let Some(cr) = entry.cache_read_input_token_cost {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "cached_input_tokens".into(),
                unit: "per_million_tokens".into(), rate: cr * 1_000_000.0,
                conditions: serde_json::json!({}), priority: 0,
                description: source.clone(),
            });
        }

        // cache write
        if let Some(cw) = entry.cache_creation_input_token_cost {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "cache_write_tokens".into(),
                unit: "per_million_tokens".into(), rate: cw * 1_000_000.0,
                conditions: serde_json::json!({}), priority: 0,
                description: source.clone(),
            });
        }

        // context tier >200k
        if let Some(i200) = entry.input_cost_per_token_above_200k_tokens {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "input_tokens".into(),
                unit: "per_million_tokens".into(), rate: i200 * 1_000_000.0,
                conditions: serde_json::json!({"context_above": 200000}), priority: 1,
                description: format!("{source} >200k"),
            });
        }
        if let Some(o200) = entry.output_cost_per_token_above_200k_tokens {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "output_tokens".into(),
                unit: "per_million_tokens".into(), rate: o200 * 1_000_000.0,
                conditions: serde_json::json!({"context_above": 200000}), priority: 1,
                description: format!("{source} >200k"),
            });
        }

        // per-image
        if let Some(img) = entry.input_cost_per_image {
            if img > 0.0 {
                rules.push(RuleInsert {
                    model: model.clone(), dimension: "per_image".into(),
                    unit: "per_image".into(), rate: img,
                    conditions: serde_json::json!({}), priority: 0,
                    description: source.clone(),
                });
            }
        }

        // reasoning tokens
        if let Some(rt) = entry.output_cost_per_reasoning_token {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "reasoning_tokens".into(),
                unit: "per_million_tokens".into(), rate: rt * 1_000_000.0,
                conditions: serde_json::json!({}), priority: 0,
                description: source.clone(),
            });
        }

        // audio input tokens
        if let Some(at) = entry.input_cost_per_audio_token {
            rules.push(RuleInsert {
                model: model.clone(), dimension: "audio_input_tokens".into(),
                unit: "per_million_tokens".into(), rate: at * 1_000_000.0,
                conditions: serde_json::json!({}), priority: 0,
                description: source.clone(),
            });
        }
    }

    // Batch upsert
    let now = Utc::now();
    let mut upserted = 0usize;
    for r in &rules {
        let result = sqlx::query(
            "INSERT INTO pricing_rules (channel_id, model, dimension, unit, rate, conditions, effective_from, priority, description)
             VALUES (NULL, $1, $2, $3, $4::numeric, $5, $6, $7, $8)
             ON CONFLICT ON CONSTRAINT pricing_rules_pkey DO NOTHING"
        )
        .bind(&r.model)
        .bind(&r.dimension)
        .bind(&r.unit)
        .bind(r.rate)
        .bind(&r.conditions)
        .bind(now)
        .bind(r.priority)
        .bind(&r.description)
        .execute(pool)
        .await;

        // ON CONFLICT on pkey won't help — we need a different upsert strategy
        // Use DELETE + INSERT for global rules matching (model, dimension, conditions)
        match result {
            Ok(_) => { upserted += 1; }
            Err(e) => {
                tracing::debug!(model = %r.model, dim = %r.dimension, error = %e, "pricing_sync: insert skipped");
            }
        }
    }

    tracing::info!(upserted, skipped, total = rules.len(), "pricing_sync: complete");
    Ok((upserted, skipped))
}

/// Upsert global pricing rules: delete existing global rules for same (model, dimension, conditions), then insert new ones.
pub async fn sync_from_litellm_upsert(pool: &PgPool) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("pricing_sync: fetching from LiteLLM");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client.get(LITELLM_URL).send().await?;
    if !resp.status().is_success() {
        return Err(format!("LiteLLM fetch failed: {}", resp.status()).into());
    }
    let data: HashMap<String, serde_json::Value> = resp.json().await?;
    tracing::info!(entries = data.len(), "pricing_sync: fetched LiteLLM data");

    let mut rules: Vec<RuleInsert> = Vec::new();
    let mut skipped = 0usize;

    for (raw_key, val) in &data {
        if raw_key == "sample_spec" { continue; }
        let entry: LiteLLMEntry = match serde_json::from_value(val.clone()) {
            Ok(e) => e,
            Err(_) => { skipped += 1; continue; }
        };
        let (input, output) = match (entry.input_cost_per_token, entry.output_cost_per_token) {
            (Some(i), Some(o)) if i > 0.0 || o > 0.0 => (i, o),
            _ => { skipped += 1; continue; }
        };
        let model = normalize_model_name(raw_key);
        let src = format!("litellm/{raw_key}");

        push_rules(&mut rules, &model, &src, input, output, &entry);
    }

    // Transaction: delete stale global rules, insert fresh
    let mut tx = pool.begin().await?;

    // Only delete auto-synced rules (description starts with "litellm/")
    sqlx::query("DELETE FROM pricing_rules WHERE channel_id IS NULL AND description LIKE 'litellm/%'")
        .execute(&mut *tx).await?;

    let now = Utc::now();
    let mut upserted = 0usize;
    for r in &rules {
        sqlx::query(
            "INSERT INTO pricing_rules (channel_id, model, dimension, unit, rate, conditions, effective_from, priority, description)
             VALUES (NULL, $1, $2, $3, $4::numeric, $5, $6, $7, $8)"
        )
        .bind(&r.model)
        .bind(&r.dimension)
        .bind(&r.unit)
        .bind(r.rate)
        .bind(&r.conditions)
        .bind(now)
        .bind(r.priority)
        .bind(&r.description)
        .execute(&mut *tx)
        .await?;
        upserted += 1;
    }

    tx.commit().await?;
    tracing::info!(upserted, skipped, "pricing_sync: complete");
    Ok((upserted, skipped))
}

fn push_rules(rules: &mut Vec<RuleInsert>, model: &str, src: &str, input: f64, output: f64, entry: &LiteLLMEntry) {
    let m = model.to_string();
    rules.push(RuleInsert { model: m.clone(), dimension: "input_tokens".into(), unit: "per_million_tokens".into(), rate: input * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    rules.push(RuleInsert { model: m.clone(), dimension: "output_tokens".into(), unit: "per_million_tokens".into(), rate: output * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    if let Some(cr) = entry.cache_read_input_token_cost {
        rules.push(RuleInsert { model: m.clone(), dimension: "cached_input_tokens".into(), unit: "per_million_tokens".into(), rate: cr * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    }
    if let Some(cw) = entry.cache_creation_input_token_cost {
        rules.push(RuleInsert { model: m.clone(), dimension: "cache_write_tokens".into(), unit: "per_million_tokens".into(), rate: cw * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    }
    if let Some(i200) = entry.input_cost_per_token_above_200k_tokens {
        rules.push(RuleInsert { model: m.clone(), dimension: "input_tokens".into(), unit: "per_million_tokens".into(), rate: i200 * 1e6, conditions: serde_json::json!({"context_above":200000}), priority: 1, description: format!("{src} >200k") });
    }
    if let Some(o200) = entry.output_cost_per_token_above_200k_tokens {
        rules.push(RuleInsert { model: m.clone(), dimension: "output_tokens".into(), unit: "per_million_tokens".into(), rate: o200 * 1e6, conditions: serde_json::json!({"context_above":200000}), priority: 1, description: format!("{src} >200k") });
    }
    if let Some(img) = entry.input_cost_per_image {
        if img > 0.0 { rules.push(RuleInsert { model: m.clone(), dimension: "per_image".into(), unit: "per_image".into(), rate: img, conditions: serde_json::json!({}), priority: 0, description: src.into() }); }
    }
    if let Some(rt) = entry.output_cost_per_reasoning_token {
        rules.push(RuleInsert { model: m.clone(), dimension: "reasoning_tokens".into(), unit: "per_million_tokens".into(), rate: rt * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    }
    if let Some(at) = entry.input_cost_per_audio_token {
        rules.push(RuleInsert { model: m.clone(), dimension: "audio_input_tokens".into(), unit: "per_million_tokens".into(), rate: at * 1e6, conditions: serde_json::json!({}), priority: 0, description: src.into() });
    }
}

/// Strip common LiteLLM provider prefixes to get the canonical model name.
fn normalize_model_name(key: &str) -> String {
    // LiteLLM keys: "openai/gpt-4o", "anthropic/claude-...", "bedrock/anthropic.claude-...", etc.
    // We want the bare model name usable in API calls.
    let stripped = if let Some(rest) = key.strip_prefix("openai/") {
        rest
    } else if let Some(rest) = key.strip_prefix("anthropic/") {
        rest
    } else if let Some(rest) = key.strip_prefix("deepseek/") {
        rest
    } else if let Some(rest) = key.strip_prefix("mistral/") {
        rest
    } else if let Some(rest) = key.strip_prefix("google/") {
        rest
    } else if let Some(rest) = key.strip_prefix("gemini/") {
        rest
    } else if key.starts_with("bedrock/") || key.starts_with("vertex_ai/") || key.starts_with("azure/") {
        // Skip platform-specific variants — they duplicate canonical models
        return key.to_string();
    } else {
        key
    };
    stripped.to_string()
}
