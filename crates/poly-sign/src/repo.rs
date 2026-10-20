//! 仓库元数据签名抽象（T-M5-05）：deb `Release` / `InRelease` 与 rpm `repomd.xml`。
//!
//! 凭据走 [`poly_core::CredentialRef`] + [`crate::secret::resolve`]（与 Android / Harmony
//! 同一后端链与载荷契约）：`keystore` = `gpg --export-secret-keys` 的字节（二进制或 ASCII
//! armor 均可），`store_password` = 私钥口令，`key_alias` 可选（fingerprint / UID，缺省用
//! 临时 keyring 里的唯一私钥）。明文口令与私钥只在 0700 一次性 GNUPGHOME 内落盘，签名后
//! kill gpg-agent 并整目录删除；argv / 错误 / 日志只出现路径与工具名。
//!
//! 签名时间戳每次不同（GPG 的固有行为）；仓库元数据本身的确定性由调用方保证——本模块
//! 只对给定字节签名 / 清签，不掺入时间。

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use poly_core::CredentialRef;

use crate::android::tail;
use crate::error::SignError;
use crate::secret::{self, SecretBackend, SigningMaterial};
use crate::temp::{TempDir, write_private};

/// 仓库签名抽象：deb / rpm 元数据共用（T-M5-06 的 pkg 仓库可复用）。
pub trait RepoSigner: Send + Sync {
    /// 分离签名（ASCII armor）：`Release.gpg` / `repomd.xml.asc`。
    fn detach_sign(&self, data: &[u8]) -> Result<Vec<u8>, SignError>;
    /// 明文签名：`InRelease`。
    fn clear_sign(&self, data: &[u8]) -> Result<Vec<u8>, SignError>;
    /// 导出公钥：`armored = false` 为二进制 keyring（apt `signed-by`），
    /// `true` 为 ASCII armor（dnf `gpgkey` / 人读张贴）。
    fn public_key(&self, armored: bool) -> Result<Vec<u8>, SignError>;
}

/// GnuPG 实现：每个实例独占一个隔离 GNUPGHOME。
pub struct GpgSigner {
    /// 一次性 GNUPGHOME（Drop 删除）。
    home: TempDir,
    /// 口令文件（0600，位于 home 内）。
    passphrase: std::path::PathBuf,
    /// 可选 `--local-user`。
    user: Option<String>,
}

impl GpgSigner {
    /// 用默认后端链（系统密钥库）解析凭据。
    pub fn with_default_backends(reference: &CredentialRef) -> Result<Self, SignError> {
        Self::new(reference, &secret::default_backends())
    }

    /// 用调用方注入的后端链解析凭据（CI：Env / Memory 注入，同 T-M2-11）。
    pub fn new(
        reference: &CredentialRef,
        backends: &[Box<dyn SecretBackend>],
    ) -> Result<Self, SignError> {
        let material = secret::resolve(reference, backends)?;
        Self::from_material(&material)
    }

    fn from_material(material: &SigningMaterial) -> Result<Self, SignError> {
        ensure_gpg()?;
        let home = TempDir::create("polypack-sign-")?;
        let passphrase = home.path().join("passphrase");
        write_private(&passphrase, material.store_password.as_bytes())?;
        let key = home.path().join("repo-key.gpg");
        write_private(&key, &material.keystore)?;
        import(home.path(), &key)?;
        // 导入后即删：私钥只留在 gpg-agent 管理的 keyring 里，随 home 整体清理。
        let _ = std::fs::remove_file(&key);
        Ok(Self {
            home,
            passphrase,
            user: material.key_alias.clone(),
        })
    }

    /// 执行 gpg（签名输入落 home 内临时文件：仓库元数据是公开内容）。
    fn sign(&self, data: &[u8], flag: &str) -> Result<Vec<u8>, SignError> {
        let payload = self.home.path().join("payload");
        std::fs::write(&payload, data).map_err(crate::error::io_err(&payload))?;
        let mut args = self.base_args();
        args.push(flag.into());
        if flag == "--detach-sign" {
            args.push("--armor".into());
        }
        args.push("--output".into());
        args.push("-".into());
        args.push(payload.into_os_string());
        let output = run(&args)?;
        non_empty(output, "gpg 未产出签名")
    }

