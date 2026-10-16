//! `package` 阶段（§5.1 汇聚点）：把 `compile` / `bundle` 的产出交给打包器矩阵（poly-bundle）。
//!
//! 实现落在 poly-cli：打包器矩阵在 poly-bundle，而依赖方向约束（`tests/dependency_direction.rs`）
//! 禁止 poly-core 依赖它，故经 [`poly_core::pipeline::plan_with_stages`] 注入。
//!
//! 只打包「本机宿主目标」（`spec.hosts` 含 `current_host()`）；交叉打包需 wine / osxcross 等（M2）。
//! 环境性缺失（宿主无打包工具、输入未就绪）如实告警并跳过本目标，不阻断编译与 UI 支线；
//! 配置非法 / 外部命令失败仍硬失败，错误码原样透传（`poly::E_BND_*`）。
//!
//! shortcut: 「跳过不置非零退出」是 M1 的取舍（本机缺打包工具不该拦住编译与 UI 支线）；
//! M2 签名 / 校验入场后按 profile 收紧——`ci` 下缺包即失败（profile 已分 local / ci）。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::error::BundleError;
use poly_bundle::inputs::{
    AndroidInputs, BundleInputs, HarmonyInputs, Icon, IconKind, IosInputs, NativeLib, ProductInfo,
};
use poly_bundle::packagers::ios::{apple_platform, swift};
use poly_core::bridge;
use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::events::{BuildEvent, EventSink};
use poly_core::job::{JobSpec, SizePolicy};
use poly_core::pipeline;
use poly_core::plan::{Stage, StageContext, StageError, StageOutcome};
use poly_targets::{Bridge, Host, TargetSpec, current_host};

use crate::project;

/// 派生图标目录约定（T-M1-14 落点）：`<root>/icons/<边长>x<边长>.png` / `icon.ico` / `icon.icns`。
const ICONS_DIR: &str = "icons";

/// 图标单一源（UI 相对路径）：`poly init` 把 `docs/assets/pax-mark.svg` 的副本落在这里。
const ICON_SVG: &str = "public/icon.svg";

/// 图标源回退（工程根相对路径）：UI 目录之外放图标时的落点。
const ICON_SVG_ROOT: &str = "icon.svg";

/// PWA manifest 候选文件名（T-M1-04 产物，web 打包器必需输入）。
const MANIFEST_NAMES: &[&str] = &["manifest.webmanifest", "manifest.json"];

/// Apple 交叉目标：`swift::package` 的 cargo `--target` 清单（缺一即造不出 XCFramework）。
///
/// T-M4-07：按目标平台从描述符派生（ios 设备 + 双模拟器 / tvos 设备 + 单模拟器）——
/// 不另持一份清单，杜绝「加了 triple 忘了前置检查」的漂移。
fn apple_triples(target_id: &str) -> Option<Vec<&'static str>> {
    let platform = apple_platform(target_id)?;
    Some(
        std::iter::once(platform.device_triple)
            .chain(platform.sim_triples.iter().copied())
            .collect(),
    )
}

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
        skip(
            ctx.events,
            spec,
            &out_dir,
            &format!(
                "无内建打包器（format = {}，矩阵 T-M1-05..08 未覆盖）",
                spec.package.format
            ),
        )?;
        return Ok(false);
    };
    let Some(inputs) = assemble_inputs(project, spec, host, &out_dir, ctx.events)? else {
        return Ok(false);
    };

    match packager.package(&inputs, spec) {
        Ok(artifact) => {
            ctx.events.emit(&BuildEvent::Packaged {
                format: artifact.format.clone(),
                bytes: artifact.bytes,
            });
            // 体积预算门禁（T-M3-08，§2 原则 4）：逐产物判定，`fail` 策略下 package 节点
            // 即失败（DAG 语义：sign / verify / publish 不再执行）。
            if let Some(verdict) = poly_core::budget::size_verdict(artifact.bytes, &ctx.job.budget)
            {
                ctx.events.emit(&verdict.event(&spec.id, &artifact.format));
                if verdict.exceeded && ctx.job.budget.policy == SizePolicy::Fail {
                    return Err(verdict.failure(&spec.id, &artifact.format).into());
                }
            }
            // 打包器如实标注的降级（如缺 appimagetool 时产出 AppDir）转发为告警
            for note in &artifact.notes {
                ctx.events.emit(&BuildEvent::Warning {
                    stage: "package".to_string(),
                    message: format!("{}：{note}", artifact.format),
                });
            }
            Ok(true)
        }
        Err(err) => environment_slot(ctx.events, spec, &out_dir, err).map(|slot| slot.is_some()),
    }
}

