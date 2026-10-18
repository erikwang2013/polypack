//! 插件制品分离签名验证（T-M8-03）：GPG detached signature + [`CredentialRef`] 信任根。
//!
//! 信任根只以引用形式出现（keyring / Vault / Env / 内存注入），载荷契约是
//! base64(JSON) `{"public_key":"<base64>"}`——只有公钥，私钥 / 口令永不进入验签路径。
//! 验签在一次性 0700 GNUPGHOME 内导入公钥后执行 `gpg --verify`；错误文本中的临时目录
//! 绝对路径在离开本模块前替换为占位符（ROADMAP §7 脱敏）。
//!
//! 签名侧复用 [`crate::GpgSigner`]（[`crate::RepoSigner::detach_sign`]）：插件发布方
//! 对 `.wasm` 制品产出分离签名，公钥随凭据引用分发；本模块只负责验证。

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use poly_core::CredentialRef;

use crate::error::SignError;
use crate::secret::{self, SecretBackend};
use crate::temp::{TempDir, write_private};

/// 插件签名验证器：持有一个只导入信任根公钥的一次性 GNUPGHOME。
pub struct ArtifactVerifier {
    /// 一次性 GNUPGHOME（Drop 删除；只含公钥）。
    home: TempDir,
}

impl ArtifactVerifier {
    /// 从 [`CredentialRef`] 解析信任根公钥并导入隔离 GNUPGHOME。
    ///
    /// 解析失败（后端不可用 / 载荷非法）或 gpg 缺失 → 稳定 [`SignError`]；
    /// 错误不自带凭据值，也不带宿主绝对路径。
    pub fn from_reference(
        reference: &CredentialRef,
        backends: &[Box<dyn SecretBackend>],
    ) -> Result<Self, SignError> {
        let key = secret::resolve_public_key(reference, backends)?;
        let home = TempDir::create("polypack-verify-")?;
        let key_file = home.path().join("trust-root.pub");
        write_private(&key_file, &key)?;
        run(
            home.path(),
            &["--import".into(), key_file.into_os_string()],
            "gpg --import（信任根公钥）",
        )?;
        Ok(Self { home })
    }

    /// 验证分离签名：成功仅表示签名与数据 / 信任根匹配。
    ///
    /// `Err` 覆盖签名不符、公钥不符、签名格式非法与 gpg 执行失败；调用方按策略映射
    /// 到自己的稳定错误码。
    pub fn verify_detached(&self, data: &[u8], signature: &[u8]) -> Result<(), SignError> {
        let data_file = self.home.path().join("artifact");
        let signature_file = self.home.path().join("artifact.sig");
        std::fs::write(&data_file, data).map_err(crate::error::io_err(&data_file))?;
        std::fs::write(&signature_file, signature)
            .map_err(crate::error::io_err(&signature_file))?;
        run(
            self.home.path(),
            &[
                "--verify".into(),
                signature_file.into_os_string(),
                data_file.into_os_string(),
            ],
            "gpg --verify",
        )
    }
}

/// 在一次性 home 内执行 gpg（`--batch --no-options`）。
///
/// `display` 是错误里展示的命令名（不含临时路径）；stderr 中的 home 绝对路径先脱敏。
fn run(home: &Path, args: &[OsString], display: &str) -> Result<(), SignError> {
    let mut full: Vec<OsString> = vec![
        "--batch".into(),
        "--no-options".into(),
        "--homedir".into(),
        home.into(),
    ];
    full.extend_from_slice(args);
    let output =
        Command::new("gpg")
            .args(&full)
            .output()
            .map_err(|source| SignError::ToolMissing {
                tool: "gpg".to_string(),
                hint: format!(
                    "安装 GnuPG（Debian/Ubuntu: apt install gnupg；Fedora: dnf install gnupg2）\
                 （底层错误：{source}）"
                ),
            })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.replace(&home.to_string_lossy().into_owned(), "<临时目录>");
    Err(SignError::ToolFailed {
        command: display.to_string(),
        status: output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |code| code.to_string()),
        stderr: crate::android::tail(&stderr, 800),
    })
}
