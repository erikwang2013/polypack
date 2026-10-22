//! NetBSD / DragonFly pkgsrc 自托管仓库（T-M5-08）：扁平 `.tgz` + `pkg_summary.gz` + 逐包 GPG。
//!
//! 目录布局（`--repo` 根）：
//!
//! ```text
//! <repo>/
//!   *.tgz                  # 包产物扁平入库
//!   pkg_summary.gz         # pkg_info -X 汇总（mtime=0，内容由产物元数据派生）
//!   polypack-repo.asc      # 客户端 GPG_KEYRING_VERIFY 公钥（ASCII armor）
//! ```
//!
//! 客户端：`PKG_PATH=file://<repo>` + `GPG_KEYRING_VERIFY=<repo>/polypack-repo.asc`；
//! pkg_add 用 libnetpgpverify 校验 `pkg_admin gpg-sign-package` 的逐包签名。
//!
//! 凭据契约沿用 T-M5-05/06（[`polypack_sign::resolve`]）：`keystore` = OpenPGP 私钥
//! （`gpg --export-secret-keys`，二进制 / armor 均可），`key_alias` = 签名 UID / fingerprint
//! （缺省用临时 keyring 里的首把私钥）。私钥只落一次性 0700 GNUPGHOME 内的 0600 文件
//! （argv / 日志只出现路径），导入后即删，签名结束 kill gpg-agent 并整目录删除；仓库目录
//! 只落公钥。`pkg_admin` 无口令通道：加密私钥会在真跑时报错，不静默降级。
//!
//! Linux 宿主通常无 pkg_info / pkg_admin：本后端如实报 `E_RELEASE_0005` 并点名 NetBSD VM
//! （T-M5-09），绝不伪造 pkg_summary / 签名；逐包签名与 `pkg_add` 冒烟归 NetBSD CI/VM。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use polypack_core::diagnostics::Diagnostic;
use polypack_sign::SigningMaterial;

use super::{
    RepoRequest, artifact_basename, copy_preserving_mtime, credential_ref, gzip, repo_error,
};
use crate::sign::sign_error;

/// 生成（或刷新）pkgsrc 仓库：产物入库 → 逐包 GPG 签名 → 公钥 → `pkg_summary.gz`。
pub(super) fn generate(request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
    ensure_tools()?;
    // 先解析凭据再碰仓库：缺密钥材料时不留下半成品目录。
    let material = polypack_sign::resolve(&credential_ref(request)?, &polypack_sign::default_backends())
        .map_err(sign_error)?;
    let repo = request.repo;
    std::fs::create_dir_all(repo)
        .map_err(|err| repo_error(format!("仓库目录 {} 不可建：{err}", repo.display())))?;
    for artifact in request.artifacts {
        let name = artifact_basename(&artifact.name)?;
        if !name.ends_with(".tgz") {
            return Err(repo_error(format!(
                "pkgsrc 仓库只收 .tgz 包：产物名 {name:?} 不符"
            )));
        }
        copy_preserving_mtime(&artifact.path, &repo.join(name))?;
    }

    let home = TempGpgHome::create()?;
    let user = import_signing_key(&home, &material)?;
    let conf = write_install_conf(&home, &user)?;
    for artifact in request.artifacts {
        sign_package(repo, artifact, &home, &conf)?;
    }
    export_public_key(&home, &user, &repo.join("polypack-repo.asc"))?;
    write_summary(repo, request.artifacts)?;
    Ok(())
}

/// pkg 工具链在场探测：缺席即如实失败（Linux 交叉发起不伪造仓库元数据）。
fn ensure_tools() -> Result<(), Diagnostic> {
    let missing: Vec<&str> = ["pkg_info", "pkg_admin", "gpg"]
        .into_iter()
        .filter(|tool| !polypack_bundle::exec::tool_present(tool))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(repo_error(format!(
        "PATH 无 {}：pkgsrc 仓库的 .tgz 签名与 pkg_summary.gz 须在 NetBSD 用户态生成\
         （T-M5-09 NetBSD VM）；本机不伪造仓库元数据",
        missing.join(" / ")
    )))
}

/// 一次性 GNUPGHOME（0700；Drop kill gpg-agent 并整目录删除）。
struct TempGpgHome {
    path: PathBuf,
}

