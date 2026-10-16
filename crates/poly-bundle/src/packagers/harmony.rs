//! HarmonyOS 打包器（T-M3-03）：工程配置生成 + `hvigorw assembleHap` 调用 + 未签名 HAP 收集。
//!
//! 工程来源两态（判定同 iOS 的复用 / 骨架）：
//! - **复用**：`<工程根>/harmony/build-profile.json5`（T-M3-01 宿主模板实例）存在即用；应用
//!   元数据（bundleName / versionCode / label）仍由 `polypack.toml` 生成 `AppScope/app.json5`，
//!   与 Android 由产品元数据生成清单同口径。
//! - **骨架**：缺失时在 `<工程根>/harmony/` 生成最小工程（与探针实证可构建的 17 文件工程同构；
//!   纯 ArkTS 占位页、无 ohpm 依赖，不需要 `ohpm install`）——`poly build --target harmonyos`
//!   开箱直出 HAP。
//!
//! 工程落点恒在 out_dir **之外**：CLI 的 package 阶段每次构建清空 out_dir，而 hvigor 增量缓存
//! （`entry/build/` 与 `.hvigor/`）必须跨构建留存——这是验收「二跑缓存命中」的硬条件。
//! 脚本落盘走 `write_text_if_changed`：同内容不重写，免得 mtime 抖动白白击穿缓存。
//!
//! 产物收集：hvigor 的 `entry-default-unsigned.hap` 复制为 `<kebab>-<version>-unsigned.hap`
//! （命名对齐 Android 未签名件，供签名阶段按同一约定查找），并做 HAP 结构检查
//! （[`check_hap_structure`]，同时供 `poly verify` 的 `hap-structure-check` 校验器复用）。
//! 签名不在本任务范围内（T-M3-04 接入），产物一律是**未签名**件。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, HarmonyInputs, IconKind, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager, ScriptFile};
use crate::packagers::android::derive_version_code;
use crate::tools::{self, HarmonyTools};
use crate::zipwrite;

/// HAP 打包器（`TargetSpec.package.format = "hap"`，builder `harmonyos-hvigor`）。
pub struct HarmonyHap;

const HARMONY: &str = "harmonyos";
const HAP: &str = "hap";
/// 宿主工程标志文件（两态判定：`<工程根>/harmony/` 或工程根自身含它即复用）。
pub const PROJECT_MARKER: &str = "build-profile.json5";
/// hvigor 未签名产物的工程内相对路径（模块 `entry`、产品 `default`、debug 模式）。
const HVIGOR_OUTPUT: &str = "entry/build/default/outputs/default/entry-default-unsigned.hap";
/// HAP 结构检查必须命中的条目：清单（module.json）/ 资源索引 / 打包元信息。
pub const HAP_REQUIRED_ENTRIES: &[&str] = &["module.json", "resources.index", "pack.info"];
/// SDK 版本读取失败时回退的 `compatibleSdkVersion` 平台段 / API 段（本机 command-line-tools
/// 5.1.1(19) 实证；读到 `sdk-pkg.json` 时用真实值）。
const DEFAULT_SDK_PLATFORM: &str = "5.1.1";
const DEFAULT_SDK_API: &str = "19";
/// 1×1 透明 PNG（70B）：骨架工程的图标占位（工程无图标输入时；hvigor 资源编译要求文件存在且是合法 PNG）。
const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

