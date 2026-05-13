//! InflightGuard — pre-debit 后的飞行中守卫。
//!
//! 在 quota middleware 预扣成功后创建，持有一笔待结算的预扣额度。
//! handler 处理完请求后调 [`settle`] 按实际用量修正预扣差额。
//!
//! 安全保障：
//! - 正常路径：`settle(actual_micros)` → 多退少补
//! - 异常路径（panic / 取消）：`Drop` 自动全额退还，避免预扣泄漏

use gate_cache::QuotaCounter;
use std::sync::Arc;

/// 请求飞行期间的预扣守卫。
///
/// 必须满足 `Send + Sync`（会跨 await 点传递）。
pub struct InflightGuard {
    quota_counter: Arc<QuotaCounter>,
    key: String,
    estimated_micros: i64,
    settled: bool,
}

impl InflightGuard {
    pub fn new(qc: Arc<QuotaCounter>, key: String, estimated_micros: i64) -> Self {
        Self {
            quota_counter: qc,
            key,
            estimated_micros,
            settled: false,
        }
    }

    /// 用实际费用结算预扣差额。
    ///
    /// - actual < estimated → 退还差额
    /// - actual > estimated → 追加扣减（best-effort，limit=MAX 不拒绝）
    /// - actual == estimated → 无操作
    pub async fn settle(&mut self, actual_micros: i64) {
        let diff = self.estimated_micros - actual_micros;
        if diff > 0 {
            let _ = self.quota_counter.refund(&self.key, diff).await;
        } else if diff < 0 {
            // 追加扣减：limit 设 i64::MAX 确保不会被拒
            let _ = self
                .quota_counter
                .debit(&self.key, -diff, i64::MAX, 86400)
                .await;
        }
        self.settled = true;
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        if !self.settled {
            // 请求异常终止（panic / 取消）—— 全额退还预扣
            // Drop 里不能 async，spawn 一个 task 完成
            let qc = self.quota_counter.clone();
            let key = self.key.clone();
            let amount = self.estimated_micros;
            tokio::spawn(async move {
                let _ = qc.refund(&key, amount).await;
            });
        }
    }
}

/// 请求 extension 载体——把一组 InflightGuard 通过 axum extension 传递给 handler。
///
/// 内部用 `Arc<Mutex<..>>` 包装以满足 axum `Extension<T>: Clone` 要求。
/// handler 通过 `take()` 取走所有 guard 做 settle。
#[derive(Clone)]
pub struct InflightGuards(Arc<std::sync::Mutex<Vec<InflightGuard>>>);

impl InflightGuards {
    pub fn new(guards: Vec<InflightGuard>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(guards)))
    }

    /// 取走所有 guard（只能调一次，后续调用返空 vec）。
    pub fn take(&self) -> Vec<InflightGuard> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：InflightGuard 内含 Arc<QuotaCounter>，而 QuotaCounter::new(pool) 需要
    // 真实 RedisPool。单元测试只验证类型约束（Send + Sync），功能测试走集成路径。

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn guard_is_send_sync() {
        _assert_send_sync::<InflightGuard>();
        _assert_send_sync::<InflightGuards>();
    }
}
