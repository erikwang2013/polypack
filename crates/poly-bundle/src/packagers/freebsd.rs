//! FreeBSD pkg 打包器（T-M5-06）：`pkg create` 原生链 + `pkg check` 结构面。
//!
//! 产物 = `<out_dir>/<name>-<version>.pkg`（FreeBSD 用户态 `pkg create` 生成），暂存树
//! `<out_dir>/<name>-<version>.pkgstage/usr/local/…` 与 `+MANIFEST` 同源生成：裸产物的
//! `usr/{bin,lib,share}` 重映射到 `/usr/local`（FreeBSD ports 前缀），manifest 里的
//! `prefix` / `name` / `version` / `origin` 由同一份产品元数据派生。
//!
//! Linux 宿主通常没有 `pkg(8)`：`pkg create` 逐项跳过并如实注记，暂存树 + 内建结构断言
//! 仍真跑；真 `pkg create` / 安装冒烟 / `pkg check` 需 FreeBSD 用户态（T-M5-09 VM）。
//!
//! shortcut: manifest 是 UCL 的最小子集（单行 `key: "value"`）。`pkg create -M` **不会**
//! 扫描 `-r` 根目录：没有 `files {}` 段或 `-p` plist 时产出空包（真跑回归）。因此打包器
//! 从同一份载荷表生成 `<name>-<version>.plist`（相对 prefix 的路径），随 manifest 一起
//! 落盘并以 `-p` 传给 `pkg create`；校验和由 pkg(8) 在读盘时计算。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// 目标 id。
const ID: &str = "freebsd";

/// 产物格式（须与 `[targets.freebsd].package.format` 一致）。
const FORMAT: &str = "pkg";

/// FreeBSD 包前缀（pkg 约定；ports 树安装到 `/usr/local`）。
const PREFIX: &str = "/usr/local";

/// FreeBSD pkg 打包器。
pub struct FreeBsdPkg;

/// pkg 暂存树结构检查结果（打包器自检与 verify 腿 `pkg-check` 的本地结构面共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgStructure {
    /// manifest `name`（kebab 包名）。
    pub name: String,
    /// manifest `version`。
    pub version: String,
    /// manifest `origin`（`category/port-dir`）。
    pub origin: String,
    /// manifest `prefix`（安装前缀，通常 `/usr/local`）。
    pub prefix: String,
    /// 暂存树内文件（相对前缀，`/` 分隔，按字典序）。
    pub files: Vec<String>,
}

impl Packager for FreeBsdPkg {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec)?;
        let bare = inputs.require_bare(ID)?;
        let manifest = manifest_path(inputs);
        let stage = stage_dir(inputs);
        let files = payload_entries(&payload_source(bare.root.as_path()))?;
        if files.is_empty() {
            return Err(BundleError::InvalidInput {
                packager: ID,
                field: "bare",
                value: bare.root.display().to_string(),
                reason: "裸产物载荷为空（pkg 没有可安装文件）".to_string(),
            });
        }

        // pkg create -M 只打包 plist 里列出的文件（不扫 -r 根目录）；plist 与 manifest
        // 同源落盘，Linux 宿主无 pkg 时也可检查文件清单。
        let plist = plist_path(inputs);
        let mut plan = BundlePlan::new()
            .with_script(manifest.clone(), manifest_text(inputs)?)
            .with_script(plist.clone(), plist_text(&files));
        if exec::tool_present("pkg") {
            plan = plan
                .with_command(Command::new(
                    "pkg",
                    [
                        "create".to_string(),
                        "-M".to_string(),
                        path_arg(&manifest),
                        "-r".to_string(),
                        path_arg(&stage),
                        "-p".to_string(),
                        path_arg(&plist),
                        "-o".to_string(),
                        path_arg(&inputs.out_dir),
                    ],
                ))
                .with_command(Command::new(
                    "pkg",
                    [
                        "info".to_string(),
                        "-F".to_string(),
                        path_arg(&pkg_path(inputs)),
                    ],
                ));
        } else {
            plan = plan.skipping(
                "pkg create",
                "PATH 无 pkg（FreeBSD pkg(8)）：暂存树与 +MANIFEST 照常生成，pkg create / \
                 pkg check 真跑需 FreeBSD 用户态（T-M5-09 VM）",
            );
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(ID)?;
        let stage = stage_dir(inputs);
        let _ = std::fs::remove_dir_all(&stage);
        fsutil::copy_tree(
            &payload_source(bare.root.as_path()),
            &stage.join(PREFIX.trim_start_matches('/')),
        )?;
        fsutil::write_scripts(&plan)?;

        exec::run_all(&plan.commands)?;

        let mut notes: Vec<String> = plan
            .skipped
            .iter()
            .map(|s| format!("未执行：{}（{}）", s.tool, s.reason))
            .collect();
        // 产出自检：暂存树必过自己的检查器（与 verify 腿同一实现）。
        let structure = check_pkg_structure(&stage, &manifest_path(inputs))?;
        notes.push(format!(
            "pkg 暂存树：{} 个文件，name={}-{}，prefix={}",
            structure.files.len(),
            structure.name,
            structure.version,
            structure.prefix
        ));

        let pkg = pkg_path(inputs);
        if pkg.is_file() {
            notes.push(format!(
                "pkg create 产出：{}",
                pkg.file_name().unwrap_or_default().to_string_lossy()
            ));
            notes.push(
                "安装冒烟：pkg install -y <pkg>（需 root）；校验：pkg check -s <name>（FreeBSD VM）"
                    .to_string(),
            );
            return fsutil::file_artifact(ID, FORMAT, &pkg, notes);
        }
        notes.push(
            "本机未生成 .pkg：pkg create 真跑与 pkg install / pkg check 归 FreeBSD VM（T-M5-09）"
                .to_string(),
        );
        fsutil::dir_artifact(ID, "pkgstage", &stage, notes)
    }
}