impl Packager for HarmonyHap {
    fn id(&self) -> &'static str {
        HARMONY
    }

    fn format(&self) -> &'static str {
        HAP
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, HAP)?;
        // 生成的工程配置直接内插产品元数据：先过白名单校验（无 JSON / 路径元字符）。
        inputs.product.validate(HAP)?;
        let harmony = harmony_inputs(inputs);
        let tools = tools::resolve_harmony(&harmony)?;
        let stage = stage(inputs, &harmony, &tools);

        let mut plan = BundlePlan::new();
        for script in metadata_scripts(inputs, &stage) {
            plan = plan.with_script(script.path, script.contents);
        }
        if stage.skeleton {
            for script in skeleton_scripts(inputs, &stage) {
                plan = plan.with_script(script.path, script.contents);
            }
        } else if !stage.project.join("oh_modules").is_dir() {
            // 复用工程（模板实例）带 ohpm 依赖（RNOH 的 har）：先装依赖，否则 hvigor 解析失败。
            let ohpm = tools.ohpm.clone().ok_or_else(missing_ohpm)?;
            plan = plan
                .with_command(Command::new(path_arg(&ohpm), ["install"]).in_dir(&stage.project));
        }
        // `--no-daemon`：构建走一次性进程（CI 与本地同形态，不留后台 daemon）。
        plan = plan.with_command(
            Command::new(path_arg(&tools.hvigorw), ["assembleHap", "--no-daemon"])
                .in_dir(&stage.project),
        );
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let harmony = harmony_inputs(inputs);
        let tools = tools::resolve_harmony(&harmony)?;
        let stage = stage(inputs, &harmony, &tools);

        // 落盘：同内容不重写（保 mtime → hvigor 二跑命中缓存）；图标 PNG 是二进制，走拷贝。
        for script in &plan.scripts {
            fsutil::write_text_if_changed(&script.path, &script.contents)?;
        }
        if stage.skeleton {
            write_icons(inputs, &stage)?;
        }

        // 原生库注入：`poly bridge ohos` 的 `.so` → `entry/libs/<abi>/`。
        let mut notes = Vec::new();
        notes.extend(inject_native_libs(&harmony, &stage)?);
        let bundle = bundle_name(&inputs.product.identifier);
        if bundle != inputs.product.identifier {
            notes.push(format!(
                "bundleName 折叠：{} → {}（HarmonyOS 段内只允许字母数字与下划线）",
                inputs.product.identifier, bundle
            ));
        }

        // 构建：ohpm install（复用工程）+ hvigorw assembleHap；日志要点进事件流。
        for command in &plan.commands {
            let output = exec::run(command)?;
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            notes.extend(hvigor_notes(&format!("{stdout}{stderr}")));
        }

        // 产物收集：hvigor 输出复制为 `<kebab>-<version>-unsigned.hap`。
        let (built, signed_by_hvigor) =
            locate_hap(&stage.project).ok_or_else(|| missing_hap(&stage.project))?;
        if signed_by_hvigor {
            notes.push(
                "工程自带 signingConfigs：hvigor 产物已签名（未产出 -unsigned 件），签名阶段将再次签名"
                    .to_string(),
            );
        }
        let artifact_path = inputs.out_dir.join(format!("{}-unsigned.hap", stage.base));
        fsutil::copy_file(&built, &artifact_path, false)?;
        notes.push(format!("未签名 HAP：{}", artifact_path.display()));

        // 结构检查：不过即硬失败（后半链（签名 / 安装）对坏归档的所有操作都是白跑）。
        let structure = check_hap_structure(&artifact_path)?;
        notes.push(format!(
            "HAP 结构检查通过：{} 个条目；bundleName {} / versionCode {} / module {}",
            structure.entries.len(),
            structure.bundle_name,
            structure.version_code,
            structure.module_name
        ));
        fsutil::file_artifact(HARMONY, HAP, &artifact_path, notes)
    }
}

/// 两态工程落点与产物命名（`plan()` 与 `package()` 共用的纯路径推导，不写盘）。
struct Stage {
    /// `<工程根>/harmony/`（两态的工程目录；out_dir 之外）。
    project: PathBuf,
    /// 骨架生成态（`false` = 复用既有工程）。
    skeleton: bool,
    /// 产物基名 `<kebab>-<version>`。
    base: String,
    /// 生效的 `versionCode`（显式值优先，否则由语义化版本派生，与 Android 同口径）。
    version_code: u32,
    /// 生效的 `versionName`（产品语义化版本）。
    version_name: String,
    /// `build-profile.json5` 的 `compatibleSdkVersion`（`5.1.1(19)` 形态）。
    compatible_sdk: String,
    /// `oh-package.json5` / `hvigor-config.json5` 的 `modelVersion`（平台段 `5.1.1`）。
    model_version: String,
}

/// 推导工程落点（只读目录状态）：`<工程根>/harmony/` 含标志文件 → 复用；
/// 工程根自身含标志文件 → 复用（host 直接指到工程）；否则骨架生成到 `<工程根>/harmony/`。
fn stage(inputs: &BundleInputs, harmony: &HarmonyInputs, tools: &HarmonyTools) -> Stage {
    // 缺省工程根 = out_dir（库调用方未给宿主根时，骨架落 `out_dir/harmony`；CLI 恒给工程根）。
    let host = harmony
        .host_project
        .clone()
        .unwrap_or_else(|| inputs.out_dir.clone());
    let nested = host.join("harmony");
    let (project, skeleton) = if nested.join(PROJECT_MARKER).is_file() {
        (nested, false)
    } else if host.join(PROJECT_MARKER).is_file() {
        (host, false)
    } else {
        (nested, true)
    };
    let (sdk_platform, sdk_api) = sdk_versions(&tools.hvigorw);
    Stage {
        base: format!("{}-{}", inputs.product.kebab_name(), inputs.product.version),
        version_code: derive_version_code(&inputs.product.version, harmony.version_code),
        version_name: inputs.product.version.clone(),
        compatible_sdk: format!("{sdk_platform}({sdk_api})"),
        model_version: sdk_platform,
        project,
        skeleton,
    }
}

