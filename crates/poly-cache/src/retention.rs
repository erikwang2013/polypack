//! 缓存侧清理与保留策略（ROADMAP §5.5，T-M1-15）：先按年龄、再按 mtime LRU 淘汰。

use std::time::{Duration, SystemTime};

use crate::digest::Digest;
use crate::error::CacheError;
use crate::store::ObjectStore;

const GIB: u64 = 1024 * 1024 * 1024;
const DAY: u64 = 86_400;

/// 保留策略（`polypack.toml` 的 `[retention]` 由 poly-core 侧映射为本类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// 保留天数上限。
    pub cache_days: u32,
    /// 容量上限（GiB）。
    pub cache_gib: u32,
}

/// 淘汰结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Eviction {
    /// 被淘汰的对象键，按淘汰顺序（先年龄、后 LRU）。
    pub removed: Vec<Digest>,
    /// 释放的总字节数。
    pub freed_bytes: u64,
}

/// 执行保留策略：先淘汰 mtime 超过 `cache_days` 的条目，再按 mtime 从旧到新
/// 淘汰到总量 ≤ `cache_gib × 1 GiB`。两条规则都为 0 时清空全部。
pub fn enforce_retention(
    store: &dyn ObjectStore,
    policy: &RetentionPolicy,
) -> Result<Eviction, CacheError> {
    let mut entries = store.entries()?;
    entries.sort_by_key(|e| e.last_used); // 最旧在前
    let now = SystemTime::now();
    let max_age = Duration::from_secs(u64::from(policy.cache_days) * DAY);
    let mut result = Eviction::default();
    let mut survivors = Vec::new();
    for entry in entries {
        // mtime 在未来（时钟回拨等）按年龄 0 处理，不淘汰
        let expired = now
            .duration_since(entry.last_used)
            .is_ok_and(|age| age > max_age);
        if expired {
            result.freed_bytes += store.remove(&entry.key)?;
            result.removed.push(entry.key);
        } else {
            survivors.push(entry);
        }
    }
    let cap = u64::from(policy.cache_gib) * GIB;
    let mut total: u64 = survivors.iter().map(|e| e.bytes).sum();
    for entry in &survivors {
        if total <= cap {
            break;
        }
        result.freed_bytes += store.remove(&entry.key)?;
        result.removed.push(entry.key.clone());
        total -= entry.bytes;
    }
    Ok(result)
}
