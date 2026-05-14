//! Redis-backed per-channel RPM/TPM 限速器。
//!
//! 实现 [`gate_providers::ChannelRateCheck`] trait，使用 [`gate_cache::RateLimiter`]
//! 的 Lua 滑动窗口做 RPM 原子检查，TPM 用 Redis INCRBY + PEXPIRE 做简单计数。
//!
//! Redis 异常时 fail-open（warn + 放行），避免 Redis 宕机阻断全站。

use gate_cache::RateLimiter;
use gate_core::id::ChannelId;
use gate_providers::ChannelRateCheck;
use std::sync::Arc;

const WINDOW_MS: u64 = 60_000; // 60s sliding window
const TPM_TTL_MS: i64 = 120_000; // 120s TTL for TPM counter

/// Redis 滑动窗口限速器。
///
/// - RPM: 复用 `RateLimiter`（Lua ZADD 滑窗，精确 sliding window log）
/// - TPM: Redis INCRBY + PEXPIRE（近似固定窗口，对 TPM 精度足够）
///
/// Redis 异常时 fail-open，仅打 warn。
pub struct RedisChannelRateLimiter {
    limiter: Arc<RateLimiter>,
}

impl RedisChannelRateLimiter {
    pub fn new(limiter: Arc<RateLimiter>) -> Self {
        Self { limiter }
    }
}

#[async_trait::async_trait]
impl ChannelRateCheck for RedisChannelRateLimiter {
    async fn check_rpm(&self, channel_id: ChannelId, rpm_limit: Option<i32>) -> bool {
        let Some(limit) = rpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u64;
        if limit == 0 {
            return false;
        }

        let key = format!("ratelimit:{}:rpm", channel_id.as_uuid());
        match self.limiter.check(&key, WINDOW_MS, limit).await {
            Ok(d) => d.allowed,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    channel_id = %channel_id.as_uuid(),
                    "Redis RPM check failed; fail-open"
                );
                true // fail-open
            }
        }
    }

    async fn check_tpm(&self, channel_id: ChannelId, tpm_limit: Option<i32>) -> bool {
        let Some(limit) = tpm_limit else {
            return true;
        };
        let limit = limit.max(0) as u64;
        if limit == 0 {
            return false;
        }

        let key = format!("ratelimit:{}:tpm", channel_id.as_uuid());
        match self.peek_tpm(&key).await {
            Ok(current) => current < limit,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    channel_id = %channel_id.as_uuid(),
                    "Redis TPM peek failed; fail-open"
                );
                true
            }
        }
    }

    async fn record_tokens(&self, channel_id: ChannelId, tokens: u32) {
        if tokens == 0 {
            return;
        }
        let key = format!("ratelimit:{}:tpm", channel_id.as_uuid());
        if let Err(e) = self.incr_tpm(&key, tokens).await {
            tracing::warn!(
                error = %e,
                channel_id = %channel_id.as_uuid(),
                tokens = tokens,
                "Redis TPM record failed"
            );
        }
    }
}

impl RedisChannelRateLimiter {
    async fn peek_tpm(&self, key: &str) -> gate_cache::CacheResult<u64> {
        use fred::interfaces::KeysInterface;
        let v: fred::types::RedisValue = self.limiter.pool().next().get(key.to_string()).await?;
        match v {
            fred::types::RedisValue::Null => Ok(0),
            other => other
                .as_u64()
                .ok_or_else(|| gate_cache::CacheError::Shape(format!("expected int, got {other:?}"))),
        }
    }

    async fn incr_tpm(&self, key: &str, tokens: u32) -> gate_cache::CacheResult<()> {
        use fred::interfaces::KeysInterface;
        let client = self.limiter.pool().next();
        let _: i64 = client.incr_by(key.to_string(), tokens as i64).await?;
        // Set TTL only if not already set (avoid shortening existing TTL)
        let ttl: i64 = client.pttl(key.to_string()).await.unwrap_or(-1);
        if ttl < 0 {
            let _: bool = client.pexpire(key.to_string(), TPM_TTL_MS, None).await?;
        }
        Ok(())
    }
}
