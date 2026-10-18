//! T-M8-04 验收：`poly plugin new` 脚手架 / 占位符 / 非空目录拒绝、`poly plugin digest`
//! SRI 口径，以及（`POLY_CONTRACT_REAL=1`）生成 → build.sh → 宿主 Sandbox 加载并 package。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use poly_core::PluginGrant;
use poly_plugin::{PackageInput, ResourceLimits, Sandbox, content_digest};

const TEMPLATE_FILES: [&str; 13] = [
    ".github/workflows/ci.yml",
    ".gitignore",
    "Cargo.toml",
    "README.md",
    "build.sh",
    "componentize/Cargo.toml",
    "componentize/src/main.rs",
    "contract/Cargo.toml",
    "contract/src/lib.rs",
    "contract/tests/contract.rs",
    "plugin/Cargo.toml",
    "plugin/src/lib.rs",
    "plugin/wit/polypack-target.wit",
];

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-plugin-new-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

fn combined(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// 目录下全部文件（相对路径 → 内容）。
fn files_under(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("目录可读").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("子路径前缀成立")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, std::fs::read(&path).expect("文件可读")));
        }
    }
    files.sort();
    files
}

fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn target_installed() -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim() == "wasm32-unknown-unknown")
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------- 契约

#[test]
fn new_writes_full_template_and_replaces_placeholders() {
    let workspace = TempDir::new("full");
    let out = poly(
        &["plugin", "new", "demo-target", "--dir", "demo-plugin"],
        workspace.path(),
    );
    assert!(out.status.success(), "生成失败: {}", combined(&out));

    let root = workspace.path().join("demo-plugin");
    let files = files_under(&root);
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, TEMPLATE_FILES, "模板文件集不符");

    for (name, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        for token in [
            "__polypack_plugin_id__",
            "__polypack_plugin_name__",
            "__polypack_plugin_crate__",
        ] {
            assert!(!text.contains(token), "{name} 残留占位符 {token}");
        }
    }

    let cargo = std::fs::read_to_string(root.join("plugin/Cargo.toml")).expect("可读");
    assert!(
        cargo.contains("name = \"polypack-target-demo-target\""),
        "{cargo}"
    );
    assert!(cargo.contains("PolyPack 插件目标：Demo Target"), "{cargo}");

    let lib = std::fs::read_to_string(root.join("plugin/src/lib.rs")).expect("可读");
    assert!(
        lib.contains("const PLUGIN_ID: &str = \"demo-target\";"),
        "{lib}"
    );

    let readme = std::fs::read_to_string(root.join("README.md")).expect("可读");
    assert!(readme.contains("# Demo Target"));
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("可读");
    assert!(ci.contains("cargo test"), "模板 CI 必须有契约测试步骤");

    // 模板 workspace 独立，且自带 `[workspace]`，不被主 workspace 吸收。
    assert!(cargo.contains("[workspace]"));
    assert!(root.join("Cargo.toml").exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(root.join("build.sh"))
            .expect("build.sh 存在")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "build.sh 必须可执行（mode {mode:o}）");
    }

    let text = combined(&out);
    assert!(text.contains("build.sh"), "下一步应提示 build.sh: {text}");
    assert!(text.contains("[[plugins]]"), "下一步应提示注册示例: {text}");
    assert!(
        text.contains("poly plugin digest"),
        "下一步应提示 SRI 来源: {text}"
    );
}

#[test]
fn new_refuses_non_empty_dir_without_force() {
    let workspace = TempDir::new("non-empty");
    let root = workspace.path().join("demo-plugin");
    std::fs::create_dir_all(&root).expect("目录可建");
    std::fs::write(root.join("keep.txt"), b"keep").expect("文件可写");

    let out = poly(
        &["plugin", "new", "demo-target", "--dir", "demo-plugin"],
        workspace.path(),
    );
    assert!(!out.status.success(), "非空目录必须拒绝");
    let text = combined(&out);
    assert!(text.contains("非空"), "{text}");
    assert!(text.contains("--force"), "{text}");
    assert_eq!(std::fs::read(root.join("keep.txt")).expect("保留"), b"keep");

    let forced = poly(
        &[
            "plugin",
            "new",
            "demo-target",
            "--dir",
            "demo-plugin",
            "--force",
        ],
        workspace.path(),
    );
    assert!(
        forced.status.success(),
        "--force 应放行: {}",
        combined(&forced)
    );
    assert_eq!(std::fs::read(root.join("keep.txt")).expect("保留"), b"keep");
    assert!(root.join("plugin/src/lib.rs").exists());
}

