//! rpm 仓库元数据（primary / filelists / other + repomd + 签名）。
//!
//! repodata 只读所需的最小组件在本模块内解析（`header.rs`；lead 批准：不改 poly-bundle
//! 的 `rpm_header.rs`，其公开面保持 name/version/release/arch/license/files 的 verify 自检集）。
//! 后续可把共享解析上移：若 poly-bundle 需要完整的 header 只读视图，再把 `header.rs` 的
//! [`Header`] 抽到上游，poly-cli 改为消费。
//!
//! 布局：`<repo>/rpm/*.rpm` + `<repo>/repodata/*.xml.gz` + `repomd.xml(.asc)`；
//! 客户端 `baseurl=file://<repo>` + `repo_gpgcheck=1` + `gpgkey=file://<repo>/polypack-repo.asc`。

use std::collections::BTreeMap;
use std::path::Path;

use poly_core::diagnostics::Diagnostic;
use poly_sign::RepoSigner;

use super::{
    RepoRequest, artifact_basename, copy_preserving_mtime, generation_time, gzip, repo_error,
    sha256_hex, write_public_keys,
};
use crate::sign::sign_error;

mod header;
mod render;

use header::Header;
use render::{render_filelists, render_other, render_primary, render_repomd};

// 标签号（rpm 标签表；只取 repodata 需要的子集）。
const TAG_NAME: u32 = 1000;
const TAG_VERSION: u32 = 1001;
const TAG_RELEASE: u32 = 1002;
const TAG_EPOCH: u32 = 1003;
const TAG_SUMMARY: u32 = 1004;
const TAG_DESCRIPTION: u32 = 1005;
const TAG_BUILDTIME: u32 = 1006;
const TAG_BUILDHOST: u32 = 1007;
const TAG_SIZE: u32 = 1009;
const TAG_VENDOR: u32 = 1011;
const TAG_LICENSE: u32 = 1014;
const TAG_PACKAGER: u32 = 1015;
const TAG_GROUP: u32 = 1016;
const TAG_URL: u32 = 1020;
const TAG_ARCH: u32 = 1022;
const TAG_FILEMODES: u32 = 1030;
const TAG_SOURCERPM: u32 = 1044;
const TAG_ARCHIVESIZE: u32 = 1046;
const TAG_PROVIDENAME: u32 = 1047;
const TAG_REQUIREFLAGS: u32 = 1048;
const TAG_REQUIRENAME: u32 = 1049;
const TAG_REQUIREVERSION: u32 = 1050;
const TAG_PROVIDEFLAGS: u32 = 1112;
const TAG_PROVIDEVERSION: u32 = 1113;
const TAG_DIRINDEXES: u32 = 1116;
const TAG_BASENAMES: u32 = 1117;
const TAG_DIRNAMES: u32 = 1118;

/// 一个入库的 rpm（repodata 所需视图）。
struct RpmPackage {
    name: String,
    version: String,
    release: String,
    epoch: u32,
    arch: String,
    summary: String,
    description: String,
    packager: String,
    url: String,
    license: String,
    vendor: String,
    group: String,
    buildhost: String,
    sourcerpm: String,
    buildtime: u32,
    installed_size: u64,
    archive_size: u64,
    header_start: u64,
    header_end: u64,
    checksum: String,
    location: String,
    size: u64,
    mtime: u64,
    provides: Vec<Dep>,
    requires: Vec<Dep>,
    files: Vec<RpmFile>,
}

/// 依赖项（provide / require 同构）。
struct Dep {
    name: String,
    flags: &'static str,
    epoch: String,
    ver: String,
    rel: String,
}

/// rpm 内的文件条目。
struct RpmFile {
    path: String,
    is_dir: bool,
}

