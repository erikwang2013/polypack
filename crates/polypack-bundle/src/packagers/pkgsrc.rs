//! NetBSD / DragonFly pkgsrc 打包器（T-M5-08）：`Makefile` + `PLIST` 骨架 + 结构自检。
//!
//! 产物 = `<out_dir>/pkgsrc/<DISTNAME>/`（可直接放进 pkgsrc 树 `misc/<DISTNAME>`）：
//! - `Makefile`（`DISTNAME` / `CATEGORIES` / `LICENSE` 等 pkgsrc 必填项 + `do-extract` /
//!   `do-install`；载荷在 `files/payload/`，无 distfile / 网络 / 校验和）；
//! - `PLIST`（由载荷树**同源**派生：每个载荷文件恰好一条，路径相对 `${PREFIX}`）；
//! - `DESCR`；
//! - `files/payload/`（安装树，`do-install` 逐文件装进 `${DESTDIR}${PREFIX}`）。
//!
//! 真构建腿（`bmake package` → `.tgz` → `pkg_add`）只在 NetBSD 用户态可跑：骨架必须位于
//! pkgsrc 树内且需要 pkgsrc `mk/` 基础设施；Linux 交叉发起只生成骨架并做内建结构自检，
//! `bmake package` / `pkg_add` 逐项跳过并如实注记（NetBSD VM 真跑归 T-M5-09）。
//!
//! shortcut: pkgsrc 的 `PKGPATH` 由包目录在树内的位置决定，本生成器不复制系统 pkgsrc 树；
//! VM 侧按产物注记把骨架放进 `${PKGSRCDIR}/misc/<DISTNAME>` 后执行 bmake。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Packager};

/// 目标 id（NetBSD 是验收 VM；DragonFly 等 pkgsrc 平台同源）。
const ID: &str = "netbsd";

/// 产物格式（须与 `[targets.netbsd].package.format` 一致）。
const FORMAT: &str = "pkgsrc";

/// pkgsrc 包前缀缺省值（`LOCALBASE`）。
const PREFIX: &str = "/usr/pkg";

/// pkgsrc 打包器。
pub struct Pkgsrc;

mod structure;
pub use structure::{PkgsrcStructure, check_pkgsrc_structure};

impl Packager for Pkgsrc {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec)?;
        let bare = inputs.require_bare(ID)?;
        let port = port_dir(inputs);
        let payload = payload_entries(&payload_source(bare.root.as_path()))?;
        if payload.is_empty() {
            return Err(BundleError::InvalidInput {
                packager: ID,
                field: "bare",
                value: bare.root.display().to_string(),
                reason: "裸产物载荷为空（pkgsrc 包没有可安装文件）".to_string(),
            });
        }
        if let Some(path) = payload.keys().find(|path| !make_path_safe(path)) {
            return Err(BundleError::InvalidInput {
                packager: ID,
                field: "payload",
                value: path.clone(),
                reason:
                    "载荷路径含控制字符 / `$` / `#` / 反斜杠 / `\"`：pkgsrc Makefile 无法安全引用"
                        .to_string(),
            });
        }

        let plan = BundlePlan::new()
            .with_script(port.join("Makefile"), makefile(inputs, &payload))
            .with_script(port.join("PLIST"), plist(&payload))
            .with_script(port.join("DESCR"), descr(inputs));

        // 骨架在 pkgsrc 树外：bmake 需要 `${PKGSRCDIR}/mk/bsd.pkg.mk` 与树内 PKGPATH，
        // 本机拼不出来也不伪造构建；VM 侧按产物注记入树后真跑（T-M5-09）。
        Ok(plan.skipping(
            "bmake package",
            format!(
                "pkgsrc 骨架在 {}（树外），bmake 需 ${{PKGSRCDIR}}/mk 基础设施与树内 PKGPATH：\
                 bmake package / pkg_add 真跑归 NetBSD VM（T-M5-09）",
                port.display()
            ),
        ))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(ID)?;
        let port = port_dir(inputs);
        let _ = std::fs::remove_dir_all(&port);
        fsutil::copy_tree(
            &payload_source(bare.root.as_path()),
            &port.join("files/payload"),
        )?;
        fsutil::write_scripts(&plan)?;

        exec::run_all(&plan.commands)?;

        let mut notes: Vec<String> = plan
            .skipped
            .iter()
            .map(|s| format!("未执行：{}（{}）", s.tool, s.reason))
            .collect();
        // 产出自检：骨架必过自己的检查器（与 verify 腿同一实现）。
        let structure = check_pkgsrc_structure(&port)?;
        notes.push(format!(
            "pkgsrc 骨架：{} 个 PLIST 条目，DISTNAME={}，PREFIX={PREFIX}",
            structure.entries.len(),
            structure.distname
        ));
        notes.push(format!(
            "NetBSD VM 真跑：cp -R {0} ${{PKGSRCDIR}}/misc/ && cd ${{PKGSRCDIR}}/misc/{1} && \
             bmake package；pkg_add -v ${{PACKAGES}}/All/{1}.tgz（T-M5-09）",
            port.display(),
            structure.distname
        ));
        fsutil::dir_artifact(ID, FORMAT, &port, notes)
    }
}

