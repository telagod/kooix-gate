//! 配额预扣 / 回滚 — Lua 原子。

use crate::error::{CacheError, CacheResult};
use fred::clients::RedisPool;
use fred::interfaces::{KeysInterface, LuaInterface};
use fred::types::RedisValue;

const DEBIT: &str = include_str!("../scripts/quota_debit.lua");
const REFUND: &str = include_str!("../scripts/quota_refund.lua");

pub struct QuotaCounter {
    pool: RedisPool,
}

#[derive(Debug, Clone, Copy)]
pub struct QuotaOutcome {
    pub ok: bool,
    pub current_used: i64,
    pub remaining: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct RefundOutcome {
    pub current_used: i64,
}

impl QuotaCounter {
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    /// 预扣 amount。`ttl_seconds` 是计数器自身的 TTL（用于周期复位，比如月配额）。
    pub async fn debit(
        &self,
        key: &str,
        amount: i64,
        limit: i64,
        ttl_seconds: i64,
    ) -> CacheResult<QuotaOutcome> {
        let v: RedisValue = self
            .pool
            .next()
            .eval(
                DEBIT,
                vec![key.to_string()],
                vec![
                    amount.to_string(),
                    limit.to_string(),
                    ttl_seconds.to_string(),
                ],
            )
            .await?;
        let arr = as_array3(v)?;
        let to_i64 = |x: &RedisValue| {
            x.as_i64()
                .ok_or_else(|| CacheError::Shape(format!("expected int, got {x:?}")))
        };
        Ok(QuotaOutcome {
            ok: to_i64(&arr[0])? == 1,
            current_used: to_i64(&arr[1])?,
            remaining: to_i64(&arr[2])?,
        })
    }

    /// 回滚 amount。返回回滚后的 current_used。
    pub async fn refund(&self, key: &str, amount: i64) -> CacheResult<RefundOutcome> {
        let v: RedisValue = self
            .pool
            .next()
            .eval(REFUND, vec![key.to_string()], vec![amount.to_string()])
            .await?;
        let current = v
            .as_i64()
            .ok_or_else(|| CacheError::Shape(format!("expected int, got {v:?}")))?;
        Ok(RefundOutcome {
            current_used: current,
        })
    }

    /// 只读当前用量（不增加、不预扣）。
    ///
    /// 用于 budget 类配额的「能否放行」判断：chat 请求成本要事后才知道，
    /// 没法在路径上预扣，因此先用这个方法判断当前用量是否已超额。
    /// key 不存在视为 0。
    pub async fn peek(&self, key: &str) -> CacheResult<i64> {
        let v: RedisValue = self.pool.next().get(key.to_string()).await?;
        match v {
            RedisValue::Null => Ok(0),
            other => other.as_i64().ok_or_else(|| {
                CacheError::Shape(format!("expected int from GET {key}, got {other:?}"))
            }),
        }
    }
}

fn as_array3(v: RedisValue) -> CacheResult<Vec<RedisValue>> {
    match v {
        RedisValue::Array(a) if a.len() == 3 => Ok(a),
        other => Err(CacheError::Shape(format!(
            "expected 3-element array, got {other:?}"
        ))),
    }
}
