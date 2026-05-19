//! 滑动窗口限流 — Lua 原子。

use crate::error::{CacheError, CacheResult};
use fred::clients::RedisPool;
use fred::interfaces::{LuaInterface, SortedSetsInterface};
use fred::types::RedisValue;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SCRIPT: &str = include_str!("../scripts/sliding_window.lua");

pub struct RateLimiter {
    pool: RedisPool,
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub current: u64,
    pub remaining: u64,
    pub retry_after_ms: u64,
}

impl RateLimiter {
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    /// 暴露内部 pool（供 ChannelRateLimiter TPM 计数等外部直接操作）。
    pub fn pool(&self) -> &RedisPool {
        &self.pool
    }

    /// 检查并增量记账。
    ///
    /// - `key`     full Redis key（建议带 namespace）
    /// - `window_ms` 窗口毫秒数
    /// - `limit`   窗口内最多多少次
    pub async fn check(
        &self,
        key: &str,
        window_ms: u64,
        limit: u64,
    ) -> CacheResult<RateLimitDecision> {
        self.check_n(key, window_ms, limit, 1).await
    }

    /// 检查并按 `amount` 增量记账。
    ///
    /// 用于 TPM 这类一次请求消耗多个单位的 quota；Lua member 记录为
    /// `amount:req_id`，同时兼容旧的无前缀 member（按 1 计）。
    pub async fn check_n(
        &self,
        key: &str,
        window_ms: u64,
        limit: u64,
        amount: u64,
    ) -> CacheResult<RateLimitDecision> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let req_id = Uuid::now_v7().simple().to_string();

        let reply: RedisValue = self
            .pool
            .next()
            .eval(
                SCRIPT,
                vec![key.to_string()],
                vec![
                    now_ms.to_string(),
                    window_ms.to_string(),
                    limit.to_string(),
                    req_id,
                    amount.to_string(),
                ],
            )
            .await?;

        parse(reply)
    }

    /// 只读当前窗口内消耗单位数，不写入新 entry。
    ///
    /// dry-run / explain 用；失败由调用方按 fail-open 处理。
    pub async fn peek_count(&self, key: &str, window_ms: u64) -> CacheResult<u64> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cutoff = now_ms.saturating_sub(window_ms as i64);
        let client = self.pool.next();
        let _: RedisValue = client
            .zremrangebyscore(key.to_string(), "-inf", cutoff)
            .await?;
        let members: Vec<String> = client
            .zrange(key.to_string(), 0, -1, None, false, None, false)
            .await?;
        Ok(members
            .iter()
            .map(|member| {
                member
                    .split_once(':')
                    .and_then(|(raw, _)| raw.parse::<u64>().ok())
                    .unwrap_or(1)
            })
            .sum())
    }
}

fn parse(v: RedisValue) -> CacheResult<RateLimitDecision> {
    let arr = match v {
        RedisValue::Array(a) => a,
        other => return Err(CacheError::Shape(format!("expected array, got {other:?}"))),
    };
    if arr.len() != 4 {
        return Err(CacheError::Shape(format!(
            "expected 4 elements, got {}",
            arr.len()
        )));
    }
    let to_u64 = |v: &RedisValue| -> CacheResult<u64> {
        v.as_i64()
            .map(|n| n.max(0) as u64)
            .ok_or_else(|| CacheError::Shape(format!("expected int, got {v:?}")))
    };

    Ok(RateLimitDecision {
        allowed: to_u64(&arr[0])? == 1,
        current: to_u64(&arr[1])?,
        remaining: to_u64(&arr[2])?,
        retry_after_ms: to_u64(&arr[3])?,
    })
}
