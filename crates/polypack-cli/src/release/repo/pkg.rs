//! FreeBSD pkg 自托管仓库（T-M5-06）：`pkg repo` catalogue + 原生 rsa/ecdsa/eddsa 签名。
//!
//! 目录布局（`--repo` 根）：
//!
//! ```text
//! <repo>/
//!   *.pkg                        # 产物扁平入库
//!   meta.conf                    # catalogue（签名类型与签名值在此）
//!   data.pkg / packagesite.pkg   # catalogue 数据
//!   polypack-repo.pub            # 客户端 PUBKEY（PEM SubjectPublicKeyInfo）
//! ```
//!
//! 客户端：`/etc/pkg/polypack.conf` 里 `url: "file://<repo>"`、`signature_type: "pubkey"`、
//! `pubkey: "<repo>/polypack-repo.pub"`（等价环境：`SIGNATURE_TYPE=PUBKEY` + `PUBKEY=...`）。
//! 签名不是 GPG：pkg(8) 只消费 `pkg repo ... rsa|ecdsa|eddsa:<keyfile>` 的 catalogue 签名。
//!
//! 凭据契约沿用 T-M2-11/T-M5-05（[`polypack_sign::resolve`]）：`keystore` = **未加密** PEM 私钥
//! （pkg repo 无口令通道），`store_password` 仅满足载荷契约、不参与签名；`key_alias` 为
//! `rsa` / `ecdsa` / `eddsa` 时作为签名算法（缺省按 PEM 头推断：EC → ecdsa，其余 → rsa；
//! PKCS#8 的 Ed25519 无头可辨，须显式给 key_alias）。私钥只落一次性 0700 目录内的 0600
//! 文件（argv / 日志只出现路径），命令结束即删；仓库目录只落公钥。
//!
//! Linux 宿主通常无 pkg(8)：本后端如实报 `E_RELEASE_0005` 并点名 FreeBSD VM（T-M5-09），
//! 绝不伪造 catalogue；`pkg repo` 真跑与 `pkg install` 冒烟归 FreeBSD 14.5 VM（bsd-ci）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use polypack_core::diagnostics::Diagnostic;
use polypack_sign::SigningMaterial;

use super::{RepoRequest, artifact_basename, copy_preserving_mtime, credential_ref, repo_error};
use crate::sign::sign_error;

/// `pkg repo` 接受的签名算法（`<type>:<keyfile>` 的 `<type>`）。
const SIGNING_TYPES: [&str; 3] = ["rsa", "ecdsa", "eddsa"];

/// 生成（或刷新）pkg 仓库 catalogue 并签名。
pub(super) fn generate(request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
    ensure_pkg()?;
    // 先解析凭据再碰仓库：缺密钥材料时不留下半成品目录。
    let material = polypack_sign::resolve(
        &credential_ref(request)?,
        &polypack_sign::default_backends(),
    )
    .map_err(sign_error)?;
    check_unencrypted(&material)?;
    let repo = request.repo;
    std::fs::create_dir_all(repo)
        .map_err(|err| repo_error(format!("仓库目录 {} 不可建：{err}", repo.display())))?;
    for artifact in request.artifacts {
        let name = artifact_basename(&artifact.name)?;
        copy_preserving_mtime(&artifact.path, &repo.join(name))?;
    }

    let temp = TempKeyDir::create()?;
    let key = temp.path.join("repo-key.pem");
    write_private(&key, &material.keystore)?;
    let kind = signing_type(&material);
    let command = polypack_bundle::Command::new(
        "pkg",
        [
            "repo".to_string(),
            repo.display().to_string(),
            format!("{kind}:{}", key.display()),
        ],
    );
    polypack_bundle::exec::run(&command)
        .map_err(|err| repo_error(format!("pkg repo 失败：{err}")))?;
    // 公钥供客户端 PUBKEY 校验（从同一私钥导出；pkg repo 本身不导出公钥）。
    export_public_key(&key, &repo.join("polypack-repo.pub"))?;

    // 真跑后核对 catalogue 三件套：跑过却没产物 = 假绿。
    for name in ["meta.conf", "data.pkg", "packagesite.pkg"] {
        let path = repo.join(name);
        if !path.is_file() {
            return Err(repo_error(format!(
                "pkg repo 未产出 {name}（仓库 {}）",
                repo.display()
            )));
        }
    }
    Ok(())
}

/// pkg(8) 在场探测：缺席即如实失败（Linux 交叉发起不生成 catalog）。
fn ensure_pkg() -> Result<(), Diagnostic> {
    if polypack_bundle::exec::tool_present("pkg") {
        return Ok(());
    }
    Err(repo_error(
        "PATH 无 pkg（FreeBSD pkg(8)）：pkg 仓库 catalogue 与签名须在 FreeBSD 用户态生成\
         （T-M5-09 FreeBSD 14.5 VM）；本机不伪造仓库元数据",
    ))
}

