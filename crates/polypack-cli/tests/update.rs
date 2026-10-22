//! T-M9-11 真跑：签名更新清单的确定性、GPG 验签与稳定错误码。
//!
//! GPG 用例使用一次性 0700 GNUPGHOME 与测试密钥；宿主没有 gpg（或 Windows 的 MSYS
//! gpg 不认 Windows 路径）时打印 SKIP 并以通过结束，绝不伪造 PASS。错误码用例不依赖
//! gpg：输入校验发生在签名之前。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// 测试口令（仅测试用）。
const PASS: &str = "update-test-pass-123";
/// 清单签名凭据的 env 载体。
const KEY_ENV: &str = "POLYPACK_UPDATE_TEST_KEY";
/// 信任根凭据的 env 载体。
const TRUST_ENV: &str = "POLYPACK_UPDATE_TEST_TRUST";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        // gpg-agent 的 unix socket 路径上限约 104 字节，用短根（同 polypack-sign 测试）。
        #[cfg(unix)]
        let base = PathBuf::from("/tmp");
        #[cfg(not(unix))]
        let base = std::env::temp_dir();
        let path = base.join(format!(
            "pp-cli-update-{}-{}",
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

/// 一次性受口令保护的签名密钥：返回（私钥导出，ASCII-armor 公钥）。
fn keypair(scratch: &Path) -> (Vec<u8>, Vec<u8>) {
    let home = scratch.join("keygen-home");
    private_dir(&home);
    let pass_file = scratch.join("keygen.pass");
    std::fs::write(&pass_file, PASS).expect("口令文件可写");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pass_file, std::fs::Permissions::from_mode(0o600))
            .expect("口令文件 0600");
    }
    let pass = pass_file.to_string_lossy().to_string();
    gpg_ok(
        &home,
        &[
            "--pinentry-mode",
            "loopback",
            "--passphrase-file",
            &pass,
            "--quick-generate-key",
            "PolyPack Update Test <update-test@polypack.invalid>",
            "rsa2048",
            "sign",
            "0",
        ],
    );
    let secret = gpg_ok(
        &home,
        &[
            "--pinentry-mode",
            "loopback",
            "--passphrase-file",
            &pass,
            "--export-secret-keys",
        ],
    )
    .stdout;
    let public = gpg_ok(&home, &["--armor", "--export"]).stdout;
    assert!(!secret.is_empty() && !public.is_empty(), "密钥导出非空");
    (secret, public)
}

/// 签名凭据载荷：base64(JSON)，与 polypack-sign 的后端契约一致。
fn key_payload(secret: &[u8]) -> String {
    let json = serde_json::json!({
        "keystore": B64.encode(secret),
        "store_password": PASS,
    });
    B64.encode(serde_json::to_vec(&json).expect("载荷 JSON"))
}

/// 信任根载荷：只含公钥。
fn trust_payload(public: &[u8]) -> String {
    let json = serde_json::json!({ "public_key": B64.encode(public) });
    B64.encode(serde_json::to_vec(&json).expect("载荷 JSON"))
}

