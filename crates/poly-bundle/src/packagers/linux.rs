//! Linux 打包器（T-M1-07）：deb（dpkg-deb 组装 + `--info` 校验）与 AppImage（appimagetool）。
//!
//! 两者都消费 T-M1-03 的裸产物（AppDir 形状：`usr/bin` + `usr/lib/<产品名>` +
//! `usr/share/{applications,icons,doc}`），deb 另加 `DEBIAN/control`，AppImage 另加
//! 根级 `<name>.desktop` / `<name>.png` / `AppRun`。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, IconKind};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::tauri;

/// deb 打包器（`dpkg-deb --build --root-owner-group` + `--info` / `--contents` 校验）。
pub struct Deb;

/// AppImage 打包器（appimagetool；工具缺失时产出 AppDir 并如实标注未打包）。
pub struct AppImage;

const DEB: &str = "deb";
const APPIMAGE: &str = "appimage";

impl Packager for Deb {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn format(&self) -> &'static str {
        DEB
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, DEB)?;
        let bare = inputs.require_bare(DEB)?;
        require_png256(inputs)?;
        let stage = stage_dir(inputs, DEB);
        let deb = inputs.out_dir.join(format!(
            "{}_{}_{}.deb",
            inputs.product.kebab_name(),
            inputs.product.version,
            deb_arch()
        ));
        Ok(BundlePlan::new()
            .with_script(stage.join("DEBIAN/control"), control_file(inputs, bare)?)
            .with_command(Command::new(
                "dpkg-deb",
                [
                    "--build".into(),
                    "--root-owner-group".into(),
                    path_arg(&stage),
                    path_arg(&deb),
                ],
            ))
            .with_command(Command::new("dpkg-deb", ["--info".into(), path_arg(&deb)]))
            .with_command(Command::new(
                "dpkg-deb",
                ["--contents".into(), path_arg(&deb)],
            )))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(DEB)?;
        let stage = stage_dir(inputs, DEB);
        let _ = std::fs::remove_dir_all(&stage);
        // 裸产物树整体进入 deb 数据段（usr/bin、usr/lib 资源、desktop、图标、doc）。
        fsutil::copy_tree(&bare.root, &stage)?;
        write_copyright(inputs, &stage)?;
        fsutil::write_scripts(&plan)?;

        exec::run_all(&plan.commands)?;

        let deb = inputs.out_dir.join(format!(
            "{}_{}_{}.deb",
            inputs.product.kebab_name(),
            inputs.product.version,
            deb_arch()
        ));
        let info = control_summary(&deb)?;
        crate::fsutil::file_artifact(
            self.id(),
            DEB,
            &deb,
            vec![
                info,
                "安装冒烟：dpkg -i <deb>（需 root）；卸载：dpkg -r <package>".to_string(),
            ],
        )
    }
}

impl Packager for AppImage {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn format(&self) -> &'static str {
        APPIMAGE
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, APPIMAGE)?;
        inputs.require_bare(APPIMAGE)?;
        let appdir = appdir_path(inputs);
        let out = appimage_path(inputs);
        let mut plan = BundlePlan::new().with_command(Command::new(
            "appimagetool",
            ["--no-appstream".into(), path_arg(&appdir), path_arg(&out)],
        ));
        if !exec::tool_present("appimagetool") {
            plan = plan.skipping(
                "appimagetool",
                "未安装：产出 AppDir 目录（可直接托管 / 供 FUSE 挂载），AppImage 单文件需 appimagetool 与 runtime",
            );
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(APPIMAGE)?;
        let icon = inputs
            .icon(IconKind::Png)
            .ok_or(BundleError::MissingInput {
                packager: APPIMAGE,
                field: "icons(PNG)",
            })?;
        let appdir = appdir_path(inputs);
        let _ = std::fs::remove_dir_all(&appdir);
        fsutil::copy_tree(&bare.root, &appdir)?;

        // AppDir 根级三件套：desktop / 图标 / AppRun（AppImage 约定）。
        let name = inputs.product.kebab_name();
        let bin = inputs.product.binary_name();
        fsutil::write_text(
            &appdir.join(format!("{name}.desktop")),
            &tauri::desktop_file(&inputs.product, &bin),
        )?;
        fsutil::copy_file(&icon.path, &appdir.join(format!("{name}.png")), false)?;
        // AppRun 指向可执行：AppImage runtime 挂载时会设置 APPDIR，直接运行 AppDir 时
        // Tauri 走 `../lib/<产品名>` 分支，两者都落到同一资源目录。
        fsutil::symlink(Path::new(&format!("usr/bin/{bin}")), &appdir.join("AppRun"))?;

        if plan.skipped.is_empty() {
            exec::run_all(&plan.commands)?;
            let out = appimage_path(inputs);
            return fsutil::file_artifact(
                self.id(),
                APPIMAGE,
                &out,
                vec!["启动冒烟：chmod +x <AppImage> && ./<AppImage> --version".to_string()],
            );
        }
        let notes = plan
            .skipped
            .iter()
            .map(|s| format!("未打包：{}（{}）", s.tool, s.reason))
            .collect();
        fsutil::dir_artifact(self.id(), "appdir", &appdir, notes)
    }
}