impl TempGpgHome {
    fn create() -> Result<Self, Diagnostic> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let path = std::env::temp_dir().join(format!(
            "polypack-pkgsrc-repo-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).map_err(|err| {
            repo_error(format!("一次性 GNUPGHOME {} 不可建：{err}", path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|err| repo_error(format!("一次性 GNUPGHOME 权限不可收紧：{err}")))?;
        }
        Ok(Self { path })
    }
}

impl Drop for TempGpgHome {
    fn drop(&mut self) {
        let _ = std::process::Command::new("gpgconf")
            .arg("--homedir")
            .arg(&self.path)
            .arg("--kill")
            .arg("gpg-agent")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 导入凭据私钥并返回 `GPG_SIGN_AS`：key_alias 优先，否则取临时 keyring 的首把私钥。
fn import_signing_key(
    home: &TempGpgHome,
    material: &SigningMaterial,
) -> Result<String, Diagnostic> {
    let key = home.path.join("repo-key.gpg");
    write_private(&key, &material.keystore)?;
    let import = polypack_bundle::Command::new(
        "gpg",
        [
            "--batch".to_string(),
            "--no-options".to_string(),
            "--homedir".to_string(),
            home.path.display().to_string(),
            "--import".to_string(),
            key.display().to_string(),
        ],
    );
    polypack_bundle::exec::run(&import)
        .map_err(|err| repo_error(format!("gpg 导入 pkgsrc 签名私钥失败：{err}")))?;
    let _ = std::fs::remove_file(&key);

    if let Some(alias) = material
        .key_alias
        .as_deref()
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
    {
        return Ok(alias.to_string());
    }
    let list = polypack_bundle::Command::new(
        "gpg",
        [
            "--batch".to_string(),
            "--no-options".to_string(),
            "--homedir".to_string(),
            home.path.display().to_string(),
            "--with-colons".to_string(),
            "--list-secret-keys".to_string(),
        ],
    );
    let output = polypack_bundle::exec::run(&list)
        .map_err(|err| repo_error(format!("gpg 列出 pkgsrc 签名私钥失败：{err}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find(|line| line.starts_with("sec:"))
        .and_then(|line| line.split(':').nth(4))
        .filter(|key_id| !key_id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            repo_error("临时 keyring 没有私钥：凭据 keystore 须为 gpg --export-secret-keys 输出")
        })
}

/// 写 `pkg_install.conf`（0600）：`pkg_admin -C` 只读签名相关两项，密钥值不进文件。
fn write_install_conf(home: &TempGpgHome, user: &str) -> Result<PathBuf, Diagnostic> {
    let path = home.path.join("pkg_install.conf");
    let text = format!("GPG=gpg\nGPG_SIGN_AS={user}\n");
    write_private(&path, text.as_bytes())?;
    Ok(path)
}

/// `pkg_admin -C <conf> gpg-sign-package <in> <out>`（GNUPGHOME 指向一次性 keyring）。
fn sign_package(
    repo: &Path,
    artifact: &super::Artifact,
    home: &TempGpgHome,
    conf: &Path,
) -> Result<(), Diagnostic> {
    let name = artifact_basename(&artifact.name)?;
    let src = repo.join(&name);
    let dst = repo.join(format!("{name}.signed"));
    let command = polypack_bundle::Command::new(
        "pkg_admin",
        [
            "-C".to_string(),
            conf.display().to_string(),
            "gpg-sign-package".to_string(),
            src.display().to_string(),
            dst.display().to_string(),
        ],
    )
    .with_env("GNUPGHOME", home.path.display().to_string());
    if let Err(err) = polypack_bundle::exec::run(&command) {
        let _ = std::fs::remove_file(&dst);
        return Err(repo_error(format!(
            "pkg_admin gpg-sign-package 失败：{err}"
        )));
    }
    if !dst.is_file() {
        return Err(repo_error(format!(
            "pkg_admin gpg-sign-package 未产出签名包 {name}"
        )));
    }
    std::fs::rename(&dst, &src)
        .map_err(|err| repo_error(format!("签名包 {name} 就地替换失败：{err}")))
}

/// `gpg --armor --export <user>` → `<repo>/polypack-repo.asc`（路径只在 argv）。
fn export_public_key(home: &TempGpgHome, user: &str, out: &Path) -> Result<(), Diagnostic> {
    let command = polypack_bundle::Command::new(
        "gpg",
        [
            "--batch".to_string(),
            "--no-options".to_string(),
            "--homedir".to_string(),
            home.path.display().to_string(),
            "--armor".to_string(),
            "--export".to_string(),
            user.to_string(),
        ],
    );
    let output = polypack_bundle::exec::run(&command)
        .map_err(|err| repo_error(format!("导出 pkgsrc 仓库公钥失败：{err}")))?;
    if output.stdout.is_empty() {
        return Err(repo_error(format!(
            "gpg 未导出公钥（GPG_SIGN_AS={user:?} 与私钥不匹配？）"
        )));
    }
    std::fs::write(out, &output.stdout)
        .map_err(|err| repo_error(format!("公钥 {} 不可写：{err}", out.display())))
}

/// 逐包 `pkg_info -X` 汇总 → 确定性 gzip（mtime=0）写 `pkg_summary.gz`。
fn write_summary(repo: &Path, artifacts: &[super::Artifact]) -> Result<(), Diagnostic> {
    let mut summary = String::new();
    for artifact in artifacts {
        let name = artifact_basename(&artifact.name)?;
        let path = repo.join(&name);
        let command =
            polypack_bundle::Command::new("pkg_info", ["-X".to_string(), path.display().to_string()]);
        let output = polypack_bundle::exec::run(&command)
            .map_err(|err| repo_error(format!("pkg_info -X {name} 失败：{err}")))?;
        let text = String::from_utf8_lossy(&output.stdout);
        if text.trim().is_empty() {
            return Err(repo_error(format!(
                "pkg_info -X {name} 输出为空（不写空摘要）"
            )));
        }
        summary.push_str(text.trim_end());
        summary.push_str("\n\n");
    }
    let compressed = gzip(&summary)?;
    let path = repo.join("pkg_summary.gz");
    std::fs::write(&path, compressed)
        .map_err(|err| repo_error(format!("{} 不可写：{err}", path.display())))
}

/// 0600 写入临时文件（创建即限定权限，不存在宽权限窗口）。
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| repo_error(format!("临时文件 {} 不可写：{err}", path.display())))?;
    file.write_all(bytes)
        .map_err(|err| repo_error(format!("临时文件 {} 写入失败：{err}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pkg_tools_fail_with_stable_code_and_vm_hint() {
        let tools = ["pkg_info", "pkg_admin", "gpg"];
        if tools
            .iter()
            .all(|tool| polypack_bundle::exec::tool_present(tool))
        {
            eprintln!("SKIP：本机三件套齐全，缺工具分支不可复现（NetBSD 真跑归 T-M5-09）");
            return;
        }
        let err = ensure_tools().expect_err("缺 pkg 工具必须失败");
        assert_eq!(err.code.to_string(), "polypack::E_RELEASE_0005");
        assert!(err.message.contains("NetBSD"), "{}", err.message);
        assert!(!err.message.contains("BEGIN"), "错误不得泄露密钥材料");
    }
}