/// 签名算法：key_alias 显式给 rsa/ecdsa/eddsa 优先，否则按 PEM 头推断（EC → ecdsa）。
fn signing_type(material: &SigningMaterial) -> &'static str {
    if let Some(alias) = material.key_alias.as_deref()
        && let Some(kind) = SIGNING_TYPES.iter().copied().find(|kind| *kind == alias)
    {
        return kind;
    }
    let text = String::from_utf8_lossy(&material.keystore);
    if text.contains("BEGIN EC PRIVATE KEY") {
        "ecdsa"
    } else {
        "rsa"
    }
}

/// pkg repo 无口令通道：加密 PEM 直接拒绝，绝不把口令塞进 argv。
fn check_unencrypted(material: &SigningMaterial) -> Result<(), Diagnostic> {
    let text = String::from_utf8_lossy(&material.keystore);
    if text.contains("ENCRYPTED PRIVATE KEY") || text.contains("Proc-Type: 4,ENCRYPTED") {
        return Err(repo_error(
            "pkg 仓库私钥是加密 PEM：pkg repo 无口令通道；请提供未加密 PKCS#8/PEM 私钥\
             （凭据载荷的 store_password 仍须非空以符合 T-M2-11 契约）",
        ));
    }
    Ok(())
}

/// 一次性私钥目录（0700；Drop 整目录删除）。
struct TempKeyDir {
    path: PathBuf,
}

impl TempKeyDir {
    fn create() -> Result<Self, Diagnostic> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let path =
            std::env::temp_dir().join(format!("polypack-pkg-repo-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).map_err(|err| {
            repo_error(format!("一次性密钥目录 {} 不可建：{err}", path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|err| repo_error(format!("一次性密钥目录权限不可收紧：{err}")))?;
        }
        Ok(Self { path })
    }
}

impl Drop for TempKeyDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 0600 写入私钥（创建即限定权限，不存在宽权限窗口）。
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
        .map_err(|err| repo_error(format!("私钥临时文件不可写：{err}")))?;
    file.write_all(bytes)
        .map_err(|err| repo_error(format!("私钥临时文件写入失败：{err}")))
}

/// `openssl pkey -in <key> -pubout -out <repo>/polypack-repo.pub`（路径只在 argv）。
fn export_public_key(key: &Path, out: &Path) -> Result<(), Diagnostic> {
    let command = polypack_bundle::Command::new(
        "openssl",
        [
            "pkey".to_string(),
            "-in".to_string(),
            key.display().to_string(),
            "-pubout".to_string(),
            "-out".to_string(),
            out.display().to_string(),
        ],
    );
    polypack_bundle::exec::run(&command).map_err(|err| {
        repo_error(format!(
            "导出 pkg 仓库公钥失败：{err}（需 openssl pkey；FreeBSD base 自带）"
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(alias: Option<&str>, pem: &str) -> SigningMaterial {
        SigningMaterial {
            keystore: pem.as_bytes().to_vec(),
            store_password: "unused".to_string(),
            key_alias: alias.map(str::to_string),
            key_password: None,
            app_cert: None,
            profile: None,
        }
    }

    #[test]
    fn signing_type_prefers_explicit_alias_then_pem_header() {
        let rsa = material(
            None,
            "-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n", // POLYPACK_TEST_FIXTURE
        );
        assert_eq!(signing_type(&rsa), "rsa", "PKCS#8 缺省 rsa");
        let ec = material(
            None,
            "-----BEGIN EC PRIVATE KEY-----\nMHc\n-----END EC PRIVATE KEY-----\n", // POLYPACK_TEST_FIXTURE
        );
        assert_eq!(signing_type(&ec), "ecdsa", "EC PEM 头推断 ecdsa");
        let ed = material(Some("eddsa"), "-----BEGIN PRIVATE KEY-----\nMIIB\n"); // POLYPACK_TEST_FIXTURE
        assert_eq!(signing_type(&ed), "eddsa", "key_alias 显式算法优先");
        let fingerprint = material(Some("ABCDEF0123"), "-----BEGIN EC PRIVATE KEY-----\nMHc\n"); // POLYPACK_TEST_FIXTURE
        assert_eq!(
            signing_type(&fingerprint),
            "ecdsa",
            "非算法名的 key_alias 退回 PEM 推断"
        );
    }

    #[test]
    fn encrypted_pem_is_rejected_before_any_repo_write() {
        let encrypted = material(None, "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIB\n"); // POLYPACK_TEST_FIXTURE
        let err = check_unencrypted(&encrypted).expect_err("加密 PEM 必须拒绝");
        assert_eq!(err.code.to_string(), "polypack::E_RELEASE_0005");
        assert!(err.message.contains("加密 PEM"), "{}", err.message);
        let legacy = material(
            None,
            "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n\nMIIB\n", // POLYPACK_TEST_FIXTURE
        );
        assert!(check_unencrypted(&legacy).is_err(), "旧式加密头同样拒绝");
        let plain = material(None, "-----BEGIN PRIVATE KEY-----\nMIIB\n"); // POLYPACK_TEST_FIXTURE
        check_unencrypted(&plain).expect("未加密 PEM 应放行");
    }
}
