//! 渠道健康巡检后台任务。
//!
//! 功能：
//! - pg_try_advisory_lock 防多实例并发
//! - 遍历 enabled + 未删除渠道，用 channel_keys 取活跃 key 做 Bearer 认证
//! - 内置渠道 GET `{base_url}/v1/models`；Plugin 渠道按 manifest probe 声明探活
//! - 连续失败 ≥3 次自动 disable
//! - 每个 channel 独立 catch panic，一个失败不影响全局

use crate::state::{AppState, Repos};
use gate_core::id::ChannelId;
use gate_crypto::EnvelopeKms;
use gate_providers::CustomHttpProvider;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

/// 健康巡检器。
pub struct HealthChecker {
    repos: Repos,
    interval: Duration,
    pool: PgPool,
    crypto: Option<Arc<EnvelopeKms>>,
    provider_router: Option<Arc<gate_providers::ProviderRouter>>,
}

impl HealthChecker {
    pub fn new(state: &AppState, interval: Duration) -> Option<Self> {
        let pool = state.repos.pool()?.clone();
        Some(Self {
            repos: state.repos.clone(),
            interval,
            pool,
            crypto: state.crypto.clone(),
            provider_router: state.provider_router.clone(),
        })
    }

    /// 启动后台 tokio task，返回 JoinHandle。
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        self.spawn_with_shutdown(CancellationToken::new())
    }

    pub fn spawn_with_shutdown(self, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run(shutdown).await;
        })
    }

    async fn run(self, shutdown: CancellationToken) {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("health_check: failed to build HTTP client");

        let mut ticker = interval(self.interval);
        let mut consecutive_failures: HashMap<ChannelId, u32> = HashMap::new();

        loop {
            tokio::select! {
                () = shutdown.cancelled() => {
                    tracing::info!("health_check: stopped");
                    break;
                }
                _ = ticker.tick() => {}
            }

            // ── Advisory lock: 只让一个实例跑巡检 ──
            let locked = match sqlx::query_scalar::<_, bool>(
                "SELECT pg_try_advisory_lock(hashtext('kooix_health_check'))",
            )
            .fetch_one(&self.pool)
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "health_check: advisory lock query failed");
                    continue;
                }
            };

            if !locked {
                tracing::debug!("health_check: another instance holds the lock, skipping");
                continue;
            }

            // 执行巡检
            self.check_all(&client, &mut consecutive_failures).await;

            // 释放 advisory lock
            let _ = sqlx::query("SELECT pg_advisory_unlock(hashtext('kooix_health_check'))")
                .execute(&self.pool)
                .await;

            if shutdown.is_cancelled() {
                tracing::info!("health_check: stopped");
                break;
            }
        }
    }

    async fn check_all(
        &self,
        client: &reqwest::Client,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
    ) {
        let channels = match self.repos.channels.list_admin_view().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "health_check: failed to list channels");
                return;
            }
        };

        let total = channels.len();
        tracing::info!(total, "health_check: starting round");

        for ch in &channels {
            // 跳过 disabled（非 unhealthy-cooldown 场景）和其他非 active 渠道
            // 但保留 disabled+unhealthy 的渠道做恢复探活
            let is_active = ch.status == "active";
            let is_cooldown = ch.status == "disabled" && ch.health == "unhealthy";

            if !is_active && !is_cooldown {
                continue;
            }

            // catch_unwind 隔离单个渠道的 panic
            let result = std::panic::AssertUnwindSafe(self.check_one(
                client,
                ch,
                consecutive_failures,
                is_cooldown,
            ));
            if let Err(e) = futures::FutureExt::catch_unwind(result).await {
                tracing::error!(
                    channel = %ch.code,
                    error = ?e,
                    "health_check: panic while checking channel"
                );
            }
        }

        tracing::info!("health_check: round complete");
    }

    async fn check_one(
        &self,
        client: &reqwest::Client,
        ch: &gate_storage::ChannelRecord,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
        is_cooldown: bool,
    ) {
        if matches!(
            ch.provider_type.as_str(),
            "plugin" | "custom" | "http" | "http_plugin"
        ) {
            self.check_plugin_one(client, ch, consecutive_failures, is_cooldown)
                .await;
            return;
        }

        let bearer_token = self.get_bearer_token(ch.channel_id).await;

        let base = ch.base_url.trim_end_matches('/');
        let (url, req_headers) = match ch.provider_type.as_str() {
            "anthropic" => {
                let url = format!("{base}/v1/models");
                let mut h = reqwest::header::HeaderMap::new();
                if let Some(ref token) = bearer_token
                    && let Ok(v) = token.parse()
                {
                    h.insert("x-api-key", v);
                }
                if let Ok(v) = "2023-06-01".parse() {
                    h.insert("anthropic-version", v);
                }
                (url, h)
            }
            _ => {
                let url = format!("{base}/v1/models");
                let mut h = reqwest::header::HeaderMap::new();
                if let Some(ref token) = bearer_token
                    && let Ok(v) = format!("Bearer {token}").parse()
                {
                    h.insert("authorization", v);
                }
                (url, h)
            }
        };

        let resp = client
            .get(&url)
            .headers(req_headers)
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                match status.as_u16() {
                    200 => {
                        // Auto-discover models from response
                        if let Ok(body) = r.json::<serde_json::Value>().await {
                            let models = extract_model_ids(&body);
                            if !models.is_empty() {
                                self.sync_models(ch, &models).await;
                            }
                        }

                        self.handle_success(ch, consecutive_failures, is_cooldown)
                            .await;
                    }
                    401 => {
                        self.handle_auth_failure(ch, consecutive_failures, 401)
                            .await;
                    }
                    429 => {
                        // 限流：瞬态错误，只记日志不改状态
                        tracing::warn!(
                            channel = %ch.code,
                            "health_check: rate limited (429), skipping status change"
                        );
                    }
                    408 => {
                        // 超时：累计失败，≥3 次 auto_disable
                        self.handle_transient_failure(ch, consecutive_failures, "timeout: 408")
                            .await;
                    }
                    other => {
                        // 其他错误码：累计
                        self.handle_transient_failure(
                            ch,
                            consecutive_failures,
                            &format!("http_error: {other}"),
                        )
                        .await;
                    }
                }
            }
            Err(e) => {
                // 网络层错误（connect timeout / DNS 等）
                let reason = if e.is_timeout() {
                    "connect_timeout".to_string()
                } else {
                    format!("network_error: {e}")
                };
                self.handle_transient_failure(ch, consecutive_failures, &reason)
                    .await;
            }
        }
    }

    async fn handle_success(
        &self,
        ch: &gate_storage::ChannelRecord,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
        is_cooldown: bool,
    ) {
        let count = consecutive_failures.entry(ch.channel_id).or_insert(0);
        if *count > 0 || ch.health != "healthy" || is_cooldown {
            *count = 0;
            if let Err(e) = self.repos.channels.re_enable(ch.channel_id).await {
                tracing::warn!(
                    channel = %ch.code, error = %e,
                    "health_check: failed to re_enable"
                );
            } else {
                if let Some(router) = &self.provider_router {
                    router.clear_channel_metrics(ch.channel_id);
                }
                tracing::info!(channel = %ch.code, "health_check: recovered → healthy");
            }
        }
    }

    async fn handle_auth_failure(
        &self,
        ch: &gate_storage::ChannelRecord,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
        status: u16,
    ) {
        if ch.status == "active" {
            if let Err(e) = self
                .repos
                .channels
                .auto_disable(ch.channel_id, &format!("auth_error: {status}"))
                .await
            {
                tracing::warn!(
                    channel = %ch.code, error = %e,
                    status,
                    "health_check: failed to auto_disable auth error"
                );
            } else {
                tracing::warn!(
                    channel = %ch.code,
                    status,
                    "health_check: auto_disabled auth error"
                );
            }
        }
        consecutive_failures.remove(&ch.channel_id);
    }

    async fn sync_models(&self, ch: &gate_storage::ChannelRecord, models: &[String]) {
        match self.repos.channels.sync_models(ch.channel_id, models).await {
            Ok(true) => {
                tracing::info!(
                    channel = %ch.code,
                    count = models.len(),
                    "health_check: auto-synced models"
                );
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    channel = %ch.code, error = %e,
                    "health_check: failed to sync models"
                );
            }
        }
    }

    async fn check_plugin_one(
        &self,
        client: &reqwest::Client,
        ch: &gate_storage::ChannelRecord,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
        is_cooldown: bool,
    ) {
        let secrets = self.get_probe_secrets(ch.channel_id, &ch.code).await;
        let provider = match CustomHttpProvider::new_with_secret_slots(
            &ch.base_url,
            secrets,
            ch.model_mapping.clone(),
            gate_providers::ProviderOpts {
                timeout_ms: (ch.timeout_ms as u64).max(5_000),
            },
        ) {
            Ok(provider) => provider,
            Err(e) => {
                self.handle_transient_failure(
                    ch,
                    consecutive_failures,
                    &format!("plugin_probe_config: {e}"),
                )
                .await;
                return;
            }
        };
        let probe = match provider.build_probe_request().await {
            Ok(probe) => probe,
            Err(e) => {
                self.handle_transient_failure(
                    ch,
                    consecutive_failures,
                    &format!("plugin_probe_build: {e}"),
                )
                .await;
                return;
            }
        };

        if let Some(max_cost_micros) = probe.max_cost_micros {
            tracing::debug!(
                channel = %ch.code,
                model = %probe.model,
                max_cost_micros,
                "health_check: plugin probe declared max cost"
            );
        }

        let resp = client
            .request(probe.method, &probe.url)
            .headers(probe.headers)
            .body(probe.body.unwrap_or_default())
            .timeout(Duration::from_millis((ch.timeout_ms as u64).max(5_000)))
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status().as_u16();
                if probe.success_status.contains(&status) {
                    if status == 200
                        && let Ok(body) = r.json::<serde_json::Value>().await
                    {
                        let models = extract_model_ids(&body);
                        if !models.is_empty() {
                            self.sync_models(ch, &models).await;
                        }
                    }
                    self.handle_success(ch, consecutive_failures, is_cooldown)
                        .await;
                } else if status == 401 || status == 403 {
                    self.handle_auth_failure(ch, consecutive_failures, status)
                        .await;
                } else if status == 429 {
                    tracing::warn!(
                        channel = %ch.code,
                        status,
                        "health_check: plugin probe rate limited, skipping status change"
                    );
                } else {
                    self.handle_transient_failure(
                        ch,
                        consecutive_failures,
                        &format!("plugin_probe_http: {status}"),
                    )
                    .await;
                }
            }
            Err(e) => {
                let reason = if e.is_timeout() {
                    "plugin_probe_timeout".to_string()
                } else {
                    format!("plugin_probe_network: {e}")
                };
                self.handle_transient_failure(ch, consecutive_failures, &reason)
                    .await;
            }
        }
    }

    /// 累计瞬态失败，连续 ≥3 次则 auto_disable。
    async fn handle_transient_failure(
        &self,
        ch: &gate_storage::ChannelRecord,
        consecutive_failures: &mut HashMap<ChannelId, u32>,
        reason: &str,
    ) {
        let count = consecutive_failures.entry(ch.channel_id).or_insert(0);
        *count += 1;

        tracing::warn!(
            channel = %ch.code,
            consecutive = *count,
            reason,
            "health_check: transient failure"
        );

        if *count >= 3 && ch.status == "active" {
            if let Err(e) = self
                .repos
                .channels
                .auto_disable(ch.channel_id, reason)
                .await
            {
                tracing::error!(
                    channel = %ch.code, error = %e,
                    "health_check: failed to auto_disable after {count} failures",
                );
            } else {
                tracing::warn!(
                    channel = %ch.code,
                    consecutive = *count,
                    reason,
                    "health_check: auto_disabled after consecutive failures"
                );
            }
        }
    }

    /// 从 channel_keys 取活跃 key 并解密，返回明文 token。
    /// 解密失败或无可用 key 时返回 None。
    async fn get_bearer_token(&self, channel_id: ChannelId) -> Option<String> {
        let key_record = self
            .repos
            .channel_keys
            .find_active_for_channel(channel_id)
            .await
            .ok()?;

        let crypto = self.crypto.as_ref()?;
        // AAD 用 channel_id（与创建时一致，同 channel 共享 AAD context）
        let aad = gate_crypto::aad::channel_key(*channel_id.as_uuid());
        let plaintext = crypto.open(&key_record.key_enc, &aad).await.ok()?;
        let token = String::from_utf8(plaintext.to_vec()).ok()?;
        Some(token)
    }

    async fn get_probe_secrets(
        &self,
        channel_id: ChannelId,
        code: &str,
    ) -> HashMap<String, String> {
        let mut secrets = CustomHttpProvider::env_secret_slots(code);
        let Some(crypto) = self.crypto.as_ref() else {
            return secrets;
        };
        let Ok(records) = self.repos.channel_keys.list_by_channel(channel_id).await else {
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
                .map(normalize_secret_slot)
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
}

fn normalize_secret_slot(slot: &str) -> String {
    let trimmed = slot.trim();
    if trimmed.is_empty() || trimmed == "api_key" {
        "primary".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn extract_model_ids(body: &serde_json::Value) -> Vec<String> {
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

/// 从环境变量读取配置并启动健康巡检。
///
/// - `KOOIX_HEALTH_CHECK_INTERVAL_SECS`: 巡检间隔秒数，默认 300（5 分钟）。
///   设为 "0" 或 "disabled" 则不启动。
pub fn spawn(state: &AppState) {
    let _ = spawn_with_shutdown(state, CancellationToken::new());
}

pub fn spawn_with_shutdown(
    state: &AppState,
    shutdown: CancellationToken,
) -> Option<tokio::task::JoinHandle<()>> {
    let raw = std::env::var("KOOIX_HEALTH_CHECK_INTERVAL_SECS").unwrap_or_default();
    if raw == "0" || raw.eq_ignore_ascii_case("disabled") {
        tracing::info!("health_check: disabled via KOOIX_HEALTH_CHECK_INTERVAL_SECS");
        return None;
    }

    let secs: u64 = raw.parse().unwrap_or(300);
    let interval = Duration::from_secs(secs);

    match HealthChecker::new(state, interval) {
        Some(checker) => {
            let handle = checker.spawn_with_shutdown(shutdown);
            tracing::info!(interval_secs = secs, "health_check: spawned");
            Some(handle)
        }
        None => {
            tracing::warn!("health_check: no PgPool available (in-memory mode?), not spawning");
            None
        }
    }
}