/// 检查 pkg 暂存树与 +MANIFEST：必填字段、prefix 安全、载荷全部落在 prefix 下且含可执行。
///
/// 打包器 `package()` 与 verify 腿共用本函数（产物缺陷硬失败，绝不降级成跳过）。
pub fn check_pkg_structure(stage: &Path, manifest: &Path) -> Result<PkgStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: manifest.to_path_buf(),
        reason,
    };
    let text =
        std::fs::read_to_string(manifest).map_err(|err| bad(format!("缺 +MANIFEST：{err}")))?;
    let mut fields = std::collections::BTreeMap::new();
    for name in [
        "name",
        "version",
        "origin",
        "comment",
        "desc",
        "www",
        "maintainer",
        "prefix",
    ] {
        let value = manifest_value(&text, name)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| bad(format!("manifest 缺必填项 {name}")))?;
        fields.insert(name, value);
    }
    let prefix = &fields["prefix"];
    if !prefix.starts_with('/')
        || prefix.split('/').any(|part| part == "..")
        || prefix.contains(['\t', '\r', '\n'])
    {
        return Err(bad(format!("prefix 不是安全的绝对路径：{prefix:?}")));
    }
    if fields["origin"].split_once('/').is_none() {
        return Err(bad(format!(
            "origin 须为 category/port-dir：{:?}",
            fields["origin"]
        )));
    }
    let name = &fields["name"];
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(bad(format!("name 须为小写 kebab 包名：{name:?}")));
    }
    if fields["version"].contains(char::is_whitespace) {
        return Err(bad(format!("version 含空白：{:?}", fields["version"])));
    }

    let root = stage.join(prefix.trim_start_matches('/'));
    if !root.is_dir() {
        return Err(bad(format!("暂存树缺安装前缀目录：{}", root.display())));
    }
    let files = payload_entries(&root).map_err(|err| bad(format!("载荷树不可读：{err}")))?;
    if files.is_empty() {
        return Err(bad("暂存树在前缀下没有任何文件".to_string()));
    }
    let bin = format!("bin/{}", name);
    if !files.contains(&bin) {
        return Err(bad(format!(
            "暂存树缺可执行 {prefix}/{bin}（manifest name 与实际载荷不一致）"
        )));
    }

    Ok(PkgStructure {
        name: name.clone(),
        version: fields["version"].clone(),
        origin: fields["origin"].clone(),
        prefix: prefix.clone(),
        files: files.into_iter().collect(),
    })
}

