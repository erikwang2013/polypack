//! 自托管仓库落地（T-M5-05 deb/rpm + T-M5-06 pkg）：元数据刷新与仓库签名。
//!
//! 目录布局（`--repo` 根；同一根可同时承载 deb / rpm / pkg）：
//!
//! ```text
//! <repo>/
//!   polypack-repo.gpg            # deb/rpm 公钥（apt signed-by；二进制 keyring）
//!   polypack-repo.asc            # deb/rpm 公钥（dnf gpgkey / 人读）
//!   pool/main/<prefix>/<pkg>/*.deb
//!   dists/<suite>/main/binary-<arch>/Packages
//!   dists/<suite>/{Release,InRelease,Release.gpg}
//!   rpm/*.rpm
//!   repodata/{primary,filelists,other}.xml.gz
//!   repodata/repomd.xml
//!   repodata/repomd.xml.asc
//!   *.pkg                        # pkg 产物扁平入库
//!   {meta.conf,data.pkg,packagesite.pkg}   # pkg catalogue（meta.conf 内含签名）
//!   polypack-repo.pub            # pkg 公钥（客户端 PUBKEY；PEM）
//!   *.tgz                        # pkgsrc 产物扁平入库（逐包 GPG 签名）
//!   pkg_summary.gz               # pkgsrc 仓库摘要（pkg_info -X 汇总）
//!   polypack-repo.asc            # pkgsrc 客户端 GPG_KEYRING_VERIFY 公钥
//! ```
//!
//! 客户端：apt 源 `deb [signed-by=<repo>/polypack-repo.gpg] file:<repo> <suite> main`；
//! dnf 源 `baseurl=file://<repo>` + `repo_gpgcheck=1` + `gpgkey=file://<repo>/polypack-repo.asc`；
//! pkg 源 `url: "file://<repo>"` + `signature_type: "pubkey"` + `pubkey: "<repo>/polypack-repo.pub"`；
//! pkgsrc 源 `PKG_PATH=file://<repo>` + `GPG_KEYRING_VERIFY=<repo>/polypack-repo.asc`。
//!
//! 确定性：deb/rpm 元数据只由产物内容 / mtime 与 `SOURCE_DATE_EPOCH` 决定（排序、gzip
//! mtime=0）；二次生成除 GPG 签名包时间戳外字节一致（签名走 [`polypack_sign::RepoSigner`]）。
//! pkg catalogue 由 `pkg repo` 生成（catalogue 排序稳定；签名时间戳可变），签名是 pkg(8)
//! 原生 rsa/ecdsa/eddsa，不是 GPG。pkgsrc 的 `pkg_summary.gz` 由 `pkg_info -X` 逐包生成
//! （gzip mtime=0），逐包签名走 `pkg_admin gpg-sign-package`（临时 GNUPGHOME，签名时间戳可变）。

mod deb;
mod pkg;
mod pkgsrc;
mod rpm;

#[cfg(test)]
mod tests;

use std::time::{SystemTime, UNIX_EPOCH};

use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use polypack_core::{CredentialRef, CredentialSource};
use polypack_sign::GpgSigner;

use super::{Artifact, RepoBackend, RepoKind, RepoRequest};
use crate::retry::parse_credential;
use crate::sign::sign_error;

/// 本地自托管仓库后端：产物入库 → 元数据刷新 → 签名，全部落在 `--repo` 根。
pub(super) struct LocalBackend;

impl RepoBackend for LocalBackend {
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
        match request.kind {
            RepoKind::Pkg => pkg::generate(request),
            RepoKind::Pkgsrc => pkgsrc::generate(request),
            RepoKind::Deb => deb::generate(request, &gpg_signer(request)?),
            RepoKind::Rpm => rpm::generate(request, &gpg_signer(request)?),
        }
    }
}

/// 解析签名凭据引用：`--credential` 只覆盖来源；缺省用目标声明的档案。
fn credential_ref(request: &RepoRequest<'_>) -> Result<CredentialRef, Diagnostic> {
    let (source, alias) = match request.credential {
        Some(value) => parse_credential(value)?,
        None => (CredentialSource::Keyring, None),
    };
    let name = alias
        .or_else(|| request.signing.map(str::to_string))
        .unwrap_or_else(|| request.target.to_string());
    Ok(CredentialRef { name, source })
}