    fn base_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "--batch".into(),
            "--no-options".into(),
            "--homedir".into(),
            self.home.path().into(),
            "--pinentry-mode".into(),
            "loopback".into(),
            "--passphrase-file".into(),
            self.passphrase.clone().into(),
            "--digest-algo".into(),
            "SHA256".into(),
        ];
        if let Some(user) = &self.user {
            args.push("--local-user".into());
            args.push(user.into());
        }
        args
    }
}

impl RepoSigner for GpgSigner {
    fn detach_sign(&self, data: &[u8]) -> Result<Vec<u8>, SignError> {
        self.sign(data, "--detach-sign")
    }

    fn clear_sign(&self, data: &[u8]) -> Result<Vec<u8>, SignError> {
        self.sign(data, "--clearsign")
    }

    fn public_key(&self, armored: bool) -> Result<Vec<u8>, SignError> {
        let mut args: Vec<OsString> = vec![
            "--batch".into(),
            "--no-options".into(),
            "--homedir".into(),
            self.home.path().into(),
        ];
        if armored {
            args.push("--armor".into());
        }
        args.push("--export".into());
        if let Some(user) = &self.user {
            args.push(user.into());
        }
        let output = run(&args)?;
        non_empty(
            output,
            "gpg --export 未导出公钥（key_alias 与实际私钥不匹配？）",
        )
    }
}

impl Drop for GpgSigner {
    fn drop(&mut self) {
        // agent 持有 home 内的 socket；先杀再让 TempDir 删目录（失败不掩盖签名结果）。
        let (_, msys) = crate::gpg::program();
        let home = crate::gpg::path_arg(self.home.path().as_os_str(), msys);
        let _ = Command::new("gpgconf")
            .arg("--homedir")
            .arg(home)
            .arg("--kill")
            .arg("gpg-agent")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// gpg 在场探测（缺失 → E_SIG_0001，绝不静默降级）。
fn ensure_gpg() -> Result<(), SignError> {
    run(&["--version".into()]).map(|_| ())
}

/// `gpg --import <file>`（私钥导入不需要口令：密钥包保持加密形态）。
fn import(home: &Path, key: &Path) -> Result<(), SignError> {
    run(&[
        "--batch".into(),
        "--no-options".into(),
        "--homedir".into(),
        home.into(),
        "--import".into(),
        key.into(),
    ])
    .map(|_| ())
}

/// 执行 gpg：缺席 → [`SignError::ToolMissing`]；非零 → [`SignError::ToolFailed`]。
fn run(args: &[OsString]) -> Result<Output, SignError> {
    let (program, msys) = crate::gpg::program();
    let args = crate::gpg::args(args, msys);
    let output = Command::new(program)
        .args(&args)
        .output()
        .map_err(|source| SignError::ToolMissing {
            tool: "gpg".to_string(),
            hint: format!(
                "安装 GnuPG（Debian/Ubuntu: apt install gnupg；Fedora: dnf install gnupg2）\
                 （底层错误：{source}）"
            ),
        })?;
    if !output.status.success() {
        let shown = args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(SignError::ToolFailed {
            command: format!("gpg {shown}"),
            status: output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stderr: tail(&String::from_utf8_lossy(&output.stderr), 800),
        });
    }
    Ok(output)
}

/// 空输出视为失败（gpg 在 key 不匹配等场景可能 0 退出但无内容）。
fn non_empty(output: Output, reason: &str) -> Result<Vec<u8>, SignError> {
    if output.stdout.is_empty() {
        return Err(SignError::ToolFailed {
            command: "gpg".to_string(),
            status: "0（输出为空）".to_string(),
            stderr: reason.to_string(),
        });
    }
    Ok(output.stdout)
}
