//! `package` 阶段（§5.1 汇聚点）：把 `compile` / `bundle` 的产出交给打包器矩阵（poly-bundle）。
//!
//! 实现落在 poly-cli：打包器矩阵在 poly-bundle，而依赖方向约束（`tests/dependency_direction.rs`）
//! 禁止 poly-core 依赖它，故经 [`poly_core::pipeline::plan_with_package`] 注入。
//!
//! 只打包「本机宿主目标」（`spec.hosts` 含 `current_host()`）；交叉打包需 wine / osxcross 等（M2）。
//! 环境性缺失（宿主无打包工具、输入未就绪）如实告警并跳过本目标，不阻断编译与 UI 支线；
//! 配置非法 / 外部命令失败仍硬失败，错误码原样透传（`poly::E_BND_*`）。
//!
//! shortcut: 「跳过不置非零退出」是 M1 的取舍（本机缺打包工具不该拦住编译与 UI 支线）；
//! M2 签名 / 校验入场后按 profile 收紧——`ci` 下缺包即失败（profile 已分 local / ci）。

use std::path::{Path, PathBuf};

use poly_bundle::error::BundleError;
use poly_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::events::BuildEvent;
use poly_core::job::JobSpec;
use poly_core::pipeline;
use poly_core::plan::{Stage, StageContext, StageError, StageOutcome};
use poly_targets::{Host, Runtime, TargetSpec, current_host};

use crate::project;

/// 派生图标目录约定（T-M1-14 落点）：`<root>/icons/<边长>x<边长>.png` / `icon.ico` / `icon.icns`。
const ICONS_DIR: &str = "icons";

/// 图标单一源（UI 相对路径）：`poly init` 把 `docs/assets/pax-mark.svg` 的副本落在这里。
const ICON_SVG: &str = "public/icon.svg";

/// 图标源回退（工程根相对路径）：UI 目录之外放图标时的落点。
const ICON_SVG_ROOT: &str = "icon.svg";

/// PWA manifest 候选文件名（T-M1-04 产物，web 打包器必需输入）。
const MANIFEST_NAMES: &[&str] = &["manifest.webmanifest", "manifest.json"];

/// `package` 阶段。
pub struct PackageStage;

impl Stage for PackageStage {
    fn name(&self) -> &'static str {
        "package"
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let Some(project) = ctx.project else {
            return Ok(StageOutcome::Skipped {
                reason: "缺少项目上下文".to_string(),
            });
        };
        let Some(host) = current_host() else {
            return Ok(StageOutcome::Skipped {
                reason: "无法识别宿主（交叉打包归 M2）".to_string(),
            });
        };
        let native: Vec<TargetSpec> = ctx
            .job
            .targets
            .iter()
            .filter(|spec| spec.hosts.contains(&host))
            .cloned()
            .collect();
        if native.is_empty() {
            return Ok(StageOutcome::Skipped {
                reason: format!("本次构建无 {host:?} 宿主目标（交叉打包归 M2）"),
            });
        }

        let mut packaged = 0;
        for spec in &native {
            if package_target(project, spec, host, ctx)? {
                packaged += 1;
            }
        }
        if packaged == 0 {
            return Ok(StageOutcome::Skipped {
                reason: format!(
                    "{} 个宿主目标均未产出安装包（见 package 告警）",
                    native.len()
                ),
            });
        }
        Ok(StageOutcome::Done)
    }
}

