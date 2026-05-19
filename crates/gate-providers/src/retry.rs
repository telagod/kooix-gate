//! Retry wrapper — exponential backoff + failover logic.

use crate::error::{ProviderError, ProviderResult};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub retryable_status_codes: Vec<u16>,
    pub retryable_error_codes: Vec<String>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff_ms: 500,
            max_backoff_ms: 10_000,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
            retryable_error_codes: Vec::new(),
        }
    }
}

impl RetryConfig {
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

    pub fn backoff_ms(&self, attempt: u32) -> u64 {
        let base = self.initial_backoff_ms * 2u64.pow(attempt);
        base.min(self.max_backoff_ms)
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
