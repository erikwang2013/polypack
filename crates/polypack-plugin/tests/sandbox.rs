//! 沙箱真跑测试：参考插件 describe/package、逃逸负向集、资源上限。
//!
//! 固定件由 `guests/build.sh` 生成（wasm32-unknown-unknown + wit-component 封装）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use polypack_core::{PluginGrant, codes};
use polypack_plugin::limits::MAX_FUEL;
use polypack_plugin::{
    PackageInput, Plugin, ResourceLimits, Sandbox, content_digest, validate_spec_version,
};

const REFERENCE: &[u8] = include_bytes!("fixtures/reference-plugin.wasm");
const PROBE: &[u8] = include_bytes!("fixtures/capability-probe.wasm");
const OUTDATED: &[u8] = include_bytes!("fixtures/outdated-version.wasm");

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("时钟可用")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "polypack-plugin-{tag}-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("临时目录可建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// 写文件（自动建父目录）。
    fn file(&self, rel: &str, contents: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("父目录可建");
        }
        std::fs::write(&path, contents).expect("测试文件可写");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn grant(fs_read: &[&str], fs_write: &[&str], net: &[&str]) -> PluginGrant {
    PluginGrant {
        id: "probe".to_string(),
        path: "capability-probe.wasm".to_string(),
        fs_read: fs_read.iter().map(|s| (*s).to_string()).collect(),
        fs_write: fs_write.iter().map(|s| (*s).to_string()).collect(),
        net: net.iter().map(|s| (*s).to_string()).collect(),
        sri: None,
        signature: None,
        trust_root: None,
    }
}

fn sandbox() -> Sandbox {
    Sandbox::new(ResourceLimits::default()).expect("沙箱可建")
}

fn load_probe(sandbox: &Sandbox, root: &Path, grant: &PluginGrant) -> Plugin {
    sandbox
        .load(PROBE, "probe", None, root, grant)
        .expect("探针可加载")
}

fn load_reference(sandbox: &Sandbox, root: &Path) -> Plugin {
    let sri = content_digest(REFERENCE);
    sandbox
        .load(
            REFERENCE,
            "reference",
            Some(&sri),
            root,
            &grant(&[], &[], &[]),
        )
        .expect("参考插件可加载")
}

fn input() -> PackageInput {
    PackageInput {
        name: "demo".to_string(),
        version: "1.2.3".to_string(),
        target: "linux".to_string(),
        entry: "index.html".to_string(),
        files: vec![
            ("index.html".to_string(), b"<html>".to_vec()),
            ("app.js".to_string(), b"JS".to_vec()),
        ],
        triple: "wasm32-unknown-unknown".to_string(),
        lib: b"CORE".to_vec(),
    }
}

/// 单次 HTTP 200 响应；返回 URL 与服务器线程。
fn serve_once(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("本地端口可绑");
    let addr = listener.local_addr().expect("地址可读");
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("有连接");
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("响应头可写");
        stream.write_all(body).expect("响应体可写");
    });
    (format!("http://{addr}/"), handle)
}

#[test]
fn reference_plugin_describes_target_spec_v1() {
    let dir = TempDir::new("describe");
    let sandbox = sandbox();
    let plugin = load_reference(&sandbox, dir.path());
    let spec = sandbox.describe(&plugin, &input()).expect("describe 成功");

    assert_eq!(spec.spec_version, "target-spec/v1");
    assert_eq!(spec.id, "linux-reference");
    assert_eq!(spec.tier, 3);
    assert_eq!(spec.runtime, "wasm");
    assert_eq!(spec.bridge, "wasm-bindgen");
    assert_eq!(spec.package_format, "reference-bin");
    assert_eq!(spec.builder, "reference-plugin");
    assert_eq!(spec.hosts, ["linux", "macos", "windows"]);
    assert_eq!(plugin.digest(), content_digest(REFERENCE));
}

#[test]
fn reference_plugin_packages_ui_and_core_lib() {
    let dir = TempDir::new("package");
    let sandbox = sandbox();
    let plugin = load_reference(&sandbox, dir.path());
    let artifact = sandbox.package(&plugin, &input()).expect("package 成功");

    assert_eq!(artifact.format, "reference-bin");
    // 参考插件契约：`<文件名>\n<内容>\n` 依序拼接，再追加核心库字节。
    assert_eq!(artifact.bytes, b"index.html\n<html>\napp.js\nJS\nCORE");
}

