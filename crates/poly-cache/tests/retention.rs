//! T-M1-15 缓存侧验收：超龄先淘汰、超容量按 mtime LRU 淘汰、命中 touch 驱动 LRU。

mod common;

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use common::{TempDir, object_path};
use poly_cache::{Digest, LocalStore, ObjectStore, RetentionPolicy, enforce_retention};

const DAY: u64 = 86_400;
const MIB: u64 = 1024 * 1024;

/// 伪造对象文件 mtime（std 1.75+）。
fn set_mtime(path: &Path, ago: Duration) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - ago).unwrap();
}

/// 把对象文件撑到 `len` 字节（稀疏扩展，不占实际磁盘）。
fn inflate(path: &Path, len: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(len).unwrap();
}

#[test]
fn age_rule_evicts_expired_entries() {
    let tmp = TempDir::new("retention-age");
    let root = tmp.path().join("cas");
    let store = LocalStore::new(&root);

    let old = Digest::of_bytes(b"old");
    let recent = Digest::of_bytes(b"recent");
    let fresh = Digest::of_bytes(b"fresh");
    for key in [&old, &recent, &fresh] {
        store.put(key, b"payload").unwrap();
    }
    set_mtime(&object_path(&root, &old), Duration::from_secs(100 * DAY));
    set_mtime(&object_path(&root, &recent), Duration::from_secs(10 * DAY));
    let old_size = fs::metadata(object_path(&root, &old)).unwrap().len();

    // 容量 1 GiB 远大于条目总量：只有年龄规则生效
    let evicted = enforce_retention(
        &store,
        &RetentionPolicy {
            cache_days: 30,
            cache_gib: 1,
        },
    )
    .unwrap();
    assert_eq!(evicted.removed, vec![old.clone()], "仅超龄条目应被淘汰");
    assert_eq!(
        evicted.freed_bytes, old_size,
        "释放字节数应等于删除的文件大小"
    );

    // 命中未被淘汰的条目仍可用
    assert_eq!(
        store.get(&recent).unwrap().as_deref(),
        Some(b"payload".as_slice())
    );
    assert_eq!(
        store.get(&fresh).unwrap().as_deref(),
        Some(b"payload".as_slice())
    );
    assert_eq!(store.get(&old).unwrap(), None);
    assert!(store.entries().unwrap().iter().all(|e| e.key != old));
}

#[test]
fn capacity_rule_evicts_lru_until_under_limit() {
    let tmp = TempDir::new("retention-lru");
    let root = tmp.path().join("cas");
    let store = LocalStore::new(&root);

    // 三个条目：对象文件各撑到 700 MiB（合计 2100 MiB > 1 GiB 上限）
    let keys: Vec<Digest> = [b"a", b"b", b"c"]
        .iter()
        .map(|s| Digest::of_bytes(*s))
        .collect();
    for key in &keys {
        store.put(key, b"seed").unwrap();
        inflate(&object_path(&root, key), 700 * MIB);
    }
    // mtime：a 最旧、c 最新（c 相当于最近使用）
    set_mtime(&object_path(&root, &keys[0]), Duration::from_secs(3000));
    set_mtime(&object_path(&root, &keys[1]), Duration::from_secs(2000));
    set_mtime(&object_path(&root, &keys[2]), Duration::from_secs(1000));

    let evicted = enforce_retention(
        &store,
        &RetentionPolicy {
            cache_days: 30,
            cache_gib: 1,
        },
    )
    .unwrap();
    // 淘汰到 ≤ 1 GiB：删 a 后 1400 MiB 仍超限，再删 b 后剩 700 MiB
    assert_eq!(evicted.removed, vec![keys[0].clone(), keys[1].clone()]);
    assert_eq!(
        evicted.freed_bytes,
        2 * 700 * MIB,
        "释放字节数应等于删除的文件大小"
    );

    let remaining = store.entries().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].key, keys[2], "最近使用的条目应保留");
    assert!(remaining.iter().map(|e| e.bytes).sum::<u64>() <= 1024 * MIB);
}

#[test]
fn age_and_capacity_combine_age_first() {
    let tmp = TempDir::new("retention-both");
    let root = tmp.path().join("cas");
    let store = LocalStore::new(&root);

    let keys: Vec<Digest> = [b"x", b"y", b"z"]
        .iter()
        .map(|s| Digest::of_bytes(*s))
        .collect();
    for key in &keys {
        store.put(key, b"seed").unwrap();
        inflate(&object_path(&root, key), 700 * MIB);
    }
    set_mtime(
        &object_path(&root, &keys[0]),
        Duration::from_secs(100 * DAY),
    ); // 超龄
    set_mtime(&object_path(&root, &keys[1]), Duration::from_secs(2000));
    set_mtime(&object_path(&root, &keys[2]), Duration::from_secs(1000));

    let evicted = enforce_retention(
        &store,
        &RetentionPolicy {
            cache_days: 30,
            cache_gib: 1,
        },
    )
    .unwrap();
    // 先按年龄删 x（1400 MiB 仍超 1 GiB），再按 LRU 删次旧的 y，保留最新 z
    assert_eq!(evicted.removed, vec![keys[0].clone(), keys[1].clone()]);
    assert_eq!(evicted.freed_bytes, 2 * 700 * MIB);
    assert_eq!(store.entries().unwrap()[0].key, keys[2]);
}

#[test]
fn hit_touches_mtime_to_drive_lru() {
    let tmp = TempDir::new("retention-touch");
    let root = tmp.path().join("cas");
    let store = LocalStore::new(&root);
    let key = Digest::of_bytes(b"touched");
    store.put(&key, b"payload").unwrap();

    let path = object_path(&root, &key);
    set_mtime(&path, Duration::from_secs(10 * DAY));
    let before = fs::metadata(&path).unwrap().modified().unwrap();

    assert_eq!(
        store.get(&key).unwrap().as_deref(),
        Some(b"payload".as_slice())
    );

    let after = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(after > before, "命中读取应 touch mtime（LRU 依赖它）");
}