/// 打包单个目标；返回是否真的产出了安装包（跳过 / 环境性缺失为 `false`）。
fn package_target(
    project: &poly_core::Project,
    spec: &TargetSpec,
    host: Host,
    ctx: &mut StageContext<'_>,
) -> Result<bool, StageError> {
    let out_dir = project::packages_root(&project.root).join(&spec.id);
    let Some(packager) = poly_bundle::for_spec(spec) else {
        return Ok(skip(
            ctx,
            spec,
            &out_dir,
            &format!(
                "无内建打包器（format = {}，矩阵 T-M1-05..08 未覆盖）",
                spec.package.format
            ),
        ));
    };

    // 图标（T-M1-14）：手工放置的 `<root>/icons/` 优先，否则从项目图标 svg 现场派生。
    // 桌面包内必须带图标（金样 `linux.artifact.json` 要求 hicolor 图标、macOS 要求 icns），
    // 两个来源都没有时不出半成品包，如实告警跳过。
    let manual = collect_icons(&project.root);
    // 只有桌面目标吃图标（Web 的 PWA 图标走 dist/icon.svg，见 web.rs）；派生一次要起
    // rsvg-convert，不为用不上的目标白跑
    let icon_svg = if manual.is_empty() && spec.runtime == Runtime::Native {
        project_icon_svg(project)
    } else {
        None
    };
    if spec.runtime == Runtime::Native && manual.is_empty() && icon_svg.is_none() {
        return Ok(skip(
            ctx,
            spec,
            &out_dir,
            &format!(
                "无图标：既没有 {ICONS_DIR}/ 也没有 {ICON_SVG}（T-M1-14 派生源）——桌面包内必须带图标"
            ),
        ));
    }

    // 确实要打包了才建暂存目录：跳过时不留空目录，免得快照里出现空的 packages/
    std::fs::remove_dir_all(&out_dir).ok();
    // 与 dist 不同：包是整份替换，每次构建重写该目标的暂存目录，免得旧版安装包残留
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| project::io_error("创建打包暂存目录", &out_dir, err))?;

    let mut inputs = BundleInputs::new(product_info(&project.config.package), &out_dir);
    inputs.core_binary = pipeline::artifact_path(project)
        .ok()
        .filter(|path| path.is_file());
    let dist = project.root.join("dist");
    if dist.is_dir() {
        inputs.web_manifest = MANIFEST_NAMES
            .iter()
            .map(|name| dist.join(name))
            .find(|path| path.is_file());
        inputs.ui_dist = Some(dist);
    }
    inputs.icons = match &icon_svg {
        // 派生 PNG 16..256 + ICO 落到暂存目录；缺 rsvg-convert 等由 environment 决定跳过
        Some(svg) => match poly_bundle::icons::derive(svg, &out_dir) {
            Ok(set) => {
                for note in &set.notes {
                    ctx.events.emit(&BuildEvent::Warning {
                        stage: "package".to_string(),
                        message: format!("图标派生：{note}"),
                    });
                }
                set.icons
            }
            Err(err) => return environment(ctx, spec, &out_dir, err),
        },
        None => manual,
    };

    if spec.runtime == Runtime::Native {
        // 裸产物（T-M1-03）：核心可执行 + WebView 资源 + 平台元数据，桌面打包器的必需输入
        match poly_bundle::tauri::bare_product(&inputs, host) {
            Ok(bare) => inputs.bare = Some(bare),
            Err(err) => return environment(ctx, spec, &out_dir, err),
        }
    }

    match packager.package(&inputs, spec) {
        Ok(artifact) => {
            ctx.events.emit(&BuildEvent::Packaged {
                format: artifact.format.clone(),
                bytes: artifact.bytes,
            });
            // 打包器如实标注的降级（如缺 appimagetool 时产出 AppDir）转发为告警
            for note in &artifact.notes {
                ctx.events.emit(&BuildEvent::Warning {
                    stage: "package".to_string(),
                    message: format!("{}：{note}", artifact.format),
                });
            }
            Ok(true)
        }
        Err(err) => environment(ctx, spec, &out_dir, err),
    }
}

/// 环境性缺失（宿主工具 / 输入未就绪）→ 告警跳过；其余（配置非法、命令失败）→ 硬失败。
fn environment(
    ctx: &mut StageContext<'_>,
    spec: &TargetSpec,
    out_dir: &Path,
    err: BundleError,
) -> Result<bool, StageError> {
    match err {
        BundleError::ToolMissing { .. } | BundleError::MissingInput { .. } => {
            Ok(skip(ctx, spec, out_dir, &err.to_string()))
        }
        other => {
            let mut diagnostic = Diagnostic::error(
                Code::new(other.code()),
                format!("目标 {} 打包失败：{other}", spec.id),
            );
            if let Some(hint) = other.hint() {
                diagnostic = diagnostic.with_hint(hint);
            }
            Err(diagnostic.into())
        }
    }
}

