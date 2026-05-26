//! /v1/admin/channels — Channel CRUD, batch ops, drain, keys, stats.
//! /v1/admin/plugin-manifest/{schema,replay} — Plugin manifest tooling.
//!
//! 0.4.129：从 admin/mod.rs 物理拆出（最大块，含 16 handler + 多 helper，~850 行）。
//! 复用 admin/mod.rs 顶层 ChannelSummary 等类型 + audit / require_confirmation helper。

use super::*;

pub(super) async fn plugin_manifest_schema(Authed(ctx): Authed) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);
    Ok(Json(gate_providers::plugin_manifest_schema_json()))
}

pub(super) async fn plugin_manifest_replay(
    Authed(ctx): Authed,
    Json(req): Json<PluginReplayRequest>,
) -> AppResult<Json<PluginReplayResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);
    if req.raw_sse.trim().is_empty() {
        return Err(AppError::BadRequest("raw_sse is required".into()));
    }
    let chunks =
        gate_providers::replay_plugin_sse(req.manifest, &req.base_url, req.raw_sse, &req.model)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(Json(PluginReplayResponse { chunks }))
}

pub(super) async fn list_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(params): Query<ChannelListParams>,
) -> AppResult<Json<PaginatedChannelsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let query = ListChannelsQuery {
        search: params.search,
        provider: params.provider,
        status: params.status,
        health: params.health,
        tag: params.tag,
        page: params.page,
        page_size: params.page_size,
        sort_by: params.sort_by,
        sort_dir: params.sort_dir,
    };

    let result = app.repos.channels.list_admin_paginated(query).await?;
    Ok(Json(PaginatedChannelsResponse {
        data: result.data.into_iter().map(record_to_summary).collect(),
        total: result.total,
        page: result.page,
        page_size: result.page_size,
    }))
}

pub(super) fn record_to_summary(r: gate_storage::ChannelRecord) -> ChannelSummary {
    let capabilities = channel_capabilities(&r);
    ChannelSummary {
        id: r.channel_id.to_string(),
        code: r.code,
        name: r.name,
        provider_type: r.provider_type,
        base_url: r.base_url,
        status: r.status,
        health: r.health,
        supported_models: r.supported_models,
        rpm_limit: r.rpm_limit,
        tpm_limit: r.tpm_limit,
        timeout_ms: r.timeout_ms,
        max_retries: r.max_retries,
        tags: r.tags,
        capabilities,
        model_mapping: r.model_mapping,
        balance: r.balance,
        balance_updated_at: r.balance_updated_at,
        last_error: r.last_error,
        last_error_at: r.last_error_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

pub(super) fn channel_audit_snapshot(r: &gate_storage::ChannelRecord) -> serde_json::Value {
    super::shared::channel_audit_snapshot(r)
}

pub(super) fn key_audit_snapshot(k: &gate_storage::ChannelKeyRecord) -> serde_json::Value {
    super::shared::key_audit_snapshot(k)
}

pub(super) fn group_audit_snapshot(
    g: &gate_storage::ChannelGroupRecord,
    channel_count: i64,
) -> serde_json::Value {
    super::shared::group_audit_snapshot(g, channel_count)
}

pub(super) fn pricing_rule_audit_snapshot(r: &gate_billing::PricingRule) -> serde_json::Value {
    super::shared::pricing_rule_audit_snapshot(r)
}

pub(super) fn user_audit_snapshot(u: &gate_core::identity::User) -> serde_json::Value {
    super::shared::user_audit_snapshot(u)
}

pub(super) fn confirmation_from_headers(headers: &HeaderMap) -> Option<&str> {
    super::shared::confirmation_from_headers(headers)
}

pub(super) fn require_confirmation(headers: &HeaderMap, expected: impl AsRef<str>) -> AppResult<()> {
    super::shared::require_confirmation(headers, expected)
}

pub(super) fn audit_meta(
    request_id: Option<Extension<KooixRequestId>>,
    headers: &HeaderMap,
) -> AuditRequestMeta {
    super::shared::audit_meta(request_id, headers)
}

pub(super) fn channel_capabilities(r: &gate_storage::ChannelRecord) -> ProviderCapabilities {
    if is_plugin_provider(&r.provider_type) {
        return gate_providers::plugin_manifest(r.model_mapping.clone(), &r.base_url)
            .map(|manifest| manifest.capabilities)
            .unwrap_or_else(|_| gate_providers::provider_capabilities(&r.provider_type));
    }
    gate_providers::provider_capabilities(&r.provider_type)
}

pub(super) async fn create_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelCreate, Scope::Platform);

    let code = req.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest("code is required".into()));
    }
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || code.len() > 64
    {
        return Err(AppError::BadRequest(
            "code must be 1-64 chars: [a-zA-Z0-9_-]".into(),
        ));
    }

    let valid_types = [
        "openai",
        "anthropic",
        "gemini",
        "azure",
        "vertex",
        "bedrock",
        "deepseek",
        "ollama",
        "mistral",
        "cohere",
        "groq",
        "together",
        "openrouter",
        "moonshot",
        "zhipu",
        "qwen",
        "yi",
        "plugin",
        "custom",
        "http",
        "http_plugin",
    ];
    if !valid_types.contains(&req.provider_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid provider_type: '{}', must be one of {:?}",
            req.provider_type, valid_types
        )));
    }

    let name = req.name.unwrap_or_else(|| code.clone());
    if is_plugin_provider(&req.provider_type) {
        let mapping = req
            .model_mapping
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        gate_providers::validate_plugin_manifest(mapping, &req.base_url)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }

    let record = app
        .repos
        .channels
        .create(CreateChannel {
            code,
            name,
            provider_type: req.provider_type,
            base_url: req.base_url,
            supported_models: req.supported_models,
            enabled: req.enabled,
            rpm_limit: req.rpm_limit,
            tpm_limit: req.tpm_limit,
            timeout_ms: req.timeout_ms,
            max_retries: req.max_retries,
            tags: req.tags,
            model_mapping: req.model_mapping,
        })
        .await?;

    app.audit.emit(
        &ctx,
        "channel.create",
        "channel",
        Some(*record.channel_id.as_uuid()),
        Some(serde_json::json!({"code": &record.code})),
    );

    Ok(Json(record_to_summary(record)))
}