/// SDK 版本：由 hvigorw 路径回溯 command-line-tools 根读 `sdk/default/sdk-pkg.json`
/// 的 `platformVersion` / `apiVersion`；读不到 → 内置缺省（见常量）。
fn sdk_versions(hvigorw: &Path) -> (String, String) {
    let pkg = hvigorw
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("sdk/default/sdk-pkg.json"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let get = |key: &str| {
        pkg.as_ref()
            .and_then(|value| value.pointer(&format!("/data/{key}")))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    (
        get("platformVersion").unwrap_or_else(|| DEFAULT_SDK_PLATFORM.to_string()),
        get("apiVersion").unwrap_or_else(|| DEFAULT_SDK_API.to_string()),
    )
}

/// 两态共用的元数据脚本：`AppScope/app.json5`（产品元数据）与 `AppScope` 应用名字符串。
fn metadata_scripts(inputs: &BundleInputs, stage: &Stage) -> Vec<ScriptFile> {
    vec![
        ScriptFile {
            path: stage.project.join("AppScope/app.json5"),
            contents: app_json(&inputs.product, stage),
        },
        ScriptFile {
            path: stage
                .project
                .join("AppScope/resources/base/element/string.json"),
            contents: app_name_json(&inputs.product),
        },
    ]
}

/// `AppScope/app.json5`：产品元数据进入 HAP 清单（`module.json` 的 `app` 段由此而来）。
fn app_json(product: &ProductInfo, stage: &Stage) -> String {
    format!(
        "{{\n  \"app\": {{\n    \"bundleName\": \"{}\",\n    \"vendor\": \"{}\",\n    \
         \"versionCode\": {},\n    \"versionName\": \"{}\",\n    \"icon\": \"$media:app_icon\",\n    \
         \"label\": \"$string:app_name\"\n  }}\n}}\n",
        bundle_name(&product.identifier),
        product.manufacturer,
        stage.version_code,
        stage.version_name,
    )
}

/// `AppScope` 的 `app_name` 字符串（HAP label 引用 `$string:app_name`）。
fn app_name_json(product: &ProductInfo) -> String {
    format!(
        "{{\n  \"string\": [\n    {{ \"name\": \"app_name\", \"value\": \"{}\" }}\n  ]\n}}\n",
        product.name
    )
}

/// bundleName 折叠：HarmonyOS 只允许 `[a-zA-Z0-9_]` 段（`-` 等非法），逐段保留合法字符、
/// 其余 → `_`（标识符来自 `poly init` 的 `com.polypack.<kebab>` 约定，连字符是常态）。
///
/// shortcut: 数字开头的段补 `_` 前缀（与 Android 折叠同款）；HarmonyOS 规范要求段首为字母，
/// 该形态真实项目未见，出现时构建器会明确报错——到那次再加字母前缀折叠。
fn bundle_name(identifier: &str) -> String {
    identifier
        .split('.')
        .map(|segment| {
            let mut mapped = String::with_capacity(segment.len() + 1);
            for (index, ch) in segment.char_indices() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    if index == 0 && ch.is_ascii_digit() {
                        mapped.push('_');
                    }
                    mapped.push(ch);
                } else {
                    mapped.push('_');
                }
            }
            mapped
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// 骨架工程的静态文件（配置 + 占位页；图标 PNG 与产品名内插由 `package()` / 参数补）。
fn skeleton_scripts(inputs: &BundleInputs, stage: &Stage) -> Vec<ScriptFile> {
    let project = &stage.project;
    let name = &inputs.product.name;
    let scripts: Vec<(&str, String)> = vec![
        (
            "build-profile.json5",
            format!(
                "{{\n  \"app\": {{\n    \"signingConfigs\": [],\n    \"products\": [\n      {{\n        \
                 \"name\": \"default\",\n        \"signingConfig\": \"default\",\n        \
                 \"compatibleSdkVersion\": \"{}\",\n        \"runtimeOS\": \"HarmonyOS\"\n      }}\n    ],\n    \
                 \"buildModeSet\": [\n      {{ \"name\": \"debug\" }},\n      {{ \"name\": \"release\" }}\n    ]\n  }},\n  \
                 \"modules\": [\n    {{\n      \"name\": \"entry\",\n      \"srcPath\": \"./entry\",\n      \
                 \"targets\": [\n        {{ \"name\": \"default\", \"applyToProducts\": [\"default\"] }}\n      ]\n    }}\n  ]\n}}\n",
                stage.compatible_sdk,
            ),
        ),
        (
            "hvigorfile.ts",
            "import { appTasks } from '@ohos/hvigor-ohos-plugin';\n\nexport default {\n  system: appTasks,\n  plugins: []\n}\n".to_string(),
        ),
        (
            "oh-package.json5",
            format!(
                "{{\n  \"modelVersion\": \"{}\",\n  \"description\": \"{}\",\n  \"dependencies\": {{}},\n  \"devDependencies\": {{}}\n}}\n",
                stage.model_version, name,
            ),
        ),
        (
            "hvigor/hvigor-config.json5",
            format!(
                "{{\n  \"modelVersion\": \"{}\",\n  \"dependencies\": {{}},\n  \"execution\": {{}},\n  \
                 \"logging\": {{ \"level\": \"info\" }},\n  \"debugging\": {{ \"stacktrace\": false }},\n  \
                 \"nodeOptions\": {{ \"maxOldSpaceSize\": 8192 }}\n}}\n",
                stage.model_version,
            ),
        ),
        (
            "entry/build-profile.json5",
            "{\n  \"apiType\": \"stageMode\",\n  \"buildOption\": {},\n  \"targets\": [\n    { \"name\": \"default\" }\n  ]\n}\n".to_string(),
        ),
        (
            "entry/hvigorfile.ts",
            "import { hapTasks } from '@ohos/hvigor-ohos-plugin';\n\nexport default {\n  system: hapTasks,\n  plugins: []\n}\n".to_string(),
        ),
        (
            "entry/oh-package.json5",
            "{\n  \"name\": \"entry\",\n  \"version\": \"1.0.0\",\n  \"description\": \"entry module\",\n  \"main\": \"\",\n  \"author\": \"\",\n  \"license\": \"\",\n  \"dependencies\": {}\n}\n".to_string(),
        ),
        (
            "entry/src/main/module.json5",
            "{\n  \"module\": {\n    \"name\": \"entry\",\n    \"type\": \"entry\",\n    \"description\": \"$string:module_desc\",\n    \"mainElement\": \"EntryAbility\",\n    \"deviceTypes\": [\"phone\", \"tablet\"],\n    \"deliveryWithInstall\": true,\n    \"installationFree\": false,\n    \"pages\": \"$profile:main_pages\",\n    \"abilities\": [\n      {\n        \"name\": \"EntryAbility\",\n        \"srcEntry\": \"./ets/entryability/EntryAbility.ets\",\n        \"description\": \"$string:EntryAbility_desc\",\n        \"icon\": \"$media:icon\",\n        \"label\": \"$string:EntryAbility_label\",\n        \"startWindowIcon\": \"$media:icon\",\n        \"startWindowBackground\": \"$color:start_window_background\",\n        \"exported\": true,\n        \"skills\": [\n          { \"entities\": [\"entity.system.home\"], \"actions\": [\"action.system.home\"] }\n        ]\n      }\n    ]\n  }\n}\n".to_string(),
        ),
        (
            "entry/src/main/ets/entryability/EntryAbility.ets",
            "import { AbilityConstant, UIAbility, Want } from '@kit.AbilityKit';\nimport { window } from '@kit.ArkUI';\n\nexport default class EntryAbility extends UIAbility {\n  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {\n    // 宿主桥接与业务装载由 T-M3-01 的 RNOH 模板接管；骨架只作可运行占位。\n  }\n\n  onWindowStageCreate(windowStage: window.WindowStage): void {\n    windowStage.loadContent('pages/Index');\n  }\n}\n".to_string(),
        ),
        (
            "entry/src/main/ets/pages/Index.ets",
            format!(
                "@Entry\n@Component\nstruct Index {{\n  @State message: string = '{name}';\n\n  build() {{\n    Column() {{\n      Text(this.message)\n        .fontSize(24)\n        .fontWeight(FontWeight.Bold)\n    }}\n    .width('100%')\n    .height('100%')\n    .justifyContent(FlexAlign.Center)\n  }}\n}}\n"
            ),
        ),
        (
            "entry/src/main/resources/base/element/string.json",
            format!(
                "{{\n  \"string\": [\n    {{ \"name\": \"module_desc\", \"value\": \"{name} entry module\" }},\n    \
                 {{ \"name\": \"EntryAbility_desc\", \"value\": \"{name}\" }},\n    \
                 {{ \"name\": \"EntryAbility_label\", \"value\": \"{name}\" }}\n  ]\n}}\n"
            ),
        ),
        (
            "entry/src/main/resources/base/element/color.json",
            "{\n  \"color\": [\n    { \"name\": \"start_window_background\", \"value\": \"#FFFFFF\" }\n  ]\n}\n".to_string(),
        ),
        (
            "entry/src/main/resources/base/profile/main_pages.json",
            "{\n  \"src\": [\n    \"pages/Index\"\n  ]\n}\n".to_string(),
        ),
    ];
    scripts
        .into_iter()
        .map(|(relative, contents)| ScriptFile {
            path: project.join(relative),
            contents,
        })
        .collect()
}

/// 骨架图标：工程无图标输入时用 1×1 占位 PNG；有 PNG 图标输入则用真实图标（两处同上）。
fn write_icons(inputs: &BundleInputs, stage: &Stage) -> Result<(), BundleError> {
    let icon = inputs
        .icon(IconKind::Png)
        .filter(|icon| icon.path.is_file());
    for relative in [
        "AppScope/resources/base/media/app_icon.png",
        "entry/src/main/resources/base/media/icon.png",
    ] {
        let target = stage.project.join(relative);
        match icon {
            Some(icon) => fsutil::copy_file(&icon.path, &target, false)?,
            None => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(crate::error::io_err(parent))?;
                }
                // 占位图内容固定：已存在即不动（保 mtime，理由同 write_text_if_changed）。
                if !target.is_file() {
                    std::fs::write(&target, PLACEHOLDER_PNG)
                        .map_err(crate::error::io_err(&target))?;
                }
            }
        }
    }
    Ok(())
}

/// 原生库注入：`entry/libs/<abi>/<name>.so`（HAP 内 `libs/<abi>/` 同域）；返回产物说明。
fn inject_native_libs(harmony: &HarmonyInputs, stage: &Stage) -> Result<Vec<String>, BundleError> {
    if harmony.native_libs.is_empty() {
        return Ok(vec![
            "无原生库注入（bridge/harmonyos/ 无 .so——纯 ArkTS / 资源产物）".to_string(),
        ]);
    }
    let mut abis: Vec<String> = Vec::new();
    for lib in &harmony.native_libs {
        // ABI 目录名进产物路径：只允许 [a-z0-9_-]（防路径穿越进 HAP）。
        if lib.abi.is_empty()
            || !lib
                .abi
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        {
            return Err(BundleError::InvalidInput {
                packager: HARMONY,
                field: "harmony.native_libs",
                value: lib.abi.clone(),
                reason: "ABI 目录名只允许 [a-z0-9_-]（如 arm64-v8a）".to_string(),
            });
        }
        let name = lib
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| BundleError::InvalidInput {
                packager: HARMONY,
                field: "harmony.native_libs",
                value: lib.path.display().to_string(),
                reason: "路径缺少文件名".to_string(),
            })?;
        fsutil::copy_file(
            &lib.path,
            &stage.project.join("entry/libs").join(&lib.abi).join(name),
            false,
        )?;
        if !abis.contains(&lib.abi) {
            abis.push(lib.abi.clone());
        }
    }
    Ok(vec![format!(
        "注入原生库 {} 枚（ABI：{}）",
        harmony.native_libs.len(),
        abis.join(", ")
    )])
}

