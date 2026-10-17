//! deb 仓库元数据（Packages / Release / InRelease / Release.gpg）。
//!
//! 解析复用 `poly-bundle::arwrite`（ar + gzip tar + `control_fields`，与打包器 / verify 腿
//! 同一实现，无第三份 deb control 解析）。pool 按 Debian 约定分层：
//! `pool/main/<prefix>/<package>/<file>.deb`（`lib*` 取前 4 字符，其余取首字符）。

use std::collections::BTreeMap;
use std::path::Path;

use poly_bundle::arwrite::{TarArchive, control_fields, read_ar};
use poly_core::diagnostics::Diagnostic;
use poly_sign::RepoSigner;

use super::{
    RepoRequest, artifact_basename, copy_preserving_mtime, generation_time, repo_error, rfc2822,
    safe_component, sha256_hex, write_public_keys,
};
use crate::sign::sign_error;

/// 仓库组件（当前只出 `main`；多组件留待后续按配置拆分）。
const COMPONENT: &str = "main";

/// 一个入库的 deb。
struct DebPackage {
    /// control `Package`。
    package: String,
    /// control `Version`。
    version: String,
    /// control `Architecture`。
    arch: String,
    /// control 全字段（BTreeMap：输出顺序稳定）。
    control: BTreeMap<String, String>,
    /// 仓库内 pool 相对路径（`/` 分隔）。
    pool: String,
    /// 字节数。
    size: u64,
    /// SHA256（hex）。
    sha256: String,
}

/// 生成 / 刷新 deb 仓库并签名。
pub(super) fn generate(
    request: &RepoRequest<'_>,
    signer: &dyn RepoSigner,
) -> Result<(), Diagnostic> {
    let suite = safe_component("suite（目标 id）", request.target)?;
    if request.artifacts.is_empty() {
        return Err(repo_error("产物集合为空：没有可入库的 .deb"));
    }

    let mut packages = Vec::with_capacity(request.artifacts.len());
    let mut seen = BTreeMap::<String, String>::new();
    for artifact in request.artifacts {
        let bytes = std::fs::read(&artifact.path)
            .map_err(|err| repo_error(format!("产物 {} 不可读：{err}", artifact.path.display())))?;
        let basename = artifact_basename(&artifact.name)?;
        let control = parse_control(&artifact.path)?;
        let package = required_field(&control, "Package", &artifact.path)?;
        let version = required_field(&control, "Version", &artifact.path)?;
        let arch = required_field(&control, "Architecture", &artifact.path)?;
        required_field(&control, "Description", &artifact.path)?;
        // Package 进 pool 路径，Architecture 进 binary-<arch> 目录：按路径组件校验。
        safe_component("Package", &package)?;
        safe_component("Architecture", &arch)?;
        if !package.is_ascii() {
            return Err(repo_error(format!(
                "Package 必须是 ASCII：{package:?}（{}）",
                artifact.path.display()
            )));
        }
        let expected = format!("{package}_{version}_{arch}.deb");
        if expected != basename {
            return Err(repo_error(format!(
                "deb 文件名与控制字段不一致：{} ≠ {expected}（control 与产物同源校验）",
                artifact.path.display()
            )));
        }
        if let Some(previous) = seen.insert(basename.clone(), package.clone()) {
            return Err(repo_error(format!(
                "仓库内 deb 基名冲突：{basename}（{previous} / {package}）"
            )));
        }
        let pool = format!("pool/{COMPONENT}/{}/{package}/{basename}", prefix(&package));
        copy_preserving_mtime(&artifact.path, &request.repo.join(&pool))?;
        packages.push(DebPackage {
            package,
            version,
            arch,
            control,
            pool,
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }

    // 按架构分 Packages（排序保证字节稳定）。
    packages
        .sort_by(|a, b| (&a.package, &a.version, &a.arch).cmp(&(&b.package, &b.version, &b.arch)));
    let mut by_arch = BTreeMap::<String, String>::new();
    for package in &packages {
        let stanza = by_arch.entry(package.arch.clone()).or_default();
        stanza.push_str(&render_stanza(package));
        stanza.push('\n');
    }

    let mut release = String::new();
    release.push_str("Origin: PolyPack\n");
    release.push_str("Label: PolyPack\n");
    release.push_str(&format!("Suite: {suite}\n"));
    release.push_str(&format!("Codename: {suite}\n"));
    release.push_str(&format!(
        "Date: {}\n",
        rfc2822(generation_time(request.artifacts))
    ));
    release.push_str(&format!(
        "Architectures: {}\n",
        by_arch.keys().cloned().collect::<Vec<_>>().join(" ")
    ));
    release.push_str(&format!("Components: {COMPONENT}\n"));
    release.push_str("Description: PolyPack self-hosted repository\n");
    release.push_str("SHA256:\n");

    let mut checksums = String::new();
    for (arch, packages_text) in &by_arch {
        // Release 的 SHA256 路径相对 `dists/<suite>/`（apt 按该相对路径查索引）。
        let rel = format!("{COMPONENT}/binary-{arch}/Packages");
        let path = request.repo.join("dists").join(&suite).join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| repo_error(format!("目录 {} 不可建：{err}", parent.display())))?;
        }
        std::fs::write(&path, packages_text.as_bytes())
            .map_err(|err| repo_error(format!("{} 不可写：{err}", path.display())))?;
        checksums.push_str(&format!(
            " {} {} {rel}\n",
            sha256_hex(packages_text.as_bytes()),
            packages_text.len()
        ));
    }
    release.push_str(&checksums);
    let release_bytes = release.into_bytes();

    let dists = request.repo.join("dists").join(&suite);
    std::fs::create_dir_all(&dists)
        .map_err(|err| repo_error(format!("目录 {} 不可建：{err}", dists.display())))?;
    std::fs::write(dists.join("Release"), &release_bytes)
        .map_err(|err| repo_error(format!("Release 不可写：{err}")))?;
    let in_release = signer.clear_sign(&release_bytes).map_err(sign_error)?;
    std::fs::write(dists.join("InRelease"), in_release)
        .map_err(|err| repo_error(format!("InRelease 不可写：{err}")))?;
    let release_gpg = signer.detach_sign(&release_bytes).map_err(sign_error)?;
    std::fs::write(dists.join("Release.gpg"), release_gpg)
        .map_err(|err| repo_error(format!("Release.gpg 不可写：{err}")))?;
    write_public_keys(request.repo, signer)
}