/// 解析单行 UCL `key: "value"`（去引号与最小转义；生成器只产出这一形态）。
fn manifest_value(text: &str, key: &str) -> Option<String> {
    let line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix(&format!("{key}:")))?;
    let value = line.trim();
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// `+MANIFEST` 文本（UCL 最小子集）。
fn manifest_text(inputs: &BundleInputs) -> Result<String, BundleError> {
    let product = &inputs.product;
    let homepage = homepage(product);
    let flat = inputs
        .bare
        .as_ref()
        .map(|bare| fsutil::tree_bytes(&payload_source(&bare.root)))
        .transpose()?
        .unwrap_or(0);
    let mut out = String::new();
    out.push_str(&format!("name: {}\n", quote(&product.kebab_name())));
    out.push_str(&format!("version: {}\n", quote(&product.version)));
    out.push_str(&format!(
        "origin: {}\n",
        quote(&format!("misc/{}", product.kebab_name()))
    ));
    out.push_str(&format!("comment: {}\n", quote(&comment(product))));
    out.push_str(&format!(
        "desc: {}\n",
        quote(&format!("{}\n\nHomepage: {homepage}", product.description))
    ));
    out.push_str(&format!("www: {}\n", quote(&homepage)));
    out.push_str(&format!(
        "maintainer: {}\n",
        quote(&format!(
            "{} <ports@{}>",
            product.manufacturer,
            domain(&product.identifier)
        ))
    ));
    out.push_str(&format!("prefix: {}\n", quote(PREFIX)));
    out.push_str("categories: [\"misc\"]\n");
    // shortcut: 许可证尚未进入 BundleInputs（M1 无 license 字段），暂按仓库 Apache-2.0。
    out.push_str("licenses: [\"Apache-2.0\"]\n");
    out.push_str(&format!("flatsize: {flat}\n"));
    Ok(out)
}

/// UCL 双引号字符串（生成器只产出单行值）。
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// 载荷源：Tauri 裸产物的 `usr/` 树（无 `usr/` 时整根）。
fn payload_source(bare_root: &Path) -> PathBuf {
    let usr = bare_root.join("usr");
    if usr.is_dir() {
        usr
    } else {
        bare_root.to_path_buf()
    }
}

/// 暂存树：`<out_dir>/<name>-<version>.pkgstage/`。
fn stage_dir(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.pkgstage", distname(inputs)))
}

/// manifest：`<out_dir>/<name>-<version>.manifest`（与暂存树同名，verify 腿同级查找）。
fn manifest_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.manifest", distname(inputs)))
}

/// 载荷清单（legacy plist）：`<out_dir>/<name>-<version>.plist`（`pkg create -p` 输入）。
fn plist_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.plist", distname(inputs)))
}

/// 目标 .pkg：`<out_dir>/<name>-<version>.pkg`。
fn pkg_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!("{}.pkg", distname(inputs)))
}

/// `<kebab>-<version>`（pkg 文件名 / 暂存树前缀）。
fn distname(inputs: &BundleInputs) -> String {
    format!("{}-{}", inputs.product.kebab_name(), inputs.product.version)
}

/// legacy plist 文本：每行一个相对 prefix 的载荷路径（BTreeSet 已排序，字节稳定）。
///
/// `pkg create -p` 依赖此清单选择文件；空清单已在 [`FreeBsdPkg::plan`] 拒绝。
fn plist_text(files: &std::collections::BTreeSet<String>) -> String {
    let mut out = String::new();
    for file in files {
        out.push_str(file);
        out.push('\n');
    }
    out
}

/// 载荷文件表（相对安装前缀；排序稳定，结构检查与 flatsize 同源）。
fn payload_entries(root: &Path) -> Result<std::collections::BTreeSet<String>, BundleError> {
    let mut entries = std::collections::BTreeSet::new();
    if root.is_dir() {
        walk(root, root, &mut entries)?;
    }
    Ok(entries)
}

fn walk(
    root: &Path,
    dir: &Path,
    entries: &mut std::collections::BTreeSet<String>,
) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(crate::error::io_err(dir))? {
        let entry = entry.map_err(crate::error::io_err(dir))?;
        let path = entry.path();
        let ty = entry.file_type().map_err(crate::error::io_err(&path))?;
        if ty.is_dir() {
            walk(root, &path, entries)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| BundleError::InvalidInput {
                packager: ID,
                field: "payload",
                value: path.display().to_string(),
                reason: "载荷路径不在安装前缀内".to_string(),
            })?
            .to_string_lossy()
            .replace('\\', "/");
        entries.insert(rel);
    }
    Ok(())
}

/// `comment`：单行、≤ 70 字符（pkg 摘要行；超出截断而不是让 pkg create 失败）。
fn comment(product: &ProductInfo) -> String {
    let line = product.description.replace(['\n', '\r', '\t'], " ");
    let mut chars = line.trim().chars();
    let mut out: String = chars.by_ref().take(70).collect();
    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
    out
}

/// 主页：产品声明优先；缺省按标识符派生。
fn homepage(product: &ProductInfo) -> String {
    product
        .homepage
        .clone()
        .unwrap_or_else(|| format!("https://{}", domain(&product.identifier)))
}

fn domain(identifier: &str) -> String {
    let mut parts = identifier.split('.').take(2);
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => format!("{b}.{a}"),
        _ => "example.com".to_string(),
    }
}

/// 打包格式与打包器声明必须一致（防「按 pkg 目标产出别的格式」）。
fn check_format(spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format == FORMAT {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{FORMAT}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests;
