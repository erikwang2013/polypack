//! poly-sign 测试公共设施：CI 三宿主上的**显式跳过**、载荷构造与一次性密钥库生成。
//!
//! 原则：apksigner / keytool 不保证每个宿主都装（Windows / macOS runner 上装，本地可能没有）；
//! 缺失时打印原因后**以通过结束**，绝不红，也绝不伪造真跑。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// 测试口令（**仅测试用**，非真实凭据；三宿主的 keytool / openssl 降级链都写这个值）。
pub const PASS: &str = "test-pass-123";
/// keytool 生成密钥库时固定的别名。
pub const ALIAS: &str = "pax";

/// 一次性测试工作目录（Drop 删除；前缀与签名临时目录区分，避免干扰残留扫描）。
pub struct Scratch(PathBuf);

impl Scratch {
    /// 在系统临时目录下建工作目录（同名残留先清）。
    pub fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("polypack-sign-it-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("测试工作目录可创建");
        Self(dir)
    }

    /// 工作目录路径。
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 串行化真跑用例：临时目录残留扫描与并发签名互斥（测试同进程多线程跑）。
pub fn temp_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 工具是否在 PATH（带 Windows 扩展名尝试）。
pub fn has_tool(tool: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| {
        dir.join(tool).is_file()
            || (cfg!(windows)
                && ["exe", "bat", "cmd"]
                    .iter()
                    .any(|ext| dir.join(format!("{tool}.{ext}")).is_file()))
    })
}

/// apksigner 可用则返回路径，否则打印 SKIP 原因并返回 `None`（调用方随即 `return`）。
pub fn apksigner_or_skip() -> Option<PathBuf> {
    match poly_sign::android::apksigner_path() {
        Ok(path) => Some(path),
        Err(err) => {
            eprintln!("SKIP：{err}（apksigner 就绪的宿主上会走真跑分支）");
            None
        }
    }
}

/// 一次性 PKCS12 密钥库夹具：keytool → openssl 降级 → 都缺则 None（调用方 SKIP）。
pub struct Keystore {
    /// 密钥库完整字节。
    pub bytes: Vec<u8>,
    /// keytool 生成的别名；openssl 导出对别名无保证，返回 None（apksigner 用唯一别名）。
    pub alias: Option<String>,
    /// 证书 DN 头部（校验证据里应出现）。
    pub dn: &'static str,
}

/// 生成测试密钥库（写在 `dir` 下）。
pub fn keystore_fixture(dir: &Path) -> Option<Keystore> {
    let path = dir.join("test.p12");
    if has_tool("keytool") {
        let keystore = path.to_string_lossy().to_string();
        let ok = Command::new("keytool")
            .args([
                "-genkeypair",
                "-keystore",
                keystore.as_str(),
                "-storetype",
                "PKCS12",
                "-alias",
                ALIAS,
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "100",
                "-storepass",
                PASS,
                "-keypass",
                PASS,
                "-dname",
                "CN=Pax Notes, O=PolyPack, C=CN",
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            return std::fs::read(&path).ok().map(|bytes| Keystore {
                bytes,
                alias: Some(ALIAS.to_string()),
                dn: "CN=Pax Notes",
            });
        }
        eprintln!("SKIP：keytool 生成密钥库失败，降级 openssl");
    }
    if has_tool("openssl") {
        let key = dir.join("key.pem");
        let cert = dir.join("cert.pem");
        let first = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-keyout",
                key.to_string_lossy().as_ref(),
                "-out",
                cert.to_string_lossy().as_ref(),
                "-days",
                "100",
                "-nodes",
                "-subj",
                "/CN=Pax",
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        let export = first
            && Command::new("openssl")
                .args([
                    "pkcs12",
                    "-export",
                    "-out",
                    path.to_string_lossy().as_ref(),
                    "-inkey",
                    key.to_string_lossy().as_ref(),
                    "-in",
                    cert.to_string_lossy().as_ref(),
                    "-password",
                    &format!("pass:{PASS}"),
                ])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
        if export {
            return std::fs::read(&path).ok().map(|bytes| Keystore {
                bytes,
                alias: None,
                dn: "CN=Pax",
            });
        }
        eprintln!("SKIP：openssl 生成密钥库失败");
    }
    eprintln!("SKIP：keytool 与 openssl 都不可用，跳过真实签名断言");
    None
}

/// 构造后端载荷：base64(JSON)。
pub fn payload(
    keystore: &[u8],
    store_password: &str,
    key_alias: Option<&str>,
    key_password: Option<&str>,
) -> Vec<u8> {
    let mut json = serde_json::json!({
        "keystore": B64.encode(keystore),
        "store_password": store_password,
    });
    if let Some(alias) = key_alias {
        json["key_alias"] = serde_json::Value::from(alias);
    }
    if let Some(key_password) = key_password {
        json["key_password"] = serde_json::Value::from(key_password);
    }
    B64.encode(serde_json::to_vec(&json).expect("载荷 JSON 可序列化"))
        .into_bytes()
}

/// 扫描本进程遗留的签名临时目录（严格匹配 `polypack-sign-<pid>-<纯数字>`）。
pub fn sign_temp_leftovers() -> Vec<PathBuf> {
    let prefix = format!("polypack-sign-{}-", std::process::id());
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(&prefix))
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect()
}
