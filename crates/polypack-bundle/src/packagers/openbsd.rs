//! OpenBSD ports 打包器（T-M5-07）：ports 目录骨架生成 + signify 包签名 + portcheck 校验。
//!
//! 产物 = `<out_dir>/ports/<DISTNAME>/`（提交ports@的骨架）：
//! - `Makefile`（`DISTNAME` / `COMMENT` / `MAINTAINER` / `do-install` 等 portcheck 必读项）；
//! - `pkg/DESCR` 与 `pkg/PLIST`（PLIST 由载荷树**同源**派生：每个载荷文件恰好一条）；
//! - `files/payload/`（安装树，`do-install` 整树拷进 `${PREFIX}`，免 distfile / 网络）。
//!
//! 真构建腿（`make package` → `pkg_sign` → `signify -Vz`）只在 OpenBSD hosts 上可跑：
//! 需要 ports 基础设施（`/usr/ports/infrastructure/mk/bsd.port.mk`）与 `pkg_sign`；Linux
//! 宿主缺这些工具时逐项跳过并如实注记（骨架生成与结构自检照常真跑），OpenBSD VM 真跑
//! 归 T-M5-09。signify 私钥 / 公钥路径经环境变量注入（`env:` 形态，密钥值零入库）：
//! `POLYPACK_OPENBSD_SIGNIFY_KEY` / `POLYPACK_OPENBSD_SIGNIFY_PUBKEY`。
//!
//! shortcut: 签名钥走环境变量**路径**而非 `CredentialRef`（M5 无签名档案后端面）；
//! 随 T-M5-05 的仓库签名抽象落定后统一升级。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// 目标 id。
const ID: &str = "openbsd";

/// 产物格式（须与 `[targets.openbsd].package.format` 一致）。
const FORMAT: &str = "ports";

/// ports 基础设施标记（OpenBSD 宿主存在 = `make package` 真腿可跑）。
const PORTS_BSD_MK: &str = "/usr/ports/infrastructure/mk/bsd.port.mk";

/// signify 私钥路径（`pkg_sign -s` 的密钥文件；空 / 未设 = 不签名）。
pub const SIGNIFY_KEY_ENV: &str = "POLYPACK_OPENBSD_SIGNIFY_KEY";

/// signify 公钥路径（`signify -Vz -p`；空 / 未设 = 签名后不做校验）。
pub const SIGNIFY_PUBKEY_ENV: &str = "POLYPACK_OPENBSD_SIGNIFY_PUBKEY";

/// OpenBSD ports 打包器。
pub struct OpenbsdPorts;

/// ports 骨架结构检查结果（打包器自检与 verify 腿 `portcheck` 的本地结构面共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortsStructure {
    /// `DISTNAME`（`<kebab>-<version>`）。
    pub distname: String,
    /// `MAINTAINER` 行。
    pub maintainer: String,
    /// PLIST 文件条目（不含 `@` 注解）。
    pub entries: Vec<String>,
}

impl Packager for OpenbsdPorts {
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

        let mut plan = BundlePlan::new()
            .with_script(port.join("Makefile"), makefile(inputs, &payload))
            .with_script(port.join("pkg/DESCR"), descr(inputs))
            .with_script(port.join("pkg/PLIST"), plist(&payload));

