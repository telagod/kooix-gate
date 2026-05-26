//! /v1/admin/pricing-rules — billing pricing rules CRUD
//!
//! 0.4.122：从 admin/mod.rs 物理拆出（原 inline `mod pricing`）。
//! 依赖 admin/mod.rs 顶层的 PricingRulesQuery / PricingRuleRow 类型与
//! audit_meta / require_confirmation / pricing_rule_audit_snapshot helper。

use super::*;
#[allow(unused_imports)]
use super::channels::{require_confirmation, audit_meta, channel_audit_snapshot, key_audit_snapshot, group_audit_snapshot, pricing_rule_audit_snapshot, user_audit_snapshot, channel_capabilities, channel_inflight, is_plugin_provider, key_fingerprint, validate_channel_key_alias, record_to_summary};


fn rule_to_row(r: &gate_billing::PricingRule) -> PricingRuleRow {
    PricingRuleRow {
        id: r.id.to_string(),
        channel_id: r
            .channel_id
            .map(|c| gate_core::id::ChannelId::from(c).to_string()),
        model: r.model.clone(),
        dimension: r.dimension.clone(),
        unit: r.unit.clone(),
        rate: r.rate,
        conditions: r.conditions.clone(),
        effective_from: r.effective_from,
        effective_until: r.effective_until,
        priority: r.priority,
        description: r.description.clone(),
    }
}

pub(super) async fn list_pricing_rules(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<PricingRulesQuery>,
) -> AppResult<Json<Vec<PricingRuleRow>>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
    let rules = pricing
        .list_rules(q.channel_id, q.model.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(rules.iter().map(rule_to_row).collect()))
}

#[derive(Deserialize)]
pub(super) struct UpsertPricingRuleRequest {
    id: Option<Uuid>,
    channel_id: Option<Uuid>,
    model: String,
    dimension: String,
    unit: String,
    rate: f64,
    #[serde(default)]
    conditions: serde_json::Value,
    effective_from: Option<DateTime<Utc>>,
    effective_until: Option<DateTime<Utc>>,
    #[serde(default)]
    priority: i32,
    description: Option<String>,
}

pub(super) async fn upsert_pricing_rule(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpsertPricingRuleRequest>,
) -> AppResult<Json<PricingRuleRow>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
    let rule_id = req.id.unwrap_or_else(Uuid::now_v7);
    require_confirmation(
        &headers,
        format!("pricing:{}:{}", req.model.trim(), req.dimension.trim()),
    )?;
    let before = pricing
        .list_rules(None, None)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .into_iter()
        .find(|r| r.id == rule_id);

    let rule = gate_billing::PricingRule {
        id: rule_id,
        channel_id: req.channel_id,
        model: req.model,
        dimension: req.dimension,
        unit: req.unit,
        rate: req.rate,
        conditions: if req.conditions.is_null() {
            serde_json::json!({})
        } else {
            req.conditions
        },
        effective_from: req.effective_from.unwrap_or_else(Utc::now),
        effective_until: req.effective_until,
        priority: req.priority,
        description: req.description,
    };

    let saved = pricing
        .upsert_rule(&rule)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "pricing_rule.upsert",
        resource_kind: "pricing_rule",
        resource_id: Some(saved.id),
        before: before.as_ref().map(pricing_rule_audit_snapshot),
        after: Some(pricing_rule_audit_snapshot(&saved)),
    });

    Ok(Json(rule_to_row(&saved)))
}

pub(super) async fn delete_pricing_rule(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<FlexUuid>,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
) -> AppResult<Json<serde_json::Value>> {
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);
    let pricing = app
        .pricing
        .as_ref()
        .ok_or_else(|| AppError::Internal("pricing not configured".into()))?;
    let before = pricing
        .list_rules(None, None)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .into_iter()
        .find(|r| r.id == *id)
        .ok_or(AppError::NotFound)?;
    require_confirmation(
        &headers,
        format!("pricing:{}:{}", before.model, before.dimension),
    )?;

    let deleted = pricing
        .delete_rule(*id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !deleted {
        return Err(AppError::NotFound);
    }

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "pricing_rule.delete",
        resource_kind: "pricing_rule",
        resource_id: Some(*id),
        before: Some(pricing_rule_audit_snapshot(&before)),
        after: Some(serde_json::json!({
            "id": id.0.to_string(),
            "deleted": true,
        })),
    });

    Ok(Json(serde_json::json!({ "deleted": true })))
}