pub(super) fn channel_inflight(app: &AppState, channel_id: ChannelId) -> i64 {
    app.provider_router
        .as_ref()
        .map(|router| router.inflight_tracker().current(channel_id))
        .unwrap_or(0)
}

pub(super) async fn drain_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let record = app
        .repos
        .channels
        .set_status(channel_id, ChannelStatus::Draining)
        .await?;
    let inflight = channel_inflight(&app, channel_id);

    app.audit.emit(
        &ctx,
        "channel.drain",
        "channel",
        Some(*id),
        Some(serde_json::json!({"inflight": inflight})),
    );

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: inflight <= 0,
    }))
}

pub(super) async fn get_channel_drain_status(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let record = app.repos.channels.find_by_id(channel_id).await?;
    let inflight = channel_inflight(&app, channel_id);

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: inflight <= 0,
    }))
}

pub(super) async fn disable_channel_when_idle(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelDrainResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let current = app.repos.channels.find_by_id(channel_id).await?;
    let inflight = channel_inflight(&app, channel_id);
    if inflight > 0 {
        return Err(AppError::BadRequest(format!(
            "channel still has {inflight} inflight request(s)"
        )));
    }

    let record = app
        .repos
        .channels
        .set_status(channel_id, ChannelStatus::Disabled)
        .await?;

    app.audit.emit(
        &ctx,
        "channel.disable_when_idle",
        "channel",
        Some(*id),
        Some(serde_json::json!({"previous_status": current.status, "inflight": inflight})),
    );

    Ok(Json(ChannelDrainResponse {
        channel: record_to_summary(record),
        inflight,
        safe_to_disable: true,
    }))
}

pub(super) async fn update_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let before = app.repos.channels.find_by_id(channel_id).await?;
    if (req.model_mapping.is_some() || req.base_url.is_some())
        && is_plugin_provider(&before.provider_type)
    {
        let mapping = req
            .model_mapping
            .clone()
            .unwrap_or_else(|| before.model_mapping.clone());
        let base_url = req.base_url.as_deref().unwrap_or(&before.base_url);
        gate_providers::validate_plugin_manifest(mapping, base_url)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }
    let record = app
        .repos
        .channels
        .update(
            channel_id,
            UpdateChannel {
                name: req.name,
                base_url: req.base_url,
                supported_models: req.supported_models,
                enabled: req.enabled,
                rpm_limit: req.rpm_limit,
                tpm_limit: req.tpm_limit,
                timeout_ms: req.timeout_ms,
                max_retries: req.max_retries,
                tags: req.tags,
                model_mapping: req.model_mapping,
            },
        )
        .await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel.update",
        resource_kind: "channel",
        resource_id: Some(*id),
        before: Some(channel_audit_snapshot(&before)),
        after: Some(channel_audit_snapshot(&record)),
    });

    Ok(Json(record_to_summary(record)))
}

