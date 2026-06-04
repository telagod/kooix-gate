//! /v1/admin/groups — Channel Group + Binding 管理 + Fallback chain + Canary。
//!
//! 0.4.127：从 admin/mod.rs 物理拆出（10 handler + 多 helper + GroupView/BindingView 等类型，~840 行）。
//! 复用 admin/mod.rs 顶层 require_confirmation / audit_meta helper。

#[allow(unused_imports)]
use super::shared::{
    audit_meta, channel_audit_snapshot, channel_capabilities, channel_inflight,
    group_audit_snapshot, is_plugin_provider, key_audit_snapshot, key_fingerprint,
    pricing_rule_audit_snapshot, record_to_summary, require_confirmation, user_audit_snapshot,
    validate_channel_key_alias,
};
use super::*;

// ============================================================================
// Channel Groups (Admin)
// ============================================================================

#[derive(Clone, Serialize)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub strategy: String,
    pub enabled: bool,
    pub fallback_group_id: Option<String>,
    pub channel_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub strategy: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fallback_group_id: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    pub strategy: Option<String>,
    pub enabled: Option<bool>,
    pub fallback_group_id: Option<Option<String>>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct BindingView {
    pub channel_id: String,
    pub channel_code: String,
    pub channel_name: String,
    pub provider_type: String,
    pub capabilities: ProviderCapabilities,
    pub priority: i32,
    pub weight: i32,
    pub canary_percent_bps: Option<i32>,
    pub model_filter: Vec<String>,
    pub enabled: bool,
    pub channel_status: String,
    pub channel_health: String,
}

#[derive(Serialize)]
pub struct CanaryStatsView {
    pub channel_id: String,
    pub channel_code: String,
    pub channel_name: String,
    pub provider_type: String,
    pub canary_percent_bps: Option<i32>,
    pub is_canary: bool,
    pub requests: i64,
    pub error_rate: f64,
    pub avg_latency_ms: Option<f64>,
    pub avg_cost_micros: Option<f64>,
}

#[derive(Serialize)]
pub struct FallbackChainNodeView {
    pub id: String,
    pub name: String,
    pub strategy: String,
    pub enabled: bool,
    pub channel_count: i64,
    pub requests: i64,
    pub share: f64,
    pub is_fallback: bool,
}

#[derive(Serialize)]
pub struct FallbackStatsView {
    pub window_hours: i64,
    pub total_requests: i64,
    pub primary_requests: i64,
    pub fallback_requests: i64,
    pub fallback_hit_rate: f64,
    pub has_cycle: bool,
    pub cycle_at: Option<String>,
}

const VALID_GROUP_STRATEGIES: [&str; 5] = [
    "priority",
    "weighted_random",
    "round_robin",
    "least_conn",
    "least_latency",
];
const MAX_FALLBACK_DEPTH: usize = 5;
const FALLBACK_STATS_WINDOW_HOURS: i64 = 24;

#[derive(Deserialize)]
pub struct AddBindingRequest {
    pub channel_id: Uuid,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    #[serde(default)]
    pub canary_percent_bps: Option<i32>,
}

pub(super) async fn list_groups(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<GroupView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let groups = app.repos.channel_groups.list_all().await?;
    let mut views = Vec::with_capacity(groups.len());
    for g in groups {
        let bindings = app.repos.channel_groups.list_bindings(g.group_id).await?;
        views.push(GroupView {
            id: g.group_id.to_string(),
            name: g.name,
            description: g.description,
            strategy: g.strategy,
            enabled: g.enabled,
            fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
            channel_count: bindings.len() as i64,
            created_at: g.created_at,
            updated_at: g.updated_at,
        });
    }
    Ok(Json(views))
}

fn validate_group_strategy(strategy: &str) -> AppResult<()> {
    if !VALID_GROUP_STRATEGIES.contains(&strategy) {
        return Err(AppError::BadRequest(format!(
            "strategy must be one of: {VALID_GROUP_STRATEGIES:?}"
        )));
    }
    Ok(())
}

fn parse_channel_group_id(value: &str, field: &str) -> AppResult<ChannelGroupId> {
    value
        .parse::<ChannelGroupId>()
        .map_err(|_| AppError::BadRequest(format!("invalid {field} UUID")))
}

async fn ensure_group_exists(app: &AppState, gid: ChannelGroupId, message: &str) -> AppResult<()> {
    app.repos
        .channel_groups
        .find_by_id(gid)
        .await
        .map(|_| ())
        .map_err(|e| match e {
            gate_storage::DbError::NotFound => AppError::BadRequest(message.into()),
            other => AppError::Db(other),
        })
}