/// 组装一次打包的全部输入。
///
/// 抽出来是为了让 `poly verify --reproducible` 复用**同一份组装**：二跑的输入必须同源，
/// 否则比对的是两份不同的输入而不是同一次构建的确定性。
///
/// `Ok(None)` = 环境性缺失（宿主缺工具 / 输入未就绪）：已发告警并清掉暂存目录，调用方跳过该目标。
pub(crate) fn assemble_inputs(
    project: &poly_core::Project,
    spec: &TargetSpec,
    host: Host,
    out_dir: &Path,
    events: &mut dyn EventSink,
) -> Result<Option<BundleInputs>, StageError> {
    // 桌面（Tauri IPC）才需要 Tauri 裸产物、才「包内必须带图标」：Android / iOS 的原生产物
    // 由各自打包器从桥产物与 UI 产物组装，套 Tauri 布局只会得到缺输入、整目标被跳过
    let desktop = spec.bridge == Bridge::TauriIpc;

    // 图标（T-M1-14）：手工放置的 `<root>/icons/` 优先，否则从项目图标 svg 现场派生。
    // 桌面包内必须带图标（金样 `linux.artifact.json` 要求 hicolor 图标、macOS 要求 icns），
    // 两个来源都没有时不出半成品包，如实告警跳过。
    let manual = collect_icons(&project.root);
    // 派生一次要起 rsvg-convert，不为用不上的目标白跑
    let icon_svg = if manual.is_empty() && desktop {
        project_icon_svg(project)
    } else {
        None
    };
    if desktop && manual.is_empty() && icon_svg.is_none() {
        return skip(
            events,
            spec,
            out_dir,
            &format!(
                "无图标：既没有 {ICONS_DIR}/ 也没有 {ICON_SVG}（T-M1-14 派生源）——桌面包内必须带图标"
            ),
        );
    }

    // 确实要打包了才建暂存目录：跳过时不留空目录，免得快照里出现空的 packages/
    std::fs::remove_dir_all(out_dir).ok();
    // 与 dist 不同：包是整份替换，每次构建重写该目标的暂存目录，免得旧版安装包残留
    std::fs::create_dir_all(out_dir)
        .map_err(|err| project::io_error("创建打包暂存目录", out_dir, err))?;

    let mut inputs = BundleInputs::new(product_info(&project.config.package), out_dir);
    inputs.core_binary = pipeline::artifact_path(project)
        .ok()
        .filter(|path| path.is_file());
    let dist = project.root.join("dist");
    if dist.is_dir() {
        inputs.web_manifest = MANIFEST_NAMES
            .iter()
            .map(|name| dist.join(name))
            .find(|path| path.is_file());
        inputs.ui_dist = Some(dist.clone());
    }
    inputs.icons = match &icon_svg {
        // 派生 PNG 16..256 + ICO 落到暂存目录；缺 rsvg-convert 等由 environment_slot 决定跳过
        Some(svg) => match poly_bundle::icons::derive(svg, out_dir) {
            Ok(set) => {
                for note in &set.notes {
                    events.emit(&BuildEvent::Warning {
                        stage: "package".to_string(),
                        message: format!("图标派生：{note}"),
                    });
                }
                set.icons
            }
            Err(err) => return environment_slot(events, spec, out_dir, err),
        },
        None => manual,
    };

    if desktop {
        // 裸产物（T-M1-03）：核心可执行 + WebView 资源 + 平台元数据，桌面打包器的必需输入
        match poly_bundle::tauri::bare_product(&inputs, host) {
            Ok(bare) => inputs.bare = Some(bare),
            Err(err) => return environment_slot(events, spec, out_dir, err),
        }
    }

    match spec.package.format.as_str() {
        // 移动端打包器的 UI 回落链是 `android.assets` → `ui_dist`：两者都空会打出**没有界面**
        // 的包而不报错（见 poly-bundle android 的 `asset_entries`）。不静默，如实告警。
        // 不硬失败：android/ios 声明的是 react-native UI，`bundle` 阶段只认 react-dom，
        // 故「本次构建没有 dist/」是常态；硬错会把所有 RN-only 构建连同真跑契约门一起打红。
        "aab" | "apk" => {
            no_ui_warning(events, spec, &dist, &inputs);
            inputs.android = Some(android_inputs(project, out_dir, events)?);
        }
        // HarmonyOS（T-M3-03）：工程两态（复用 `<root>/harmony/` 模板实例 / 骨架生成）由打包器
        // 判定；CLI 只递宿主根与桥产物 `.so`。骨架缺 hvigorw 属环境性缺失 → 如实跳过。
        "hap" => {
            no_ui_warning(events, spec, &dist, &inputs);
            inputs.harmony = Some(harmony_inputs(project, events));
        }
        "ipa" => {
            no_ui_warning(events, spec, &dist, &inputs);
            inputs.ios = Some(ios_inputs(project)?);
            // XCFramework（T-M2-03）是 Apple 打包器（ios / tvos）的必需输入，而会造它的只有
            // `swift::package`（`poly bridge generate` 只产绑定）——缺则现建，否则打包器必
            // MissingInput 整目标跳过。
            if !swift::xcframework_path(&inputs).is_dir() {
                // 缺 std 交叉目标属工具链缺失：cargo 会在 swift::package 里硬失败，
                // 按 skip-if-missing 先如实跳过并点名要装的 triple（rustup 不在场则不拦）
                let installed = rustup_targets();
                if let Some(missing) = first_missing_apple_triple(installed.as_deref(), &spec.id) {
                    return skip(
                        events,
                        spec,
                        out_dir,
                        &format!(
                            "缺交叉目标 {missing}（rustup target add {missing}）——XCFramework 无从构建"
                        ),
                    );
                }
                match swift::package(&inputs, spec) {
                    Ok(artifact) => events.emit(&BuildEvent::Packaged {
                        format: artifact.format,
                        bytes: artifact.bytes,
                    }),
                    Err(err) => return environment_slot(events, spec, out_dir, err),
                }
            }
        }
        // 小程序族（T-M3-06；快应用 T-M3-12 起 format = `rpk`）：宿主工程根交打包器走候选链解析
        // （<root>/ui → <root> → 包内 host/），Taro 编译与代码包组装（快应用含 hap build）都在
        // 打包器内（编译腿缺席由 environment_slot 按环境性缺失跳过）；wasm 沙箱出口（T-M3-07）
        // 走 `poly bridge wasm` 缺省落点，在场即随代码包分发
        "weapp" | "alipay" | "tt" | "rpk" => {
            inputs.miniapp_host = Some(project.root.clone());
            inputs.miniapp_bridge = Some(project.root.join("bridge").join("wasm-bindgen"));
        }
        // web 系 TV 四目标（T-M4-08/09/10）：载荷源 = 工程根 `dist/`（上方统一接线），
        // wasm 沙箱出口（T-M3-07）走 `poly bridge wasm` 缺省落点，在场即随包分发
        "wgt" | "ipk" | "web-app" => {
            inputs.miniapp_bridge = Some(project.root.join("bridge").join("wasm-bindgen"));
        }
        _ => {}
    }
    Ok(Some(inputs))
}