pub(super) fn is_plugin_provider(provider_type: &str) -> bool {
    matches!(provider_type, "plugin" | "custom" | "http" | "http_plugin")
}

pub(super) async fn delete_channel(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let before = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("delete:{}", before.code))?;
    app.repos.channels.soft_delete(channel_id).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel.delete",
        resource_kind: "channel",
        resource_id: Some(*id),
        before: Some(channel_audit_snapshot(&before)),
        after: Some(serde_json::json!({
            "id": before.channel_id.to_string(),
            "code": before.code,
            "deleted": true,
            "status": "disabled",
        })),
    });

    Ok(Json(serde_json::json!({"deleted": true})))
}

pub(super) async fn batch_enable_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_set_enabled(&ids, true).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_enable",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

pub(super) async fn batch_disable_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelUpdate, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_set_enabled(&ids, false).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_disable",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

pub(super) async fn batch_delete_channels(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<BatchChannelRequest>,
) -> AppResult<Json<BatchResult>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelDelete, Scope::Platform);
    let ids: Vec<ChannelId> = req.ids.into_iter().map(ChannelId::from).collect();
    let affected = app.repos.channels.batch_soft_delete(&ids).await?;
    app.audit.emit(
        &ctx,
        "channel.batch_delete",
        "channel",
        None,
        Some(serde_json::json!({"count": affected})),
    );
    Ok(Json(BatchResult { affected }))
}

// ============================================================================
// Channel Keys
// ============================================================================

/// Key 摘要（不含 key_enc）。
#[derive(Serialize)]
pub struct ChannelKeySummary {
    pub id: String,
    pub channel_id: String,
    pub label: Option<String>,
    pub fingerprint: String,
    pub weight: i32,
    pub health: String,
    pub total_requests: i64,
    pub total_errors: i64,
    pub consecutive_errors: i32,
    pub last_error_code: Option<i32>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    /// 明文 API key，服务端加密后存储。
    pub secret: String,
    #[serde(default)]
    pub alias: Option<String>,
}

/// 计算 key fingerprint：SHA-256 前 16 字节 hex。
pub(super) fn key_fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(secret.as_bytes());
    hex::encode(&hash[..16])
}

pub(super) fn validate_channel_key_alias(alias: &str) -> AppResult<()> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Ok(());
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::BadRequest(
            "key alias must use [a-zA-Z0-9_-]".into(),
        ));
    }
    Ok(())
}