/// deb/rpm 的 GPG 签名器（凭据只传引用）。
fn gpg_signer(request: &RepoRequest<'_>) -> Result<GpgSigner, Diagnostic> {
    GpgSigner::with_default_backends(&credential_ref(request)?).map_err(sign_error)
}

/// 仓库元数据错误统一口径（签名域错误码由 `sign_error` 原样透传）。
fn repo_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_RELEASE_0005, message.into()).with_hint(
        "核对 --repo 目录权限、产物可读性与 --credential 引用；\
         `polypack release --dry-run` 只校验产物面，不触仓库",
    )
}

/// SHA256（hex）。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// XML 文本 / 属性转义（包名与文件路径可能含 `&` / `<`）。
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 确定性的 gzip（mtime=0、无文件名 / 注释）。
fn gzip(xml: &str) -> Result<Vec<u8>, Diagnostic> {
    use std::io::Write;
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(xml.as_bytes())
        .and_then(|()| encoder.finish())
        .map_err(|err| repo_error(format!("元数据 gzip 失败：{err}")))
}

/// 元数据时刻：`SOURCE_DATE_EPOCH` 优先，否则取本批产物 mtime 的最大值。
fn generation_time(artifacts: &[Artifact]) -> u64 {
    if let Some(secs) = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        return secs;
    }
    artifacts
        .iter()
        .filter_map(|artifact| {
            let modified = std::fs::metadata(&artifact.path).ok()?.modified().ok()?;
            modified
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        })
        .max()
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        })
}

/// Debian `Release` 的 `Date`（RFC 2822，UTC；不引日历库）。
fn rfc2822(secs: u64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (secs / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    let weekday = ((days + 4).rem_euclid(7)) as usize; // 1970-01-01 = 周四
    let sod = secs % 86_400;
    format!(
        "{}, {:02} {} {year} {:02}:{:02}:{:02} UTC",
        DAYS[weekday],
        day,
        MONTHS[(month - 1) as usize],
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)（Howard Hinnant `civil_from_days`）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// 复制产物并保留源 mtime（重复生成的内容 / 时间均稳定）。
fn copy_preserving_mtime(src: &std::path::Path, dst: &std::path::Path) -> Result<(), Diagnostic> {
    if src == dst {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| repo_error(format!("目录 {} 不可建：{err}", parent.display())))?;
    }
    let modified = std::fs::metadata(src)
        .and_then(|meta| meta.modified())
        .map_err(|err| repo_error(format!("产物 {} 不可读：{err}", src.display())))?;
    std::fs::copy(src, dst)
        .map_err(|err| repo_error(format!("产物复制到 {} 失败：{err}", dst.display())))?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(dst)
        .and_then(|file| file.set_modified(modified))
        .map_err(|err| repo_error(format!("产物 {} mtime 归一失败：{err}", dst.display())))?;
    Ok(())
}

/// 写公钥两态（二进制供 apt `signed-by`，ASCII 供 dnf `gpgkey`）。
fn write_public_keys(
    root: &std::path::Path,
    signer: &dyn polypack_sign::RepoSigner,
) -> Result<(), Diagnostic> {
    let binary = signer.public_key(false).map_err(sign_error)?;
    let armored = signer.public_key(true).map_err(sign_error)?;
    for (name, bytes) in [
        ("polypack-repo.gpg", binary),
        ("polypack-repo.asc", armored),
    ] {
        let path = root.join(name);
        std::fs::write(&path, bytes)
            .map_err(|err| repo_error(format!("公钥 {} 不可写：{err}", path.display())))?;
    }
    Ok(())
}

/// 产物基名（仓库内扁平存放；拒绝分隔符与空名）。
fn artifact_basename(name: &str) -> Result<String, Diagnostic> {
    let basename = std::path::Path::new(name)
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|part| !part.is_empty())
        .ok_or_else(|| repo_error(format!("产物名 {name} 没有可用基名")))?;
    if basename == "." || basename == ".." {
        return Err(repo_error(format!("产物名 {name} 非法")));
    }
    Ok(basename.to_string())
}

/// 路径组件安全化（suite / 包名 / 版本等直接进目录名）。
fn safe_component(what: &str, value: &str) -> Result<String, Diagnostic> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
    {
        return Err(repo_error(format!("{what} 非法：{value:?}")));
    }
    Ok(value.to_string())
}