/// 解析 `.deb` 的 control（gzip tar；T-M5-01 产物固定 `-Zgzip`）。
fn parse_control(deb: &Path) -> Result<BTreeMap<String, String>, Diagnostic> {
    let members = read_ar(deb).map_err(|err| {
        repo_error(format!(
            "{} 不是合法 deb（ar 解析失败）：{err}",
            deb.display()
        ))
    })?;
    let control = members
        .iter()
        .find(|member| member.name == "control.tar.gz")
        .ok_or_else(|| {
            repo_error(format!(
                "{} 缺 control.tar.gz 成员（poly 的 deb 产物固定 gzip control；xz/zst 暂不解析，避免静默漏字段）",
                deb.display()
            ))
        })?;
    let tar = TarArchive::from_tar_gz(&control.bytes, deb).map_err(|err| {
        repo_error(format!(
            "{} 的 control.tar.gz 解析失败：{err}",
            deb.display()
        ))
    })?;
    let text = tar
        .text("./control")
        .ok_or_else(|| repo_error(format!("{} 的 control.tar.gz 缺 ./control", deb.display())))?;
    Ok(control_fields(&text))
}

fn required_field(
    control: &BTreeMap<String, String>,
    field: &str,
    deb: &Path,
) -> Result<String, Diagnostic> {
    control
        .get(field)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| repo_error(format!("{} 的 control 缺必填字段 {field}", deb.display())))
}

/// Debian pool 前缀：`lib*` 取前 4 字符，其余取首字符。
fn prefix(package: &str) -> &str {
    if package.len() >= 4 && package.starts_with("lib") {
        package.get(..4).unwrap_or(package)
    } else {
        package.get(..1).unwrap_or(package)
    }
}

/// 单个 Packages stanza：控制字段白名单 + 仓库定位字段。
fn render_stanza(package: &DebPackage) -> String {
    let mut out = String::new();
    for (key, value) in [
        ("Package", package.package.as_str()),
        ("Version", package.version.as_str()),
        (
            "Architecture",
            package
                .control
                .get("Architecture")
                .map_or("", String::as_str),
        ),
        (
            "Maintainer",
            package.control.get("Maintainer").map_or("", String::as_str),
        ),
        (
            "Installed-Size",
            package
                .control
                .get("Installed-Size")
                .map_or("", String::as_str),
        ),
        (
            "Depends",
            package.control.get("Depends").map_or("", String::as_str),
        ),
        (
            "Section",
            package.control.get("Section").map_or("", String::as_str),
        ),
        (
            "Priority",
            package.control.get("Priority").map_or("", String::as_str),
        ),
        (
            "Homepage",
            package.control.get("Homepage").map_or("", String::as_str),
        ),
    ] {
        if !value.is_empty() {
            out.push_str(&format!("{key}: {value}\n"));
        }
    }
    out.push_str(&format!(
        "Description: {}\n",
        package
            .control
            .get("Description")
            .map_or("", String::as_str)
    ));
    out.push_str(&format!("Filename: {}\n", package.pool));
    out.push_str(&format!("Size: {}\n", package.size));
    out.push_str(&format!("SHA256: {}\n", package.sha256));
    out
}