#[test]
fn new_rejects_invalid_id() {
    let workspace = TempDir::new("bad-id");
    for id in ["Bad_Id", "-lead", "a/b", ""] {
        let out = poly(&["plugin", "new", id], workspace.path());
        assert!(!out.status.success(), "非法 id 必须拒绝: {id:?}");
    }
    assert!(
        !workspace.path().join("Bad_Id").exists(),
        "非法 id 不得落盘"
    );
}

#[test]
fn digest_matches_host_sri_format() {
    let workspace = TempDir::new("digest");
    let wasm = workspace.path().join("plugin.wasm");
    let bytes = b"\0asm fake plugin bytes";
    std::fs::write(&wasm, bytes).expect("测试文件可写");

    let out = poly(
        &["plugin", "digest", wasm.to_str().expect("路径 UTF-8")],
        workspace.path(),
    );
    assert!(out.status.success(), "digest 失败: {}", combined(&out));
    let expected = format!("blake3-{}", blake3::hash(bytes).to_hex());
    // 人读通道带启动横幅 / 结束行（T-M1-14），摘要行按 `blake3-` 前缀取。
    let stdout = String::from_utf8_lossy(&out.stdout);
    let printed = stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("blake3-"))
        .unwrap_or_else(|| panic!("stdout 无摘要行: {stdout}"));
    assert_eq!(printed, expected);

    let json = poly(
        &[
            "--json",
            "plugin",
            "digest",
            wasm.to_str().expect("路径 UTF-8"),
        ],
        workspace.path(),
    );
    assert!(json.status.success());
    let line = String::from_utf8_lossy(&json.stdout).trim().to_string();
    let value: serde_json::Value = serde_json::from_str(&line).expect("JSON Line");
    assert_eq!(value["event"], "plugin-digest");
    assert_eq!(value["digest"], expected);
}

/// 端到端：新生成工程 → build.sh 产出组件 → 宿主 Sandbox 加载（SRI）→ describe/package。
///
/// 重活（真编译 wasm + 组件封装）只在 `POLY_CONTRACT_REAL=1` 时执行；默认 SKIP-with-print，
/// 与仓库其它真实链路契约测试一致。
#[test]
fn scaffolded_plugin_loads_and_packages_via_host_sandbox() {
    if !real_mode() {
        eprintln!("SKIP：端到端真跑需 POLY_CONTRACT_REAL=1（模板编译 wasm + 宿主加载）");
        return;
    }
    if !cfg!(unix) || !target_installed() {
        eprintln!("SKIP：端到端需 unix + wasm32-unknown-unknown target");
        return;
    }
    let workspace = TempDir::new("e2e");
    let out = poly(&["plugin", "new", "e2e-target"], workspace.path());
    assert!(out.status.success(), "生成失败: {}", combined(&out));
    let root = workspace.path().join("e2e-target");

    let started = Instant::now();
    let build = Command::new("bash")
        .arg("build.sh")
        .current_dir(&root)
        // 不用父 cargo 的 target dir（避免测试期间的目标目录锁竞争）。
        .env_remove("CARGO_TARGET_DIR")
        .env("CARGO_BUILD_JOBS", "1")
        .output()
        .expect("bash 可执行");
    assert!(
        build.status.success(),
        "build.sh 失败: {}",
        combined(&build)
    );
    let wasm_path = root.join("dist/plugin.wasm");
    let wasm = std::fs::read(&wasm_path).expect("产物可读");

    let digest = content_digest(&wasm);
    let sandbox = Sandbox::new(ResourceLimits::default()).expect("沙箱可建");
    let grant = PluginGrant {
        id: "e2e-target".to_string(),
        path: "dist/plugin.wasm".to_string(),
        fs_read: Vec::new(),
        fs_write: Vec::new(),
        net: Vec::new(),
    };
    let plugin = sandbox
        .load(&wasm, "e2e-target", Some(&digest), &root, &grant)
        .expect("宿主加载（SRI 校验）通过");
    let input = PackageInput {
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
    };
    let spec = sandbox.describe(&plugin, &input).expect("describe 通过");
    assert_eq!(spec.spec_version, "target-spec/v1");
    assert_eq!(spec.id, "e2e-target");
    let artifact = sandbox.package(&plugin, &input).expect("package 通过");
    assert_eq!(artifact.format, "example-bin");
    assert_eq!(artifact.bytes, b"index.html\n<html>\napp.js\nJS\nCORE");
    assert_eq!(plugin.digest(), digest);

    eprintln!(
        "PASS e2e scaffold→build→load→package：wasm={}B digest={} 耗时={:.1}s",
        wasm.len(),
        digest,
        started.elapsed().as_secs_f64()
    );
}