/// deb `Architecture` 字段（本机打包；交叉打包由 T-M2+ 的 triple 决定）。
fn deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        "riscv64" => "riscv64",
        other => other,
    }
}

/// deb 需要 256 PNG 图标（金样条目 `usr/share/icons/hicolor/256x256/apps/*`）：缺失响亮拒绝。
///
/// 图标由 T-M1-14 派生管线（[`crate::icons::derive`]）从品牌 SVG 产出后填入
/// `BundleInputs.icons`；其它尺寸可有可无，256 是发行包桌面条目的最低要求。
fn require_png256(inputs: &BundleInputs) -> Result<(), BundleError> {
    if inputs.icon(IconKind::Png).is_none() {
        return Err(BundleError::MissingInput {
            packager: DEB,
            field: "icons(PNG)",
        });
    }
    if !inputs
        .icons
        .iter()
        .any(|i| i.kind == IconKind::Png && i.size == Some(256))
    {
        return Err(BundleError::MissingInput {
            packager: DEB,
            field: "icons(PNG 256)",
        });
    }
    Ok(())
}

/// 打包格式与打包器声明必须一致（防「按 appimage 目标产出 deb」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "linux",
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 中间暂存目录。
fn stage_dir(inputs: &BundleInputs, tag: &str) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}-{tag}.stage", inputs.product.kebab_name()))
}

/// AppDir 目录。
fn appdir_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.AppDir", inputs.product.kebab_name()))
}

/// AppImage 文件路径。
fn appimage_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}-{}.AppImage",
        inputs.product.name.replace(' ', "-"),
        inputs.product.version,
        deb_arch()
    ))
}

/// `dpkg-deb` 的 control 文件（Debian Policy §5.3 必备字段）。
fn control_file(
    inputs: &BundleInputs,
    bare: &crate::inputs::BareProduct,
) -> Result<String, BundleError> {
    let product = &inputs.product;
    let installed = fsutil::tree_bytes(&bare.root)?.div_ceil(1024);
    let mut out = String::new();
    out.push_str(&format!("Package: {}\n", product.kebab_name()));
    out.push_str(&format!("Version: {}\n", product.version));
    out.push_str(&format!("Architecture: {}\n", deb_arch()));
    out.push_str(&format!("Maintainer: {}\n", product.manufacturer));
    out.push_str(&format!("Installed-Size: {installed}\n"));
    out.push_str("Section: utils\n");
    out.push_str("Priority: optional\n");
    if let Some(home) = &product.homepage {
        out.push_str(&format!("Homepage: {home}\n"));
    }
    out.push_str(&format!("Description: {}\n", product.description));
    Ok(out)
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

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deb_arch_is_a_debian_name() {
        assert!(
            ["amd64", "arm64", "i386", "armhf", "riscv64"].contains(&deb_arch()),
            "未映射的架构 {}",
            deb_arch()
        );
    }
}
