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
use gate_billing::{OutboxRepo, PricingRepo, UsageEvent, compute_cost_micros};
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
    let pricing_row = match pricing.find_for(ctx.channel_id, &ctx.model, now).await {
        Ok(Some(p)) => p,
        Ok(None) => {
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

    let cost_micros = compute_cost_micros(&usage, &pricing_row);

    let event = UsageEvent {
        request_id: ctx.request_id,
        idempotency_key: Some(ctx.idempotency_key.clone()),
        api_key_id: ctx.api_key_id,
        project_id: ctx.project_id,
        org_id: ctx.org_id,
        channel_id: ctx.channel_id,
        model: ctx.model,
        prompt_tokens: usage.prompt_tokens as i32,
        completion_tokens: usage.completion_tokens as i32,
        cached_tokens: usage.cached_tokens as i32,
        cost_micros,
        occurred_at: now,
        status,
    };

    if let Err(e) = outbox.enqueue(&event).await {
        crate::metrics::record_billing_settle_failure("outbox_enqueue");
        tracing::warn!(error = %e, "billing outbox enqueue failed");
    }
}
