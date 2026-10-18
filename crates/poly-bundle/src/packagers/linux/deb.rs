//! deb 打包器（T-M1-07 首版；T-M5-01 成熟化）。
//!
//! 产物 = `<out_dir>/<kebab>_<version>_<arch>.deb`：dpkg-deb 以 gzip 压缩组装（跨宿主 /
//! 跨 dpkg 版本的确定性选择），control 带 `Depends` 运行时依赖，维护者脚本刷新桌面 /
//! 图标缓存；数据段含 T-M1-14 图标、desktop 文件与 AppStream 元数据。

use std::path::Path;

use poly_targets::TargetSpec;

use super::{
    DEB, DEB_TARGETS, check_format, install_appstream, path_arg, require_png256, source_date_epoch,
    stage_dir, with_epoch,
};
use crate::arwrite::{self, TarArchive};
use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BareProduct, BundleInputs};
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// deb 打包器（`dpkg-deb --build` + `--info` / `--contents` 校验）。
pub struct Deb;

/// 打包器自称的 L1 id（产物 `target` 字段用真实 `spec.id`，发行版目标不冒名）。
const ID: &str = "linux";

/// Linux 桌面运行时依赖（Debian 包名；glibc + Tauri WebView 链）。
pub const DEB_RUNTIME_DEPENDS: &str = "libc6, libwebkit2gtk-4.1-0, libgtk-3-0, libsoup-3.0-0";

/// `postinst`：刷新 desktop / 图标缓存（工具缺席时静默跳过，不引硬依赖）。
const POSTINST: &str = "#!/bin/sh\n\
set -e\n\
if [ \"$1\" = \"configure\" ]; then\n\
    if command -v update-desktop-database >/dev/null 2>&1; then\n\
        update-desktop-database -q /usr/share/applications || true\n\
    fi\n\
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then\n\
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true\n\
    fi\n\
fi\n\
exit 0\n";

/// `postrm`：卸载 / 清除后同样刷新缓存。
const POSTRM: &str = "#!/bin/sh\n\
set -e\n\
if [ \"$1\" = \"remove\" ] || [ \"$1\" = \"purge\" ]; then\n\
    if command -v update-desktop-database >/dev/null 2>&1; then\n\
        update-desktop-database -q /usr/share/applications || true\n\
    fi\n\
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then\n\
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true\n\
    fi\n\
fi\n\
exit 0\n";

/// deb 结构检查结果（打包器自检与 verify 腿 `lintian` 的本地结构面共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebStructure {
    /// ar 成员名（序即 Debian 约定序）。
    pub members: Vec<String>,
    /// control 的 `Package`。
    pub package: String,
    /// control 的 `Version`。
    pub version: String,
    /// control 的 `Architecture`。
    pub architecture: String,
    /// control 的 `Depends`（许可为空）。
    pub depends: String,
    /// `data.tar.gz` 条目名（排序）。
    pub entries: Vec<String>,
}

impl Packager for Deb {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        DEB
    }

    fn serves(&self, target_id: &str) -> bool {
        DEB_TARGETS.contains(&target_id)
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, DEB)?;
        let bare = inputs.require_bare(DEB)?;
        require_png256(inputs)?;
        let stage = stage_dir(inputs, DEB);
        let deb = deb_path(inputs);
        let epoch = source_date_epoch();
        Ok(BundlePlan::new()
            .with_script(stage.join("DEBIAN/control"), control_file(inputs, bare)?)
            .with_script(stage.join("DEBIAN/postinst"), POSTINST)
            .with_script(stage.join("DEBIAN/postrm"), POSTRM)
            .with_command(with_epoch(
                Command::new(
                    "dpkg-deb",
                    [
                        "--build".into(),
                        "-Zgzip".into(),
                        "--root-owner-group".into(),
                        path_arg(&stage),
                        path_arg(&deb),
                    ],
                ),
                epoch,
            ))
            .with_command(Command::new("dpkg-deb", ["--info".into(), path_arg(&deb)]))
            .with_command(Command::new(
                "dpkg-deb",
                ["--contents".into(), path_arg(&deb)],
            )))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let bare = inputs.require_bare(DEB)?;
        let plan = self.plan(inputs, spec)?;
        let stage = stage_dir(inputs, DEB);
        let _ = std::fs::remove_dir_all(&stage);
        // 裸产物树整体进入 deb 数据段（usr/bin、usr/lib 资源、desktop、图标、doc）。
        fsutil::copy_tree(&bare.root, &stage)?;
        write_copyright(inputs, &stage)?;
        write_changelog(inputs, &stage)?;
        install_appstream(&stage.join("usr/share"), &inputs.product)?;
        fsutil::write_scripts(&plan)?;
        super::normalize_modes(&stage)?;
        // dpkg-deb 要求维护者脚本权限 0555..0775（write_text 缺省 0644 会被拒）。
        fsutil::make_executable(&stage.join("DEBIAN/postinst"))?;
        fsutil::make_executable(&stage.join("DEBIAN/postrm"))?;
        // 可复现：数据段的 mtime 归一到 SOURCE_DATE_EPOCH（未设置则保持原样）。
        super::apply_epoch(&stage)?;

        exec::run_all(&plan.commands)?;

        let deb = deb_path(inputs);
        let structure = check_deb_structure(&deb)?;
        let info = control_summary(&deb)?;
        crate::fsutil::file_artifact(
            &spec.id,
            DEB,
            &deb,
            vec![
                info,
                format!(
                    "结构检查：{} 个数据段条目，Depends={}（lintian 门禁见 poly verify）",
                    structure.entries.len(),
                    if structure.depends.is_empty() {
                        "(无)"
                    } else {
                        &structure.depends
                    }
                ),
                format!(
                    "安装冒烟：dpkg -i <deb>（需 root）；卸载：dpkg -r {}",
                    structure.package
                ),
            ],
        )
    }
}