/// 载荷源：Tauri 裸产物的 `usr/` 树（无 `usr/` 时整根）；其子项即安装树（`bin/`、`lib/…`）。
fn payload_source(bare_root: &Path) -> PathBuf {
    let usr = bare_root.join("usr");
    if usr.is_dir() {
        usr
    } else {
        bare_root.to_path_buf()
    }
}

/// 骨架目录：`<out_dir>/pkgsrc/<DISTNAME>/`。
fn port_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("pkgsrc").join(distname(inputs))
}

/// `DISTNAME` = `<kebab>-<version>`（pkgsrc 包名惯例：小写 + 版本）。
fn distname(inputs: &BundleInputs) -> String {
    format!("{}-{}", inputs.product.kebab_name(), inputs.product.version)
}

/// 载荷文件表：安装树相对路径 → 是否可执行（排序稳定，PLIST 与安装命令行同源）。
fn payload_entries(root: &Path) -> Result<BTreeMap<String, bool>, BundleError> {
    let mut entries = BTreeMap::new();
    if root.is_dir() {
        walk(root, root, &mut entries)?;
    }
    Ok(entries)
}

fn walk(root: &Path, dir: &Path, entries: &mut BTreeMap<String, bool>) -> Result<(), BundleError> {
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
                reason: "载荷路径不在安装树内".to_string(),
            })?
            .to_string_lossy()
            .replace('\\', "/");
        entries.insert(rel, is_executable(&path));
    }
    Ok(())
}

/// 可执行判定：`bin/` 前缀（跨宿主同口径）或 Unix 执行位。
fn is_executable(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component.as_os_str() == "bin")
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    false
}

/// Makefile 可安全引用的载荷路径：允许空格（安装命令行加引号），拒绝控制字符与
/// `$` / `#` / 反斜杠 / `"`（make 展开、注释与 shell 引号无法可靠转义）。
fn make_path_safe(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('@')
        && !path
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '$' | '#' | '\\' | '"'))
}

/// `INSTALLATION_DIRS` 走 pkgsrc 的 shell 循环（无引号），目录名须无空白。
fn installation_dir_safe(dir: &str) -> bool {
    !dir.chars().any(char::is_whitespace)
}