fn polypack(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_polypack"));
    command.args(args).env_remove(KEY_ENV).env_remove(TRUST_ENV);
    for (name, value) in envs {
        command.env(name, value);
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

/// 产物目录：一个 AppImage 及其 `.sig`（内容即 Tauri 清单的 signature 字段）。
fn artifact_dir(scratch: &Path) -> PathBuf {
    let dir = scratch.join("artifacts");
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    std::fs::write(
        dir.join("Pax Notes_1.2.3_x64.AppImage"),
        b"not-a-real-binary",
    )
    .expect("产物可写");
    std::fs::write(
        dir.join("Pax Notes_1.2.3_x64.AppImage.sig"),
        b"dummy-artifact-signature\n",
    )
    .expect("产物签名可写");
    dir
}

#[test]
fn manifest_is_deterministic_signs_and_verifies() {
    if cfg!(windows) || !has_gpg() {
        eprintln!("SKIP：宿主没有可用的 gpg，跳过更新清单签名真跑（Linux/macOS 会走真跑分支）");
        return;
    }
    let scratch = TempDir::new();
    let key = keypair(scratch.path());
    let encrypted_key = key_payload(&key.0);
    let trust = trust_payload(&key.1);
    let dir = artifact_dir(scratch.path());
    let dir_text = dir.to_string_lossy().to_string();
    let envs = [(KEY_ENV, encrypted_key.as_str())];
    let base = [
        "--json",
        "update",
        "manifest",
        "--channel",
        "stable",
        "--version",
        "1.2.3",
        "--notes",
        "fix crash on launch",
        "--base-url",
        "https://updates.example.com/stable/",
        "--platform",
        "linux",
        "--arch",
        "x86_64",
        "--package-dir",
        &dir_text,
        "--credential",
        &format!("env:{KEY_ENV}"),
        "--pub-date",
        "2026-10-20T00:00:00Z",
    ];

    let first = polypack(&base, &envs);
    assert!(first.status.success(), "首次生成失败：{}", combined(&first));
    let latest = dir.join("stable/latest.json");
    let first_bytes = std::fs::read(&latest).expect("清单可读");
    let manifest: serde_json::Value = serde_json::from_slice(&first_bytes).expect("清单 JSON");
    assert_eq!(manifest["version"], "1.2.3");
    assert_eq!(manifest["notes"], "fix crash on launch");
    assert_eq!(manifest["pub_date"], "2026-10-20T00:00:00Z");
    assert_eq!(
        manifest["platforms"]["linux-x86_64"]["signature"],
        "dummy-artifact-signature\n"
    );
    assert_eq!(
        manifest["platforms"]["linux-x86_64"]["url"],
        "https://updates.example.com/stable/Pax%20Notes_1.2.3_x64.AppImage"
    );
    assert!(dir.join("stable/latest.json.sig").is_file(), "清单签名落盘");
    assert!(dir.join("stable/1.2.3.json").is_file(), "版本副本保留");
    assert!(
        dir.join("stable/1.2.3.json.sig").is_file(),
        "版本副本签名保留"
    );

    let second = polypack(&base, &envs);
    assert!(
        second.status.success(),
        "二次生成失败：{}",
        combined(&second)
    );
    assert_eq!(
        first_bytes,
        std::fs::read(&latest).expect("二次清单可读"),
        "同输入二跑清单字节一致"
    );

    let check = polypack(
        &[
            "--json",
            "update",
            "check",
            "--manifest",
            &latest.to_string_lossy(),
            "--signature",
            &latest.with_file_name("latest.json.sig").to_string_lossy(),
            "--trust-root",
            &format!("env:{TRUST_ENV}"),
        ],
        &[(TRUST_ENV, trust.as_str())],
    );
    assert!(check.status.success(), "验签失败：{}", combined(&check));
    let event: serde_json::Value = serde_json::from_slice(
        String::from_utf8_lossy(&check.stdout)
            .lines()
            .last()
            .expect("JSON 事件行")
            .as_bytes(),
    )
    .expect("check JSON");
    assert_eq!(event["signature_verified"], true);

    let unsigned = polypack(
        &[
            "--json",
            "update",
            "check",
            "--manifest",
            &latest.to_string_lossy(),
        ],
        &[],
    );
    assert!(
        unsigned.status.success(),
        "无签名结构检查失败：{}",
        combined(&unsigned)
    );
    assert!(combined(&unsigned).contains("\"signature_verified\":false"));

    // 未给 --pub-date 时由 SOURCE_DATE_EPOCH 固定时刻（写独立目录，不覆盖已验签清单）。
    let mut epoch_args = base[..base.len() - 2].to_vec();
    let epoch_output = dir.join("epoch/latest.json");
    let epoch_output_text = epoch_output.to_string_lossy().to_string();
    epoch_args.extend_from_slice(&["--output", &epoch_output_text]);
    let epoch = polypack(
        &epoch_args,
        &[
            (KEY_ENV, encrypted_key.as_str()),
            ("SOURCE_DATE_EPOCH", "0"),
        ],
    );
    assert!(
        epoch.status.success(),
        "SOURCE_DATE_EPOCH 生成失败：{}",
        combined(&epoch)
    );
    let epoch_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&epoch_output).expect("epoch 清单可读"))
            .expect("epoch 清单 JSON");
    assert_eq!(epoch_manifest["pub_date"], "1970-01-01T00:00:00Z");

    // 篡改清单字节 → 验签必须失败（E_UPDATE_0006），不得静默通过。
    let tampered = String::from_utf8(first_bytes)
        .expect("UTF-8")
        .replace("fix crash on launch", "fix crash on launch (tampered)");
    std::fs::write(&latest, tampered).expect("篡改清单可写");
    let rejected = polypack(
        &[
            "update",
            "check",
            "--manifest",
            &latest.to_string_lossy(),
            "--signature",
            &latest.with_file_name("latest.json.sig").to_string_lossy(),
            "--trust-root",
            &format!("env:{TRUST_ENV}"),
        ],
        &[(TRUST_ENV, trust.as_str())],
    );
    assert!(!rejected.status.success(), "篡改清单必须验签失败");
    assert!(
        combined(&rejected).contains("E_UPDATE_0006"),
        "稳定错误码：{}",
        combined(&rejected)
    );
}