async fn validate_fallback_target(
    app: &AppState,
    gid: ChannelGroupId,
    fallback: Option<ChannelGroupId>,
) -> AppResult<()> {
    validate_fallback_chain(app, Some(gid), fallback).await
}

async fn validate_fallback_chain(
    app: &AppState,
    source: Option<ChannelGroupId>,
    fallback: Option<ChannelGroupId>,
) -> AppResult<()> {
    let Some(fallback) = fallback else {
        return Ok(());
    };

    if Some(fallback) == source {
        return Err(AppError::BadRequest(
            "fallback_group_id cannot point to itself".into(),
        ));
    }
    ensure_group_exists(app, fallback, "fallback group not found").await?;

    let mut visited = source.into_iter().collect::<HashSet<_>>();
    let mut current = fallback;
    let mut depth = 1usize;
    loop {
        if !visited.insert(current) {
            return Err(AppError::BadRequest(format!(
                "fallback cycle detected at {current}"
            )));
        }
        if depth >= MAX_FALLBACK_DEPTH {
            return Err(AppError::BadRequest(format!(
                "fallback chain exceeds max depth {MAX_FALLBACK_DEPTH}"
            )));
        }
        let group = app
            .repos
            .channel_groups
            .find_by_id(current)
            .await
            .map_err(|e| match e {
                gate_storage::DbError::NotFound => {
                    AppError::BadRequest("fallback group not found".into())
                }
                other => AppError::Db(other),
            })?;
        match group.fallback_group_id {
            Some(next) => {
                current = next;
                depth += 1;
            }
            None => return Ok(()),
        }
    }
}

async fn build_fallback_chain_records(
    app: &AppState,
    root: gate_storage::ChannelGroupRecord,
) -> AppResult<(
    Vec<gate_storage::ChannelGroupRecord>,
    bool,
    Option<ChannelGroupId>,
)> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = root;
    let mut depth = 0usize;

    loop {
        if !visited.insert(current.group_id) {
            return Ok((chain, true, Some(current.group_id)));
        }
        let next = current.fallback_group_id;
        chain.push(current);
        let Some(next_id) = next else {
            return Ok((chain, false, None));
        };
        if depth >= MAX_FALLBACK_DEPTH {
            return Ok((chain, true, Some(next_id)));
        }
        current = match app.repos.channel_groups.find_by_id(next_id).await {
            Ok(group) => group,
            Err(gate_storage::DbError::NotFound) => return Ok((chain, false, None)),
            Err(e) => return Err(AppError::Db(e)),
        };
        depth += 1;
    }
}

async fn fallback_request_counts(
    app: &AppState,
    group_ids: &[ChannelGroupId],
    window_hours: i64,
) -> AppResult<HashMap<ChannelGroupId, i64>> {
    let Some(pool) = app.repos.pool() else {
        return Ok(HashMap::new());
    };
    if group_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let ids: Vec<Uuid> = group_ids.iter().map(|id| *id.as_uuid()).collect();
    let rows = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT group_id, COUNT(*)::BIGINT AS requests \
         FROM request_events \
         WHERE group_id = ANY($1) \
           AND ts >= NOW() - ($2::BIGINT * INTERVAL '1 hour') \
         GROUP BY group_id",
    )
    .bind(&ids)
    .bind(window_hours)
    .fetch_all(pool)
    .await
    .map_err(gate_storage::DbError::from)?;

    Ok(rows
        .into_iter()
        .map(|(id, count)| (ChannelGroupId::from(id), count))
        .collect())
}

