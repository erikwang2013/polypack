//! T-M8-08 验收 1/3：共享目录 CAS 后端（生产形态）与降级后备。

mod common;

use std::fs;
use std::sync::{Arc, Mutex};

use common::TempDir;
use poly_cache::{Digest, DirSharedStore, FallbackStore, LocalStore, ObjectStore};

/// 共享目录后端：写入 → 命中 → 损坏检出并删除 → 条目列表。
#[test]
fn shared_dir_store_roundtrips_and_evicts_corruption() {
    let tmp = TempDir::new("shared");
    let root = tmp.path().join("cas");
    let store = DirSharedStore::new(&root);
    let key = Digest::of_bytes(b"shared-key");
    let payload = b"\x00shared artifact\xff";

    assert_eq!(store.get(&key).unwrap(), None, "空后端应未命中");
    store.put(&key, payload).unwrap();
    assert_eq!(
        store.get(&key).unwrap().as_deref(),
        Some(payload.as_slice()),
        "不同进程（新实例）读同一共享目录应命中"
    );

    // 模拟跨机传输损坏：篡改载荷 → 未命中且损坏条目被删除
    let path = root.join(&key.as_str()[..2]).join(key.as_str());
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&path, &bytes).unwrap();
    assert_eq!(store.get(&key).unwrap(), None, "损坏条目视为未命中");
    assert!(!path.exists(), "损坏条目应被删除");

    // 条目列表只收本后端写的对象（临时文件 / 非摘要文件名不入列）
    store.put(&key, payload).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, key);
    assert_eq!(store.remove(&key).unwrap(), entries[0].bytes);
    assert!(store.entries().unwrap().is_empty());
}

/// 主后端不可用（路径被普通文件占住）→ 告警一次并落到后备后端。
#[test]
fn fallback_store_degrades_on_primary_io_error() {
    let tmp = TempDir::new("fallback");
    let blocked = tmp.path().join("blocked");
    fs::write(&blocked, b"not a directory").unwrap();
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let store = FallbackStore::new(
        DirSharedStore::new(blocked.join("shared")),
        LocalStore::new(tmp.path().join("local")),
        {
            let warnings = Arc::clone(&warnings);
            move |message: &str| {
                warnings.lock().unwrap().push(message.to_string());
            }
        },
    );
    let key = Digest::of_bytes(b"degraded-key");

    // 读：主后端 IO 错误 → 后备（空）未命中，构建侧看到的只是普通 miss
    assert_eq!(store.get(&key).unwrap(), None);
    // 写：主后端失败不阻断，后备写入成功
    store.put(&key, b"payload").unwrap();
    // 再读：主后端仍错误，但后备命中
    assert_eq!(
        store.get(&key).unwrap().as_deref(),
        Some(b"payload".as_slice())
    );

    let warnings = warnings.lock().unwrap();
    assert_eq!(warnings.len(), 1, "降级告警只报一次: {warnings:?}");
    assert!(warnings[0].contains("降级"), "告警应说明降级: {warnings:?}");
}
