//! Fitbit OS 应用包打包器（T-M7-07）：`.fba` = 应用清单 + `device-<family>.zip`
//! （+ 可选 `companion.zip`），结构与官方 SDK 6.1 的 `appPackageManifest` /
//! `componentManifest` 同形。核心不直接运行（`runtime = none`）：包内是平台 JS SDK 的
//! `app/index.js` 与资源，能力由 JS 侧实现；本项目不做 TS / rollup 编译，`app/index.js`
//! 按工程原样入包。
//!
//! 输入口径：`BundleInputs.miniapp_host` 下的 `fitbit/`（或宿主根自身）须是 Fitbit SDK
//! 工程（`package.json` 的 `config.fitbit` + `app/index.js`）。`fitbit-build` 是**校验腿**：
//! verify 阶段在场时自检、缺席 SKIP-with-print；打包产物与检查器不依赖官方 CLI。
//!
//! 确定性：buildId 由 appUUID + 版本 + 名称派生，`bundleDate` 固定 1980-01-01（官方 CLI
//! 用当前时间）——同输入二跑字节一致，金样可复现。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp;
use crate::zipwrite;

mod check;

pub use check::{FitbitStructure, check_fitbit_structure};

/// 目标 id / 产物格式（与 `builtin.rs` 的 `fitbit` 声明逐字一致）。
const ID: &str = "fitbit";
const FORMAT: &str = "fba";

/// 打包暂存目录（`<out_dir>/fitbit/`）。
const WORK_DIR: &str = "fitbit";

/// Fitbit SDK 6.1.0 的 API 版本映射（`lib/sdkVersion.js`：6.1 → device 8.2.0 / companion 3.3.0）。
const DEVICE_API: &str = "8.2.0";
const COMPANION_API: &str = "3.3.0";

/// 设备平台串（官方 buildTargets.js：atlas / vulcan 均为 `128.1.1+`）。
const PLATFORM: &str = "128.1.1+";

/// 内建设备家族（官方 buildTargets.js；未知家族硬失败，绝不发明设备 id）。
const FAMILIES: [&str; 2] = ["atlas", "vulcan"];

/// 确定性构建时间（zip 纪元起点；官方 CLI 用当前时间）。
const BUNDLE_DATE: &str = "1980-01-01T00:00:00.000Z";

/// Fitbit 应用包打包器。
pub struct Fitbit;

impl Packager for Fitbit {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_plan(inputs, spec)?;
        Ok(BundlePlan::new())
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        check_plan(inputs, spec)?;
        let project = project_root(inputs)?;
        let config = FitbitConfig::load(&project, inputs)?;
        let has_companion = project.join("companion/index.js").is_file();
        let work = inputs.out_dir.join(WORK_DIR);
        let _ = std::fs::remove_dir_all(&work);
        let package_dir = work.join("package");
        std::fs::create_dir_all(&package_dir).map_err(io_err(&package_dir))?;
        let build_id = build_id(
            &config.app_uuid,
            &inputs.product.version,
            &config.display_name,
        );

        for family in &config.build_targets {
            let tree = work.join(format!("tree-{family}"));
            fsutil::copy_tree(&project.join("app"), &tree.join("app"))?;
            fsutil::copy_tree(&project.join("resources"), &tree.join("resources"))?;
            fsutil::write_text(
                &tree.join("manifest.json"),
                &device_manifest(&config, &build_id),
            )?;
            zipwrite::write_zip(
                &webapp::collect_files(&tree)?,
                &package_dir.join(format!("device-{family}.zip")),
            )?;
        }
        if has_companion {
            let tree = work.join("tree-companion");
            fsutil::copy_tree(&project.join("companion"), &tree.join("companion"))?;
            fsutil::write_text(
                &tree.join("manifest.json"),
                &companion_manifest(&config, &build_id),
            )?;
            zipwrite::write_zip(
                &webapp::collect_files(&tree)?,
                &package_dir.join("companion.zip"),
            )?;
        }
        fsutil::write_text(
            &package_dir.join("manifest.json"),
            &app_manifest(&config, &build_id, has_companion),
        )?;

        let package = inputs.out_dir.join(format!(
            "{}-{}.fba",
            inputs.product.kebab_name(),
            inputs.product.version
        ));
        zipwrite::write_zip(&webapp::collect_files(&package_dir)?, &package)?;
        let structure = check_fitbit_structure(&package)?;

