//! /v1/admin/channels/:id/health-score + /v1/admin/groups/:id/health-weights
//! + /v1/admin/health-dashboard
//!
//! M5.1 N1.6：ADR-0007 Channel Health Score 的管理面。
//!
//! 6 个 endpoint：
//! - `GET    /channels/:id/health-score`           — 单 channel 评分详情
//! - `POST   /channels/:id/health/unban`           — 人工解封（Banned → Cooldown）
//! - `POST   /channels/:id/health/cooldown`        — 强制 cooldown（管理员）
//! - `GET    /groups/:id/health-weights`           — group 权重覆写
//! - `PUT    /groups/:id/health-weights`           — 更新 group 权重 + use_health_score
//! - `GET    /health-dashboard`                    — 全局健康仪表盘聚合视图

use super::*;
use chrono::Duration as ChronoDuration;
use gate_providers::health_score::Weights;
use gate_storage::{ChannelHealthScore, HealthState, ScoreUpdate};

// ============================================================================
// 类型
// ============================================================================

#[derive(Serialize)]
pub struct ChannelHealthResponse {
    pub channel_id: String,
    pub channel_code: String,
    pub state: String,
    pub score: f64,
    pub success_rate: f64,
    pub latency_p99_ms: i32,
    pub banned_signal: f64,
    pub quota_remaining_norm: f64,
    pub consecutive_failures: i32,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub banned_reason: Option<String>,
    pub last_transition_at: DateTime<Utc>,
    pub window_total: i32,
    pub window_success: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct ForceCooldownRequest {
    /// 秒数；0 或 None 用默认 base_cooldown=30s。
    #[serde(default)]
    pub cooldown_secs: Option<i64>,
    /// 可选 reason 字符串记到 banned_reason 字段（即使非 Banned 状态也保留 trace）。
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct GroupHealthConfig {
    pub group_id: String,
    pub group_name: String,
    pub use_health_score: bool,
    /// `null` = 用全局默认权重。
    pub weights: Option<Weights>,
}

#[derive(Deserialize)]
pub struct UpdateGroupHealthConfig {
    /// 切换 opt-in 开关；`null` 保持当前值。
    #[serde(default)]
    pub use_health_score: Option<bool>,
    /// 权重覆写；`null` 清空回退默认；`Some({...})` 校验后写入。
    #[serde(default)]
    pub weights: Option<Option<Weights>>,
}

#[derive(Serialize)]
pub struct DashboardSummary {
    pub by_state: HashMap<String, usize>,
    pub channels: Vec<ChannelHealthResponse>,
    pub alerts: Vec<HealthAlert>,
}

#[derive(Serialize)]
pub struct HealthAlert {
    pub channel_id: String,
    pub channel_code: String,
    pub severity: &'static str, // "critical" | "warn"
    pub message: String,
}

// ============================================================================
// helpers
// ============================================================================

fn score_to_response(score: &ChannelHealthScore, channel_code: &str) -> ChannelHealthResponse {
    ChannelHealthResponse {
        channel_id: score.channel_id.to_string(),
        channel_code: channel_code.to_string(),
        state: score.state.as_str().to_string(),
        score: score.score,
        success_rate: score.success_rate,
        latency_p99_ms: score.latency_p99_ms,
        banned_signal: score.banned_signal,
        quota_remaining_norm: score.quota_remaining_norm,
        consecutive_failures: score.consecutive_failures,
        cooldown_until: score.cooldown_until,
        banned_reason: score.banned_reason.clone(),
        last_transition_at: score.last_transition_at,
        window_total: score.window_total,
        window_success: score.window_success,
        updated_at: score.updated_at,
    }
}

fn score_update_from(score: &ChannelHealthScore, new_state: HealthState) -> ScoreUpdate {
    ScoreUpdate {
        score: score.score,
        success_rate: score.success_rate,
        latency_p99_ms: score.latency_p99_ms,
        banned_signal: score.banned_signal,
        quota_remaining_norm: score.quota_remaining_norm,
        consecutive_failures: score.consecutive_failures,
        state: new_state,
        cooldown_until: score.cooldown_until,
        banned_reason: score.banned_reason.clone(),
        window_total: score.window_total,
        window_success: score.window_success,
        window_started_at: score.window_started_at,
    }
}

// ============================================================================
// handlers
// ============================================================================

/// GET /v1/admin/channels/:id/health-score — 单 channel 评分详情。
pub(super) async fn get_channel_health_score(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<ChannelHealthResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;
    // 没有 score 行就懒建一条乐观初值
    app.repos
        .channel_health_score
        .ensure_row(ch.channel_id)
        .await?;
    let score = app
        .repos
        .channel_health_score
        .get(ch.channel_id)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(score_to_response(&score, &ch.code)))
}

/// POST /v1/admin/channels/:id/health/unban —— Banned → Cooldown，触发 probe 流量。
pub(super) async fn unban_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
) -> AppResult<Json<ChannelHealthResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;

    let score = app
        .repos
        .channel_health_score
        .get(ch.channel_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if score.state != HealthState::Banned {
        return Err(AppError::BadRequest(format!(
            "channel {} is in state {:?}; only Banned channels can be unbanned",
            ch.code, score.state
        )));
    }

    // Banned → Cooldown（不是直接 Healthy；让状态机 + 路由探针自然恢复）
    let mut update = score_update_from(&score, HealthState::Cooldown);
    update.cooldown_until = Some(Utc::now() + ChronoDuration::seconds(30));
    update.banned_reason = None;
    app.repos
        .channel_health_score
        .apply_update(ch.channel_id, &update)
        .await?;

    let after = app
        .repos
        .channel_health_score
        .get(ch.channel_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(score_to_response(&after, &ch.code)))
}