/// 在工程内定位 hvigor 产物：标准落点优先，再扫模块目录（模块名不一定是 `entry`）；
/// 返回（路径, 是否为工程自带签名配置产出的已签名件）。
fn locate_hap(project: &Path) -> Option<(PathBuf, bool)> {
    let standard = project.join(HVIGOR_OUTPUT);
    if standard.is_file() {
        return Some((standard, false));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(project).ok()?.flatten() {
        let out = entry.path().join("build/default/outputs/default");
        let Ok(files) = std::fs::read_dir(&out) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "hap") {
                found.push(path);
            }
        }
    }
    found.sort();
    let unsigned = found
        .iter()
        .find(|path| path.to_string_lossy().contains("-unsigned"))
        .cloned();
    match unsigned {
        Some(path) => Some((path, false)),
        // 工程自带 signingConfigs：只有已签名件——照收并如实注记（签名阶段会再次签名）。
        None => found.into_iter().next().map(|path| (path, true)),
    }
}

/// 缺 HAP 产物时的错误（hvigorw 零退出但约定落点无文件：多为模块名 / 产品名非常规）。
fn missing_hap(project: &Path) -> BundleError {
    BundleError::Archive {
        path: project.to_path_buf(),
        reason: format!(
            "hvigor 构建完成但未找到 HAP（查找 {HVIGOR_OUTPUT} 及各模块 outputs/default/*.hap）"
        ),
    }
}