        let notes = vec![
            format!(
                "产物：{}（{} 条目；应用清单 + {} 个 device-<family>.zip{}）",
                package.display(),
                structure.entries.len(),
                structure.families.len(),
                if structure.companion {
                    " + companion.zip"
                } else {
                    ""
                }
            ),
            format!(
                "应用清单：appId={} buildId={} deviceApi={}（官方 SDK 6.1 同形）",
                structure.app_id, structure.build_id, DEVICE_API
            ),
            format!(
                "设备家族：{}（平台 {}；入口 {}）",
                structure.families.join(", "),
                PLATFORM,
                structure.device_main
            ),
            "核心不直接运行、能力由 JS 侧实现：runtime = none，包内为平台 JS SDK 的应用与资源；\
             app/index.js 按工程原样入包（官方 fitbit-build 的 TS / rollup 编译腿不在本项目）"
                .to_string(),
            "fitbit-build 自检归 verify 腿：工具缺席 SKIP-with-print，内建结构断言真跑；\
             签名 / 提审归 Fitbit Gallery 平台侧"
                .to_string(),
        ];
        fsutil::file_artifact(ID, FORMAT, &package, notes)
    }
}

/// 打包计划校验（打包与 plan 共用）。
fn check_plan(inputs: &BundleInputs, spec: &TargetSpec) -> Result<(), BundleError> {
    check_format(spec)?;
    project_root(inputs)?;
    inputs.product.validate(ID)
}

/// Fitbit SDK 工程根：`<miniapp_host>/fitbit` 优先，否则宿主根自身（须有 package.json + app/index.js）。
fn project_root(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    let host = inputs
        .miniapp_host
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: ID,
            field: "miniapp_host",
        })?;
    let nested = host.join("fitbit");
    let root = if is_project(&nested) {
        nested
    } else if is_project(host) {
        host.to_path_buf()
    } else {
        return Err(BundleError::InvalidInput {
            packager: ID,
            field: "fitbit.project",
            value: host.display().to_string(),
            reason: "缺 Fitbit SDK 工程（package.json 的 config.fitbit + app/index.js）；\
                     放 <工程根>/fitbit 或直接放工程根"
                .to_string(),
        });
    };
    Ok(root)
}

fn is_project(dir: &Path) -> bool {
    dir.join("package.json").is_file() && dir.join("app/index.js").is_file()
}

/// `package.json` 的 `config.fitbit`（官方 ProjectConfiguration 的字段子集）。
struct FitbitConfig {
    app_uuid: String,
    display_name: String,
    app_type: String,
    icon_file: String,
    wipe_color: String,
    requested_permissions: Vec<String>,
    build_targets: Vec<String>,
    default_language: String,
}

impl FitbitConfig {
    fn load(project: &Path, inputs: &BundleInputs) -> Result<Self, BundleError> {
        let path = project.join("package.json");
        let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        let root: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| bad("package.json", &path, format!("非法 JSON：{err}")))?;
        let config = root.pointer("/config/fitbit").ok_or_else(|| {
            bad(
                "package.json",
                &path,
                "缺 config.fitbit（官方 SDK 工程配置面）".to_string(),
            )
        })?;
        let app_uuid = config
            .get("appUUID")
            .and_then(serde_json::Value::as_str)
            .filter(|value| uuid_is_valid(value))
            .ok_or_else(|| {
                bad(
                    "config.fitbit.appUUID",
                    &path,
                    "须为合法 UUID（npx fitbit-build generate-appid 生成）".to_string(),
                )
            })?
            .to_string();
        let display_name = config
            .get("appDisplayName")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&inputs.product.name)
            .to_string();
        let icon_file = config
            .get("iconFile")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("resources/icon.png")
            .to_string();
        if !project.join(&icon_file).is_file() {
            return Err(bad(
                "config.fitbit.iconFile",
                &project.join(&icon_file),
                "图标文件不在工程内".to_string(),
            ));
        }
        let mut build_targets: Vec<String> = match config.get("buildTargets") {
            None => FAMILIES
                .iter()
                .map(|family| (*family).to_string())
                .collect(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| {
                    bad(
                        "config.fitbit.buildTargets",
                        &path,
                        "须为字符串数组".to_string(),
                    )
                })?
                .iter()
                .map(|item| item.as_str().unwrap_or_default().to_string())
                .collect(),
        };
        if build_targets.is_empty() {
            return Err(bad(
                "config.fitbit.buildTargets",
                &path,
                "至少启用一个设备家族（atlas / vulcan）".to_string(),
            ));
        }
        for family in &build_targets {
            if !FAMILIES.contains(&family.as_str()) {
                return Err(bad(
                    "config.fitbit.buildTargets",
                    &path,
                    format!("未知家族 {family}（已知：{}）", FAMILIES.join(", ")),
                ));
            }
        }
        build_targets.sort();
        let requested_permissions = config
            .get("requestedPermissions")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            app_uuid,
            display_name,
            app_type: config
                .get("appType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("app")
                .to_string(),
            icon_file,
            wipe_color: config
                .get("wipeColor")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("#ffffff")
                .to_string(),
            requested_permissions,
            build_targets,
            default_language: config
                .get("defaultLanguage")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("en-US")
                .to_string(),
        })
    }
}