async fn canary_stats_for_bindings(
    app: &AppState,
    group_id: ChannelGroupId,
    bindings: &[BindingView],
    window_hours: i64,
) -> AppResult<Vec<CanaryStatsView>> {
    let mut metrics: HashMap<Uuid, (i64, i64, Option<f64>, Option<f64>)> = HashMap::new();
    if let Some(pool) = app.repos.pool()
        && !bindings.is_empty()
    {
        let channel_ids: Vec<Uuid> = bindings
            .iter()
            .filter_map(|binding| binding.channel_id.parse::<ChannelId>().ok())
            .map(|id| *id.as_uuid())
            .collect();
        if !channel_ids.is_empty() {
            let rows = sqlx::query(
                "SELECT channel_id, \
                        COUNT(*)::BIGINT AS requests, \
                        COUNT(*) FILTER (WHERE status >= 400 OR error_code IS NOT NULL)::BIGINT AS errors, \
                        AVG(latency_ms)::float8 AS avg_latency_ms, \
                        AVG(cost_micros)::float8 AS avg_cost_micros \
                 FROM request_events \
                 WHERE group_id = $1 \
                   AND channel_id = ANY($2) \
                   AND ts >= NOW() - ($3::BIGINT * INTERVAL '1 hour') \
                 GROUP BY channel_id",
            )
            .bind(group_id.as_uuid())
            .bind(&channel_ids)
            .bind(window_hours)
            .fetch_all(pool)
            .await
            .map_err(gate_storage::DbError::from)?;

            for row in rows {
                let channel_id: Uuid = row
                    .try_get("channel_id")
                    .map_err(gate_storage::DbError::from)?;
                let requests: i64 = row
                    .try_get("requests")
                    .map_err(gate_storage::DbError::from)?;
                let errors: i64 = row.try_get("errors").map_err(gate_storage::DbError::from)?;
                let avg_latency_ms: Option<f64> = row.try_get("avg_latency_ms").unwrap_or(None);
                let avg_cost_micros: Option<f64> = row.try_get("avg_cost_micros").unwrap_or(None);
                metrics.insert(
                    channel_id,
                    (requests, errors, avg_latency_ms, avg_cost_micros),
                );
            }
        }
    }

    Ok(bindings
        .iter()
        .filter_map(|binding| {
            let channel_id = binding.channel_id.parse::<ChannelId>().ok()?;
            let (requests, errors, avg_latency_ms, avg_cost_micros) = metrics
                .get(channel_id.as_uuid())
                .copied()
                .unwrap_or((0, 0, None, None));
            Some(CanaryStatsView {
                channel_id: binding.channel_id.clone(),
                channel_code: binding.channel_code.clone(),
                channel_name: binding.channel_name.clone(),
                provider_type: binding.provider_type.clone(),
                canary_percent_bps: binding.canary_percent_bps,
                is_canary: binding.canary_percent_bps.is_some(),
                requests,
                error_rate: if requests > 0 {
                    errors as f64 / requests as f64
                } else {
                    0.0
                },
                avg_latency_ms,
                avg_cost_micros,
            })
        })
        .collect())
}

async fn group_channel_count(app: &AppState, gid: ChannelGroupId) -> AppResult<i64> {
    Ok(app.repos.channel_groups.list_bindings(gid).await?.len() as i64)
}

fn validate_canary_percent_bps(canary: Option<i32>) -> AppResult<()> {
    if let Some(bps) = canary
        && !(100..=500).contains(&bps)
    {
        return Err(AppError::BadRequest(
            "canary_percent_bps must be between 100 and 500 (1%-5%), or null".into(),
        ));
    }
    Ok(())
}

pub(super) async fn create_group(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    validate_group_strategy(&req.strategy)?;
    let requested_fallback = req
        .fallback_group_id
        .as_deref()
        .map(|id| parse_channel_group_id(id, "fallback_group_id"))
        .transpose()?;
    validate_fallback_chain(&app, None, requested_fallback).await?;

    let mut g = app
        .repos
        .channel_groups
        .create(&req.name, &req.strategy)
        .await?;
    if req.description.is_some() || requested_fallback.is_some() {
        g = app
            .repos
            .channel_groups
            .update(
                g.group_id,
                None,
                None,
                None,
                if requested_fallback.is_some() {
                    Some(requested_fallback)
                } else {
                    None
                },
                req.description.as_deref(),
            )
            .await?;
    }
    app.audit.emit(
        &ctx,
        "channel_group.create",
        "channel_group",
        Some(*g.group_id.as_uuid()),
        None,
    );

    Ok(Json(GroupView {
        id: g.group_id.to_string(),
        name: g.name,
        description: g.description,
        strategy: g.strategy,
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: 0,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }))
}

