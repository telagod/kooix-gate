//! In-memory `ChannelHealthScore` cache (ADR-0007 / M5.1 N1.5)。
//!
//! 路由热路径每个请求会 [`crate::router::ProviderRouter::health_view_for_group`]
//! 查 score。N1.4 默认实现是同步 PG 读，本模块加一层 in-memory TTL cache，把热路径
//! PG round-trip 拉到 sub-microsecond 级。
//!
//! ## 一致性模型
//!
//! - **读优先 cache**：路由读取 `get_many` —— 缺失或过期回 PG（回填）
//! - **写优先 cache**：`ScoreFlusher` 完成 `apply_update` 后 `put`，cache 立即看到
//! - **TTL** 默认 1s：跨进程读写一致性窗口可控；过期不阻塞读，懒回收
//! - **invalidate** 显式失效：channel deactivate / score reset 等控制面操作
//!
//! ## 并发
//!
//! 用 `parking_lot::RwLock`（不 await）+ `Arc` 共享。`get` 是 read lock，`put` 短暂
//! write lock；TTL 检查在 read lock 内基于 `std::time::Instant`。

use chrono::{DateTime, Utc};
use gate_core::id::ChannelId;
use gate_storage::ChannelHealthScore;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认 TTL：1s。
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
struct CachedScore {
    snapshot: ChannelHealthScore,
    fetched_at: Instant,
}

/// In-memory `ChannelHealthScore` cache。线程安全，可 `Arc::clone` 共享。
#[derive(Debug, Clone)]
pub struct HealthScoreCache {
    inner: Arc<RwLock<HashMap<ChannelId, CachedScore>>>,
    ttl: Duration,
}

impl Default for HealthScoreCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL)
    }
}

impl HealthScoreCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// 单 channel 读。返 None 表示 cache miss 或过期，调用方应回 PG。
    pub fn get(&self, id: ChannelId) -> Option<ChannelHealthScore> {
        let now = Instant::now();
        let guard = self.inner.read();
        let entry = guard.get(&id)?;
        if now.duration_since(entry.fetched_at) > self.ttl {
            return None;
        }
        Some(entry.snapshot.clone())
    }

    /// 批量读。返回 cache 命中的部分；miss 列表通过 `missing` 输出参数返回。
    pub fn get_many(
        &self,
        ids: &[ChannelId],
    ) -> (HashMap<ChannelId, ChannelHealthScore>, Vec<ChannelId>) {
        let now = Instant::now();
        let guard = self.inner.read();
        let mut hit = HashMap::with_capacity(ids.len());
        let mut miss = Vec::new();
        for id in ids {
            match guard.get(id) {
                Some(entry) if now.duration_since(entry.fetched_at) <= self.ttl => {
                    hit.insert(*id, entry.snapshot.clone());
                }
                _ => miss.push(*id),
            }
        }
        (hit, miss)
    }

    /// 单 channel 写入（覆盖式）。`fetched_at` 重置为现在。
    pub fn put(&self, score: ChannelHealthScore) {
        let id = score.channel_id;
        let entry = CachedScore {
            snapshot: score,
            fetched_at: Instant::now(),
        };
        self.inner.write().insert(id, entry);
    }

    /// 批量写。
    pub fn put_many<I: IntoIterator<Item = ChannelHealthScore>>(&self, scores: I) {
        let now = Instant::now();
        let mut guard = self.inner.write();
        for score in scores {
            let id = score.channel_id;
            guard.insert(
                id,
                CachedScore {
                    snapshot: score,
                    fetched_at: now,
                },
            );
        }
    }

    /// 显式失效一条。常见于 channel deactivate / score reset / 人工解封。
    pub fn invalidate(&self, id: ChannelId) {
        self.inner.write().remove(&id);
    }

    /// 清空整个 cache。
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// 主动回收过期条目。可作为长尾保活策略：N1.6 admin 周期 task 调。
    /// 返回被清理的条目数。
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let mut guard = self.inner.write();
        let before = guard.len();
        guard.retain(|_, entry| now.duration_since(entry.fetched_at) <= self.ttl);
        before - guard.len()
    }

    /// 当前 cache 条目数（含过期但未回收的）。监控用。
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