/// 移动端无 UI 载荷（`ui_dist` 与 `android.assets` 均缺）时如实告警——产物能构建、但没有界面。
fn no_ui_warning(
    events: &mut dyn EventSink,
    spec: &TargetSpec,
    dist: &Path,
    inputs: &BundleInputs,
) {
    let has_assets = inputs
        .android
        .as_ref()
        .is_some_and(|android| android.assets.is_some());
    if inputs.ui_dist.is_some() || has_assets {
        return;
    }
    events.emit(&BuildEvent::Warning {
        stage: "package".to_string(),
        message: format!(
            "目标 {}：无 UI 载荷（{} 不存在）——产物不含界面资源（react-native 侧 UI 打包归 M2 的 RN 支线）",
            spec.id,
            dist.display()
        ),
    });
}

/// Android 打包输入：桥产物 AAR（有则解包取 `jni/<abi>/*.so` 与 `classes.jar`）。
///
/// 其余字段留空——manifest / res / assets 由打包器按产品元数据 + 图标 + `ui_dist` 派生，
/// CLI 不重复造一遍。没有 AAR 时给最小输入：打包器仍能出包（只是不含原生库）。
fn android_inputs(
    project: &poly_core::Project,
    out_dir: &Path,
    events: &mut dyn EventSink,
) -> Result<AndroidInputs, StageError> {
    let mut inputs = AndroidInputs::default();
    let Some(aar) = bridge_aar(project) else {
        return Ok(inputs);
    };
    let unpacked = bridge::aar::unpack(&aar, out_dir)?;
    inputs.native_libs = unpacked
        .libs
        .into_iter()
        .map(|(abi, path)| NativeLib {
            abi: abi.dir.to_string(),
            path,
        })
        .collect();
    inputs.dex_inputs = unpacked.classes_jar.into_iter().collect();
    events.emit(&BuildEvent::Warning {
        stage: "package".to_string(),
        message: format!(
            "Android 输入：{}（{} 个 ABI，classes.jar {}）",
            aar.display(),
            inputs.native_libs.len(),
            if inputs.dex_inputs.is_empty() {
                "缺——无 Java 侧时产物无 dex 属正常"
            } else {
                "随包"
            }
        ),
    });
    Ok(inputs)
}