/// `Makefile`：自包含预置载荷（`files/payload/`，无 distfile / 网络 / 校验和）。
fn makefile(inputs: &BundleInputs, payload: &BTreeMap<String, bool>) -> String {
    let product = &inputs.product;
    let mut out = String::new();
    out.push_str("# $NetBSD$\n#\n");
    out.push_str(
        "# Generated by polypack (T-M5-08). 预置载荷（files/payload），无 distfile / 网络。\n",
    );
    out.push_str(&format!(
        "# 用法：把本目录放进 pkgsrc 树（${{PKGSRCDIR}}/misc/{}）后执行 bmake package。\n\n",
        distname(inputs)
    ));
    out.push_str(&format!("DISTNAME=\t{}\n", distname(inputs)));
    out.push_str("PKGNAME=\t${DISTNAME}\n");
    out.push_str("CATEGORIES=\tmisc\n");
    out.push_str("MASTER_SITES=\t# none\n");
    out.push_str("DISTFILES=\t# none\n\n");
    out.push_str(&format!(
        "MAINTAINER=\t{} <ports@{}>\n",
        simple_value(&product.manufacturer),
        domain(&product.identifier)
    ));
    out.push_str(&format!(
        "HOMEPAGE=\t{}\n",
        simple_value(&homepage(product))
    ));
    out.push_str(&format!("COMMENT=\t{}\n", comment(product)));
    out.push_str("LICENSE=\tapache-2.0\n\n");
    out.push_str("NO_CHECKSUM=\tyes\nNO_CONFIGURE=\tyes\nNO_BUILD=\tyes\nNO_TEST=\tyes\n");
    out.push_str("USE_LANGUAGES=\t# none\n\n");

    let dirs = install_dirs(payload);
    for dir in &dirs {
        if installation_dir_safe(dir) {
            out.push_str(&format!("INSTALLATION_DIRS+=\t{dir}\n"));
        }
    }
    out.push_str("\ndo-extract:\n");
    out.push_str("\t${MKDIR} ${WRKSRC}\n");
    out.push_str("\t${CP} -R ${FILESDIR}/payload/. ${WRKSRC}/\n");
    out.push_str("\ndo-install:\n");
    // 含空白的目录无法进 INSTALLATION_DIRS（pkgsrc 的 shell 循环不加引号），显式建目录。
    for dir in &dirs {
        if !installation_dir_safe(dir) {
            out.push_str(&format!("\t${{MKDIR}} \"${{DESTDIR}}${{PREFIX}}/{dir}\"\n"));
        }
    }
    for (path, exec) in payload {
        let tool = if *exec {
            "${INSTALL_PROGRAM}"
        } else {
            "${INSTALL_DATA}"
        };
        out.push_str(&format!(
            "\t{tool} \"${{WRKSRC}}/{path}\" \"${{DESTDIR}}${{PREFIX}}/{path}\"\n"
        ));
    }
    // pkgsrc 基础设施入口：包目录须位于 <PKGSRCDIR>/misc/<DISTNAME>，相对路径才成立。
    out.push_str("\n.include \"../../mk/bsd.pkg.mk\"\n");
    out
}

/// 安装目录集合：载荷文件的所有父目录（`INSTALLATION_DIRS` 由 pkgsrc 建到 `DESTDIR`）。
fn install_dirs(payload: &BTreeMap<String, bool>) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for path in payload.keys() {
        let mut parts: Vec<&str> = path.split('/').collect();
        parts.pop();
        let mut dir = String::new();
        for part in parts {
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(part);
            dirs.insert(dir.clone());
        }
    }
    dirs
}

/// `PLIST`：载荷文件逐条（相对 `${PREFIX}`；目录由 `INSTALLATION_DIRS` 建）。
fn plist(payload: &BTreeMap<String, bool>) -> String {
    let mut out = String::new();
    for path in payload.keys() {
        out.push_str(path);
        out.push('\n');
    }
    out
}

/// `DESCR`：描述 + 主页。
fn descr(inputs: &BundleInputs) -> String {
    format!(
        "{}\n\nHomepage: {}\n",
        inputs.product.description,
        homepage(&inputs.product)
    )
}

/// `COMMENT`：单行、≤ 70 字符（pkgsrc 摘要行；超出截断而不是让 bmake 失败）。
fn comment(product: &ProductInfo) -> String {
    let line = product.description.replace(['\n', '\r', '\t'], " ");
    let mut chars = line.trim().chars();
    let mut out: String = chars.by_ref().take(70).collect();
    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
    simple_value(&out)
}

/// Makefile 值转义：控制字符归一为空格，`$` / `#` 不触发 make 展开 / 注释。
fn simple_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '$' => out.push_str("$$"),
            '#' => out.push_str("\\#"),
            ch if ch.is_control() || ch == '\t' => out.push(' '),
            ch => out.push(ch),
        }
    }
    out
}

/// 主页：产品声明优先；缺省按标识符（`com.polypack.paxnotes` → `polypack.com`）派生。
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

/// 打包格式与打包器声明必须一致（防「按 pkgsrc 目标产出别的格式」）。
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

#[cfg(test)]
mod tests;
