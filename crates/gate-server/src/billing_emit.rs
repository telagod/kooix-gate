//! billing_emit — 把一次成功（或失败）调用的 usage 写到 billing outbox。
//!
//! 设计：
//! - 仅 ApiKey 主体计费（User 主体直调没有 api_key 归属）
//! - 未挂 outbox / pricing → warn-only，不阻断请求
//! - 找不到 pricing → warn 并跳过 enqueue（避免 cost_micros = 0 污染统计）
//! - enqueue 失败 → warn-only（不影响客户端响应；丢一条算业务可接受）

use chrono::Utc;
use gate_auth::AuthContext;
use gate_auth::context::Subject;
use gate_billing::{CostContext, OutboxRepo, PricingRepo, UsageEvent, compute_cost};
use gate_providers::Usage;
use std::sync::Arc;
use uuid::Uuid;

/// 一次 chat 调用的计费上下文（handler 在认证 / 路由阶段就能拿到）。
#[derive(Clone)]
pub struct BillingCtx {
    pub api_key_id: Uuid,
    pub project_id: Uuid,
    pub org_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub model: String,
    pub request_id: Uuid,
    pub idempotency_key: String,
}

impl BillingCtx {
    /// 从 AuthContext 抽 ApiKey 主体的归属信息；非 ApiKey 主体返回 None。
    ///
    /// User 主体直调（X-Kooix-Project）当前不计费 —— 没有 api_key 归属。
    pub fn from_auth(
        ctx: &AuthContext,
        channel_id: Option<Uuid>,
        group_id: Option<Uuid>,
        model: &str,
        request_id: Uuid,
    ) -> Option<Self> {
        match ctx.subject()? {
            Subject::ApiKey {
                api_key_id,
                project_id,
                org_id,
            } => Some(BillingCtx {
                api_key_id: *api_key_id.as_uuid(),
                project_id: *project_id.as_uuid(),
                org_id: *org_id.as_uuid(),
                channel_id,
                group_id,
                model: model.to_string(),
                request_id,
                idempotency_key: request_id.to_string(),
            }),
            _ => None,
        }
    }
}

/// 异步推一条 UsageEvent 到 outbox。不阻断 caller。
///
/// - outbox / pricing 任一未配置 → 静默 warn-only
/// - pricing 查不到 → warn 并 skip
/// - enqueue 失败 → warn 并 skip
pub async fn emit_usage(
    outbox: Option<Arc<dyn OutboxRepo>>,
    pricing: Option<Arc<dyn PricingRepo>>,
    ctx: BillingCtx,
    usage: Usage,
    status: i16,
) {
    let (Some(outbox), Some(pricing)) = (outbox, pricing) else {
        tracing::debug!(
            api_key_id = %ctx.api_key_id,
            model = %ctx.model,
            "billing skipped: outbox or pricing not configured"
        );
        return;
    };

    let now = Utc::now();
    let request_started_at = ctx
        .request_id
        .get_timestamp()
        .map(std::time::SystemTime::from)
        .map(chrono::DateTime::<Utc>::from)
        .unwrap_or(now);
    crate::metrics::record_billing_outbox_lag_seconds(
        (now - request_started_at).num_milliseconds().max(0) as f64 / 1000.0,
    );
    let pricing_rules = match pricing.find_rules(ctx.channel_id, &ctx.model, now).await {
        Ok(rules) if !rules.is_empty() => rules,
        Ok(_) => {
            crate::metrics::record_billing_settle_failure("pricing_miss");
            tracing::warn!(
                api_key_id = %ctx.api_key_id,
                channel_id = ?ctx.channel_id,
                model = %ctx.model,
                "billing skipped: no pricing found"
            );
            return;
        }
        Err(e) => {
            crate::metrics::record_billing_settle_failure("pricing_lookup");
            tracing::warn!(
                error = %e,
                api_key_id = %ctx.api_key_id,
                model = %ctx.model,
                "billing skipped: pricing lookup failed"
            );
            return;
        }
    };

    let cost_micros = compute_cost(&cost_context_from_usage(&usage), &pricing_rules);

    let event = UsageEvent {
        request_id: ctx.request_id,
        idempotency_key: Some(ctx.idempotency_key.clone()),
        api_key_id: ctx.api_key_id,
        project_id: ctx.project_id,
        org_id: ctx.org_id,
        channel_id: ctx.channel_id,
        group_id: ctx.group_id,
        model: ctx.model,
        prompt_tokens: usage.prompt_tokens as i32,
        completion_tokens: usage.completion_tokens as i32,
        cached_tokens: usage.cached_tokens as i32,
        reasoning_tokens: usage.reasoning_tokens.unwrap_or_default() as i32,
        image_units: usage.image_units.unwrap_or_default() as i32,
        audio_seconds: usage.audio_seconds.unwrap_or_default(),
        raw_usage: usage.raw.clone(),
        cost_micros,
        occurred_at: now,
        status,
    };

    if let Err(e) = outbox.enqueue(&event).await {
        crate::metrics::record_billing_settle_failure("outbox_enqueue");
        tracing::warn!(error = %e, "billing outbox enqueue failed");
    }
}

fn cost_context_from_usage(usage: &Usage) -> CostContext {
    let mut ctx = CostContext::from_tokens(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens,
    );
    ctx.reasoning_tokens = usage.reasoning_tokens.unwrap_or_default();
    ctx.images_generated = usage.image_units.unwrap_or_default();
    ctx.audio_minutes = usage.audio_seconds.unwrap_or_default() / 60.0;
    if let Some(raw) = usage.raw.as_ref().and_then(|v| v.as_object()) {
        ctx.image_quality = raw
            .get("quality")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        ctx.image_size = raw
            .get("size")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        ctx.tts_characters = raw
            .get("tts_characters")
            .and_then(|v| v.as_u64())
            .map(|n| n.min(u32::MAX as u64) as u32)
            .unwrap_or_default();
    }
    ctx
}