/// HarmonyOS 打包输入：桥产物 `.so`（`<根>/bridge/harmonyos/<abi>/*.so`）+ 宿主工程根。
///
/// 工程两态由打包器按 `<根>/harmony/build-profile.json5` 判定；CLI 只递路径不代造工程
/// （骨架的 AppScope 元数据由打包器从 polypack.toml 生成）。没有桥产物时给最小输入：
/// 打包器仍能出包（纯 ArkTS / 资源产物；缺原生库的注记由产物说明承载）。
fn harmony_inputs(project: &poly_core::Project, events: &mut dyn EventSink) -> HarmonyInputs {
    let mut inputs = HarmonyInputs {
        host_project: Some(project.root.clone()),
        ..HarmonyInputs::default()
    };
    let dir = project.root.join("bridge").join("harmonyos");
    let mut libs: Vec<NativeLib> = Vec::new();
    if let Ok(abis) = std::fs::read_dir(&dir) {
        for abi_entry in abis.flatten().filter(|entry| entry.path().is_dir()) {
            let abi = abi_entry.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(abi_entry.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_some_and(|ext| ext == "so") {
                    libs.push(NativeLib {
                        abi: abi.clone(),
                        path,
                    });
                }
            }
        }
    }
    // 目录序无保证：按（ABI，路径）排序定序（note 与产物注入顺序可复现）。
    libs.sort_by(|a, b| (&a.abi, &a.path).cmp(&(&b.abi, &b.path)));
    if !libs.is_empty() {
        events.emit(&BuildEvent::Warning {
            stage: "package".to_string(),
            message: format!("HarmonyOS 输入：bridge/harmonyos/（{} 枚 .so）", libs.len()),
        });
    }
    inputs.native_libs = libs;
    inputs
}