/// deb 产物路径（与测试同一推导）。
pub(crate) fn deb_path(inputs: &BundleInputs) -> std::path::PathBuf {
    inputs.out_dir.join(format!(
        "{}_{}_{}.deb",
        inputs.product.kebab_name(),
        inputs.product.version,
        super::deb_arch()
    ))
}

/// `dpkg-deb` 的 control 文件（Debian Policy §5.3 必备字段 + T-M5-01 的 Depends）。
fn control_file(inputs: &BundleInputs, bare: &BareProduct) -> Result<String, BundleError> {
    let product = &inputs.product;
    let installed = fsutil::tree_bytes(&bare.root)?.div_ceil(1024);
    let maintainer = maintainer(product);
    let synopsis = synopsis(product);
    let mut out = String::new();
    out.push_str(&format!("Package: {}\n", product.kebab_name()));
    out.push_str(&format!("Version: {}\n", product.version));
    out.push_str(&format!("Architecture: {}\n", super::deb_arch()));
    out.push_str(&format!("Maintainer: {maintainer}\n"));
    out.push_str(&format!("Installed-Size: {installed}\n"));
    out.push_str("Section: utils\n");
    out.push_str("Priority: optional\n");
    out.push_str(&format!("Depends: {DEB_RUNTIME_DEPENDS}\n"));
    if let Some(home) = &product.homepage {
        out.push_str(&format!("Homepage: {home}\n"));
    }
    // 扩展描述行（Debian 规范：续行以空格开头）：避免 lintian 的 empty-extended-description。
    out.push_str(&format!(
        "Description: {synopsis}\n {}：由 PolyPack 打包的桌面应用（Tauri WebView 运行时）。\n",
        product.name
    ));
    Ok(out)
}

/// Debian 维护者字段：缺邮箱时补项目默认联系地址（lintian `malformed-contact`）。
fn maintainer(product: &crate::inputs::ProductInfo) -> String {
    if product.manufacturer.contains('<') {
        product.manufacturer.clone()
    } else {
        format!("{} <noreply@polypack.dev>", product.manufacturer)
    }
}

/// Debian 短描述：为空、过短或与包名相同时给出策略友好的兜底
/// （lintian `description-is-pkg-name` / `description-starts-with-package-name` / `too-short`）。
fn synopsis(product: &crate::inputs::ProductInfo) -> String {
    let description = product.description.trim();
    let kebab = product.kebab_name();
    let starts_with_pkg = description.to_ascii_lowercase().starts_with(kebab.as_str());
    if description.is_empty()
        || description.len() < 10
        || description.eq_ignore_ascii_case(&product.name)
        || description.eq_ignore_ascii_case(&kebab)
        || starts_with_pkg
    {
        "Desktop application packaged by PolyPack".to_string()
    } else {
        description.to_string()
    }
}

/// `usr/share/doc/<name>/copyright`（lintian 无 copyright 文件为 error 级）。
///
/// shortcut: 许可证尚未进入 BundleInputs（M1 无 license 字段），暂按仓库
/// Apache-2.0 声明；许可证明细随 T-M1-14 / M2 的产物元数据落库后改为读输入。
fn write_copyright(inputs: &BundleInputs, stage: &Path) -> Result<(), BundleError> {
    let product = &inputs.product;
    fsutil::write_text(
        &stage.join(format!("usr/share/doc/{}/copyright", product.kebab_name())),
        &format!(
            "Upstream: {}\nName: {}\nLicense: Apache-2.0\n",
            product.homepage.as_deref().unwrap_or("(未提供)"),
            product.name
        ),
    )
}

