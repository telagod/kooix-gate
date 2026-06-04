//! /v1/admin/channels/:id/{probe,test,balance} — channel diagnostics
//!
//! 0.4.125：从 admin/mod.rs 物理拆出（5 handler + helper，~480 行）。
//! probe_channel_models 探测上游模型；test_channel 真发 chat 测试；
//! get_channel_balance 拉余额。复用 admin/mod.rs 顶层 ProbeResponse / TestResponse
//! / BalanceResponse 类型与 channel_audit_snapshot / channel_capabilities helper。

#[allow(unused_imports)]
use super::shared::{
    audit_meta, channel_audit_snapshot, channel_capabilities, channel_inflight,
    group_audit_snapshot, is_plugin_provider, key_audit_snapshot, key_fingerprint,
    pricing_rule_audit_snapshot, record_to_summary, require_confirmation, user_audit_snapshot,
    validate_channel_key_alias,
};
use super::*;

/// POST /v1/admin/channels/:id/probe — 调用上游模型端点或 plugin manifest probe 获取可用模型列表。
pub(super) async fn probe_channel_models(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<ProbeResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let channel_id_typed = ChannelId::from(channel_id.0);
    let (url, method, headers, body, success_status, probe_model, max_cost_micros) =
        if is_plugin_provider(&ch.provider_type) {
            let provider = gate_providers::CustomHttpProvider::new_with_secret_slots(
                &ch.base_url,
                resolve_probe_secrets(&app, channel_id_typed, &ch.code).await,
                ch.model_mapping.clone(),
                gate_providers::ProviderOpts {
                    timeout_ms: (ch.timeout_ms as u64).max(5_000),
                },
            )
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
            let probe = provider
                .build_probe_request()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            (
                probe.url,
                probe.method,
                probe.headers,
                probe.body,
                probe.success_status,
                Some(probe.model),
                probe.max_cost_micros,
            )
        } else {
            let api_key = resolve_probe_key(&app, channel_id_typed, &ch.code).await;
            let base = ch.base_url.trim_end_matches('/');
            let (url, headers) = match ch.provider_type.as_str() {
                "anthropic" => {
                    let url = format!("{base}/v1/models");
                    let mut h = reqwest::header::HeaderMap::new();
                    if let Ok(v) = api_key.parse() {
                        h.insert("x-api-key", v);
                    }
                    if let Ok(v) = "2023-06-01".parse() {
                        h.insert("anthropic-version", v);
                    }
                    (url, h)
                }
                _ => {
                    let url = format!("{base}/models");
                    let mut h = reqwest::header::HeaderMap::new();
                    if !api_key.is_empty()
                        && let Ok(v) = format!("Bearer {api_key}").parse()
                    {
                        h.insert("authorization", v);
                    }
                    (url, h)
                }
            };
            (
                url,
                reqwest::Method::GET,
                headers,
                None,
                vec![200],
                None,
                None,
            )
        };

    let resp = client
        .request(method, &url)
        .headers(headers)
        .body(body.unwrap_or_default())
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("probe failed: {e}")))?;

    let status = resp.status().as_u16();
    if !success_status.contains(&status) {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            channel_id = %channel_id,
            status = status,
            body = %body,
            "probe upstream returned error"
        );
        return Err(AppError::Internal(format!(
            "probe failed: upstream returned {status}"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("probe parse error: {e}")))?;

    let models = extract_probe_model_ids(&body);

    Ok(Json(ProbeResponse {
        channel_id: channel_id.to_string(),
        provider_type: ch.provider_type.clone(),
        models,
        probe_model,
        max_cost_micros,
    }))
}

#[derive(Serialize)]
pub(super) struct ProbeResponse {
    channel_id: String,
    provider_type: String,
    models: Vec<String>,
    probe_model: Option<String>,
    max_cost_micros: Option<i64>,
}