        if Path::new(PORTS_BSD_MK).is_file() && exec::tool_present("make") {
            // 包输出强制落到 port 目录内（`PACKAGES` 命令行变量覆盖 bsd.port.mk 的缺省）。
            plan = plan.with_command(
                Command::new(
                    "make",
                    [
                        "package".to_string(),
                        format!("PACKAGES={}", packages_dir(inputs).display()),
                    ],
                )
                .in_dir(&port),
            );
            match sign_command(inputs) {
                Ok(command) => plan = plan.with_command(command),
                Err(reason) => plan = plan.skipping("pkg_sign", reason),
            }
        } else {
            plan = plan.skipping(
                "make package",
                format!(
                    "缺 {PORTS_BSD_MK} 或 make（ports 基础设施）：骨架照常生成，`make package` \
                     真跑需 OpenBSD VM（T-M5-09）"
                ),
            );
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(ID)?;
        let port = port_dir(inputs);
        let _ = std::fs::remove_dir_all(&port);
        let payload = port.join("files/payload");
        fsutil::copy_tree(&payload_source(bare.root.as_path()), &payload)?;
        fsutil::write_scripts(&plan)?;

        exec::run_all(&plan.commands)?;

        let mut notes: Vec<String> = plan
            .skipped
            .iter()
            .map(|s| format!("未执行：{}（{}）", s.tool, s.reason))
            .collect();
        notes.extend(verify_signatures(inputs)?);
        notes.push(
            "portcheck 真跑归 verify（OpenBSD VM）；本机结构自检已过（Makefile / PLIST / DESCR）"
                .to_string(),
        );

        // 产出自检：骨架必过自己的检查器（与 verify 腿同一实现）。
        let structure = check_ports_structure(&port)?;
        notes.push(format!(
            "ports 骨架：{} 个 PLIST 条目，DISTNAME={}",
            structure.entries.len(),
            structure.distname
        ));
        fsutil::dir_artifact(ID, FORMAT, &port, notes)
    }
}

/// 签名命令：`pkg_sign -s signify2 -s <私钥> -o <signed> -S all`（cwd = 未签包目录）。
fn sign_command(inputs: &BundleInputs) -> Result<Command, String> {
    let key = env_path(SIGNIFY_KEY_ENV)
        .ok_or_else(|| format!("未设 {SIGNIFY_KEY_ENV}（signify 私钥路径）——包已构建但未签名"))?;
    if !exec::tool_present("pkg_sign") {
        return Err("PATH 无 pkg_sign（随 OpenBSD base 提供）——包已构建但未签名".to_string());
    }
    Ok(Command::new(
        "pkg_sign",
        [
            "-s".to_string(),
            "signify2".to_string(),
            "-s".to_string(),
            key.display().to_string(),
            "-o".to_string(),
            signed_dir(inputs).display().to_string(),
            "-S".to_string(),
            "all".to_string(),
        ],
    )
    .in_dir(packages_dir(inputs)))
}