/// POST /v1/admin/channels/:id/health/cooldown — 人工强制 cooldown。
pub(super) async fn force_cooldown_channel(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(channel_id): Path<FlexUuid>,
    Json(req): Json<ForceCooldownRequest>,
) -> AppResult<Json<ChannelHealthResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let ch = app
        .repos
        .channels
        .find_by_id(ChannelId::from(channel_id.0))
        .await?;
    app.repos
        .channel_health_score
        .ensure_row(ch.channel_id)
        .await?;
    let score = app
        .repos
        .channel_health_score
        .get(ch.channel_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let secs = req.cooldown_secs.unwrap_or(30).max(1);
    let mut update = score_update_from(&score, HealthState::Cooldown);
    update.cooldown_until = Some(Utc::now() + ChronoDuration::seconds(secs));
    if req.reason.is_some() {
        update.banned_reason = req.reason;
    }
    app.repos
        .channel_health_score
        .apply_update(ch.channel_id, &update)
        .await?;

    let after = app
        .repos
        .channel_health_score
        .get(ch.channel_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(score_to_response(&after, &ch.code)))
}

/// GET /v1/admin/groups/:id/health-weights — 读 group 权重 + opt-in flag。
pub(super) async fn get_group_health_weights(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(group_id): Path<FlexUuid>,
) -> AppResult<Json<GroupHealthConfig>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let group = app
        .repos
        .channel_groups
        .find_by_id(ChannelGroupId::from(group_id.0))
        .await?;
    let weights: Option<Weights> = group
        .health_weights
        .clone()
        .and_then(|v| serde_json::from_value(v).ok());
    Ok(Json(GroupHealthConfig {
        group_id: group.group_id.to_string(),
        group_name: group.name.clone(),
        use_health_score: group.use_health_score,
        weights,
    }))
}

/// PUT /v1/admin/groups/:id/health-weights — 更新 opt-in + 权重覆写。
pub(super) async fn update_group_health_weights(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Path(group_id): Path<FlexUuid>,
    Json(req): Json<UpdateGroupHealthConfig>,
) -> AppResult<Json<GroupHealthConfig>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    // 校验 weights
    if let Some(Some(ref w)) = req.weights {
        w.validate().map_err(AppError::BadRequest)?;
    }

    let weights_json: Option<Option<serde_json::Value>> = req.weights.map(|outer| {
        outer.map(|w| serde_json::to_value(w).expect("Weights is a struct with known shape"))
    });

    let gid = ChannelGroupId::from(group_id.0);
    app.repos
        .channel_groups
        .update_health_config(gid, req.use_health_score, weights_json)
        .await?;

    // 读回最新
    let group = app.repos.channel_groups.find_by_id(gid).await?;
    let weights: Option<Weights> = group
        .health_weights
        .clone()
        .and_then(|v| serde_json::from_value(v).ok());
    Ok(Json(GroupHealthConfig {
        group_id: group.group_id.to_string(),
        group_name: group.name.clone(),
        use_health_score: group.use_health_score,
        weights,
    }))
}

/// GET /v1/admin/health-dashboard — 全局健康概览（5 状态饼 + channel × score 表 + 告警）。
pub(super) async fn health_dashboard(
    State(app): State<AppState>,
    Authed(ctx): Authed,
) -> AppResult<Json<DashboardSummary>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    // 1. 拉所有 channel
    let all = app.repos.channels.list_admin_view().await?;
    let channels = gate_storage::PaginatedChannels {
        total: all.len() as i64,
        data: all,
        page: 1,
        page_size: 1000,
    };
    let mut by_state: HashMap<String, usize> = HashMap::new();
    by_state.insert("healthy".to_string(), 0);
    by_state.insert("degraded".to_string(), 0);
    by_state.insert("cooldown".to_string(), 0);
    by_state.insert("recovering".to_string(), 0);
    by_state.insert("banned".to_string(), 0);

    // 2. 批量拉 score
    let ids: Vec<ChannelId> = channels.data.iter().map(|c| c.channel_id).collect();
    let score_map = app.repos.channel_health_score.get_many(&ids).await?;

    let mut channel_responses = Vec::with_capacity(channels.data.len());
    let mut alerts = Vec::new();

    for ch in &channels.data {
        // 没 score 行的 channel 默认 fresh
        let score = score_map
            .get(&ch.channel_id)
            .cloned()
            .unwrap_or_else(|| ChannelHealthScore::fresh(ch.channel_id));

        *by_state
            .entry(score.state.as_str().to_string())
            .or_insert(0) += 1;

        match score.state {
            HealthState::Banned => alerts.push(HealthAlert {
                channel_id: ch.channel_id.to_string(),
                channel_code: ch.code.clone(),
                severity: "critical",
                message: format!(
                    "Banned · reason={}",
                    score.banned_reason.as_deref().unwrap_or("(none)")
                ),
            }),
            HealthState::Cooldown => alerts.push(HealthAlert {
                channel_id: ch.channel_id.to_string(),
                channel_code: ch.code.clone(),
                severity: "warn",
                message: format!(
                    "Cooldown until {}",
                    score
                        .cooldown_until
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
            }),
            HealthState::Recovering => alerts.push(HealthAlert {
                channel_id: ch.channel_id.to_string(),
                channel_code: ch.code.clone(),
                severity: "warn",
                message: format!("Recovering · score={:.2}", score.score),
            }),
            _ => {}
        }

        channel_responses.push(score_to_response(&score, &ch.code));
    }

    Ok(Json(DashboardSummary {
        by_state,
        channels: channel_responses,
        alerts,
    }))
}