fn extract_probe_model_ids(body: &serde_json::Value) -> Vec<String> {
    let mut models: Vec<String> = body
        .get("data")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| {
            m.get("id")
                .or_else(|| m.get("name"))
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect();
    if models.is_empty() {
        models = body
            .get("models")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|m| {
                m.as_str().map(str::to_string).or_else(|| {
                    m.get("id")
                        .or_else(|| m.get("name"))
                        .and_then(|id| id.as_str())
                        .map(str::to_string)
                })
            })
            .collect();
    }
    models.sort();
    models.dedup();
    models
}

/// GET /v1/admin/channels/:id/test — 发送最小 chat completion 验证渠道可用性。
pub(super) async fn test_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
    Query(query): Query<TestChannelQuery>,
) -> AppResult<Json<TestResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;

    let test_model = query.model.clone().unwrap_or_else(|| {
        ch.supported_models
            .first()
            .cloned()
            .unwrap_or_else(|| "gpt-4o-mini".to_string())
    });

    let api_key = resolve_probe_key(&app, ChannelId::from(channel_id.0), &ch.code).await;

    let timeout_ms = (ch.timeout_ms as u64).max(5000);
    let opts = gate_providers::ProviderOpts { timeout_ms };

    let provider: Arc<dyn gate_providers::Provider> = match ch.provider_type.as_str() {
        "anthropic" => Arc::new(
            gate_providers::anthropic::AnthropicProvider::new_with_opts(
                &ch.base_url,
                &api_key,
                opts,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        "azure" => Arc::new(
            gate_providers::azure::AzureProvider::new_with_opts(&ch.base_url, &api_key, None, opts)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        "plugin" | "custom" | "http" | "http_plugin" => Arc::new(
            gate_providers::CustomHttpProvider::new_with_secret_slots(
                &ch.base_url,
                resolve_probe_secrets(&app, ChannelId::from(channel_id.0), &ch.code).await,
                ch.model_mapping.clone(),
                opts,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
        _ => Arc::new(
            gate_providers::openai::OpenAiProvider::new_with_opts(&ch.base_url, &api_key, opts)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        ),
    };

    let start = std::time::Instant::now();
    let req = ChatRequest {
        model: test_model.clone(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Text("Hi".to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        max_tokens: Some(1),
        temperature: Some(0.0),
        stream: false,
        ..Default::default()
    };

    let result = provider.chat(req).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => Ok(Json(TestResponse {
            success: true,
            model: test_model,
            response_time_ms: elapsed_ms,
            message: resp
                .choices
                .first()
                .and_then(|c| c.message.content.as_ref())
                .map(|c| c.to_text()),
            error: None,
        })),
        Err(e) => Ok(Json(TestResponse {
            success: false,
            model: test_model,
            response_time_ms: elapsed_ms,
            message: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Deserialize)]
pub(super) struct TestChannelQuery {
    model: Option<String>,
}

#[derive(Serialize)]
pub(super) struct TestResponse {
    success: bool,
    model: String,
    response_time_ms: u64,
    message: Option<String>,
    error: Option<String>,
}

// ============================================================================
// Channel Balance (P4.5)
// ============================================================================

/// GET /v1/admin/channels/:id/balance — 查询上游账户余额。
/// 目前仅 OpenAI 支持；其他 provider 返回 supported=false。
pub(super) async fn get_channel_balance(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<BalanceResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;
    let api_key = resolve_probe_key(&app, ChannelId::from(channel_id.0), &ch.code).await;

    match ch.provider_type.as_str() {
        "openai" => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let url = format!(
                "{}/dashboard/billing/subscription",
                ch.base_url.trim_end_matches('/')
            );
            let resp = client.get(&url).bearer_auth(&api_key).send().await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let body: serde_json::Value = r
                        .json()
                        .await
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    let hard_limit = body.get("hard_limit_usd").and_then(|v| v.as_f64());
                    let used = body.get("soft_limit_usd").and_then(|v| v.as_f64());

                    if let Some(limit) = hard_limit {
                        update_channel_balance(&app, ChannelId::from(channel_id.0), limit).await;
                    }

                    Ok(Json(BalanceResponse {
                        channel_id: channel_id.to_string(),
                        provider_type: ch.provider_type.clone(),
                        supported: true,
                        balance_usd: hard_limit,
                        used_usd: used,
                        message: None,
                    }))
                }
                Ok(r) => {
                    let status = r.status().as_u16();
                    Ok(Json(BalanceResponse {
                        channel_id: channel_id.to_string(),
                        provider_type: ch.provider_type.clone(),
                        supported: true,
                        balance_usd: None,
                        used_usd: None,
                        message: Some(format!("billing API returned {status}")),
                    }))
                }
                Err(e) => Ok(Json(BalanceResponse {
                    channel_id: channel_id.to_string(),
                    provider_type: ch.provider_type.clone(),
                    supported: true,
                    balance_usd: None,
                    used_usd: None,
                    message: Some(format!("failed to reach billing API: {e}")),
                })),
            }
        }
        _ => Ok(Json(BalanceResponse {
            channel_id: channel_id.to_string(),
            provider_type: ch.provider_type.clone(),
            supported: false,
            balance_usd: None,
            used_usd: None,
            message: Some("balance checking not supported for this provider type".into()),
        })),
    }
}

#[derive(Serialize)]
pub(super) struct BalanceResponse {
    channel_id: String,
    provider_type: String,
    supported: bool,
    balance_usd: Option<f64>,
    used_usd: Option<f64>,
    message: Option<String>,
}

async fn update_channel_balance(app: &AppState, id: ChannelId, balance: f64) {
    if let Some(pool) = app.repos.pool() {
        let _ = sqlx::query(
            "UPDATE channels SET balance = $1, balance_updated_at = NOW() WHERE id = $2",
        )
        .bind(balance)
        .bind(id.as_uuid())
        .execute(pool)
        .await;
    }
}

/// 解析 channel 探测/测试用的 API key。
/// 优先从 DB 加密 key 池取；fallback 到环境变量。
async fn resolve_probe_key(app: &AppState, channel_id: ChannelId, code: &str) -> String {
    if let (Ok(record), Some(crypto)) = (
        app.repos
            .channel_keys
            .find_active_for_channel(channel_id)
            .await,
        app.crypto.as_ref(),
    ) {
        let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
        if let Ok(plaintext) = crypto.open(&record.key_enc, &aad).await
            && let Ok(s) = String::from_utf8(plaintext.to_vec())
        {
            return s;
        }
    }
    // Fallback: 环境变量
    let env_key = format!(
        "KOOIX_CH_{}_KEY",
        code.to_uppercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    std::env::var(&env_key)
        .or_else(|_| std::env::var("KOOIX_API_KEY"))
        .unwrap_or_default()
}

async fn resolve_probe_secrets(
    app: &AppState,
    channel_id: ChannelId,
    code: &str,
) -> HashMap<String, String> {
    let mut secrets = gate_providers::CustomHttpProvider::env_secret_slots(code);
    let Some(crypto) = app.crypto.as_ref() else {
        return secrets;
    };
    let Ok(records) = app.repos.channel_keys.list_by_channel(channel_id).await else {
        return secrets;
    };

    let mut active: Vec<_> = records
        .into_iter()
        .filter(|record| record.health == "healthy")
        .filter(|record| {
            record
                .cooldown_until
                .is_none_or(|until| until < chrono::Utc::now())
        })
        .collect();
    active.sort_by_key(|record| (-record.weight, record.created_at));

    let mut best_active: Option<String> = None;
    let mut selected_primary: Option<String> = None;
    for record in active {
        let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
        let Ok(plaintext) = crypto.open(&record.key_enc, &aad).await else {
            continue;
        };
        let Ok(secret) = String::from_utf8(plaintext.to_vec()) else {
            continue;
        };
        best_active.get_or_insert_with(|| secret.clone());
        let slot = record
            .label
            .as_deref()
            .map(normalize_probe_secret_slot)
            .unwrap_or_else(|| "primary".to_string());
        secrets
            .entry(slot.clone())
            .or_insert_with(|| secret.clone());
        if slot == "primary" && selected_primary.is_none() {
            selected_primary = Some(secret);
        }
    }

    if let Some(primary) = selected_primary.or(best_active)
        && !primary.is_empty()
    {
        secrets.insert("primary".to_string(), primary);
    }
    secrets
}

pub(crate) fn normalize_probe_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}
