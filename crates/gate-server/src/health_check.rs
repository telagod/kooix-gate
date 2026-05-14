//! 渠道健康巡检后台任务。
//!
//! 功能：
//! - pg_try_advisory_lock 防多实例并发
//! - 遍历 enabled + 未删除渠道，用 channel_keys 取活跃 key 做 Bearer 认证
//! - GET `{base_url}/v1/models` 按 HTTP 状态码分级处理
//! - 连续失败 ≥3 次自动 disable
//! - 每个 channel 独立 catch panic，一个失败不影响全局

use crate::state::{AppState, Repos};
use gate_core::id::ChannelId;
use gate_crypto::EnvelopeKms;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

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
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(self) {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("health_check: failed to build HTTP client");

        let mut ticker = interval(self.interval);
        let mut consecutive_failures: HashMap<ChannelId, u32> = HashMap::new();

        loop {
            ticker.tick().await;

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
            let result = std::panic::AssertUnwindSafe(
                self.check_one(client, ch, consecutive_failures, is_cooldown),
            );
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
        // 取活跃 key 做 Bearer 认证
        let bearer_token = match self.get_bearer_token(ch.channel_id).await {
            Some(t) => t,
            None => {
                // 没有可用 key，跳过（不标记失败，因为可能尚未配 key）
                tracing::debug!(channel = %ch.code, "health_check: no active key, skipping");
                return;
            }
        };

        let url = format!(
            "{}/v1/models",
            ch.base_url.trim_end_matches('/')
        );

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {bearer_token}"))
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                match status.as_u16() {
                    200 => {
                        // 健康：如果之前不健康，恢复之
                        let count = consecutive_failures.entry(ch.channel_id).or_insert(0);
                        if *count > 0 || ch.health != "healthy" || is_cooldown {
                            *count = 0;
                            if let Err(e) = self.repos.channels.re_enable(ch.channel_id).await {
                                tracing::warn!(
                                    channel = %ch.code, error = %e,
                                    "health_check: failed to re_enable"
                                );
                            } else {
                                // 清除 router metrics 窗口
                                if let Some(router) = &self.provider_router {
                                    router.clear_channel_metrics(ch.channel_id);
                                }
                                tracing::info!(channel = %ch.code, "health_check: recovered → healthy");
                            }
                        }
                    }
                    401 => {
                        // 认证错误：直接 auto_disable
                        if ch.status == "active" {
                            if let Err(e) = self
                                .repos
                                .channels
                                .auto_disable(ch.channel_id, "auth_error: 401")
                                .await
                            {
                                tracing::warn!(
                                    channel = %ch.code, error = %e,
                                    "health_check: failed to auto_disable (401)"
                                );
                            } else {
                                tracing::warn!(
                                    channel = %ch.code,
                                    "health_check: auto_disabled — auth_error: 401"
                                );
                            }
                        }
                        consecutive_failures.remove(&ch.channel_id);
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
                        self.handle_transient_failure(
                            ch,
                            consecutive_failures,
                            "timeout: 408",
                        )
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
        let aad = gate_crypto::aad::channel_key(*key_record.id.as_uuid());
        let plaintext = crypto.open(&key_record.key_enc, &aad).await.ok()?;
        let token = String::from_utf8(plaintext.to_vec()).ok()?;
        Some(token)
    }
}

/// 从环境变量读取配置并启动健康巡检。
///
/// - `KOOIX_HEALTH_CHECK_INTERVAL_SECS`: 巡检间隔秒数，默认 300（5 分钟）。
///   设为 "0" 或 "disabled" 则不启动。
pub fn spawn(state: &AppState) {
    let raw = std::env::var("KOOIX_HEALTH_CHECK_INTERVAL_SECS").unwrap_or_default();
    if raw == "0" || raw.eq_ignore_ascii_case("disabled") {
        tracing::info!("health_check: disabled via KOOIX_HEALTH_CHECK_INTERVAL_SECS");
        return;
    }

    let secs: u64 = raw.parse().unwrap_or(300);
    let interval = Duration::from_secs(secs);

    match HealthChecker::new(state, interval) {
        Some(checker) => {
            checker.spawn();
            tracing::info!(interval_secs = secs, "health_check: spawned");
        }
        None => {
            tracing::warn!("health_check: no PgPool available (in-memory mode?), not spawning");
        }
    }
}
