//! T-M8-03 验收：插件制品 detached 签名链（SRI 共存 + 负向集 + §7 脱敏）。
//!
//! 签名密钥与信任根公钥都在测试进程内生成、只经 `CredentialRef` + 内存后端注入；
//! 明文私钥只存在于测试内存与一次性 GNUPGHOME，随目录删除。宿主没有 gpg（或 Windows
//! MSYS 路径不兼容）时打印 SKIP 并以通过结束，绝不伪造真跑。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use poly_core::{CredentialRef, CredentialSource, PluginGrant, codes};
use poly_plugin::{PackageInput, ResourceLimits, Sandbox, SignaturePolicy, content_digest};
use poly_sign::secret::{KEYRING_ID, MemoryBackend, SecretBackend};
use poly_sign::{GpgSigner, RepoSigner};

const REFERENCE: &[u8] = include_bytes!("fixtures/reference-plugin.wasm");
const UID: &str = "PolyPack Plugin Test <plugin-test@polypack.invalid>";
const PASS: &str = "plugin-sign-pass-123";
/// 签名凭据档案名（配置只出现名字，不出现值）。
const SIGNER: &str = "plugin-signer";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("时钟可用")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "polypack-plugin-sign-{tag}-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
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

fn grant() -> PluginGrant {
    PluginGrant {
        id: "reference".to_string(),
        path: "reference-plugin.wasm".to_string(),
        ..PluginGrant::default()
    }
}

fn sandbox() -> Sandbox {
    Sandbox::new(ResourceLimits::default()).expect("沙箱可建")
}

fn input() -> PackageInput {
    PackageInput {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        target: "linux".to_string(),
        entry: "index.html".to_string(),
        files: vec![("index.html".to_string(), b"<html>".to_vec())],
        triple: "wasm32-unknown-unknown".to_string(),
        lib: b"CORE".to_vec(),
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

fn has_gpg() -> bool {
    std::env::var_os("PATH").is_some_and(|path_var| {
        std::env::split_paths(&path_var).any(|dir| dir.join("gpg").is_file())
    })
}

/// 一次性密钥对：(私钥导出字节, ASCII-armor 公钥字节)；环境不满足 → SKIP。
fn keypair(scratch: &Path, tag: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    // Git for Windows 的 MSYS gpg 不认 Windows 绝对路径参数（同 poly-sign 真跑用例）。
    if cfg!(windows) || !has_gpg() {
        eprintln!("SKIP：宿主没有可用的 gpg，跳过插件签名真跑（Linux/macOS 会走真跑分支）");
        return None;
    }
    let home = scratch.join(format!("keygen-{tag}"));
    private_dir(&home);
    let pass_file = scratch.join("keygen.pass");
    std::fs::write(&pass_file, PASS).expect("写口令文件");
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
            UID,
            "ed25519",
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
    Some((secret, public))
}

/// 私钥凭据载荷 base64(JSON)；只进内存后端。
fn signing_payload(secret: &[u8]) -> Vec<u8> {
    let json = format!(
        r#"{{"keystore":"{}","store_password":"{PASS}"}}"#,
        B64.encode(secret)
    );
    B64.encode(json).into_bytes()
}

/// 信任根载荷 base64(JSON)：只含 public_key（绝无口令 / 私钥字段）。
fn public_payload(public: &[u8]) -> Vec<u8> {
    let json = format!(r#"{{"public_key":"{}"}}"#, B64.encode(public));
    B64.encode(json).into_bytes()
}

fn credential(name: &str) -> CredentialRef {
    CredentialRef {
        name: name.to_string(),
        source: CredentialSource::Keyring,
    }
}

/// 发布方对制品做 detached 签名（复用 poly-sign GpgSigner）。
fn sign(secret: &[u8], data: &[u8]) -> Vec<u8> {
    let reference = credential(SIGNER);
    let backend = MemoryBackend::with_entry(SIGNER, signing_payload(secret)).with_id(KEYRING_ID);
    let backends: Vec<Box<dyn SecretBackend>> = vec![Box::new(backend)];
    let signer = GpgSigner::new(&reference, &backends).expect("签名器可建（测试密钥）");
    signer.detach_sign(data).expect("分离签名可产出")
}

/// 信任根凭据后端链。
fn trust_backends(root: &CredentialRef, public: &[u8]) -> Vec<Box<dyn SecretBackend>> {
    vec![Box::new(
        MemoryBackend::with_entry(root.name.clone(), public_payload(public)).with_id(KEYRING_ID),
    )]
}

fn policy<'a>(
    detached: Option<&'a [u8]>,
    trust_root: &'a CredentialRef,
    backends: &'a [Box<dyn SecretBackend>],
) -> SignaturePolicy<'a> {
    SignaturePolicy {
        detached,
        trust_root,
        backends,
    }
}

fn expect_err(
    result: Result<poly_plugin::Plugin, poly_plugin::PluginError>,
) -> poly_plugin::PluginError {
    match result {
        Ok(_) => panic!("签名策略必须拒绝该制品"),
        Err(err) => err,
    }
}

/// 正确签名 + SRI 通过，且可实例化调用 describe。
#[test]
fn valid_signature_with_matching_sri_loads_and_runs() {
    let dir = TempDir::new("ok");
    let Some((secret, public)) = keypair(dir.path(), "ok") else {
        return;
    };
    let signature = sign(&secret, REFERENCE);
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &public);
    let sri = content_digest(REFERENCE);
    let sandbox = sandbox();
    let plugin = sandbox
        .load_signed(
            REFERENCE,
            "reference",
            Some(&sri),
            Some(&policy(Some(&signature), &root, &backends)),
            dir.path(),
            &grant(),
        )
        .expect("正确签名 + SRI 必须通过");
    assert_eq!(plugin.digest(), sri.as_str());
    let spec = sandbox
        .describe(&plugin, &input())
        .expect("验签通过的插件可实例化");
    assert_eq!(spec.spec_version, "target-spec/v1");
}

