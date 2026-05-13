//! gate-cache: Redis 操作层
//!
//! 关键能力：
//! - [`RateLimiter`]: 滑动窗口限流（Lua 原子）
//! - [`QuotaCounter`]: 配额预扣/回滚（Lua 原子）
//!
//! 设计原则：
//! - 所有热路径逻辑放进 Lua，避免 round-trip → 远端 race
//! - Rust 层只负责脚本加载、参数序列化、结果解析
//! - 错误经 [`CacheError`] 统一收口

pub mod error;
pub mod quota;
pub mod rate_limit;

pub use error::{CacheError, CacheResult};
pub use fred::clients::RedisPool;
pub use quota::{QuotaCounter, QuotaOutcome, RefundOutcome};
pub use rate_limit::{RateLimitDecision, RateLimiter};

use fred::interfaces::ClientLike;
use fred::types::{Builder, ReconnectPolicy, RedisConfig};

/// 连 Redis 拿一个连接池。
///
/// `url` 形如 `redis://localhost:6379` 或 `rediss://...`。
pub async fn connect(url: &str, pool_size: usize) -> CacheResult<RedisPool> {
    let config = RedisConfig::from_url(url).map_err(|e| CacheError::Connect(e.to_string()))?;
    let pool = Builder::from_config(config)
        .with_connection_config(|c| {
            c.connection_timeout = std::time::Duration::from_secs(5);
        })
        .set_policy(ReconnectPolicy::new_exponential(0, 100, 5_000, 2))
        .build_pool(pool_size)
        .map_err(|e| CacheError::Connect(e.to_string()))?;
    pool.init()
        .await
        .map_err(|e| CacheError::Connect(e.to_string()))?;
    Ok(pool)
}