/// 打包格式与打包器声明必须一致（防「按 ipa 目标产出 hap」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: HARMONY,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// HarmonyOS 打包输入（`None` → 按产品元数据在其宿主根下落骨架工程）。
fn harmony_inputs(inputs: &BundleInputs) -> HarmonyInputs {
    inputs.harmony.clone().unwrap_or_default()
}

/// 缺 ohpm 时的错误（复用工程带 ohpm 依赖，没它 hvigor 解析不了 har）。
fn missing_ohpm() -> BundleError {
    BundleError::ToolMissing {
        packager: HARMONY,
        tool: "ohpm".to_string(),
        hint: "复用工程带 ohpm 依赖（如 RNOH 的 har）：装 command-line-tools（含 bin/ohpm）\
               并设 $HARMONYOS_TOOLS，或用 HarmonyInputs.ohpm 显式指定"
            .to_string(),
    }
}

/// 从 hvigor / ohpm 输出提取要点（构建结果、缓存命中、告警行）——构建日志进事件流的口径。
///
/// 纯文本扫描：hvigor 输出格式随版本漂移，命中不了就少几条注记，不影响构建判定。
fn hvigor_notes(text: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let mut up_to_date = 0usize;
    let mut warns: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains("BUILD SUCCESSFUL") {
            notes.push("hvigor：BUILD SUCCESSFUL".to_string());
        } else if line.contains("UP-TO-DATE") {
            up_to_date += 1;
        } else if line.contains("WARN") && warns.len() < 5 {
            warns.push(format!("hvigor 告警：{line}"));
        }
    }
    if up_to_date > 0 {
        notes.push(format!(
            "hvigor：{up_to_date} 处 UP-TO-DATE（增量缓存命中）"
        ));
    }
    notes.extend(warns);
    notes
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

/// HAP 结构检查结果（T-M3-03 验收「清单 / 模块 / 资源清单项」；`poly verify` 的
/// `hap-structure-check` 校验器复用同一实现）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HapStructure {
    /// zip 内条目名（zip 内序）。
    pub entries: Vec<String>,
    /// `module.json` 的 `app.bundleName`。
    pub bundle_name: String,
    /// `module.json` 的 `app.versionCode`。
    pub version_code: u64,
    /// `module.json` 的 `app.versionName`。
    pub version_name: String,
    /// `module.json` 的 `module.name`（模块名，如 `entry`）。
    pub module_name: String,
}