/// 签名后校验（`signify -Vz -p <公钥> -m <包>`）；返回如实注记。
///
/// 失败（公钥在场但签名验不过）是硬错误——签了却验不过不能报绿。
fn verify_signatures(inputs: &BundleInputs) -> Result<Vec<String>, BundleError> {
    let dir = signed_dir(inputs);
    let mut packages: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tgz"))
        .collect();
    packages.sort();
    if packages.is_empty() {
        return Ok(Vec::new());
    }
    let Some(pubkey) = env_path(SIGNIFY_PUBKEY_ENV) else {
        return Ok(vec![format!(
            "签名产物未校验：设 {SIGNIFY_PUBKEY_ENV}（signify 公钥路径）后重跑"
        )]);
    };
    if !exec::tool_present("signify") {
        return Ok(vec![
            "签名产物未校验：PATH 无 signify（OpenBSD base / signify-openbsd）".to_string(),
        ]);
    }
    for package in &packages {
        exec::run(&Command::new(
            "signify",
            [
                "-Vz".to_string(),
                "-p".to_string(),
                pubkey.display().to_string(),
                "-m".to_string(),
                package.display().to_string(),
            ],
        ))?;
    }
    Ok(vec![format!(
        "signify 签名校验通过：{} 个包（{}）",
        packages.len(),
        packages
            .iter()
            .map(|p| p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    )])
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

/// 载荷文件表：安装树相对路径 → 是否可执行（排序稳定，PLIST 与拷贝同源）。
fn payload_entries(root: &Path) -> Result<BTreeMap<String, bool>, BundleError> {
    let mut entries = BTreeMap::new();
    walk(root, root, &mut entries)?;
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

/// 端口目录：`<out_dir>/ports/<DISTNAME>/`。
fn port_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("ports").join(distname(inputs))
}

/// `DISTNAME` = `<kebab>-<version>`（OpenBSD 端口名惯例：小写、连字符）。
fn distname(inputs: &BundleInputs) -> String {
    format!("{}-{}", inputs.product.kebab_name(), inputs.product.version)
}

fn packages_dir(inputs: &BundleInputs) -> PathBuf {
    port_dir(inputs).join("packages")
}

fn signed_dir(inputs: &BundleInputs) -> PathBuf {
    port_dir(inputs).join("signed")
}

/// `Makefile`：无 distfile 的自包含端口（载荷在 `files/payload/`，`do-install` 整树安装）。
fn makefile(inputs: &BundleInputs, payload: &BTreeMap<String, bool>) -> String {
    let product = &inputs.product;
    let homepage = homepage(product);
    let bin = format!("bin/{}", product.binary_name());
    let mut out = String::new();
    out.push_str(&format!("COMMENT =\t{}\n", comment(product)));
    out.push_str(&format!("DISTNAME =\t{}\n", distname(inputs)));
    out.push_str("CATEGORIES =\tx11\n");
    out.push_str(&format!("HOMEPAGE =\t{homepage}\n"));
    out.push_str(&format!(
        "MAINTAINER =\t{} <ports@{}>\n",
        product.manufacturer,
        domain(&product.identifier)
    ));
    out.push_str("\n# Apache-2.0\nPERMIT_PACKAGE =\tYes\n");
    out.push_str("\nNO_BUILD =\tYes\nNO_TEST =\tYes\n");
    out.push_str("\ndo-install:\n\tcp -R ${FILESDIR}/payload/. ${PREFIX}/\n");
    if payload.contains_key(&bin) {
        out.push_str(&format!("\tchmod 755 ${{PREFIX}}/{bin}\n"));
    }
    // ports(7)：Makefile 必须以 bsd.port.mk 收尾，否则 make package 缺 bsd.port.subdir.mk
    // 的默认目标链（T-M5-09 交叉复核指出的缺口）。
    out.push_str("\n.include <bsd.port.mk>\n");
    out
}

/// `COMMENT`：单行、≤ 60 字符（portcheck 约束；超出截断而不是让真构建失败）。
fn comment(product: &ProductInfo) -> String {
    let line = product.description.replace(['\n', '\r', '\t'], " ");
    let mut chars = line.trim().chars();
    let mut out: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
    out
}

/// `pkg/DESCR`：描述 + 主页。
fn descr(inputs: &BundleInputs) -> String {
    format!(
        "{}\n\nHomepage: {}\n",
        inputs.product.description,
        homepage(&inputs.product)
    )
}

/// `pkg/PLIST`：载荷文件逐条（可执行文件加 `@bin`），目录由 pkg_create 隐式创建。
fn plist(payload: &BTreeMap<String, bool>) -> String {
    let mut out = String::new();
    for (path, exec) in payload {
        if *exec {
            out.push_str(&format!("@bin {path}\n"));
        } else {
            out.push_str(&format!("{path}\n"));
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

/// 环境变量里的密钥路径（未设 / 空白 → `None`；不做存在性检查，交给工具如实报错）。
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 打包格式与打包器声明必须一致（防「按 ports 目标产出别的格式」）。
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

/// 结构检查：`Makefile` 必填项 + `pkg/DESCR` 非空 + PLIST 与载荷树双向一致。
///
/// 打包器 `package()` 与 verify 腿共用本函数（产物缺陷硬失败，绝不降级成跳过）。
pub fn check_ports_structure(port: &Path) -> Result<PortsStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: port.to_path_buf(),
        reason,
    };
    let makefile = std::fs::read_to_string(port.join("Makefile"))
        .map_err(|err| bad(format!("缺 Makefile：{err}")))?;
    let mut fields = BTreeMap::new();
    for name in [
        "COMMENT",
        "DISTNAME",
        "CATEGORIES",
        "MAINTAINER",
        "PERMIT_PACKAGE",
    ] {
        let value = make_var(&makefile, name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("Makefile 缺必填项 {name}")))?;
        fields.insert(name, value.to_string());
    }
    if !makefile.lines().any(|line| line.trim() == "do-install:") {
        return Err(bad(
            "Makefile 缺 do-install 目标（prebuilt 载荷无安装步骤）".to_string(),
        ));
    }
    if !makefile.contains(".include <bsd.port.mk>") {
        return Err(bad(
            "Makefile 缺 `.include <bsd.port.mk>`（make package 无法走 ports 目标链）".to_string(),
        ));
    }
    if let Some(missing) = ["COMMENT", "DISTNAME", "MAINTAINER"]
        .iter()
        .find(|name| fields[*name].contains(['\t', '\r']))
    {
        return Err(bad(format!("Makefile 的 {missing} 含控制字符")));
    }

    let descr = std::fs::read_to_string(port.join("pkg/DESCR"))
        .map_err(|err| bad(format!("缺 pkg/DESCR：{err}")))?;
    if descr.trim().is_empty() {
        return Err(bad("pkg/DESCR 为空".to_string()));
    }

    let plist_text = std::fs::read_to_string(port.join("pkg/PLIST"))
        .map_err(|err| bad(format!("缺 pkg/PLIST：{err}")))?;
    let mut entries = Vec::new();
    for line in plist_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = line.strip_prefix("@bin ").unwrap_or(line);
        if path.starts_with('@') {
            // 其他注解（@comment / @mode …）本生成器不产出；结构检查只核文件条目。
            continue;
        }
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            return Err(bad(format!(
                "PLIST 条目不是 ${{PREFIX}} 下的相对路径：{line}"
            )));
        }
        entries.push(path.to_string());
    }
    if entries.is_empty() {
        return Err(bad("pkg/PLIST 没有任何文件条目".to_string()));
    }

    let payload_root = port.join("files/payload");
    let payload =
        payload_entries(&payload_root).map_err(|err| bad(format!("载荷树不可读：{err}")))?;
    for entry in &entries {
        if !payload.contains_key(entry) {
            return Err(bad(format!(
                "PLIST 条目 {entry} 在 files/payload/ 下没有对应文件"
            )));
        }
    }
    for path in payload.keys() {
        if !entries.iter().any(|entry| entry == path) {
            return Err(bad(format!(
                "载荷文件 {path} 未进 PLIST（安装树与 packing list 不一致）"
            )));
        }
    }

    Ok(PortsStructure {
        distname: fields["DISTNAME"].clone(),
        maintainer: fields["MAINTAINER"].clone(),
        entries,
    })
}