/// 生成 / 刷新 rpm 仓库并签名。
pub(super) fn generate(
    request: &RepoRequest<'_>,
    signer: &dyn RepoSigner,
) -> Result<(), Diagnostic> {
    if request.artifacts.is_empty() {
        return Err(repo_error("产物集合为空：没有可入库的 .rpm"));
    }
    let timestamp = generation_time(request.artifacts);

    let mut packages = Vec::with_capacity(request.artifacts.len());
    let mut seen = BTreeMap::<String, ()>::new();
    for artifact in request.artifacts {
        let basename = artifact_basename(&artifact.name)?;
        if seen.insert(basename.clone(), ()).is_some() {
            return Err(repo_error(format!("仓库内 rpm 基名冲突：{basename}")));
        }
        let bytes = std::fs::read(&artifact.path)
            .map_err(|err| repo_error(format!("产物 {} 不可读：{err}", artifact.path.display())))?;
        let mut package = parse_rpm(&artifact.path, &bytes, &basename)?;
        package.mtime = std::fs::metadata(&artifact.path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(timestamp, |duration| duration.as_secs());
        package.size = bytes.len() as u64;
        package.checksum = sha256_hex(&bytes);
        package.location = format!("rpm/{basename}");
        copy_preserving_mtime(&artifact.path, &request.repo.join(&package.location))?;
        packages.push(package);
    }
    // 包序稳定：同内容同输入 → 同 XML 字节。
    packages.sort_by(|a, b| {
        (&a.name, &a.version, &a.release, &a.arch).cmp(&(&b.name, &b.version, &b.release, &b.arch))
    });

    let primary = render_primary(&packages);
    let filelists = render_filelists(&packages);
    let other = render_other(&packages);
    let mut parts = Vec::with_capacity(3);
    for (kind, xml, href) in [
        ("primary", &primary, "repodata/primary.xml.gz"),
        ("filelists", &filelists, "repodata/filelists.xml.gz"),
        ("other", &other, "repodata/other.xml.gz"),
    ] {
        let gz = gzip(xml)?;
        parts.push((kind, xml.as_bytes(), gz, href));
    }
    let repomd = render_repomd(&parts, timestamp);
    let repomd_bytes = repomd.into_bytes();

    let repodata = request.repo.join("repodata");
    std::fs::create_dir_all(&repodata)
        .map_err(|err| repo_error(format!("目录 {} 不可建：{err}", repodata.display())))?;
    for (_, _, gz, href) in &parts {
        let path = request.repo.join(href);
        std::fs::write(&path, gz)
            .map_err(|err| repo_error(format!("{} 不可写：{err}", path.display())))?;
    }
    std::fs::write(repodata.join("repomd.xml"), &repomd_bytes)
        .map_err(|err| repo_error(format!("repomd.xml 不可写：{err}")))?;
    let signature = signer.detach_sign(&repomd_bytes).map_err(sign_error)?;
    std::fs::write(repodata.join("repomd.xml.asc"), signature)
        .map_err(|err| repo_error(format!("repomd.xml.asc 不可写：{err}")))?;
    write_public_keys(request.repo, signer)
}

/// 解析 rpm：lead + 签名头 + 主头（tag 偏移均相对于本头的数据段）。
fn parse_rpm(path: &Path, bytes: &[u8], basename: &str) -> Result<RpmPackage, Diagnostic> {
    if bytes.len() < 96 || bytes.get(0..4) != Some(&[0xed, 0xab, 0xee, 0xdb][..]) {
        return Err(repo_error(format!(
            "{} 不是合法 rpm（lead 魔数不符）",
            path.display()
        )));
    }
    let signature = Header::parse(bytes, 96, path)?;
    // 签名头后按 8 字节对齐到主头（注意是「向上取整到 8 的倍数」，不是向上取整为字节数）。
    let main_start = signature.end.div_ceil(8) * 8;
    let main = Header::parse(bytes, main_start, path)?;

    let name = required(&main, TAG_NAME, "NAME", path)?;
    let version = required(&main, TAG_VERSION, "VERSION", path)?;
    let release = required(&main, TAG_RELEASE, "RELEASE", path)?;
    let arch = required(&main, TAG_ARCH, "ARCH", path)?;
    let expected = format!("{name}-{version}-{release}.{arch}.rpm");
    if expected != basename {
        return Err(repo_error(format!(
            "rpm 文件名与主头标签不一致：{basename} ≠ {expected}（标签与产物同源校验）"
        )));
    }

    let epoch = main.int32(TAG_EPOCH).unwrap_or(0);
    let summary = main.i18n(TAG_SUMMARY).unwrap_or_else(|| name.clone());
    let description = main
        .i18n(TAG_DESCRIPTION)
        .unwrap_or_else(|| summary.clone());
    let provides = dependencies(&main, TAG_PROVIDENAME, TAG_PROVIDEFLAGS, TAG_PROVIDEVERSION);
    let requires = dependencies(&main, TAG_REQUIRENAME, TAG_REQUIREFLAGS, TAG_REQUIREVERSION);
    let files = files(&main);

    Ok(RpmPackage {
        name,
        version,
        release,
        epoch,
        arch,
        summary,
        description,
        packager: main.string(TAG_PACKAGER).unwrap_or_default(),
        url: main.string(TAG_URL).unwrap_or_default(),
        license: main.string(TAG_LICENSE).unwrap_or_default(),
        vendor: main.string(TAG_VENDOR).unwrap_or_default(),
        group: main.i18n(TAG_GROUP).unwrap_or_default(),
        buildhost: main.string(TAG_BUILDHOST).unwrap_or_default(),
        sourcerpm: main.string(TAG_SOURCERPM).unwrap_or_default(),
        buildtime: main.int32(TAG_BUILDTIME).unwrap_or(0),
        installed_size: u64::from(main.int32(TAG_SIZE).unwrap_or(0)),
        archive_size: main
            .int32(TAG_ARCHIVESIZE)
            .map_or_else(|| (bytes.len() - main.end) as u64, u64::from),
        header_start: main_start as u64,
        header_end: main.end as u64,
        checksum: String::new(),
        location: String::new(),
        size: 0,
        mtime: 0,
        provides,
        requires,
        files,
    })
}

fn required(header: &Header<'_>, tag: u32, label: &str, path: &Path) -> Result<String, Diagnostic> {
    header
        .string_or_i18n(tag)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| repo_error(format!("{} 的主头缺 tag {tag}（{label}）", path.display())))
}