/// 校验 HAP 归档结构：必需条目（[`HAP_REQUIRED_ENTRIES`]）在场 + `module.json` 可解析且
/// 清单字段齐全；任何一项缺即 [`BundleError::Archive`]（不猜、不降级）。
pub fn check_hap_structure(hap: &Path) -> Result<HapStructure, BundleError> {
    let entries = zipwrite::entry_names(hap)?;
    let missing: Vec<&str> = HAP_REQUIRED_ENTRIES
        .iter()
        .filter(|required| !entries.iter().any(|entry| entry == *required))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(BundleError::Archive {
            path: hap.to_path_buf(),
            reason: format!(
                "缺必需条目：{}（HAP 须含 module.json / resources.index / pack.info）",
                missing.join("、")
            ),
        });
    }
    let text = zipwrite::entry_text(hap, "module.json")?
        .ok_or_else(|| archive_err(hap, "module.json 条目读取为空"))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| archive_err(hap, &format!("module.json 不可解析：{err}")))?;
    let text_field = |pointer: &str| -> Result<String, BundleError> {
        manifest
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| archive_err(hap, &format!("module.json 缺字段 {pointer}（或非字符串）")))
    };
    let version_code = manifest
        .pointer("/app/versionCode")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| archive_err(hap, "module.json 缺字段 /app/versionCode（或非整数）"))?;
    Ok(HapStructure {
        entries,
        bundle_name: text_field("/app/bundleName")?,
        version_code,
        version_name: text_field("/app/versionName")?,
        module_name: text_field("/module/name")?,
    })
}