/// Cache miss → PG → 回填的 helper。返完整 HealthView。
///
/// 等价于：cache.get_many → for misses query repo → cache.put_many → 合并结果。
pub async fn cached_get_many<R>(
    cache: &HealthScoreCache,
    repo: &R,
    ids: &[ChannelId],
) -> Result<HashMap<ChannelId, ChannelHealthScore>, gate_storage::DbError>
where
    R: gate_storage::ChannelHealthScoreRepo + ?Sized,
{
    let (mut hit, miss) = cache.get_many(ids);
    if miss.is_empty() {
        return Ok(hit);
    }
    let from_pg = repo.get_many(&miss).await?;
    let to_cache: Vec<ChannelHealthScore> = from_pg.values().cloned().collect();
    cache.put_many(to_cache);
    hit.extend(from_pg);
    Ok(hit)
}

// 让 ChannelHealthScore 用 chrono types（避免误导 import 用户）
#[allow(dead_code)]
fn _phantom_chrono(_: DateTime<Utc>) {}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gate_storage::ChannelHealthScore;
    use std::thread::sleep;
    use uuid::Uuid;

    fn cid() -> ChannelId {
        ChannelId::from(Uuid::new_v4())
    }

    #[test]
    fn miss_returns_none() {
        let cache = HealthScoreCache::default();
        assert!(cache.get(cid()).is_none());
    }

    #[test]
    fn put_then_get_within_ttl() {
        let cache = HealthScoreCache::new(Duration::from_secs(60));
        let id = cid();
        let mut s = ChannelHealthScore::fresh(id);
        s.score = 0.42;
        cache.put(s);
        let got = cache.get(id).unwrap();
        assert!((got.score - 0.42).abs() < 1e-9);
    }

    #[test]
    fn get_expires_after_ttl() {
        let cache = HealthScoreCache::new(Duration::from_millis(20));
        let id = cid();
        cache.put(ChannelHealthScore::fresh(id));
        assert!(cache.get(id).is_some());
        sleep(Duration::from_millis(40));
        assert!(cache.get(id).is_none());
    }

    #[test]
    fn cleanup_removes_expired_only() {
        let cache = HealthScoreCache::new(Duration::from_millis(20));
        let live = cid();
        let stale = cid();
        cache.put(ChannelHealthScore::fresh(stale));
        sleep(Duration::from_millis(40));
        cache.put(ChannelHealthScore::fresh(live));
        let removed = cache.cleanup_expired();
        assert_eq!(removed, 1);
        assert!(cache.get(live).is_some());
        assert!(cache.get(stale).is_none());
    }

    #[test]
    fn invalidate_removes_entry() {
        let cache = HealthScoreCache::default();
        let id = cid();
        cache.put(ChannelHealthScore::fresh(id));
        cache.invalidate(id);
        assert!(cache.get(id).is_none());
    }

    #[test]
    fn get_many_partitions_hit_and_miss() {
        let cache = HealthScoreCache::new(Duration::from_secs(60));
        let a = cid();
        let b = cid();
        let c = cid();
        cache.put(ChannelHealthScore::fresh(a));
        cache.put(ChannelHealthScore::fresh(c));
        let (hit, miss) = cache.get_many(&[a, b, c]);
        assert_eq!(hit.len(), 2);
        assert!(hit.contains_key(&a));
        assert!(hit.contains_key(&c));
        assert_eq!(miss, vec![b]);
    }

    #[test]
    fn put_many_bulk_insert() {
        let cache = HealthScoreCache::default();
        let a = cid();
        let b = cid();
        cache.put_many(vec![
            ChannelHealthScore::fresh(a),
            ChannelHealthScore::fresh(b),
        ]);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn clear_empties_cache() {
        let cache = HealthScoreCache::default();
        cache.put(ChannelHealthScore::fresh(cid()));
        cache.put(ChannelHealthScore::fresh(cid()));
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn arc_clone_shares_state() {
        let a = HealthScoreCache::default();
        let b = a.clone();
        let id = cid();
        a.put(ChannelHealthScore::fresh(id));
        assert!(b.get(id).is_some());
    }

    #[tokio::test]
    async fn cached_get_many_backfills_from_repo() {
        use gate_storage::{ChannelHealthScoreRepo, InMemoryChannelHealthScoreRepo};

        let repo = InMemoryChannelHealthScoreRepo::new();
        let cache = HealthScoreCache::new(Duration::from_secs(60));
        let a = cid();
        let b = cid();
        // 只有 b 在 repo 里
        repo.ensure_row(b).await.unwrap();

        let result = cached_get_many(&cache, &repo, &[a, b]).await.unwrap();
        // 只有 b 命中（a 在 repo 也不存在 → 返回不含 a）
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&b));
        // 回填后 cache 含 b
        assert!(cache.get(b).is_some());
    }
}
