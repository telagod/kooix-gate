//! Retry wrapper — exponential backoff + failover logic.
//!
//! 0.4.70（product-review §2.4）：
//! - `backoff_ms` 加 ±25% jitter，防 N 个客户端同步退避后形成"雷暴"重试洪峰。
//! - `RetryConfig::stream_safe()` factory：max_retries=0，给流式路径用。
//!   流式建立后任何失败都不能 retry：客户端已收到部分 chunks，重发会乱序；
//!   inflight pre-debit 也已经扣了一次，重试会双计费。

use crate::error::{ProviderError, ProviderResult};
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub retryable_status_codes: Vec<u16>,
    pub retryable_error_codes: Vec<String>,
    /// 0.4.70: 是否给 backoff 加 jitter（默认 true）。
    /// 测试时关掉以让 backoff 可预测。
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff_ms: 500,
            max_backoff_ms: 10_000,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
            retryable_error_codes: Vec::new(),
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// 0.4.70: 流式路径专用 config —— 禁用 retry。
    /// 流建立后失败不能 retry：客户端已收 chunk + inflight 已 pre-debit。
    pub fn stream_safe() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }

    pub fn is_retryable(&self, err: &ProviderError) -> bool {
        match err {
            ProviderError::RateLimited { .. } => true,
            ProviderError::Network(_) => true,
            ProviderError::Upstream { status, .. } => self.retryable_status_codes.contains(status),
            ProviderError::Mapped {
                status,
                code,
                metadata,
                ..
            } => {
                metadata.retryable
                    || status.is_some_and(|status| self.retryable_status_codes.contains(&status))
                    || code
                        .as_deref()
                        .is_some_and(|code| self.retryable_error_codes.iter().any(|c| c == code))
            }
            _ => false,
        }
    }

    /// 0.4.70: exponential backoff + 可选 ±25% jitter。
    /// jitter 用 fast PRNG（thread_rng），不要求加密强度——只防雷暴同步。
    pub fn backoff_ms(&self, attempt: u32) -> u64 {
        let base = self
            .initial_backoff_ms
            .saturating_mul(2u64.saturating_pow(attempt));
        let capped = base.min(self.max_backoff_ms);
        if !self.jitter || capped == 0 {
            return capped;
        }
        let span = (capped / 4).max(1); // ±25%
        let mut rng = rand::thread_rng();
        let delta: i64 = rng.gen_range(-(span as i64)..=(span as i64));
        let with_jitter = (capped as i64).saturating_add(delta);
        with_jitter.max(0) as u64
    }
}

pub async fn with_retry<F, Fut, T>(config: &RetryConfig, mut f: F) -> ProviderResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ProviderResult<T>>,
{
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if attempt == config.max_retries || !config.is_retryable(&e) {
                    return Err(e);
                }
                let backoff = config.backoff_ms(attempt);
                // If rate limited with retry_after, use that instead
                let wait = match &e {
                    ProviderError::RateLimited {
                        retry_after_ms: Some(ms),
                    } => (*ms).min(config.max_backoff_ms),
                    ProviderError::Mapped {
                        metadata:
                            crate::error::ProviderErrorMetadata {
                                retry_after_ms,
                                cooldown_ms,
                                ..
                            },
                        ..
                    } => retry_after_ms
                        .or(*cooldown_ms)
                        .map(|ms| ms.min(config.max_backoff_ms))
                        .unwrap_or(backoff),
                    _ => backoff,
                };
                tracing::warn!(
                    attempt,
                    wait_ms = wait,
                    error = %e,
                    "retrying after error"
                );
                last_err = Some(e);
                sleep(Duration::from_millis(wait)).await;
            }
        }
    }
    Err(last_err.unwrap_or(ProviderError::Config("retry exhausted".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_safe_disables_retry() {
        let cfg = RetryConfig::stream_safe();
        assert_eq!(cfg.max_retries, 0, "stream_safe must not retry");
    }

    #[test]
    fn backoff_without_jitter_is_deterministic_exponential() {
        let cfg = RetryConfig {
            jitter: false,
            ..RetryConfig::default()
        };
        assert_eq!(cfg.backoff_ms(0), 500);
        assert_eq!(cfg.backoff_ms(1), 1000);
        assert_eq!(cfg.backoff_ms(2), 2000);
        assert_eq!(cfg.backoff_ms(3), 4000);
        assert_eq!(cfg.backoff_ms(10), 10_000); // capped
    }

    #[test]
    fn backoff_with_jitter_stays_within_25_percent_band() {
        let cfg = RetryConfig::default(); // jitter: true
        // 重复 200 次，验证范围 + 不全相等（jitter 真生效）
        let mut samples = std::collections::HashSet::new();
        for _ in 0..200 {
            let v = cfg.backoff_ms(2); // base = 2000, span = 500
            assert!(
                v >= 1500 && v <= 2500,
                "backoff out of jitter band: {v}"
            );
            samples.insert(v);
        }
        assert!(
            samples.len() > 5,
            "jitter should produce many distinct values, got {} unique samples",
            samples.len()
        );
    }

    #[test]
    fn backoff_ms_does_not_panic_on_huge_attempt() {
        let cfg = RetryConfig::default();
        // saturating_pow / saturating_mul 防溢出
        let v = cfg.backoff_ms(64);
        assert!(v <= cfg.max_backoff_ms + cfg.max_backoff_ms / 4);
    }

    #[tokio::test]
    async fn stream_safe_returns_immediately_on_first_error() {
        let cfg = RetryConfig::stream_safe();
        let count = std::sync::atomic::AtomicU32::new(0);
        let result: ProviderResult<()> = with_retry(&cfg, || async {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err::<(), _>(ProviderError::Network("boom".into()))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "stream_safe must not retry"
        );
    }
}
