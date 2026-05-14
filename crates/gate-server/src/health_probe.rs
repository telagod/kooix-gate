//! 渠道健康探活后台任务。

use crate::state::AppState;
use std::collections::HashMap;
use std::time::Duration;
use gate_core::id::ChannelId;
use tokio::time::interval;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let mut failures: HashMap<ChannelId, u32> = HashMap::new();

        loop {
            ticker.tick().await;
            let channels = match state.repos.channels.list_admin_view().await {
                Ok(c) => c,
                Err(e) => { tracing::warn!(error = %e, "health_probe: failed to list channels"); continue; }
            };

            for ch in channels {
                if ch.status != "active" { continue; }
                let url = format!("{}/models", ch.base_url.trim_end_matches('/'));
                let ok = client.get(&url).timeout(Duration::from_secs(5)).send().await.is_ok();

                let count = failures.entry(ch.channel_id).or_insert(0);
                if ok {
                    if *count > 0 || ch.health != "healthy" {
                        *count = 0;
                        update_health(&state, ch.channel_id, "healthy").await;
                        tracing::info!(channel = %ch.code, "health_probe: recovered → healthy");
                    }
                } else {
                    *count += 1;
                    if *count >= 3 && ch.health == "healthy" {
                        update_health(&state, ch.channel_id, "unhealthy").await;
                        tracing::warn!(channel = %ch.code, failures = *count, "health_probe: marked unhealthy");
                    }
                }
            }
        }
    });
}

async fn update_health(state: &AppState, id: ChannelId, health: &str) {
    if let Some(pool) = state.repos.pool() {
        let _ = sqlx::query("UPDATE channels SET health = $1, updated_at = NOW() WHERE id = $2")
            .bind(health)
            .bind(id.as_uuid())
            .execute(pool)
            .await;
    }
}
