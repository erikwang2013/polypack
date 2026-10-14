//! T-M1-09 验收 1：缓存键对五类输入任一变化敏感；`of_tree` 确定性。

mod common;

use std::fs;
use std::path::Path;

use common::TempDir;
use poly_cache::{CacheInputs, Digest, cache_key};

/// 在 root 下写相对路径与内容（自动建父目录）。
fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("相对路径含父目录")).unwrap();
    fs::write(path, content).unwrap();
}

/// 以 root 为源树构造基准输入。
fn inputs(root: &Path) -> CacheInputs {
    CacheInputs {
        sources: Digest::of_tree(root, &["target"]).unwrap(),
        lockfile: Digest::of_bytes("Cargo.lock 内容".as_bytes()),
        target_spec: "linux-x64: deb, msi".into(),
        toolchain: "rustc 1.99.0".into(),
        profile: "ci".into(),
    }
}

#[test]
fn cache_key_sensitive_to_every_input() {
    let tmp = TempDir::new("key-sensitivity");
    write(tmp.path(), "src/main.rs", "fn main() {}");
    let base = inputs(tmp.path());
    let base_key = cache_key(&base);

    // 稳定性：相同输入 → 相同键
    assert_eq!(cache_key(&inputs(tmp.path())), base_key);

    // 1) 源树改一个字节
    let mut changed = base.clone();
    write(tmp.path(), "src/main.rs", "fn main() {  }");
    changed.sources = Digest::of_tree(tmp.path(), &["target"]).unwrap();
    assert_ne!(changed.sources, base.sources, "源树内容变化未改摘要");
    assert_ne!(cache_key(&changed), base_key, "源树变化未改键");

    // 2) 依赖锁文件
    let mut changed = base.clone();
    changed.lockfile = Digest::of_bytes("Cargo.lock 内容（多一行）".as_bytes());
    assert_ne!(cache_key(&changed), base_key, "锁文件变化未改键");

    // 3) TargetSpec 版本
    let mut changed = base.clone();
    changed.target_spec = "macos-arm64: dmg".into();
    assert_ne!(cache_key(&changed), base_key, "target_spec 变化未改键");

    // 4) 工具链版本
    let mut changed = base.clone();
    changed.toolchain = "rustc 1.98.0".into();
    assert_ne!(cache_key(&changed), base_key, "toolchain 变化未改键");

    // 5) 构建参数 profile
    let mut changed = base.clone();
    changed.profile = "local".into();
    assert_ne!(cache_key(&changed), base_key, "profile 变化未改键");
}

#[test]
fn cache_key_has_no_field_boundary_ambiguity() {
    let base = CacheInputs {
        sources: Digest::of_bytes(b"s"),
        lockfile: Digest::of_bytes(b"l"),
        target_spec: "ab".into(),
        toolchain: "c".into(),
        profile: "ci".into(),
    };
    let shifted = CacheInputs {
        target_spec: "a".into(),
        toolchain: "bc".into(),
        ..base.clone()
    };
    assert_ne!(
        cache_key(&shifted),
        cache_key(&base),
        "字段边界不同不应碰撞"
    );
}

#[test]
fn of_tree_is_deterministic_and_excludes() {
    let a = TempDir::new("tree-a");
    let b = TempDir::new("tree-b");
    // 相同内容，不同创建顺序（含子目录）
    write(a.path(), "src/lib.rs", "pub fn f() {}");
    write(a.path(), "README.md", "hi");
    write(a.path(), "src/deep/mod.rs", "mod deep;");
    write(b.path(), "src/deep/mod.rs", "mod deep;");
    write(b.path(), "README.md", "hi");
    write(b.path(), "src/lib.rs", "pub fn f() {}");
    let plain_a = Digest::of_tree(a.path(), &[]).unwrap();
    assert_eq!(
        plain_a,
        Digest::of_tree(b.path(), &[]).unwrap(),
        "创建顺序不应影响摘要"
    );

    // 排除目录的增删改都不参与
    write(a.path(), "node_modules/junk.js", "x");
    write(a.path(), "node_modules/sub/y.js", "y");
    write(b.path(), "node_modules/other.js", "完全不同");
    assert_eq!(
        Digest::of_tree(a.path(), &["node_modules"]).unwrap(),
        Digest::of_tree(b.path(), &["node_modules"]).unwrap(),
        "排除目录内容不应参与摘要"
    );
    // 不排除时 node_modules 参与摘要
    assert_ne!(Digest::of_tree(a.path(), &[]).unwrap(), plain_a);

    // 路径前缀式排除（target/debug）不误伤同名前缀
    let c = TempDir::new("tree-c");
    write(c.path(), "target/debug/bin", "b");
    let with_debug = Digest::of_tree(c.path(), &["target/debug"]).unwrap();
    write(c.path(), "target/debug/other", "b2");
    assert_eq!(
        Digest::of_tree(c.path(), &["target/debug"]).unwrap(),
        with_debug,
        "前缀排除目录内新增文件不应参与摘要"
    );
    write(c.path(), "target/release/bin", "b");
    assert_ne!(
        Digest::of_tree(c.path(), &["target/debug"]).unwrap(),
        with_debug,
        "前缀之外的目录仍应参与摘要"
    );
}

#[cfg(unix)]
#[test]
fn of_tree_hashes_symlink_target_text() {
    use std::os::unix::fs::symlink;

    let a = TempDir::new("tree-link-a");
    let b = TempDir::new("tree-link-b");
    for dir in [a.path(), b.path()] {
        write(dir, "real.txt", "hi");
        symlink("real.txt", dir.join("link")).unwrap();
    }
    assert_eq!(
        Digest::of_tree(a.path(), &[]).unwrap(),
        Digest::of_tree(b.path(), &[]).unwrap()
    );
    // 目标文本不同 → 摘要不同（按链接文本参与，不跟随）
    fs::remove_file(b.path().join("link")).unwrap();
    symlink("other.txt", b.path().join("link")).unwrap();
    assert_ne!(
        Digest::of_tree(a.path(), &[]).unwrap(),
        Digest::of_tree(b.path(), &[]).unwrap()
    );
}

#[test]
fn digest_parse_roundtrips_and_rejects_junk() {
    let digest = Digest::of_bytes(b"abc");
    assert_eq!(digest.as_str().len(), 64);
    assert!(
        digest
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(Digest::parse(digest.as_str()).unwrap(), digest);
    assert_eq!(digest.to_string(), digest.as_str());
    // 大写归一化
    assert_eq!(
        Digest::parse(&digest.as_str().to_uppercase()).unwrap(),
        digest
    );
    // 长度 / 字符非法
    assert!(Digest::parse("abc").is_err());
    assert!(Digest::parse(&"z".repeat(64)).is_err());
    // Serialize 为字符串，可被 parse 读回
    let json = serde_json::to_string(&digest).unwrap();
    assert_eq!(Digest::parse(json.trim_matches('"')).unwrap(), digest);
    assert_eq!(Digest::of_bytes(b"abc"), Digest::of_bytes(b"abc"));
    assert_ne!(Digest::of_bytes(b"abc"), Digest::of_bytes(b"abd"));
}