/// 桥产出的 AAR（`<根>/bridge/android/*.aar`；多个取排序首个）。
fn bridge_aar(project: &poly_core::Project) -> Option<PathBuf> {
    let dir = project.root.join("bridge").join("android");
    let mut aars: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "aar"))
        .collect();
    aars.sort();
    aars.into_iter().next()
}

/// iOS 打包输入：核心 crate + 静态库名 + Swift 绑定目录（XCFramework 切片的构建输入）。
fn ios_inputs(project: &poly_core::Project) -> Result<IosInputs, StageError> {
    let core_crate = project.root.join(&project.config.core.crate_path);
    Ok(IosInputs {
        lib_name: Some(poly_core::compile::read_crate_name(&core_crate)?.replace('-', "_")),
        core_crate: Some(core_crate),
        host_project: Some(project.root.clone()),
        // T-M2-01 生成器的 Swift 落点，与 poly-bundle 的 bridge_dir 约定同源
        bindings_dir: Some(project.root.join("bridge").join("swift")),
    })
}

/// 环境性缺失（宿主工具 / 输入未就绪）→ 告警跳过（`None`）；其余（配置非法、命令失败）→ 硬失败。
fn environment_slot(
    events: &mut dyn EventSink,
    spec: &TargetSpec,
    out_dir: &Path,
    err: BundleError,
) -> Result<Option<BundleInputs>, StageError> {
    match err {
        BundleError::ToolMissing { .. } | BundleError::MissingInput { .. } => {
            skip(events, spec, out_dir, &err.to_string())
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
fn skip(
    events: &mut dyn EventSink,
    spec: &TargetSpec,
    out_dir: &Path,
    reason: &str,
) -> Result<Option<BundleInputs>, StageError> {
    std::fs::remove_dir_all(out_dir).ok();
    events.emit(&BuildEvent::Warning {
        stage: "package".to_string(),
        message: format!("目标 {} 未打包：{reason}", spec.id),
    });
    Ok(None)
}

/// rustup 报告的已装目标；rustup 不在场或调用失败 → `None`（不拦：cargo 会自己报缺 triple）。
fn rustup_targets() -> Option<Vec<String>> {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
    )
}

/// 已装清单里缺的第一个 Apple 交叉目标（清单按目标平台派生：ios 三 / tvos 二）；
/// `None`（rustup 不在场/调用失败）→ `None`（放行给 cargo）。
///
/// 清单为空串与「rustup 不可用」必须分开：前者是真没装任何目标（点名要装的 triple），
/// 后者只是查不到——查不到就拦会把非 rustup 工具链的机器误判成缺目标。
fn first_missing_apple_triple(
    installed: Option<&[String]>,
    target_id: &str,
) -> Option<&'static str> {
    let installed = installed?;
    apple_triples(target_id)?
        .into_iter()
        .find(|triple| !installed.iter().any(|have| have.as_str() == *triple))
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
pub(crate) fn product_info(section: &poly_core::config::PackageSection) -> ProductInfo {
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
    use poly_core::events::HumanSink;

    /// 最小工程配置（`bridge.rs` 集成测试同源文本，保持单一口径）。
    const FIXTURE_TOML: &str = "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n";

    /// 建最小工程骨架（核心 crate 名带 `-`，用于验证下划线化）；返回临时根目录。
    fn fixture(tag: &str) -> (PathBuf, poly_core::Project) {
        let root = std::env::temp_dir().join(format!("polypack-pkg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("core/src")).expect("建核心 crate");
        std::fs::write(
            root.join("core/Cargo.toml"),
            "[package]\nname = \"pax-core\"\nversion = \"0.0.0\"\n",
        )
        .expect("写核心 Cargo.toml");
        let config =
            poly_core::config::PolypackConfig::parse(FIXTURE_TOML).expect("fixture 配置应合法");
        let project = poly_core::Project {
            root: root.clone(),
            config,
        };
        (root, project)
    }

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

    /// Android 不是 Tauri 目标：无图标、无裸产物也必须组装出输入，否则整目标被跳过
    /// （T-M2-06「直出 AAB/APK」就不成立）。这条同时钉住 `desktop` 分支的作用域。
    #[test]
    fn android_target_assembles_without_tauri_bare_product() {
        let (root, project) = fixture("android-slot");
        let registry = poly_targets::Registry::builtin();
        let spec = registry.get("android").expect("内建 android 目标");
        let out_dir = project::packages_root(&root).join("android");
        let mut sink = HumanSink { out: Vec::new() };

        let slot = assemble_inputs(&project, spec, Host::Linux, &out_dir, &mut sink)
            .expect("无图标不是错误");
        let inputs = slot.expect("Android 目标不该被跳过");
        assert!(inputs.bare.is_none(), "Android 不套 Tauri 裸产物");
        assert!(inputs.android.is_some(), "应填好 Android 输入");
        assert!(out_dir.is_dir(), "真要打包才建暂存目录");
        // 无 dist/ 时产物不含界面：必须如实告警，不许静默出空包
        let notes = String::from_utf8_lossy(&sink.out);
        assert!(notes.contains("无 UI 载荷"), "缺 UI 载荷应告警: {notes}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 桌面（TauriIpc）无图标即跳过，且跳过时不留空暂存目录（快照里不该出现空 packages/）。
    #[test]
    fn desktop_target_without_icons_is_skipped_cleanly() {
        let (root, project) = fixture("desktop-skip");
        let registry = poly_targets::Registry::builtin();
        let spec = registry.get("linux").expect("内建 linux 目标");
        let out_dir = project::packages_root(&root).join("linux");
        let mut sink = HumanSink { out: Vec::new() };

        let slot = assemble_inputs(&project, spec, Host::Linux, &out_dir, &mut sink)
            .expect("环境性缺失不是错误");
        assert!(slot.is_none(), "无图标应跳过");
        assert!(!out_dir.exists(), "跳过时不留暂存目录");
        let notes = String::from_utf8_lossy(&sink.out);
        assert!(notes.contains("无图标"), "应告警原因: {notes}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 桥产物 AAR → `native_libs`（ABI 用目录名、按 ABIS 表序）与 `dex_inputs`。
    #[test]
    fn android_inputs_take_libs_and_dex_from_bridge_aar() {
        let (root, project) = fixture("android-aar");
        let out_dir = project::packages_root(&root).join("android");
        // 用生产组装器造 AAR（不手搓 zip）：两个 ABI 的假 .so + 假 classes.jar
        let libs_dir = root.join("libs");
        std::fs::create_dir_all(&libs_dir).expect("建库目录");
        let mut libs = Vec::new();
        for abi in [bridge::aar::ABIS[2], bridge::aar::ABIS[0]] {
            let path = libs_dir.join(format!("libpax_core-{}.so", abi.dir));
            std::fs::write(&path, b"\x7fELF").expect("写假 .so");
            libs.push((abi, path));
        }
        let jar = libs_dir.join("pax_core.classes.jar");
        std::fs::write(&jar, b"PK").expect("写假 classes.jar");
        let aar = root.join("bridge/android/pax_core.aar");
        bridge::aar::assemble(
            &aar,
            &bridge::aar::manifest_xml(
                "com.polypack.fixture",
                bridge::aar::DEFAULT_MIN_SDK,
                "0.0.1",
            ),
            &libs,
            Some(&jar),
            &libs_dir,
        )
        .expect("组装 AAR");

        let mut sink = HumanSink { out: Vec::new() };
        let inputs = android_inputs(&project, &out_dir, &mut sink).expect("解包 AAR");

        assert_eq!(
            inputs
                .native_libs
                .iter()
                .map(|lib| lib.abi.as_str())
                .collect::<Vec<_>>(),
            ["arm64-v8a", "x86_64"],
            "ABI 用 Android 目录名，且按 ABIS 表序（与写入顺序无关）"
        );
        for lib in &inputs.native_libs {
            assert!(lib.path.is_file(), "{:?} 应已解包落地", lib.path);
            assert!(
                lib.path.starts_with(out_dir.join("jniLibs")),
                "{:?} 应在 jniLibs 下",
                lib.path
            );
        }
        assert_eq!(
            inputs.dex_inputs,
            vec![out_dir.join("pax_core.classes.jar")]
        );
        assert_eq!(inputs.min_sdk, 24, "缺省 minSdk 与打包器同口径");
        let notes = String::from_utf8_lossy(&sink.out);
        assert!(notes.contains("Android 输入"), "应发一条输入告警: {notes}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 无桥产物（纯 UI 工程）给最小输入：打包器仍能出包，只是不含原生库 / dex。
    #[test]
    fn android_inputs_without_bridge_aar_are_minimal() {
        let (root, project) = fixture("android-min");
        let out_dir = project::packages_root(&root).join("android");
        let mut sink = HumanSink { out: Vec::new() };

        let inputs = android_inputs(&project, &out_dir, &mut sink).expect("无 AAR 不是错误");
        assert!(inputs.native_libs.is_empty());
        assert!(inputs.dex_inputs.is_empty());
        assert_eq!(inputs.min_sdk, 24);
        assert!(sink.out.is_empty(), "无 AAR 时不必告警");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 桥产物 `.so` → `native_libs`（ABI 用目录名；缺桥产物时给最小输入）。
    #[test]
    fn harmony_inputs_take_libs_from_bridge_dir() {
        let (root, project) = fixture("harmony-libs");
        let lib = root.join("bridge/harmonyos/arm64-v8a/libpax_core.so");
        std::fs::create_dir_all(lib.parent().expect("父目录")).expect("建桥产物目录");
        std::fs::write(&lib, b"\x7fELF").expect("写假 .so");
        // 非 .so 文件与目录外的文件不入列。
        std::fs::write(lib.parent().unwrap().join("notes.txt"), b"x").expect("写旁路文件");

        let mut sink = HumanSink { out: Vec::new() };
        let inputs = harmony_inputs(&project, &mut sink);
        assert_eq!(
            inputs.host_project,
            Some(root.clone()),
            "工程根须指向宿主根"
        );
        assert_eq!(inputs.native_libs.len(), 1, "{:?}", inputs.native_libs);
        assert_eq!(inputs.native_libs[0].abi, "arm64-v8a");
        assert_eq!(inputs.native_libs[0].path, lib);
        let notes = String::from_utf8_lossy(&sink.out);
        assert!(
            notes.contains("HarmonyOS 输入"),
            "应发一条输入告警: {notes}"
        );

        // 无桥产物：最小输入且不告警。
        let (root2, project2) = fixture("harmony-empty");
        let mut sink2 = HumanSink { out: Vec::new() };
        let inputs2 = harmony_inputs(&project2, &mut sink2);
        assert!(inputs2.native_libs.is_empty());
        assert!(sink2.out.is_empty(), "无 .so 时不必告警");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    /// iOS 输入：`lib<name>.a` 用 crate 名的下划线形态，绑定落点与 poly-bundle 约定同源。
    #[test]
    fn ios_inputs_carry_crate_lib_name_and_bindings_dir() {
        let (root, project) = fixture("ios");
        let inputs = ios_inputs(&project).expect("iOS 输入");

        assert_eq!(
            inputs.lib_name.as_deref(),
            Some("pax_core"),
            "`-` 应转下划线（cargo 库文件名形态）"
        );
        assert_eq!(inputs.core_crate, Some(root.join("core")));
        assert_eq!(inputs.host_project, Some(root.clone()));
        assert_eq!(inputs.bindings_dir, Some(root.join("bridge/swift")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 缺哪个交叉目标由 rustup 清单点名（skip 原因可直接拷进终端）；查不到清单不拦；
    /// 清单按目标平台取（T-M4-07：tvos 只两条 target）。
    #[test]
    fn missing_apple_triple_is_named_from_installed_list() {
        let all: Vec<String> = apple_triples("ios")
            .unwrap()
            .iter()
            .map(|t| t.to_string())
            .collect();
        assert_eq!(first_missing_apple_triple(Some(&all), "ios"), None);
        let without_sim_arm64: Vec<String> = ["aarch64-apple-ios", "x86_64-apple-ios"]
            .map(str::to_string)
            .to_vec();
        assert_eq!(
            first_missing_apple_triple(Some(&without_sim_arm64), "ios"),
            Some(swift::SIM_ARM64_TRIPLE)
        );
        assert_eq!(
            first_missing_apple_triple(Some(&[]), "ios"),
            Some(swift::DEVICE_TRIPLE),
            "清单在但空 = 真没装：点名要装的 triple"
        );
        assert_eq!(
            first_missing_apple_triple(None, "ios"),
            None,
            "rustup 不在场 → 放行，由 cargo 报真错"
        );

        // tvOS：清单只有设备 + 单模拟器两条；缺模拟器立即点名
        let tvos_all: Vec<String> = apple_triples("tvos")
            .unwrap()
            .iter()
            .map(|t| t.to_string())
            .collect();
        assert_eq!(tvos_all.len(), 2, "tvOS 交叉目标两条");
        assert_eq!(first_missing_apple_triple(Some(&tvos_all), "tvos"), None);
        let tvos_device_only: Vec<String> = vec!["aarch64-apple-tvos".to_string()];
        assert_eq!(
            first_missing_apple_triple(Some(&tvos_device_only), "tvos"),
            Some("aarch64-apple-tvos-sim")
        );
        assert_eq!(
            first_missing_apple_triple(Some(&tvos_device_only), "ios"),
            Some(swift::DEVICE_TRIPLE),
            "同一份清单按目标切换期望（ios 缺设备 triple 先点名）"
        );
    }

    /// 不变式：前置校验清单须与 `swift::package` 真跑的 cargo `--target` 同源——
    /// 漏一个新 triple 会让缺工具链的机器绕过跳过、直接硬失败。ios / tvos 两目标同断言。
    #[test]
    fn apple_triple_precheck_matches_swift_chain() {
        let (root, project) = fixture("ios-triples");
        let mut inputs = BundleInputs::new(product_info(&project.config.package), root.clone());
        inputs.ios = Some(ios_inputs(&project).expect("iOS 输入"));

        for target_id in ["ios", "tvos"] {
            let mut spec = TargetSpec::new_stub(target_id, 1, &[Host::Macos]);
            spec.package.format = "ipa".to_string();
            let plan = swift::plan(&inputs, &spec).expect("纯组装，任意宿主可跑");
            let mut built: Vec<&str> = plan
                .commands
                .iter()
                .filter(|command| command.program == "cargo")
                .flat_map(|command| command.args.windows(2))
                .filter(|pair| pair[0] == "--target")
                .map(|pair| pair[1].as_str())
                .collect();
            built.sort_unstable();
            let mut expected = apple_triples(target_id).expect("Apple 平台描述符");
            expected.sort_unstable();
            assert_eq!(
                built, expected,
                "{target_id} 前置校验清单须覆盖 swift 链的全部交叉目标"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