pub(super) async fn update_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    request_id: Option<Extension<KooixRequestId>>,
    Json(req): Json<UpdateGroupRequest>,
) -> AppResult<Json<GroupView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    if let Some(ref s) = req.strategy {
        validate_group_strategy(s)?;
    }

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let before = app.repos.channel_groups.find_by_id(gid).await?;
    let before_count = group_channel_count(&app, gid).await?;
    if before.enabled && req.enabled == Some(false) {
        require_confirmation(&headers, format!("disable:{}", before.name))?;
    }

    // Parse fallback_group_id: Option<Option<String>> -> Option<Option<ChannelGroupId>>
    let fallback: Option<Option<ChannelGroupId>> = match req.fallback_group_id {
        None => None,             // don't change
        Some(None) => Some(None), // clear
        Some(Some(ref s)) => {
            let fb = parse_channel_group_id(s, "fallback_group_id")?;
            Some(Some(fb))
        }
    };
    if fallback.is_some() {
        validate_fallback_target(&app, gid, fallback.flatten()).await?;
    }

    let g = app
        .repos
        .channel_groups
        .update(
            gid,
            req.name.as_deref(),
            req.strategy.as_deref(),
            req.enabled,
            fallback,
            req.description.as_deref(),
        )
        .await?;
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;

    app.audit.emit_change(AuditChange {
        ctx: &ctx,
        meta: audit_meta(request_id, &headers),
        action: "channel_group.update",
        resource_kind: "channel_group",
        resource_id: Some(*id),
        before: Some(group_audit_snapshot(&before, before_count)),
        after: Some(group_audit_snapshot(&g, bindings.len() as i64)),
    });

    Ok(Json(GroupView {
        id: g.group_id.to_string(),
        name: g.name,
        description: g.description,
        strategy: g.strategy,
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: bindings.len() as i64,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }))
}

pub(super) async fn delete_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    app.repos.channel_groups.delete(gid).await?;
    app.audit.emit(
        &ctx,
        "channel_group.delete",
        "channel_group",
        Some(*id),
        None,
    );

    Ok(Json(serde_json::json!({"deleted": true})))
}

pub(super) async fn list_group_bindings(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<BindingView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;
    Ok(Json(bindings.into_iter().map(binding_to_view).collect()))
}

fn binding_to_view(b: gate_storage::ChannelBinding) -> BindingView {
    let capabilities = channel_capabilities(&b.channel);
    BindingView {
        channel_id: b.channel.channel_id.to_string(),
        channel_code: b.channel.code,
        channel_name: b.channel.name,
        provider_type: b.channel.provider_type,
        capabilities,
        priority: b.priority,
        weight: b.weight,
        canary_percent_bps: b.canary_percent_bps,
        model_filter: b.model_filter,
        enabled: b.enabled,
        channel_status: b.channel.status,
        channel_health: b.channel.health,
    }
}