#[test]
fn sri_mismatch_is_rejected() {
    let dir = TempDir::new("sri");
    let sandbox = sandbox();
    let result = sandbox.load(
        REFERENCE,
        "reference",
        Some("blake3-00"),
        dir.path(),
        &grant(&[], &[], &[]),
    );
    let err = match result {
        Ok(_) => panic!("摘要不符必须拒绝"),
        Err(err) => err,
    };
    assert_eq!(err.code, codes::E_PLUGIN_0006);
}

#[test]
fn non_v1_spec_version_is_rejected_with_migration_hint() {
    let err = validate_spec_version("target-spec/v2").expect_err("非 v1 必须拒绝");
    assert_eq!(err.code, codes::E_PLUGIN_0005);
    assert!(err.hint.expect("有迁移提示").contains("target-spec/v1"));
    assert!(validate_spec_version("target-spec/v1").is_ok());
}

#[test]
fn plugin_declaring_non_v1_is_rejected_by_host() {
    let dir = TempDir::new("outdated");
    let sandbox = sandbox();
    let plugin = sandbox
        .load(
            OUTDATED,
            "outdated",
            None,
            dir.path(),
            &grant(&[], &[], &[]),
        )
        .expect("旧版本插件可加载");
    let err = sandbox
        .describe(&plugin, &input())
        .expect_err("target-spec/v2 必须被宿主拒绝");
    assert_eq!(err.code, codes::E_PLUGIN_0005);
    assert!(err.hint.expect("有迁移提示").contains("target-spec/v1"));
}

#[test]
fn empty_grant_denies_filesystem_and_network_by_default() {
    let dir = TempDir::new("default-deny");
    dir.file("allowed/a.txt", b"hello");
    dir.file("out/keep.txt", b"");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &[]));

    let read = sandbox
        .probe(&plugin, "read", "allowed/a.txt")
        .expect("探针可跑");
    assert!(read.starts_with("polypack::E_PLUGIN_0001|"), "读到: {read}");
    let write = sandbox
        .probe(&plugin, "write", "out/new.txt")
        .expect("探针可跑");
    assert!(write.starts_with("polypack::E_PLUGIN_0001|"), "写入: {write}");
    let fetch = sandbox
        .probe(&plugin, "fetch", "http://example.com/")
        .expect("探针可跑");
    assert!(fetch.starts_with("polypack::E_PLUGIN_0002|"), "出网: {fetch}");
}

#[test]
fn traversal_and_absolute_paths_are_denied() {
    let dir = TempDir::new("traversal");
    dir.file("allowed/a.txt", b"hello");
    let secret = dir.file("secret.txt", b"top-secret");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&["allowed"], &[], &[]));

    let relative = sandbox
        .probe(&plugin, "read", "../secret.txt")
        .expect("探针可跑");
    assert!(
        relative.starts_with("polypack::E_PLUGIN_0001|"),
        "相对穿越: {relative}"
    );
    let absolute = secret.to_str().expect("路径是 UTF-8");
    let absolute = sandbox.probe(&plugin, "read", absolute).expect("探针可跑");
    assert!(
        absolute.starts_with("polypack::E_PLUGIN_0001|"),
        "绝对路径: {absolute}"
    );
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_denied() {
    let dir = TempDir::new("symlink");
    let secret = dir.file("secret.txt", b"top-secret");
    dir.file("allowed/a.txt", b"hello");
    std::os::unix::fs::symlink(&secret, dir.path().join("allowed/link")).expect("符号链接可建");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&["allowed"], &[], &[]));

    let out = sandbox
        .probe(&plugin, "read", "allowed/link")
        .expect("探针可跑");
    assert!(
        out.starts_with("polypack::E_PLUGIN_0001|"),
        "符号链接逃逸: {out}"
    );
}

