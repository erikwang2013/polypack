//! macOS DMG 打包器（T-M1-06）：`hdiutil` 组装 UDZO 磁盘映像 + 拖拽安装布局。
//!
//! 暂存目录 `<out>/dmg-stage/` 内放裸产物里的 `<产品名>.app` 与指向 `/Applications` 的
//! `Applications` 符号链接（打开 DMG 后拖拽即完成安装），`hdiutil create -srcfolder` 打卷、
//! `hdiutil verify` 校验。非 macOS 宿主无 `hdiutil` → [`BundleError::ToolMissing`]（mock 模式
//! 契约测试以此断言「真机执行阻塞（宿主）」）。实现不按宿主 `#[cfg]` 分叉，同一代码路径可测。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// DMG 打包器（脚本 `<out>/dmg-stage/README.txt`，产物 `<产品名>_<版本>_x64.dmg`）。
pub struct Dmg;

const DMG: &str = "dmg";

impl Packager for Dmg {
    fn id(&self) -> &'static str {
        "macos"
    }

    fn format(&self) -> &'static str {
        DMG
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, DMG)?;
        inputs.require_bare(DMG)?;
        let stage = stage_dir(inputs);
        let dmg = dmg_path(inputs);
        Ok(BundlePlan::new()
            .with_script(stage.join("README.txt"), readme(inputs))
            .with_command(Command::new(
                "hdiutil",
                [
                    "create".into(),
                    "-volname".into(),
                    volume_name(inputs),
                    "-srcfolder".into(),
                    path_arg(&stage),
                    "-ov".into(),
                    "-format".into(),
                    "UDZO".into(),
                    path_arg(&dmg),
                ],
            ))
            .with_command(Command::new("hdiutil", ["verify".into(), path_arg(&dmg)])))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        self.stage(inputs, spec)?;
        let plan = self.plan(inputs, spec)?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        fsutil::file_artifact(
            self.id(),
            DMG,
            &dmg_path(inputs),
            vec![
                format!(
                    "安装：打开 DMG 后将 {}.app 拖入 Applications（stage 内 Applications 为符号链接）",
                    inputs.product.name
                ),
                "真机（macos-14）执行 hdiutil verify 与挂载/卸载冒烟；本宿主无 hdiutil，真机执行阻塞（宿主）"
                    .to_string(),
            ],
        )
    }
}

impl Dmg {
    /// 组装拖拽安装布局（公开供契约测试直接断言）：裸产物内 `*.app` 拷入暂存目录 + `Applications` 链接。
    ///
    /// 返回暂存目录路径（`hdiutil create -srcfolder` 的输入）。
    pub fn stage(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<PathBuf, BundleError> {
        check_format(spec, DMG)?;
        let bare = inputs.require_bare(DMG)?;
        let stage = stage_dir(inputs);
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage).map_err(io_err(&stage))?;

        let mut apps = 0usize;
        for entry in std::fs::read_dir(&bare.root).map_err(io_err(&bare.root))? {
            let entry = entry.map_err(io_err(&bare.root))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() && name.ends_with(".app") {
                fsutil::copy_tree(&path, &stage.join(entry.file_name()))?;
                apps += 1;
            }
        }
        if apps == 0 {
            // 空 DMG 会「打包成功但装不了东西」：缺 .app 即快速失败。
            return Err(BundleError::MissingInput {
                packager: DMG,
                field: "bare.root/*.app",
            });
        }
        fsutil::symlink(Path::new("/Applications"), &stage.join("Applications"))?;
        Ok(stage)
    }
}

/// 打包格式与打包器声明必须一致（防「按 deb 目标产出 dmg」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "macos",
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 暂存目录（DMG 的 `-srcfolder`）。
fn stage_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("dmg-stage")
}

/// DMG 产物路径（与 tauri-bundler 命名一致：`<产品名>_<版本>_x64.dmg`）。
fn dmg_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}_{}_x64.dmg",
        inputs.product.name, inputs.product.version
    ))
}

/// 卷名（挂载后 Finder 侧栏显示的名字）。
fn volume_name(inputs: &BundleInputs) -> String {
    format!("{} {}", inputs.product.name, inputs.product.version)
}

/// 暂存目录内的安装说明。
fn readme(inputs: &BundleInputs) -> String {
    let product = &inputs.product;
    format!(
        "{name} {version}\n\n将 {name}.app 拖入 Applications 文件夹即可完成安装（暂存目录内的 Applications 是指向 /Applications 的符号链接）。\n卸载：把 {name}.app 从 Applications 移入废纸篓。\n",
        name = product.name,
        version = product.version,
    )
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::ProductInfo;

    #[test]
    fn artifact_names_follow_bundler_convention() {
        let inputs = BundleInputs::new(
            ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "示例".to_string(),
                homepage: None,
            },
            "/out",
        );
        assert_eq!(
            dmg_path(&inputs),
            PathBuf::from("/out/Pax Notes_1.2.3_x64.dmg")
        );
        assert_eq!(volume_name(&inputs), "Pax Notes 1.2.3");
        assert!(readme(&inputs).contains("拖入 Applications"));
    }
}