/// 告警并跳过本目标的打包；顺带清掉已建的暂存目录（跳过时不留半成品 / 空目录）。
fn skip(ctx: &mut StageContext<'_>, spec: &TargetSpec, out_dir: &Path, reason: &str) -> bool {
    std::fs::remove_dir_all(out_dir).ok();
    ctx.events.emit(&BuildEvent::Warning {
        stage: "package".to_string(),
        message: format!("目标 {} 未打包：{reason}", spec.id),
    });
    false
}

/// 项目图标 svg（T-M1-14 派生源）：优先 UI 目录下的 `public/icon.svg`（模板约定），
/// 回退工程根 `icon.svg`。
fn project_icon_svg(project: &poly_core::Project) -> Option<PathBuf> {
    let from_ui = poly_core::web::ui_dir(project)
        .ok()
        .map(|ui| ui.join(ICON_SVG));
    from_ui
        .into_iter()
        .chain([project.root.join(ICON_SVG_ROOT)])
        .find(|path| path.is_file())
}

/// 产品元数据：M0 的 `[package]` 只有 name / version，其余按名称派生。
///
/// shortcut: 派生值（`com.polypack.<kebab>`）仅供本机打包，跨宿主发布（M2）前应在
/// `[package]` 补 identifier / manufacturer / description 字段。
fn product_info(section: &poly_core::config::PackageSection) -> ProductInfo {
    let mut product = ProductInfo {
        name: section.name.clone(),
        version: section.version.clone(),
        identifier: String::new(),
        manufacturer: "PolyPack".to_string(),
        description: section.name.clone(),
        homepage: None,
    };
    product.identifier = format!("com.polypack.{}", product.kebab_name());
    product
}

/// 派生图标收集：`<root>/icons/` 下的 `<边长>x<边长>.png` / `icon.ico` / `icon.icns`。
fn collect_icons(root: &Path) -> Vec<Icon> {
    let Ok(entries) = std::fs::read_dir(root.join(ICONS_DIR)) else {
        return Vec::new();
    };
    let mut icons: Vec<Icon> = entries
        .flatten()
        .filter_map(|entry| {
            let path: PathBuf = entry.path();
            let name = path.file_name()?.to_str()?.to_ascii_lowercase();
            let (kind, size) = if let Some(stem) = name.strip_suffix(".png") {
                // hicolor 目录名要边长，故 PNG 一律「<边长>x<边长>.png」（不认识的名字忽略）
                let (width, height) = stem.split_once('x')?;
                if width != height {
                    return None;
                }
                (IconKind::Png, Some(width.parse().ok()?))
            } else if name.ends_with(".ico") {
                (IconKind::Ico, None)
            } else if name.ends_with(".icns") {
                (IconKind::Icns, None)
            } else {
                return None;
            };
            Some(Icon { path, kind, size })
        })
        .collect();
    icons.sort_by(|a, b| a.path.cmp(&b.path)); // 目录序无关
    icons
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_collection_needs_size_in_name() {
        let root = std::env::temp_dir().join(format!("polypack-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join(ICONS_DIR);
        std::fs::create_dir_all(&dir).expect("图标目录可建");
        for name in ["256x256.png", "logo.png", "icon.ico", "README.md"] {
            std::fs::write(dir.join(name), b"x").expect("文件可写");
        }

        let icons = collect_icons(&root);
        assert_eq!(
            icons.len(),
            2,
            "只认识带边长的 PNG 与 ico / icns: {icons:?}"
        );
        assert_eq!(icons[0].kind, IconKind::Png);
        assert_eq!(icons[0].size, Some(256));
        assert_eq!(icons[1].kind, IconKind::Ico);
        assert!(icons[1].size.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn derived_product_metadata_passes_bundle_validation() {
        let section = poly_core::config::PackageSection {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
        };
        let product = product_info(&section);
        assert_eq!(product.identifier, "com.polypack.pax-notes");
        assert_eq!(product.kebab_name(), "pax-notes");
        // 派生值必须过 poly-bundle 的注入白名单（打包脚本边界）
        product.validate("test").expect("派生元数据应合法");
    }
}