/// `usr/share/doc/<name>/changelog.gz`（lintian `no-changelog`）。
///
/// 使用固定日期保证可复现；版本与包版本一致，避免 changelog 与 control 脱节。
fn write_changelog(inputs: &BundleInputs, stage: &Path) -> Result<(), BundleError> {
    use std::io::Write as _;

    let product = &inputs.product;
    let path = stage.join(format!(
        "usr/share/doc/{}/changelog.gz",
        product.kebab_name()
    ));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let text = format!(
        "{} ({}) unstable; urgency=medium\n\n  * PolyPack build.\n\n -- {}  Thu, 01 Jan 2026 00:00:00 +0000\n",
        product.kebab_name(),
        product.version,
        maintainer(product)
    );
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).map_err(io_err(&path))?;
    let bytes = encoder.finish().map_err(io_err(&path))?;
    std::fs::write(&path, bytes).map_err(io_err(&path))
}

/// 解析 `dpkg-deb --info` 的关键字段作为产物证据。
fn control_summary(deb: &Path) -> Result<String, BundleError> {
    let info = exec::run(&Command::new("dpkg-deb", ["--info".into(), path_arg(deb)]))?;
    let text = String::from_utf8_lossy(&info.stdout);
    let fields = ["Package", "Version", "Architecture", "Installed-Size"]
        .iter()
        .filter_map(|key| {
            text.lines()
                .find(|l| l.trim_start().starts_with(&format!("{key}:")))
                .map(|l| l.trim().to_string())
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("dpkg-deb --info: {fields}"))
}

/// `.deb` 结构检查：ar 成员序 + `debian-binary` + control 必填字段 + 数据段载荷。
///
/// 纯 Rust 读取（gzip tar，与打包器 `-Zgzip` 同口径），不依赖 dpkg-deb——verify 腿在
/// 只装了 Python/Perl lintian 的宿主上也能跑结构面；官方 lintian 在场时再叠加判词。
pub fn check_deb_structure(deb: &Path) -> Result<DebStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: deb.to_path_buf(),
        reason,
    };
    let members = arwrite::read_ar(deb)?;
    let names: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
    let expected = ["debian-binary", "control.tar.gz", "data.tar.gz"];
    if names.len() != expected.len() || !names.iter().zip(expected).all(|(got, want)| got == want) {
        return Err(bad(format!(
            "ar 成员面不符（须按序 {expected:?}，实际 {names:?}）"
        )));
    }
    if members[0].bytes.as_slice() != b"2.0\n" {
        return Err(bad("debian-binary 内容须为 `2.0\\n`".to_string()));
    }
    let control = TarArchive::from_tar_gz(&members[1].bytes, deb)?;
    let control_text = control
        .text("./control")
        .ok_or_else(|| bad("control.tar.gz 内缺 ./control".to_string()))?;
    let fields = arwrite::control_fields(&control_text);
    for required in [
        "Package",
        "Version",
        "Architecture",
        "Maintainer",
        "Description",
    ] {
        if fields.get(required).is_none_or(|value| value.is_empty()) {
            return Err(bad(format!("control 缺必填字段 {required}")));
        }
    }
    let data = TarArchive::from_tar_gz(&members[2].bytes, deb)?;
    let mut entries = data.names();
    entries.sort();
    for required in [".desktop", ".metainfo.xml"] {
        if !entries.iter().any(|entry| entry.ends_with(required)) {
            return Err(bad(format!(
                "数据段缺 `{required}`（desktop 入口 / AppStream 元数据）"
            )));
        }
    }
    Ok(DebStructure {
        members: names,
        package: fields["Package"].clone(),
        version: fields["Version"].clone(),
        architecture: fields["Architecture"].clone(),
        depends: fields.get("Depends").cloned().unwrap_or_default(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 维护者脚本必须是可执行的 POSIX sh 且只在 configure / remove 时动作。
    #[test]
    fn maintainer_scripts_are_policy_shaped() {
        for script in [POSTINST, POSTRM] {
            assert!(script.starts_with("#!/bin/sh\n"));
            assert!(script.ends_with("exit 0\n"));
            assert!(script.contains("command -v update-desktop-database"));
            assert!(!script.contains("which "));
        }
        assert!(POSTINST.contains("[ \"$1\" = \"configure\" ]"));
        assert!(POSTRM.contains("[ \"$1\" = \"remove\" ]"));
    }
}
