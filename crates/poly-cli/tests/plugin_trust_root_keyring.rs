//! ENV-15：CLI 经 keyring 信任根载体解析公钥，并加载真实插件制品（端到端）。
//!
//! 复用 `plugin_trust_root_env` 的工程夹具（仓库内真实 `reference-plugin.wasm` + 一次性
//! 0700 GNUPGHOME 的 gpg 分离签名 + 最小 `dist/`），只把信任根换成
//! `trust_root = { name = "plugin-trust-root", source = "keyring" }`。条目按后端契约写入
//! 隔离 Secret Service：service = `polypack`，username = 凭据名 `plugin-trust-root`，
//! 值 = `base64(JSON) {"public_key":"<base64>"}`（写 / 删用 keyring crate，与
//! `poly-sign/tests/keyring_real.rs` 同一 dev-dependency API；CLI 读路径走生产
//! `KeyringBackend`）。
//!
//! 门控：`POLY_KEYRING_REAL=1` **且** `DBUS_SESSION_BUS_ADDRESS` 存在才真跑；缺则打印
//! SKIP 并以通过结束。headless 隔离配方见 `crates/poly-sign/tests/keyring_real.rs` 文件头
//! （dbus-run-session + `gnome-keyring-daemon --unlock --foreground` + 非空密码 +
//! `GNOME_KEYRING_CUSTOM_LOCAL_PATH`）。CI 作业不得把 SKIP 当通过（日志 grep 兜底）。
//!
//! 正向 `poly build --target linux-reference --json` 断言 `plugin_granted` / `packaged`
//! 与暂存产物；删除条目后的负向断言 `E_PLUGIN_0010`。宿主无 gpg 同样 SKIP（与 env 测试
//! 同口径）；夹具缺失是编译期 `include_bytes!` 失败，绝不静默跳过。

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use poly_sign::secret::KeyringBackend;

/// 宿主参考插件（真实 `target-spec/v1` 组件，非占位字节）。
const WASM: &[u8] = include_bytes!("../../poly-plugin/tests/fixtures/reference-plugin.wasm");
/// 凭据名 = keyring 条目的 username（= `CredentialRef.name`）。
const TRUST_NAME: &str = "plugin-trust-root";
/// 插件 id；参考插件把 describe 的输入 target 拼成 `<target>-reference`。
const PLUGIN_ID: &str = "linux";
const TARGET_ID: &str = "linux-reference";
const PACKAGE_NAME: &str = "trust-root-keyring";
const PACKAGE_VERSION: &str = "1.0.0";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        // gpg-agent 的 unix socket 路径上限约 104 字节，用短根（同 poly-plugin 签名测试）。
        let path = PathBuf::from("/tmp").join(format!(
            "pp-cli-keyring-{}-{}",
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
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path).expect("目录可建");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("目录可设 0700");
}

fn has_gpg() -> bool {
    std::env::var_os("PATH").is_some_and(|path_var| {
        std::env::split_paths(&path_var).any(|dir| dir.join("gpg").is_file())
    })
}

/// 门控：`true` 才真跑。
fn real_env() -> bool {
    if std::env::var("POLY_KEYRING_REAL").ok().as_deref() != Some("1") {
        eprintln!(
            "SKIP：CLI keyring 信任根 E2E 需 POLY_KEYRING_REAL=1\
             （headless 配方见 crates/poly-sign/tests/keyring_real.rs 文件头）"
        );
        return false;
    }
    if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
        eprintln!(
            "SKIP：没有会话总线（DBUS_SESSION_BUS_ADDRESS 未设置）——用 dbus-run-session 包一层"
        );
        return false;
    }
    // 运行记录：隔离线索进输出（--nocapture 下可见），便于事后核对跑的是哪个库。
    eprintln!(
        "真跑：HOME={} GNOME_KEYRING_CUSTOM_LOCAL_PATH={}",
        std::env::var("HOME").unwrap_or_else(|_| "<未设置>".into()),
        std::env::var("GNOME_KEYRING_CUSTOM_LOCAL_PATH")
            .unwrap_or_else(|_| "<未设置（非 Deepin 补丁版可忽略）>".into())
    );
    true
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

/// 临时工程：真实组件 + 分离签名 + 最小 `dist/` 输入；信任根只引用 keyring 条目。
fn write_project(root: &Path, signature: &[u8]) {
    std::fs::create_dir_all(root.join("plugins")).expect("plugins 目录可建");
    std::fs::write(root.join("plugins/reference-plugin.wasm"), WASM).expect("插件可写");
    std::fs::write(root.join("plugins/reference-plugin.wasm.sig"), signature)
        .expect("分离签名可写");
    std::fs::create_dir_all(root.join("dist")).expect("dist 目录可建");
    std::fs::write(
        root.join("dist/index.html"),
        "<!doctype html><html><body>trust-root-keyring</body></html>",
    )
    .expect("UI 输入可写");
    std::fs::write(
        root.join("polypack.toml"),
        format!(
            "schema = 1\n\n[package]\nname = \"{PACKAGE_NAME}\"\nversion = \"{PACKAGE_VERSION}\"\n\n\
             [ui]\nframework = \"react\"\nentry = \"index.html\"\n\n[core]\ncrate = \"core\"\n\n\
             [[plugins]]\nid = \"{PLUGIN_ID}\"\npath = \"plugins/reference-plugin.wasm\"\n\
             signature = \"plugins/reference-plugin.wasm.sig\"\n\
             trust_root = {{ name = \"{TRUST_NAME}\", source = \"keyring\" }}\n"
        ),
    )
    .expect("配置可写");
}

fn poly_build(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(["build", "--json", "--target", TARGET_ID])
        .current_dir(root)
        .output()
        .expect("poly 可执行")
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
fn cli_resolves_trust_root_from_keyring_and_loads_signed_plugin() {
    if !real_env() {
        return;
    }
    if !has_gpg() {
        eprintln!("SKIP：宿主没有可用的 gpg，跳过 CLI keyring 信任根端到端");
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

    // 真写：隔离 Secret Service 的登录集合（配方已 --unlock）；服务名 / 条目名与读路径一致。
    let entry = keyring::Entry::new(KeyringBackend::SERVICE, TRUST_NAME).expect("条目句柄");
    // 幂等前置：上一轮残留先清掉（无条目时删除报 NoEntry，忽略）。
    let _ = entry.delete_credential();
    entry
        .set_password(&trust_root_payload(&signer_public))
        .expect("写入隔离 Secret Service");

    // 正向：keyring 载体解析公钥 → 验签 → describe → package，产物落在暂存目录。
    let accepted = poly_build(dir.path());
    assert!(
        accepted.status.success(),
        "keyring 信任根 + 真实验签必须通过：\n{}",
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

    // 负向：删除条目 → 信任根不可用（E_PLUGIN_0010）。
    entry.delete_credential().expect("真删条目");
    let missing = poly_build(dir.path());
    assert!(!missing.status.success(), "缺 keyring 条目必须拒绝加载");
    let missing_text = combined(&missing);
    assert!(
        missing_text.contains("poly::E_PLUGIN_0010"),
        "期望 E_PLUGIN_0010：{missing_text}"
    );
}
