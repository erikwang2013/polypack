//! GA §7 残余：CLI 经 env 凭据载体解析信任根，并加载真实插件制品（端到端）。
//!
//! 工程配置只出现 `CredentialRef`（`source = { env = { variable } }`），信任根公钥以
//! `base64(JSON) {"public_key":"<base64>"}` 经子进程环境注入；分离签名由一次性 0700
//! GNUPGHOME 内的真实 gpg 密钥对产出。正向跑 `polypack build --target linux-reference`
//! 断言插件被接受且 `describe` / `package` 结果落到产物；负向用另一把公钥与缺失
//! 环境变量分别断言 `E_PLUGIN_0009` / `E_PLUGIN_0010`。
//!
//! 宿主没有 gpg（或 Windows MSYS 路径不兼容）时打印 SKIP 并以通过结束；夹具缺失是
//! 编译期 `include_bytes!` 失败，绝不静默跳过。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// 宿主参考插件（真实 `target-spec/v1` 组件，非占位字节）。
const WASM: &[u8] = include_bytes!("../../polypack-plugin/tests/fixtures/reference-plugin.wasm");
/// 信任根凭据的 env 载体（配置里只写变量名）。
const TRUST_ENV: &str = "POLYPACK_PLUGIN_TRUST_ROOT";
/// 插件 id；参考插件把 describe 的输入 target 拼成 `<target>-reference`。
const PLUGIN_ID: &str = "linux";
const TARGET_ID: &str = "linux-reference";
const PACKAGE_NAME: &str = "trust-root-env";
const PACKAGE_VERSION: &str = "1.0.0";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        // gpg-agent 的 unix socket 路径上限约 104 字节，用短根（同 polypack-plugin 签名测试）。
        #[cfg(unix)]
        let base = PathBuf::from("/tmp");
        #[cfg(not(unix))]
        let base = std::env::temp_dir();
        let path = base.join(format!(
            "pp-cli-trust-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn private_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("目录可建");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .expect("目录可设 0700");
    }
}

fn has_gpg() -> bool {
    std::env::var_os("PATH").is_some_and(|path_var| {
        std::env::split_paths(&path_var).any(|dir| dir.join("gpg").is_file())
    })
}

fn gpg(home: &Path, args: &[&str]) -> Output {
    Command::new("gpg")
        .arg("--batch")
        .arg("--no-options")
        .arg("--homedir")
        .arg(home)
        .args(args)
        .output()
        .expect("gpg 可执行")
}