/// 篡改字节并同步更新 SRI：签名仍指向原制品 → 验签失败拒绝。
#[test]
fn tampered_bytes_with_updated_sri_fail_signature() {
    let dir = TempDir::new("tamper");
    let Some((secret, public)) = keypair(dir.path(), "tamper") else {
        return;
    };
    let signature = sign(&secret, REFERENCE);
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &public);
    let mut tampered = REFERENCE.to_vec();
    let index = tampered.len() / 2;
    tampered[index] ^= 0x01;
    let sri = content_digest(&tampered);
    let err = expect_err(sandbox().load_signed(
        &tampered,
        "reference",
        Some(&sri),
        Some(&policy(Some(&signature), &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0009);
}

/// 配置要求签名但制品没有签名 → 拒绝。
#[test]
fn missing_signature_is_rejected() {
    let dir = TempDir::new("missing");
    let Some((_secret, public)) = keypair(dir.path(), "missing") else {
        return;
    };
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &public);
    let err = expect_err(sandbox().load_signed(
        REFERENCE,
        "reference",
        None,
        Some(&policy(None, &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0009);
    assert!(err.hint.expect("有修复提示").contains("签名"));
}

/// 错公钥（另一把密钥的信任根）→ 验签失败拒绝。
#[test]
fn wrong_public_key_is_rejected() {
    let dir = TempDir::new("wrong-key");
    let Some((secret, _public)) = keypair(dir.path(), "signer") else {
        return;
    };
    let Some((_other_secret, other_public)) = keypair(dir.path(), "other") else {
        return;
    };
    let signature = sign(&secret, REFERENCE);
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &other_public);
    let err = expect_err(sandbox().load_signed(
        REFERENCE,
        "reference",
        None,
        Some(&policy(Some(&signature), &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0009);
}

/// 有效签名但 SRI 不符：SRI 先行拒绝（E_PLUGIN_0006），不进入验签。
#[test]
fn sri_mismatch_is_rejected_even_with_valid_signature() {
    let dir = TempDir::new("sri-first");
    let Some((secret, public)) = keypair(dir.path(), "sri") else {
        return;
    };
    let signature = sign(&secret, REFERENCE);
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &public);
    let err = expect_err(sandbox().load_signed(
        REFERENCE,
        "reference",
        Some("blake3-00"),
        Some(&policy(Some(&signature), &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0006);
}

/// 信任根不可用（后端链缺 keyring）→ E_PLUGIN_0010，且错误不含宿主绝对路径。
#[test]
fn unavailable_trust_root_is_reported_without_host_paths() {
    let dir = TempDir::new("no-backend");
    let root = credential("plugin-trust-root");
    let backends: Vec<Box<dyn SecretBackend>> = Vec::new();
    let err = expect_err(sandbox().load_signed(
        REFERENCE,
        "reference",
        None,
        Some(&policy(Some(b"not-a-signature"), &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0010);
    assert_no_host_path(&err, dir.path());
}

/// §7 脱敏：验签失败 / 信任根错误 / 签名产物都不带宿主绝对路径或私钥材料。
#[test]
fn errors_and_artifacts_leak_no_host_paths_or_private_material() {
    let dir = TempDir::new("redact");
    let Some((secret, public)) = keypair(dir.path(), "redact") else {
        return;
    };
    let signature = sign(&secret, REFERENCE);
    let root = credential("plugin-trust-root");
    let backends = trust_backends(&root, &public);
    let mut tampered = REFERENCE.to_vec();
    tampered[0] ^= 0xFF;
    let err = expect_err(sandbox().load_signed(
        &tampered,
        "reference",
        Some(&content_digest(&tampered)),
        Some(&policy(Some(&signature), &root, &backends)),
        dir.path(),
        &grant(),
    ));
    assert_eq!(err.code, codes::E_PLUGIN_0009);
    assert_no_host_path(&err, dir.path());

    // 签名产物只有 GPG 签名块，绝无明文私钥；信任根载荷只有 public_key 字段。
    let signature_text = String::from_utf8_lossy(&signature);
    assert!(!signature_text.contains("PRIVATE KEY"));
    assert!(!signature_text.contains(&B64.encode(&secret)));
    let root_payload = String::from_utf8_lossy(&public_payload(&public)).into_owned();
    assert!(!root_payload.contains("store_password"));
    assert!(!root_payload.contains("PRIVATE KEY"));
    assert!(!root_payload.contains(&B64.encode(&secret)));
}

/// 配置里的信任根只以 CredentialRef 引用出现；`polypack.toml` 不内联私钥。
#[test]
fn config_references_trust_root_without_inline_secrets() {
    let config_text = "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n\
         [ui]\nframework = \"react\"\nentry = \"index.html\"\n\
         [core]\ncrate = \"core\"\n\
         [[plugins]]\nid = \"reference\"\npath = \"plugins/reference.wasm\"\n\
         signature = \"plugins/reference.wasm.sig\"\n\
         trust_root = { name = \"plugin-trust-root\", source = \"keyring\" }\n";
    let config = poly_core::config::PolypackConfig::parse(config_text).expect("配置可解析");
    let plugin = &config.plugins[0];
    assert_eq!(
        plugin.signature.as_deref(),
        Some("plugins/reference.wasm.sig")
    );
    assert_eq!(
        plugin.trust_root.as_ref().map(|root| root.name.as_str()),
        Some("plugin-trust-root")
    );
    assert!(!config_text.contains("PRIVATE KEY"));
    assert!(!config_text.contains("store_password"));
}

/// 断言错误消息 / 提示 / Display 都不含宿主绝对路径与测试临时目录。
fn assert_no_host_path(err: &poly_plugin::PluginError, scratch: &Path) {
    let rendered = format!("{err} {:?}", err.hint);
    assert!(!rendered.contains("/home/"), "错误泄露 /home/: {rendered}");
    assert!(
        !rendered.contains(&scratch.to_string_lossy().into_owned()),
        "错误泄露临时目录: {rendered}"
    );
    assert!(
        !rendered.contains("PRIVATE KEY") && !rendered.contains("store_password"),
        "错误泄露私钥材料: {rendered}"
    );
}
