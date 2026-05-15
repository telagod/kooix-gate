//! InflightGuard — pre-debit 后的飞行中守卫。
//!
//! 在 quota middleware 预扣成功后创建，持有一笔待结算的预扣额度。
//! handler 处理完请求后调 [`settle`] 按实际用量修正预扣差额。
//!
//! 安全保障：
//! - 正常路径：`settle(actual_micros)` → 多退少补 + DELETE inflight_requests 行
//! - 异常路径（panic / 取消）：`Drop` 自动全额退还 + DELETE 行
//! - 进程崩溃（kill -9）：后台 sweep 定时扫 expired 行退还

use gate_cache::QuotaCounter;
use gate_storage::InFlightRepo;
use std::sync::Arc;
use uuid::Uuid;

pub struct InflightGuard {
    quota_counter: Arc<QuotaCounter>,
    pub key: String,
    pub estimated_micros: i64,
    settled: bool,
    request_id: Option<Uuid>,
    inflight_repo: Option<Arc<dyn InFlightRepo>>,
}

impl InflightGuard {
    pub fn new(qc: Arc<QuotaCounter>, key: String, estimated_micros: i64) -> Self {
        Self {
            quota_counter: qc,
            key,
            estimated_micros,
            settled: false,
            request_id: None,
            inflight_repo: None,
        }
    }

    pub fn with_db(mut self, request_id: Uuid, repo: Arc<dyn InFlightRepo>) -> Self {
        self.request_id = Some(request_id);
        self.inflight_repo = Some(repo);
        self
    }

    pub async fn settle(&mut self, actual_micros: i64) {
        let diff = self.estimated_micros - actual_micros;
        if diff > 0 {
            let _ = self.quota_counter.refund(&self.key, diff).await;
        } else if diff < 0 {
            let _ = self
                .quota_counter
                .debit(&self.key, -diff, i64::MAX, 86400)
                .await;
        }
        self.settled = true;
        self.cleanup_db();
    }

    fn cleanup_db(&self) {
        if let (Some(rid), Some(repo)) = (self.request_id, self.inflight_repo.clone()) {
            tokio::spawn(async move {
                let _ = repo.delete(rid).await;
            });
        }
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        if !self.settled {
            let qc = self.quota_counter.clone();
            let key = self.key.clone();
            let amount = self.estimated_micros;
            tokio::spawn(async move {
                let _ = qc.refund(&key, amount).await;
            });
        }
        // Always clean up DB row (whether settled or not, we're done)
        if !self.settled {
            self.cleanup_db();
        }
    }
}

#[derive(Clone)]
pub struct InflightGuards(Arc<parking_lot::Mutex<Vec<InflightGuard>>>);

impl InflightGuards {
    pub fn new(guards: Vec<InflightGuard>) -> Self {
        Self(Arc::new(parking_lot::Mutex::new(guards)))
    }

    pub fn take(&self) -> Vec<InflightGuard> {
        std::mem::take(&mut *self.0.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn guard_is_send_sync() {
        _assert_send_sync::<InflightGuard>();
        _assert_send_sync::<InflightGuards>();
    }
}