pub(super) async fn add_group_binding(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<AddBindingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(req.channel_id);
    validate_canary_percent_bps(req.canary_percent_bps)?;
    app.repos
        .channel_groups
        .add_binding(
            gid,
            cid,
            req.priority.unwrap_or(100),
            req.weight.unwrap_or(1),
            req.canary_percent_bps,
        )
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

pub(super) async fn remove_group_binding(
    State(app): State<AppState>,
    Path((id, channel_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(channel_id.0);
    app.repos.channel_groups.remove_binding(gid, cid).await?;

    Ok(Json(serde_json::json!({"removed": true})))
}

#[derive(Deserialize)]
pub struct UpdateBindingRequest {
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_optional_json_patch")]
    pub canary_percent_bps: Option<serde_json::Value>,
    pub model_filter: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

fn deserialize_optional_json_patch<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

fn parse_canary_percent_bps_patch(
    value: Option<serde_json::Value>,
) -> AppResult<Option<Option<i32>>> {
    match value {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::Number(n)) => {
            let bps = n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| {
                    AppError::BadRequest("canary_percent_bps must be an integer".into())
                })?;
            Ok(Some(Some(bps)))
        }
        Some(_) => Err(AppError::BadRequest(
            "canary_percent_bps must be an integer or null".into(),
        )),
    }
}

pub(super) async fn update_group_binding(
    State(app): State<AppState>,
    Path((id, channel_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateBindingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let cid = gate_core::id::ChannelId::from(channel_id.0);
    let canary_percent_bps = parse_canary_percent_bps_patch(req.canary_percent_bps)?;
    if let Some(canary) = canary_percent_bps {
        validate_canary_percent_bps(canary)?;
    }
    app.repos
        .channel_groups
        .update_binding(
            gid,
            cid,
            UpdateChannelBinding {
                priority: req.priority,
                weight: req.weight,
                canary_percent_bps,
                model_filter: req.model_filter,
                enabled: req.enabled,
            },
        )
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Serialize)]
pub struct GroupDetailView {
    #[serde(flatten)]
    pub group_fields: GroupView,
    pub group: GroupView,
    pub bindings: Vec<BindingView>,
    pub projects_using: Vec<String>,
    pub project_ids: Vec<String>,
    pub fallback_chain: Vec<FallbackChainNodeView>,
    pub fallback_stats: FallbackStatsView,
    pub canary_stats: Vec<CanaryStatsView>,
}

pub(super) async fn get_group_detail(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<GroupDetailView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let gid = gate_core::id::ChannelGroupId::from(id.0);
    let g = app.repos.channel_groups.find_by_id(gid).await?;
    let bindings = app.repos.channel_groups.list_bindings(gid).await?;
    let projects = app
        .repos
        .channel_groups
        .list_projects_using_group(gid)
        .await?;

    let binding_views: Vec<BindingView> = bindings.into_iter().map(binding_to_view).collect();
    let canary_stats =
        canary_stats_for_bindings(&app, gid, &binding_views, FALLBACK_STATS_WINDOW_HOURS).await?;
    let group_view = GroupView {
        id: g.group_id.to_string(),
        name: g.name.clone(),
        description: g.description.clone(),
        strategy: g.strategy.clone(),
        enabled: g.enabled,
        fallback_group_id: g.fallback_group_id.map(|fb| fb.to_string()),
        channel_count: binding_views.len() as i64,
        created_at: g.created_at,
        updated_at: g.updated_at,
    };

    let (chain_records, has_cycle, cycle_at) = build_fallback_chain_records(&app, g).await?;
    let chain_group_ids: Vec<ChannelGroupId> =
        chain_records.iter().map(|group| group.group_id).collect();
    let request_counts =
        fallback_request_counts(&app, &chain_group_ids, FALLBACK_STATS_WINDOW_HOURS).await?;
    let total_requests: i64 = chain_group_ids
        .iter()
        .map(|gid| request_counts.get(gid).copied().unwrap_or_default())
        .sum();
    let primary_requests = request_counts.get(&gid).copied().unwrap_or_default();
    let fallback_requests = total_requests.saturating_sub(primary_requests);
    let fallback_hit_rate = if total_requests > 0 {
        fallback_requests as f64 / total_requests as f64
    } else {
        0.0
    };

    let mut fallback_chain = Vec::with_capacity(chain_records.len());
    for (index, group) in chain_records.into_iter().enumerate() {
        let requests = request_counts
            .get(&group.group_id)
            .copied()
            .unwrap_or_default();
        let share = if total_requests > 0 {
            requests as f64 / total_requests as f64
        } else {
            0.0
        };
        let channel_count = if group.group_id == gid {
            binding_views.len() as i64
        } else {
            group_channel_count(&app, group.group_id).await?
        };
        fallback_chain.push(FallbackChainNodeView {
            id: group.group_id.to_string(),
            name: group.name,
            strategy: group.strategy,
            enabled: group.enabled,
            channel_count,
            requests,
            share,
            is_fallback: index > 0,
        });
    }

    let project_ids: Vec<String> = projects.into_iter().map(|p| p.to_string()).collect();

    Ok(Json(GroupDetailView {
        group_fields: group_view.clone(),
        group: group_view,
        bindings: binding_views,
        projects_using: project_ids.clone(),
        project_ids,
        fallback_chain,
        fallback_stats: FallbackStatsView {
            window_hours: FALLBACK_STATS_WINDOW_HOURS,
            total_requests,
            primary_requests,
            fallback_requests,
            fallback_hit_rate,
            has_cycle,
            cycle_at: cycle_at.map(|id| id.to_string()),
        },
        canary_stats,
    }))
}

#[derive(Deserialize)]
pub struct SetDefaultGroupRequest {
    pub group_id: Option<String>,
}

pub(super) async fn set_project_default_group(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<SetDefaultGroupRequest>,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let project_id = gate_core::id::ProjectId::from(id.0);
    // Validate the project exists
    let _ = app.repos.projects.find_by_id(project_id).await?;

    let group_id = match req.group_id {
        None => None,
        Some(ref s) => {
            let gid_uuid = s
                .parse::<Uuid>()
                .map_err(|_| AppError::BadRequest("invalid group_id UUID".into()))?;
            let gid = gate_core::id::ChannelGroupId::from(gid_uuid);
            // Validate the group exists
            let _ = app.repos.channel_groups.find_by_id(gid).await?;
            Some(gid)
        }
    };

    app.repos
        .channel_groups
        .set_project_default_group(project_id, group_id)
        .await?;

    app.audit.emit(
        &ctx,
        "project.set_default_group",
        "project",
        Some(*id),
        Some(serde_json::json!({"group_id": req.group_id})),
    );

    Ok(Json(serde_json::json!({"ok": true})))
}