pub(super) async fn list_channel_keys(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<ChannelKeySummary>>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let records = app.repos.channel_keys.list_by_channel(channel_id).await?;
    Ok(Json(
        records
            .into_iter()
            .map(|r| ChannelKeySummary {
                id: r.id.to_string(),
                channel_id: r.channel_id.to_string(),
                label: r.label,
                fingerprint: r.key_fingerprint,
                weight: r.weight,
                health: r.health,
                total_requests: r.total_requests,
                total_errors: r.total_errors,
                consecutive_errors: r.consecutive_errors,
                last_error_code: r.last_error_code,
                last_error_at: r.last_error_at,
                cooldown_until: r.cooldown_until,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

pub(super) async fn create_channel_key(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }
    if let Some(alias) = req.alias.as_deref() {
        validate_channel_key_alias(alias)?;
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id.0);
    // 先确认 channel 存在
    let _ = app.repos.channels.find_by_id(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);

    // AAD 用 channel_id：同 channel 的所有 key 共享 AAD context，
    // 防止密文跨 channel 移植，同时避免先有 key_id 再加密的鸡生蛋问题。
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto
        .seal(req.secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("encrypt channel key failed: {e}")))?;

    let key_id = app
        .repos
        .channel_keys
        .create(channel_id, &key_enc, &fingerprint, req.alias.as_deref())
        .await?;
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

    app.audit.emit(
        &ctx,
        "channel_key.create",
        "channel_key",
        Some(*key_id.as_uuid()),
        Some(serde_json::json!({"channel_id": id.to_string()})),
    );

    Ok(Json(ChannelKeySummary {
        id: key_id.to_string(),
        channel_id: channel_id.to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        total_requests: 0,
        total_errors: 0,
        consecutive_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
    }))
}

pub(super) async fn rotate_channel_key(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<CreateKeyRequest>,
) -> AppResult<Json<ChannelKeySummary>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    if req.secret.is_empty() {
        return Err(AppError::BadRequest("secret is required".into()));
    }
    if let Some(alias) = req.alias.as_deref() {
        validate_channel_key_alias(alias)?;
    }

    let crypto = app.crypto.as_ref().ok_or_else(|| {
        AppError::Internal("crypto (EnvelopeKms) not configured; cannot encrypt channel key".into())
    })?;

    let channel_id = ChannelId::from(id.0);
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("rotate:{}", channel.code))?;
    let before_keys = app.repos.channel_keys.list_by_channel(channel_id).await?;

    let fingerprint = key_fingerprint(&req.secret);
    let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
    let key_enc = crypto
        .seal(req.secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("encrypt channel key failed: {e}")))?;

    let key_id = app
        .repos
        .channel_keys
        .rotate(channel_id, &key_enc, &fingerprint, req.alias.as_deref())
        .await?;
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_key.rotate",
        resource_kind: "channel_key",
        resource_id: Some(*key_id.as_uuid()),
        before: Some(serde_json::json!({
            "channel_id": channel_id.to_string(),
            "channel_code": channel.code,
            "keys": before_keys.iter().map(key_audit_snapshot).collect::<Vec<_>>(),
        })),
        after: Some(serde_json::json!({
            "channel_id": channel_id.to_string(),
            "new_key_id": key_id.to_string(),
            "new_fingerprint": fingerprint,
            "alias": req.alias,
        })),
    });

    Ok(Json(ChannelKeySummary {
        id: key_id.to_string(),
        channel_id: channel_id.to_string(),
        label: req.alias,
        fingerprint,
        weight: 1,
        health: "healthy".to_string(),
        total_requests: 0,
        total_errors: 0,
        consecutive_errors: 0,
        last_error_code: None,
        last_error_at: None,
        cooldown_until: None,
        created_at: Utc::now(),
    }))
}

pub(super) async fn revoke_channel_key(
    State(app): State<AppState>,
    Path((id, key_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelKeyManage, Scope::Platform);

    // 验证 channel 存在
    let channel_id = ChannelId::from(id.0);
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    require_confirmation(&headers, format!("revoke:{}", key_id.0))?;
    let before = app
        .repos
        .channel_keys
        .list_by_channel(channel_id)
        .await?
        .into_iter()
        .find(|k| *k.id.as_uuid() == key_id.0)
        .ok_or(AppError::NotFound)?;

    let ck_id = ChannelKeyId::from(key_id.0);
    app.repos.channel_keys.revoke(ck_id).await?;
    if let Some(router) = &app.provider_router {
        router.invalidate_channel_key_cache(channel_id);
    }

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_key.revoke",
        resource_kind: "channel_key",
        resource_id: Some(*key_id),
        before: Some(key_audit_snapshot(&before)),
        after: Some(serde_json::json!({
            "id": key_id.0.to_string(),
            "channel_id": channel_id.to_string(),
            "channel_code": channel.code,
            "revoked": true,
            "health": "disabled",
        })),
    });

    Ok(Json(serde_json::json!({"revoked": true})))
}

// ============================================================================
// Channel Stats
// ============================================================================

#[derive(Serialize)]
pub struct ChannelStatsResponse {
    pub channel: ChannelSummary,
    pub keys_count: i64,
    pub keys_healthy: i64,
    pub total_requests: i64,
    pub total_errors: i64,
}

pub(super) async fn get_channel_stats(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<ChannelStatsResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::ChannelRead, Scope::Platform);

    let channel_id = ChannelId::from(id.0);
    let channel = app.repos.channels.find_by_id(channel_id).await?;
    let keys = app.repos.channel_keys.list_by_channel(channel_id).await?;

    let keys_count = keys.len() as i64;
    let keys_healthy = keys.iter().filter(|k| k.health == "healthy").count() as i64;
    let total_requests: i64 = keys.iter().map(|k| k.total_requests).sum();
    let total_errors: i64 = keys.iter().map(|k| k.total_errors).sum();

    Ok(Json(ChannelStatsResponse {
        channel: record_to_summary(channel),
        keys_count,
        keys_healthy,
        total_requests,
        total_errors,
    }))
}