fn gpg_ok(home: &Path, args: &[&str]) -> Output {
    let output = gpg(home, args);
    assert!(
        output.status.success(),
        "gpg {args:?} 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// 一次性密钥对：签名私钥留在 home，返回 ASCII-armor 公钥。
fn keypair(scratch: &Path, tag: &str) -> (PathBuf, Vec<u8>) {
    let home = scratch.join(format!("gpg-{tag}"));
    private_dir(&home);
    let batch = scratch.join(format!("keygen-{tag}.cfg"));
    std::fs::write(
        &batch,
        "%no-protection\nKey-Type: eddsa\nKey-Curve: ed25519\nKey-Usage: sign\n\
         Name-Real: PolyPack CLI Trust Root\n\
         Name-Email: cli-trust-root@polypack.invalid\nExpire-Date: 0\n%commit\n",
    )
    .expect("keygen 配置可写");
    gpg_ok(&home, &["--generate-key", &batch.to_string_lossy()]);
    let public = gpg_ok(&home, &["--armor", "--export"]).stdout;
    assert!(!public.is_empty(), "公钥导出非空");
    (home, public)
}

/// 信任根载荷契约：base64(JSON) `{"public_key":"<base64>"}`，只含公钥。
fn trust_root_payload(public: &[u8]) -> String {
    let json = format!(r#"{{"public_key":"{}"}}"#, B64.encode(public));
    B64.encode(json)
}

/// 临时工程：真实组件 + 分离签名 + 最小 `dist/` 输入；配置只引用 env 载体。
fn write_project(root: &Path, signature: &[u8]) {
    std::fs::create_dir_all(root.join("plugins")).expect("plugins 目录可建");
    std::fs::write(root.join("plugins/reference-plugin.wasm"), WASM).expect("插件可写");
    std::fs::write(root.join("plugins/reference-plugin.wasm.sig"), signature)
        .expect("分离签名可写");
    std::fs::create_dir_all(root.join("dist")).expect("dist 目录可建");
    std::fs::write(
        root.join("dist/index.html"),
        "<!doctype html><html><body>trust-root-env</body></html>",
    )
    .expect("UI 输入可写");
    std::fs::write(
        root.join("polypack.toml"),
        format!(
            "schema = 1\n\n[package]\nname = \"{PACKAGE_NAME}\"\nversion = \"{PACKAGE_VERSION}\"\n\n\
             [ui]\nframework = \"react\"\nentry = \"index.html\"\n\n[core]\ncrate = \"core\"\n\n\
             [[plugins]]\nid = \"{PLUGIN_ID}\"\npath = \"plugins/reference-plugin.wasm\"\n\
             signature = \"plugins/reference-plugin.wasm.sig\"\n\
             trust_root = {{ name = \"plugin-trust-root\", source = {{ env = {{ variable = \"{TRUST_ENV}\" }} }} }}\n"
        ),
    )
    .expect("配置可写");
}

/// 跑 CLI 的插件加载路径；`trust_root` 为 `None` 时移除环境变量（信任根不可用）。
fn polypack_build(root: &Path, trust_root: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_polypack"));
    command
        .args(["build", "--json", "--target", TARGET_ID])
        .current_dir(root)
        .env_remove(TRUST_ENV);
    if let Some(value) = trust_root {
        command.env(TRUST_ENV, value);
    }
    command.output().expect("polypack 可执行")
}

fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// stdout 的 JSON Lines 事件（`--json` 下 stdout 保持纯 JSON）。
fn events(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn artifact(root: &Path) -> PathBuf {
    let dir = root.join(".polypack/packages").join(TARGET_ID);
    std::fs::read_dir(&dir)
        .expect("插件暂存目录存在")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "reference-bin"))
        .expect("插件产物存在")
}

#[test]
fn cli_resolves_trust_root_from_env_and_loads_signed_plugin() {
    // Windows 的 MSYS gpg 不认 Windows 绝对路径参数；与 polypack-plugin 签名真跑同口径。
    if cfg!(windows) || !has_gpg() {
        eprintln!("SKIP：宿主没有可用的 gpg，跳过 CLI 信任根端到端（Linux/macOS 会走真跑分支）");
        return;
    }

    let dir = TempDir::new();
    let (signer_home, signer_public) = keypair(dir.path(), "signer");
    let source = dir.path().join("plugins/reference-plugin.wasm");
    std::fs::create_dir_all(source.parent().expect("父目录")).expect("plugins 目录可建");
    std::fs::write(&source, WASM).expect("插件可写");
    let signature = dir.path().join("reference-plugin.wasm.sig");
    gpg_ok(
        &signer_home,
        &[
            "--yes",
            "--detach-sign",
            "--output",
            &signature.to_string_lossy(),
            &source.to_string_lossy(),
        ],
    );
    let signature_bytes = std::fs::read(&signature).expect("签名可读");
    write_project(dir.path(), &signature_bytes);

    // 正向：env 载体解析公钥 → 验签 → describe → package，产物落在暂存目录。
    let accepted = polypack_build(dir.path(), Some(&trust_root_payload(&signer_public)));
    assert!(
        accepted.status.success(),
        "env 信任根 + 真实验签必须通过：\n{}",
        combined(&accepted)
    );
    let accepted_events = events(&accepted);
    assert!(
        accepted_events
            .iter()
            .any(|event| event["event"] == "plugin_granted" && event["plugin"] == PLUGIN_ID),
        "缺少插件的 plugin_granted 事件：{}",
        combined(&accepted)
    );
    let packaged = accepted_events
        .iter()
        .find(|event| event["event"] == "packaged")
        .expect("插件 package 结果必须进事件流");
    assert_eq!(packaged["format"], "reference-bin");
    assert!(packaged["bytes"].as_u64().expect("字节数") > 0);
    let bytes = std::fs::read(artifact(dir.path())).expect("插件产物可读");
    assert!(
        bytes.starts_with(b"index.html\n"),
        "产物应来自插件 package（describe 声明 reference-bin）：{bytes:?}"
    );

    // 负向：信任根换成另一把公钥 → 验签失败（E_PLUGIN_0009）。
    let (_other_home, other_public) = keypair(dir.path(), "other");
    let wrong = polypack_build(dir.path(), Some(&trust_root_payload(&other_public)));
    assert!(!wrong.status.success(), "错信任根必须拒绝加载");
    let wrong_text = combined(&wrong);
    assert!(
        wrong_text.contains("polypack::E_PLUGIN_0009"),
        "期望 E_PLUGIN_0009：{wrong_text}"
    );
    assert!(!wrong_text.contains("polypack::E_PLUGIN_0010"));

    // 负向：环境变量缺失 → 信任根不可用（E_PLUGIN_0010）。
    let missing = polypack_build(dir.path(), None);
    assert!(!missing.status.success(), "缺信任根载体必须拒绝加载");
    let missing_text = combined(&missing);
    assert!(
        missing_text.contains("polypack::E_PLUGIN_0010"),
        "期望 E_PLUGIN_0010：{missing_text}"
    );
}