#[test]
fn write_outside_granted_dir_leaves_no_file() {
    let dir = TempDir::new("write-deny");
    dir.file("out/keep.txt", b"");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &["out"], &[]));

    let out = sandbox
        .probe(&plugin, "write", "out/../escaped.txt")
        .expect("探针可跑");
    assert!(out.starts_with("polypack::E_PLUGIN_0001|"), "越界写: {out}");
    assert!(!dir.path().join("escaped.txt").exists(), "越界写不得落盘");
}

#[test]
fn granted_read_and_write_are_allowed() {
    let dir = TempDir::new("allow");
    dir.file("allowed/a.txt", b"hello");
    dir.file("out/keep.txt", b"");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&["allowed"], &["out"], &[]));

    assert_eq!(
        sandbox
            .probe(&plugin, "read", "allowed/a.txt")
            .expect("探针可跑"),
        "ok:5"
    );
    assert_eq!(
        sandbox
            .probe(&plugin, "write", "out/new.txt")
            .expect("探针可跑"),
        "ok:5"
    );
    assert_eq!(
        std::fs::read(dir.path().join("out/new.txt")).expect("已写入"),
        b"probe"
    );
}

#[test]
fn unauthorized_domain_is_denied_before_network_access() {
    let dir = TempDir::new("net-deny");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &["allowed.example"]));

    let out = sandbox
        .probe(&plugin, "fetch", "http://127.0.0.1:9/")
        .expect("探针可跑");
    assert!(out.starts_with("polypack::E_PLUGIN_0002|"), "未授权域名: {out}");
}

#[test]
fn explicitly_granted_localhost_domain_can_fetch() {
    let (url, server) = serve_once(b"hello-egress");
    let host = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let dir = TempDir::new("net-allow");
    let sandbox = sandbox();
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &[host.as_str()]));

    let out = sandbox.probe(&plugin, "fetch", &url).expect("探针可跑");
    server.join().expect("服务器线程结束");
    assert_eq!(out, "ok:12", "授权域名应可出网");
}

#[test]
fn infinite_loop_hits_fuel_limit() {
    let limits = ResourceLimits::new(1_000_000, 60_000, 16 << 20).expect("上限合法");
    let sandbox = Sandbox::new(limits).expect("沙箱可建");
    let dir = TempDir::new("fuel");
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &[]));

    let err = sandbox
        .probe(&plugin, "loop", "")
        .expect_err("死循环必须被截断");
    assert_eq!(err.code, codes::E_PLUGIN_0003);
}

#[test]
fn infinite_loop_hits_epoch_timeout() {
    // fuel 放到硬上限，保证先撞 epoch（1 tick = 10ms）而不是 fuel。
    let limits = ResourceLimits::new(MAX_FUEL, 1, 16 << 20).expect("上限合法");
    let sandbox = Sandbox::new(limits).expect("沙箱可建");
    let dir = TempDir::new("epoch");
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &[]));

    let err = sandbox
        .probe(&plugin, "loop", "")
        .expect_err("死循环必须被墙钟截断");
    assert_eq!(err.code, codes::E_PLUGIN_0003);
    assert!(err.message.contains("epoch"), "应归类为 epoch 超时: {err}");
}

#[test]
fn memory_growth_beyond_limit_is_denied() {
    let limits = ResourceLimits::new(1_000_000_000, 500, 16 << 20).expect("上限合法");
    let sandbox = Sandbox::new(limits).expect("沙箱可建");
    let dir = TempDir::new("memory");
    let plugin = load_probe(&sandbox, dir.path(), &grant(&[], &[], &[]));

    assert_eq!(
        sandbox.probe(&plugin, "memory", "").expect("探针可跑"),
        "denied",
        "超过内存上限的 memory.grow 必须失败"
    );
}

#[test]
fn fixtures_carry_no_host_paths_or_debug_info() {
    for bytes in [REFERENCE, PROBE, OUTDATED] {
        let text = String::from_utf8_lossy(bytes);
        assert!(
            !text.contains(env!("CARGO_MANIFEST_DIR")),
            "制品含构建目录绝对路径"
        );
        assert!(!text.contains("/home/"), "制品含宿主绝对路径");
        assert!(!text.contains("CARGO_MANIFEST_DIR"), "制品含环境变量名");
        assert!(
            !bytes
                .windows(b".debug_info".len())
                .any(|w| w == b".debug_info"),
            "制品含调试符号"
        );
    }
}
