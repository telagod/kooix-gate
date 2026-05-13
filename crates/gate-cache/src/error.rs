//! gate-cache 错误。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("connect: {0}")]
    Connect(String),

    #[error("redis: {0}")]
    Redis(String),

    #[error("script execution: {0}")]
    Script(String),

    #[error("unexpected reply shape: {0}")]
    Shape(String),
}

impl From<fred::error::RedisError> for CacheError {
    fn from(e: fred::error::RedisError) -> Self {
        Self::Redis(e.to_string())
    }
}

pub type CacheResult<T> = Result<T, CacheError>;