/// 解析 Makefile 的 `NAME = value` / `NAME=value`（取首个匹配；`\t` 后值）。
fn make_var<'a>(makefile: &'a str, name: &str) -> Option<&'a str> {
    makefile.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use polypack_targets::{Bridge, Host, PackageSpec, Runtime};

    fn spec(id: &str, format: &str) -> TargetSpec {
        let mut spec = TargetSpec::new_stub(id, 5, &[Host::Linux]);
        spec.runtime = Runtime::Native;
        spec.bridge = Bridge::TauriIpc;
        spec.package = PackageSpec {
            format: format.to_string(),
            builder: "openbsd-ports".to_string(),
        };
        spec
    }

    fn inputs(tag: &str) -> (PathBuf, BundleInputs) {
        let root =
            std::env::temp_dir().join(format!("polypack-bundle-ports-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bare = root.join("bare");
        fsutil::write_text(&bare.join("usr/bin/pax-notes"), "#!/bin/sh\nexit 0\n").unwrap();
        fsutil::write_text(&bare.join("usr/lib/pax-notes/app.js"), "console.log(1)").unwrap();
        fsutil::write_text(
            &bare.join("usr/share/applications/pax-notes.desktop"),
            "[Desktop Entry]\nName=Pax Notes\n",
        )
        .unwrap();
        let mut inputs = BundleInputs::new(
            ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "Pax 示例笔记应用".to_string(),
                homepage: None,
            },
            root.join("out"),
        );
        inputs.bare = Some(crate::inputs::BareProduct {
            root: bare.clone(),
            executable: bare.join("usr/bin/pax-notes"),
            host: Host::Linux,
        });
        (root, inputs)
    }

    #[test]
    fn generates_ports_skeleton_matching_payload() {
        let (root, inputs) = inputs("gen");
        let artifact = OpenbsdPorts
            .package(&inputs, &spec("openbsd", FORMAT))
            .expect("ports 骨架应生成");
        assert_eq!(artifact.format, FORMAT);
        let port = port_dir(&inputs);
        assert!(port.join("Makefile").is_file());
        assert!(port.join("pkg/DESCR").is_file());
        assert!(port.join("pkg/PLIST").is_file());

        let makefile = std::fs::read_to_string(port.join("Makefile")).unwrap();
        assert!(
            makefile.contains("DISTNAME =\tpax-notes-1.2.3"),
            "{makefile}"
        );
        assert!(makefile.contains("MAINTAINER =\tPolyPack <ports@polypack.com>"));
        assert!(makefile.contains("do-install:"));
        assert!(
            makefile.trim_end().ends_with(".include <bsd.port.mk>"),
            "Makefile 须以 bsd.port.mk include 收尾：{makefile}"
        );

        let plist = std::fs::read_to_string(port.join("pkg/PLIST")).unwrap();
        assert!(plist.contains("@bin bin/pax-notes"), "{plist}");
        assert!(plist.contains("lib/pax-notes/app.js"), "{plist}");
        assert!(
            plist.contains("share/applications/pax-notes.desktop"),
            "{plist}"
        );

        let structure = check_ports_structure(&port).expect("自产骨架必过结构检查");
        assert_eq!(structure.distname, "pax-notes-1.2.3");
        assert_eq!(structure.entries.len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn structure_check_fails_hard_on_plist_drift() {
        let (root, inputs) = inputs("drift");
        OpenbsdPorts
            .package(&inputs, &spec("openbsd", FORMAT))
            .unwrap();
        let port = port_dir(&inputs);
        // 载荷多一个文件但 PLIST 没跟上 → 硬失败（安装树与 packing list 不一致）。
        fsutil::write_text(&port.join("files/payload/lib/pax-notes/extra.bin"), "x").unwrap();
        let err = check_ports_structure(&port).expect_err("PLIST 漏项必须失败");
        assert_eq!(err.code(), crate::error::codes::E_BND_0006);
        assert!(err.to_string().contains("extra.bin"), "{err}");

        // Makefile 缺 COMMENT → 硬失败。
        fsutil::write_text(&port.join("files/payload/lib/pax-notes/extra.bin"), "").ok();
        let makefile = std::fs::read_to_string(port.join("Makefile")).unwrap();
        std::fs::write(
            port.join("Makefile"),
            makefile.replace("COMMENT =", "# COMMENT ="),
        )
        .unwrap();
        let err = check_ports_structure(&port).expect_err("缺 COMMENT 必须失败");
        assert!(err.to_string().contains("COMMENT"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn format_mismatch_is_rejected() {
        let (root, inputs) = inputs("format");
        let err = OpenbsdPorts
            .plan(&inputs, &spec("openbsd", "pkg"))
            .expect_err("格式不符必须拒绝");
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
        let _ = std::fs::remove_dir_all(&root);
    }
}