#[test]
fn manifest_input_errors_are_stable() {
    fn args<'a>(channel: &'a str, dir: &'a str) -> Vec<&'a str> {
        vec![
            "update",
            "manifest",
            "--channel",
            channel,
            "--version",
            "1.2.3",
            "--notes",
            "notes",
            "--base-url",
            "https://updates.example.com/stable",
            "--platform",
            "linux",
            "--arch",
            "x86_64",
            "--package-dir",
            dir,
        ]
    }

    let scratch = TempDir::new();
    let dir = artifact_dir(scratch.path());
    let dir_text = dir.to_string_lossy().to_string();

    let bad_channel = polypack(&args("../stable", &dir_text), &[]);
    assert!(!bad_channel.status.success());
    assert!(
        combined(&bad_channel).contains("E_UPDATE_0001"),
        "{}",
        combined(&bad_channel)
    );

    let empty = scratch.path().join("empty");
    std::fs::create_dir_all(&empty).expect("空目录可建");
    let empty_text = empty.to_string_lossy().to_string();
    let missing_artifact = polypack(&args("stable", &empty_text), &[]);
    assert!(!missing_artifact.status.success());
    assert!(
        combined(&missing_artifact).contains("E_UPDATE_0003"),
        "{}",
        combined(&missing_artifact)
    );

    let missing_credential = polypack(&args("stable", &dir_text), &[]);
    assert!(!missing_credential.status.success());
    assert!(
        combined(&missing_credential).contains("E_UPDATE_0002"),
        "{}",
        combined(&missing_credential)
    );

    std::fs::remove_file(dir.join("Pax Notes_1.2.3_x64.AppImage.sig")).expect("删签名");
    let mut missing_signature_args = args("stable", &dir_text);
    missing_signature_args.extend_from_slice(&["--credential", "env:POLYPACK_UPDATE_TEST_KEY"]);
    let missing_signature = polypack(&missing_signature_args, &[]);
    assert!(!missing_signature.status.success());
    assert!(
        combined(&missing_signature).contains("E_UPDATE_0004"),
        "{}",
        combined(&missing_signature)
    );
}

#[test]
fn check_rejects_invalid_manifest_schema() {
    let scratch = TempDir::new();
    let manifest = scratch.path().join("latest.json");
    std::fs::write(
        &manifest,
        br#"{"version":"1.2.3","platforms":{"linux-x86_64":{"url":"https://example.com/a"}}}"#,
    )
    .expect("清单可写");
    let output = polypack(
        &["update", "check", "--manifest", &manifest.to_string_lossy()],
        &[],
    );
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("E_UPDATE_0005"),
        "{}",
        combined(&output)
    );
}

#[test]
fn cli_help_lists_update_subcommands_and_flags() {
    let top = polypack(&["update", "--help"], &[]);
    let top_text = combined(&top);
    assert!(top.status.success(), "{top_text}");
    assert!(top_text.contains("manifest"), "{top_text}");
    assert!(top_text.contains("check"), "{top_text}");

    let manifest = polypack(&["update", "manifest", "--help"], &[]);
    let manifest_text = combined(&manifest);
    assert!(manifest.status.success(), "{manifest_text}");
    for flag in ["--channel", "--package-dir", "--credential", "--pub-date"] {
        assert!(manifest_text.contains(flag), "缺少 {flag}：{manifest_text}");
    }
}
