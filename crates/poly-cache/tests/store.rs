//! T-M1-09 验收 2/3：二跑命中、命中统计、目录往返、损坏检出。

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, object_path};
use poly_cache::{Cache, CacheStats, Digest, LocalStore, ObjectStore};

/// 在 root 下写相对路径与内容（自动建父目录）。
fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("相对路径含父目录")).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn second_lookup_hits_and_stats_track_rates() {
    let tmp = TempDir::new("hit");
    let cache = Cache::from_root(tmp.path().join("cas"));
    let key = Digest::of_bytes(b"cache-key-1");
    let payload = b"\x00build artifact\xff".to_vec();

    // 无请求时命中率 0.0
    assert_eq!(cache.stats(), CacheStats { hits: 0, misses: 0 });
    assert_eq!(cache.stats().hit_rate(), 0.0);

    // 一跑：未命中
    assert_eq!(cache.lookup(&key).unwrap(), None);
    assert_eq!(cache.stats(), CacheStats { hits: 0, misses: 1 });
    assert_eq!(cache.stats().hit_rate(), 0.0);

    // 写入后二跑：命中且字节一致
    cache.store(&key, &payload).unwrap();
    assert_eq!(
        cache.lookup(&key).unwrap().as_deref(),
        Some(payload.as_slice())
    );
    assert_eq!(cache.stats(), CacheStats { hits: 1, misses: 1 });
    assert!((cache.stats().hit_rate() - 0.5).abs() < f64::EPSILON);

    // 三跑仍是命中：命中率上升
    assert_eq!(
        cache.lookup(&key).unwrap().as_deref(),
        Some(payload.as_slice())
    );
    assert!((cache.stats().hit_rate() - 2.0 / 3.0).abs() < 1e-12);

    // 新缓存实例（新进程语义）读同一根目录：仍然命中
    let reopened = Cache::from_root(tmp.path().join("cas"));
    assert_eq!(
        reopened.lookup(&key).unwrap().as_deref(),
        Some(payload.as_slice())
    );
}

#[test]
fn store_and_restore_tree_roundtrip() {
    let tmp = TempDir::new("tree");
    let cache = Cache::from_root(tmp.path().join("cas"));
    let src = tmp.path().join("dist");
    write(&src, "index.html", "<html>hi</html>");
    write(&src, "assets/app.js", "console.log(1)");
    write(&src, "assets/img/logo.bin", "\u{0}\u{1}\u{2}");
    let before = Digest::of_tree(&src, &[]).unwrap();

    let key = Digest::of_bytes(b"ui-dist-key");
    // 未入库 → 未命中
    assert!(!cache.restore_tree(&key, &tmp.path().join("nope")).unwrap());
    assert_eq!(cache.stats().misses, 1);

    cache.store_tree(&key, &src).unwrap();

    // 还原到空目录：逐文件落盘、含子目录
    let dest = tmp.path().join("restored");
    assert!(cache.restore_tree(&key, &dest).unwrap());
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(
        Digest::of_tree(&dest, &[]).unwrap(),
        before,
        "还原后目录摘要应与源树一致"
    );
    assert_eq!(
        fs::read_to_string(dest.join("assets/app.js")).unwrap(),
        "console.log(1)"
    );

    // 还原到已存在的目标：覆盖旧内容
    write(&dest, "index.html", "旧内容");
    assert!(cache.restore_tree(&key, &dest).unwrap());
    assert_eq!(
        fs::read_to_string(dest.join("index.html")).unwrap(),
        "<html>hi</html>"
    );
}

#[test]
fn restore_tree_misses_when_blob_evicted() {
    let tmp = TempDir::new("tree-blob");
    let root = tmp.path().join("cas");
    let cache = Cache::from_root(&root);
    let src = tmp.path().join("dist");
    write(&src, "a.txt", "alpha");
    let key = Digest::of_bytes(b"tree-key");
    cache.store_tree(&key, &src).unwrap();

    // 从清单里找出 blob 键并删除（模拟 blob 被 LRU 单独淘汰）
    let manifest = cache.lookup(&key).unwrap().expect("清单应已入库");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    let blob = Digest::parse(manifest[0]["digest"].as_str().unwrap()).unwrap();
    let store = LocalStore::new(&root);
    let size = object_size(&root, &blob);
    assert_eq!(store.remove(&blob).unwrap(), size);

    let dest = tmp.path().join("restored");
    assert!(
        !cache.restore_tree(&key, &dest).unwrap(),
        "blob 缺失应视为未命中"
    );
}

#[test]
fn corrupt_object_is_detected_and_removed() {
    let tmp = TempDir::new("corrupt");
    let root = tmp.path().join("cas");
    let cache = Cache::from_root(&root);

    // 篡改载荷中的一个字节
    let key = Digest::of_bytes(b"artifact-key");
    cache.store(&key, b"good payload").unwrap();
    let path = object_path(&root, &key);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&path, &bytes).unwrap();
    assert_eq!(cache.lookup(&key).unwrap(), None, "损坏条目应视为未命中");
    assert!(!path.exists(), "损坏条目应被删除");

    // 整文件截断成垃圾
    let key = Digest::of_bytes(b"artifact-key-2");
    cache.store(&key, b"another payload").unwrap();
    let path = object_path(&root, &key);
    fs::write(&path, b"junk").unwrap();
    assert_eq!(cache.lookup(&key).unwrap(), None);
    assert!(!path.exists(), "截断条目应被删除");

    assert_eq!(cache.stats(), CacheStats { hits: 0, misses: 2 });
    // 损坏条目不再出现在条目列表里
    assert!(LocalStore::new(&root).entries().unwrap().is_empty());
}

/// 对象文件字节数（释放字节数的期望值）。
fn object_size(root: &Path, key: &Digest) -> u64 {
    fs::metadata(object_path(root, key)).unwrap().len()
}