/// 依赖三元组：名字数组 + 标志位数组 + 版本数组按下标配对。
fn dependencies(header: &Header<'_>, names: u32, flags: u32, versions: u32) -> Vec<Dep> {
    let names = header.string_array(names);
    let flags = header.int32_array(flags);
    let versions = header.string_array(versions);
    let mut deps = Vec::new();
    for (index, name) in names.iter().enumerate() {
        // rpmlib(...) 是 rpm 自身的解析能力，createrepo 也不落 repodata。
        if name.is_empty() || name.starts_with("rpmlib(") {
            continue;
        }
        let bits = flags.get(index).copied().unwrap_or(0);
        let version = versions.get(index).map_or("", String::as_str);
        let (epoch, ver, rel) = split_evr(version);
        deps.push(Dep {
            name: name.clone(),
            flags: flags_text(bits),
            epoch,
            ver,
            rel,
        });
    }
    deps.sort_by(|a, b| {
        (&a.name, a.flags, &a.epoch, &a.ver, &a.rel)
            .cmp(&(&b.name, b.flags, &b.epoch, &b.ver, &b.rel))
    });
    deps.dedup_by(|a, b| {
        a.name == b.name
            && a.flags == b.flags
            && a.epoch == b.epoch
            && a.ver == b.ver
            && a.rel == b.rel
    });
    deps
}

/// 依赖标志位（2=LT / 4=GT / 8=EQ；组合 10=LE、12=GE）。
pub(super) fn flags_text(bits: u32) -> &'static str {
    match bits & 0x0e {
        0x02 => "LT",
        0x04 => "GT",
        0x08 => "EQ",
        0x0a => "LE",
        0x0c => "GE",
        _ => "",
    }
}

/// `EPOCH:VERSION-RELEASE` → 三段（缺省空串；epoch 非数字按无 epoch 处理）。
pub(super) fn split_evr(version: &str) -> (String, String, String) {
    let (epoch, rest) = match version.split_once(':') {
        Some((epoch, rest)) if !epoch.is_empty() && epoch.chars().all(|c| c.is_ascii_digit()) => {
            (epoch.to_string(), rest)
        }
        _ => ("0".to_string(), version),
    };
    match rest.rsplit_once('-') {
        Some((ver, rel)) => (epoch, ver.to_string(), rel.to_string()),
        None => (epoch, rest.to_string(), String::new()),
    }
}

/// 文件表：`DIRNAMES[DIRINDEXES[i]] + BASENAMES[i]`，目录位来自 FILEMODES。
fn files(header: &Header<'_>) -> Vec<RpmFile> {
    let dirnames = header.string_array(TAG_DIRNAMES);
    let basenames = header.string_array(TAG_BASENAMES);
    let indexes = header.int32_array(TAG_DIRINDEXES);
    let modes = header.int32_array(TAG_FILEMODES);
    let mut files = Vec::new();
    for (index, basename) in basenames.iter().enumerate() {
        let Some(dir) = indexes
            .get(index)
            .and_then(|dir| dirnames.get(*dir as usize))
        else {
            continue;
        };
        let path = format!("{dir}{basename}");
        if !path.starts_with('/') {
            continue;
        }
        let is_dir = modes
            .get(index)
            .is_some_and(|mode| mode & 0o170000 == 0o040000);
        files.push(RpmFile { path, is_dir });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    files
}