/// 应用包清单（`app.fba` 根；官方 appPackageManifest.js 同形）。
fn app_manifest(config: &FitbitConfig, build_id: &str, has_companion: bool) -> String {
    let watch: serde_json::Map<String, serde_json::Value> = config
        .build_targets
        .iter()
        .map(|family| {
            (
                family.clone(),
                serde_json::json!({
                    "platform": [PLATFORM],
                    "filename": format!("device-{family}.zip"),
                }),
            )
        })
        .collect();
    let mut components = serde_json::Map::new();
    components.insert("watch".to_string(), serde_json::Value::Object(watch));
    if has_companion {
        components.insert(
            "companion".to_string(),
            serde_json::json!({ "filename": "companion.zip" }),
        );
    }
    let mut sdk_version = serde_json::json!({ "deviceApi": DEVICE_API });
    if has_companion {
        sdk_version["companionApi"] = serde_json::json!(COMPANION_API);
    }
    pretty(serde_json::json!({
        "buildId": build_id,
        "components": components,
        "sourceMaps": {},
        "manifestVersion": 6,
        "sdkVersion": sdk_version,
        "requestedPermissions": config.requested_permissions.clone(),
        "appId": config.app_uuid.clone(),
    }))
}

/// 设备组件清单（`device-<family>.zip` 根；官方 componentManifest.js 同形）。
fn device_manifest(config: &FitbitConfig, build_id: &str) -> String {
    let mut i18n = serde_json::Map::new();
    i18n.insert(config.default_language.clone(), serde_json::json!({}));
    pretty(serde_json::json!({
        "apiVersion": DEVICE_API,
        "buildId": build_id,
        "bundleDate": BUNDLE_DATE,
        "uuid": config.app_uuid.clone(),
        "name": config.display_name.clone(),
        "requestedPermissions": config.requested_permissions.clone(),
        "i18n": i18n,
        "appManifestVersion": 1,
        "main": "app/index.js",
        "appType": config.app_type.clone(),
        "iconFile": config.icon_file.clone(),
        "wipeColor": config.wipe_color.clone(),
    }))
}

/// 伴生组件清单（`companion.zip` 根；官方 makeCompanionManifest 同形）。
fn companion_manifest(config: &FitbitConfig, build_id: &str) -> String {
    pretty(serde_json::json!({
        "manifestVersion": 2,
        "companion": { "main": "companion/index.js" },
        "apiVersion": COMPANION_API,
        "buildId": build_id,
        "bundleDate": BUNDLE_DATE,
        "uuid": config.app_uuid.clone(),
        "name": config.display_name.clone(),
        "requestedPermissions": config.requested_permissions.clone(),
    }))
}

fn pretty(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value)
        .map(|text| format!("{text}\n"))
        .unwrap_or_else(|_| value.to_string())
}

fn build_id(app_uuid: &str, version: &str, name: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{app_uuid}\0{version}\0{name}").as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("0x0{}", &hex[..15])
}

fn uuid_is_valid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8usize, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

/// 打包格式与打包器声明必须一致。
fn check_format(spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format == FORMAT {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{FORMAT}`"),
    })
}

fn bad(field: &'static str, path: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field,
        value: path.display().to_string(),
        reason,
    }
}