/// 结构检查失败的错误（归档域 E_BND_0006）。
fn archive_err(hap: &Path, reason: &str) -> BundleError {
    BundleError::Archive {
        path: hap.to_path_buf(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内建 harmonyos 目标声明（测试经注册表取真声明，避免手抄字段漂移）。
    fn harmony_spec() -> TargetSpec {
        poly_targets::Registry::builtin()
            .get("harmonyos")
            .expect("内建 harmonyos 目标")
            .clone()
    }

    fn product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用".to_string(),
            homepage: None,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poly-hap-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn format_mismatch_is_rejected() {
        let mut spec = harmony_spec();
        spec.package.format = "aab".to_string();
        let inputs = BundleInputs::new(product(), scratch("fmt").join("out"));
        let err = HarmonyHap
            .plan(&inputs, &spec)
            .expect_err("格式不符必须报错");
        assert!(matches!(err, BundleError::InvalidInput { .. }));
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
    }

    #[test]
    fn bundle_name_folds_hyphens_for_harmonyos() {
        assert_eq!(
            bundle_name("com.polypack.pax-notes"),
            "com.polypack.pax_notes"
        );
        assert_eq!(
            bundle_name("com.polypack.paxnotes"),
            "com.polypack.paxnotes"
        );
        assert_eq!(bundle_name("com.1pax.app"), "com._1pax.app");
    }

    #[test]
    fn project_source_prefers_template_then_skeleton() {
        // 宿主根下有 harmony/ 实例 → 复用。
        let host = scratch("reuse");
        let project = host.join("harmony");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(PROJECT_MARKER), "{}").unwrap();
        let inputs = BundleInputs::new(product(), host.join("out"));
        let harmony = HarmonyInputs {
            host_project: Some(host.clone()),
            hvigorw: Some(PathBuf::from("/poly-no-hvigorw")),
            ..HarmonyInputs::default()
        };
        let staged = stage(&inputs, &harmony, &fake_tools());
        assert!(!staged.skeleton);
        assert_eq!(staged.project, project);

        // 宿主根自身是工程（host 直接指到工程）→ 复用。
        let direct = scratch("direct");
        std::fs::write(direct.join(PROJECT_MARKER), "{}").unwrap();
        let harmony = HarmonyInputs {
            host_project: Some(direct.clone()),
            ..HarmonyInputs::default()
        };
        let staged = stage(&inputs, &harmony, &fake_tools());
        assert!(!staged.skeleton);
        assert_eq!(staged.project, direct);

        // 都没有 → 骨架落到 <宿主根>/harmony/。
        let empty = scratch("skeleton");
        let harmony = HarmonyInputs {
            host_project: Some(empty.clone()),
            ..HarmonyInputs::default()
        };
        let staged = stage(&inputs, &harmony, &fake_tools());
        assert!(staged.skeleton);
        assert_eq!(staged.project, empty.join("harmony"));
    }

    fn fake_tools() -> HarmonyTools {
        HarmonyTools {
            hvigorw: PathBuf::from("/poly-no-hvigorw"),
            ohpm: None,
        }
    }

    #[test]
    fn skeleton_plan_carries_configs_and_hvigor_command() {
        // 假 hvigorw（is_file 即可）让 plan 走完整纯函数路径；骨架不碰 ohpm。
        let base = scratch("plan");
        let fake = base.join("hvigorw");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        let host = base.join("project");
        std::fs::create_dir_all(&host).unwrap();
        let inputs = BundleInputs::new(product(), base.join("out"));
        let spec = harmony_spec();
        let mut bounds = inputs.clone();
        bounds.harmony = Some(HarmonyInputs {
            host_project: Some(host.clone()),
            hvigorw: Some(fake.clone()),
            ..HarmonyInputs::default()
        });

        let plan = HarmonyHap.plan(&bounds, &spec).expect("骨架计划");
        let paths: Vec<String> = plan
            .scripts
            .iter()
            .map(|script| script.path.display().to_string())
            .collect();
        for required in [
            "build-profile.json5",
            "hvigorfile.ts",
            "oh-package.json5",
            "hvigor/hvigor-config.json5",
            "AppScope/app.json5",
            "entry/build-profile.json5",
            "entry/hvigorfile.ts",
            "entry/oh-package.json5",
            "entry/src/main/module.json5",
            "entry/src/main/ets/entryability/EntryAbility.ets",
            "entry/src/main/ets/pages/Index.ets",
            "entry/src/main/resources/base/profile/main_pages.json",
        ] {
            let want = host.join("harmony").join(required);
            assert!(
                paths.contains(&want.display().to_string()),
                "骨架应含 {required}：{paths:?}"
            );
        }
        // 计划是纯函数：不落盘（目录不得出现标志文件）。
        assert!(!host.join("harmony/build-profile.json5").exists());
        assert_eq!(plan.commands.len(), 1, "骨架无 ohpm 步：{plan:?}");
        let line = plan.commands[0].line();
        assert!(
            line.contains("assembleHap") && line.contains("--no-daemon"),
            "{line}"
        );
        assert_eq!(
            plan.commands[0].cwd.as_deref(),
            Some(host.join("harmony").as_path())
        );
        // 应用元数据来自 polypack.toml（versionCode 由语义化版本派生）。
        let app = plan
            .scripts
            .iter()
            .find(|script| script.path.ends_with("AppScope/app.json5"))
            .expect("app.json5");
        assert!(
            app.contents.contains("com.polypack.paxnotes"),
            "{}",
            app.contents
        );
        assert!(app.contents.contains("10203"), "{}", app.contents);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reuse_project_requires_ohpm_install() {
        let base = scratch("reuse-plan");
        let fake = base.join("hvigorw");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        std::fs::write(base.join("ohpm"), "#!/bin/sh\n").unwrap();
        let project = base.join("sample/harmony");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(PROJECT_MARKER), "{}").unwrap();
        let inputs = BundleInputs::new(product(), base.join("out"));
        let mut bounds = inputs.clone();
        bounds.harmony = Some(HarmonyInputs {
            host_project: Some(base.join("sample")),
            hvigorw: Some(fake),
            ohpm: Some(base.join("ohpm")),
            ..HarmonyInputs::default()
        });
        let plan = HarmonyHap.plan(&bounds, &harmony_spec()).expect("复用计划");
        assert_eq!(
            plan.commands.len(),
            2,
            "复用工程应先 ohpm install：{plan:?}"
        );
        assert!(plan.commands[0].line().contains("install"));
        assert_eq!(plan.commands[0].cwd.as_deref(), Some(project.as_path()));
        // oh_modules 在场（已装过）→ 不再 install。
        std::fs::create_dir_all(project.join("oh_modules")).unwrap();
        let plan = HarmonyHap
            .plan(&bounds, &harmony_spec())
            .expect("复用计划（已装依赖）");
        assert_eq!(plan.commands.len(), 1, "{plan:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sdk_versions_reads_pkg_json_with_fallback() {
        let base = scratch("sdk");
        let bin = base.join("bin");
        std::fs::create_dir_all(base.join("sdk/default")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let hvigorw = bin.join("hvigorw");
        std::fs::write(&hvigorw, "").unwrap();
        // 无 sdk-pkg.json → 缺省。
        assert_eq!(
            sdk_versions(&hvigorw),
            (
                DEFAULT_SDK_PLATFORM.to_string(),
                DEFAULT_SDK_API.to_string()
            )
        );
        // 有 → 读真实值。
        std::fs::write(
            base.join("sdk/default/sdk-pkg.json"),
            "{\"data\":{\"apiVersion\":\"20\",\"platformVersion\":\"6.0.0\"}}",
        )
        .unwrap();
        assert_eq!(
            sdk_versions(&hvigorw),
            ("6.0.0".to_string(), "20".to_string())
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 合成一个 HAP（zip）夹具：`module.json` + 其余必需条目的可选项。
    fn synthetic_hap(base: &Path, tag: &str, with_resources_index: bool) -> PathBuf {
        let src = base.join(format!("src-{tag}"));
        std::fs::create_dir_all(&src).unwrap();
        let module_json = src.join("module.json");
        std::fs::write(
            &module_json,
            "{\"app\":{\"bundleName\":\"com.polypack.paxnotes\",\"versionCode\":10203,\
             \"versionName\":\"1.2.3\"},\"module\":{\"name\":\"entry\",\"type\":\"entry\"}}",
        )
        .unwrap();
        let pack_info = src.join("pack.info");
        std::fs::write(&pack_info, "pack-info").unwrap();
        let index = src.join("resources.index");
        std::fs::write(&index, "index").unwrap();
        let mut entries = vec![
            ("module.json".to_string(), module_json),
            ("pack.info".to_string(), pack_info),
        ];
        if with_resources_index {
            entries.push(("resources.index".to_string(), index));
        }
        let hap = base.join(format!("{tag}.hap"));
        zipwrite::write_zip(&entries, &hap).unwrap();
        hap
    }

    #[test]
    fn structure_check_accepts_minimal_hap_and_flags_missing_entries() {
        let base = scratch("structure");
        let good = synthetic_hap(&base, "good", true);
        let structure = check_hap_structure(&good).expect("结构检查应通过");
        assert_eq!(structure.bundle_name, "com.polypack.paxnotes");
        assert_eq!(structure.version_code, 10203);
        assert_eq!(structure.version_name, "1.2.3");
        assert_eq!(structure.module_name, "entry");
        assert!(structure.entries.contains(&"resources.index".to_string()));

        let bad = synthetic_hap(&base, "bad", false);
        let err = check_hap_structure(&bad).expect_err("缺条目必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0006);
        assert!(err.to_string().contains("resources.index"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn locate_hap_prefers_unsigned_and_falls_back_to_signed() {
        let base = scratch("locate");
        let standard = base.join(HVIGOR_OUTPUT);
        std::fs::create_dir_all(standard.parent().unwrap()).unwrap();
        std::fs::write(&standard, "hap").unwrap();
        let (found, signed) = locate_hap(&base).expect("标准落点");
        assert_eq!(found, standard);
        assert!(!signed);

        // 非 entry 模块名：扫描 outputs/default。
        let alt = scratch("locate-alt");
        let other = alt.join("main/build/default/outputs/default/main-default-unsigned.hap");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, "hap").unwrap();
        let (found, signed) = locate_hap(&alt).expect("扫描落点");
        assert_eq!(found, other);
        assert!(!signed);

        // 只有已签名件（工程自带 signingConfigs）→ 照收并标记。
        let signed_only = scratch("locate-signed");
        let file = signed_only.join("entry/build/default/outputs/default/entry-default-signed.hap");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "hap").unwrap();
        let (found, signed) = locate_hap(&signed_only).expect("已签名件");
        assert_eq!(found, file);
        assert!(signed);

        for dir in [base, alt, signed_only] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn hvigor_notes_extract_summary_cache_and_warns() {
        let text = "\
            > hvigor UP-TO-DATE: entry:default@PreBuild\n\
            > hvigor Finished :entry:default@CompileArkTS... after 12 ms\n\
            WARN: Will skip sign 'hos_hap'. No signingConfigs profile is configured.\n\
            > hvigor UP-TO-DATE: entry:default@PackageHap\n\
            BUILD SUCCESSFUL in 14 s 402 ms\n";
        let notes = hvigor_notes(text);
        assert!(
            notes.contains(&"hvigor：BUILD SUCCESSFUL".to_string()),
            "{notes:?}"
        );
        assert!(
            notes.contains(&"hvigor：2 处 UP-TO-DATE（增量缓存命中）".to_string()),
            "{notes:?}"
        );
        assert!(
            notes.iter().any(|note| note.contains("skip sign")),
            "{notes:?}"
        );
    }

    #[test]
    fn native_lib_injection_targets_entry_libs() {
        let base = scratch("libs");
        let project = base.join("harmony");
        std::fs::create_dir_all(&project).unwrap();
        let lib = base.join("bridge/harmonyos/arm64-v8a/libpax.so");
        std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
        std::fs::write(&lib, "so").unwrap();
        let inputs = BundleInputs::new(product(), base.join("out"));
        let harmony = HarmonyInputs {
            host_project: Some(base.clone()),
            native_libs: vec![crate::inputs::NativeLib {
                abi: "arm64-v8a".to_string(),
                path: lib,
            }],
            ..HarmonyInputs::default()
        };
        let staged = stage(&inputs, &harmony, &fake_tools());
        let notes = inject_native_libs(&harmony, &staged).expect("注入");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("arm64-v8a"), "{notes:?}");
        assert!(project.join("entry/libs/arm64-v8a/libpax.so").is_file());

        // 非法 ABI 目录名：拒（防路径穿越进 HAP）。
        let bad = HarmonyInputs {
            native_libs: vec![crate::inputs::NativeLib {
                abi: "../evil".to_string(),
                path: base.join("x.so"),
            }],
            ..HarmonyInputs::default()
        };
        assert!(matches!(
            inject_native_libs(&bad, &staged),
            Err(BundleError::InvalidInput { .. })
        ));
        let _ = std::fs::remove_dir_all(&base);
    }
}
