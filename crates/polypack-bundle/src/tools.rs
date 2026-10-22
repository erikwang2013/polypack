//! Android SDK 与工具解析（T-M2-06 / T-M2-07）与 HarmonyOS 构建工具解析（T-M3-03）。
//!
//! Android SDK 根解析顺序：`AndroidInputs.sdk_root`（显式配置即权威）→ `$ANDROID_HOME` →
//! `$ANDROID_SDK_ROOT` → `~/Android/Sdk`（Linux 惯例）→ `~/Library/Android/sdk`（macOS 惯例）。
//! build-tools 取版本号最大的目录（`aapt2` / `apksigner` / `zipalign` / `d8` 都在其中），
//! 平台取 `android-<NN>` 里 NN 最大的（预览名如 `android-Baklava` 无数值 API，跳过）。
//! `aapt2` 与 `android.jar` 任一缺失即 [`BundleError::ToolMissing`]（E_BND_0001）。
//!
//! HarmonyOS：`hvigorw` / `ohpm` 解析顺序为 `HarmonyInputs.hvigorw|ohpm`（显式配置即权威）→
//! `$HARMONYOS_TOOLS`（command-line-tools 根，试 `<root>/bin/<tool>` 与 `<root>/<tool>`）→ PATH。

use std::path::{Path, PathBuf};

use crate::error::BundleError;
use crate::exec;
use crate::inputs::{AndroidInputs, HarmonyInputs};

/// 解析出的 Android 工具链（路径一律绝对，不经 PATH）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidTools {
    /// SDK 根。
    pub sdk_root: PathBuf,
    /// 选中的 build-tools 目录（版本号最大）。
    pub build_tools: PathBuf,
    /// `aapt2`。
    pub aapt2: PathBuf,
    /// `apksigner`（签名由 T-M2-09 接入，此处仅供签名阶段与测试使用）。
    pub apksigner: Option<PathBuf>,
    /// `zipalign`。
    pub zipalign: Option<PathBuf>,
    /// `d8`（`.jar` / `.class` → `classes.dex`）。
    pub d8: Option<PathBuf>,
    /// 平台 `android.jar`（`aapt2 link -I`）。
    pub android_jar: PathBuf,
    /// 选中平台的 API 等级（来自 `platforms/android-<NN>`）。
    pub platform_api: u32,
    /// `bundletool-all.jar`（`java -jar` 调用，不是可执行文件）。
    pub bundletool: Option<PathBuf>,
    /// PATH 上的 `java`（bundletool / d8 的运行时）。
    pub java: Option<PathBuf>,
}

/// 解析工具链：缺 `aapt2` / `android.jar` 即 [`BundleError::ToolMissing`]。
pub fn resolve(inputs: &AndroidInputs) -> Result<AndroidTools, BundleError> {
    probe(inputs).ok_or_else(|| BundleError::ToolMissing {
        packager: "android",
        tool: "aapt2".to_string(),
        hint: sdk_hint(inputs),
    })
}

/// 探测式解析（不报错）：`plan()` 的契约测试与「缺工具须如实报错」类断言用。
pub fn probe(inputs: &AndroidInputs) -> Option<AndroidTools> {
    let (sdk_root, build_tools, android_jar, platform_api) = locate(inputs)?;
    let aapt2 = tool_in(&build_tools, "aapt2")?;
    Some(AndroidTools {
        sdk_root,
        build_tools: build_tools.clone(),
        aapt2,
        apksigner: tool_in(&build_tools, "apksigner"),
        zipalign: tool_in(&build_tools, "zipalign"),
        d8: tool_in(&build_tools, "d8"),
        android_jar,
        platform_api,
        bundletool: locate_bundletool(inputs),
        java: exec::tool_path("java"),
    })
}

/// 在候选 SDK 根内定位 build-tools（版本号最大）、平台（`android-NN` 里 NN 最大）与 `android.jar`。
fn locate(inputs: &AndroidInputs) -> Option<(PathBuf, PathBuf, PathBuf, u32)> {
    for root in candidate_roots(inputs) {
        let Some(build_tools) = latest_build_tools(&root) else {
            continue;
        };
        let Some((platform_api, android_jar)) = latest_platform(&root) else {
            continue;
        };
        return Some((root, build_tools, android_jar, platform_api));
    }
    None
}

/// SDK 根候选：显式配置独占（指向不存在的 SDK 不静默回退到环境变量），否则按惯例依次尝试。
fn candidate_roots(inputs: &AndroidInputs) -> Vec<PathBuf> {
    if let Some(root) = &inputs.sdk_root {
        return vec![root.clone()];
    }
    let mut roots: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from))
        .collect();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        roots.push(home.join("Android/Sdk"));
        roots.push(home.join("Library/Android/sdk"));
    }
    roots
}

/// build-tools 里版本号最大的目录（`34.0.0` / `36.1.0` / …；非数值目录跳过）。
fn latest_build_tools(sdk_root: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for entry in std::fs::read_dir(sdk_root.join("build-tools"))
        .ok()?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(version) = version_key(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(current, _)| version > *current) {
            best = Some((version, path));
        }
    }
    best.map(|(_, path)| path)
}

/// `platforms/android-<NN>` 里 NN 最大的平台与它的 `android.jar`。
fn latest_platform(sdk_root: &Path) -> Option<(u32, PathBuf)> {
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in std::fs::read_dir(sdk_root.join("platforms"))
        .ok()?
        .flatten()
    {
        let name = entry.file_name();
        let Some(api) = name
            .to_string_lossy()
            .strip_prefix("android-")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let jar = entry.path().join("android.jar");
        if !jar.is_file() {
            continue;
        }
        if best.as_ref().is_none_or(|(current, _)| api > *current) {
            best = Some((api, jar));
        }
    }
    best
}

/// 版本目录名的数值键（`36.1.0` → `[36, 1, 0]`；任一段非数值即 None）。
fn version_key(name: &str) -> Option<Vec<u64>> {
    let parts = name
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

/// build-tools 内的工具文件（Windows 上是 `.exe` / `.bat`）。
fn tool_in(dir: &Path, name: &str) -> Option<PathBuf> {
    ["", ".exe", ".bat", ".cmd"]
        .iter()
        .map(|ext| dir.join(format!("{name}{ext}")))
        .find(|path| path.is_file())
}

/// bundletool-all.jar 解析：显式配置（权威：指向不存在的 jar 视为未配置）→ `$POLYPACK_BUNDLETOOL`
/// （环境变量是提示，指向不存在的文件继续往下找）→ PATH 上的 `bundletool` → 本地工具缓存。
fn locate_bundletool(inputs: &AndroidInputs) -> Option<PathBuf> {
    if let Some(jar) = &inputs.bundletool {
        return jar.is_file().then(|| jar.clone());
    }
    if let Some(jar) = std::env::var_os("POLYPACK_BUNDLETOOL").map(PathBuf::from)
        && jar.is_file()
    {
        return Some(jar);
    }
    if let Some(found) = exec::tool_path("bundletool") {
        return Some(found);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let cached = PathBuf::from(home).join(".cache/polypack/tools/bundletool-all.jar");
    cached.is_file().then_some(cached)
}

/// 解析出的 HarmonyOS 构建工具（路径一律绝对，不经 PATH）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarmonyTools {
    /// `hvigorw`（hvigor 构建入口；`hvigorw assembleHap` 出未签名 HAP）。
    pub hvigorw: PathBuf,
    /// `ohpm`（复用工程装依赖 `ohpm install` 用；工程无依赖时非必需）。
    pub ohpm: Option<PathBuf>,
}

/// 解析 HarmonyOS 构建工具：缺 `hvigorw` 即 [`BundleError::ToolMissing`]。
pub fn resolve_harmony(inputs: &HarmonyInputs) -> Result<HarmonyTools, BundleError> {
    probe_harmony(inputs).ok_or_else(|| BundleError::ToolMissing {
        packager: "harmonyos",
        tool: "hvigorw".to_string(),
        hint: hvigor_hint(inputs),
    })
}

/// 探测式解析（不报错）：`plan()` 的契约测试用。
pub fn probe_harmony(inputs: &HarmonyInputs) -> Option<HarmonyTools> {
    Some(HarmonyTools {
        hvigorw: locate_harmony_tool(&inputs.hvigorw, "hvigorw")?,
        ohpm: locate_harmony_tool(&inputs.ohpm, "ohpm"),
    })
}

/// `hvigorw` / `ohpm` 解析：显式配置独占（指向不存在的路径不回退），否则
/// `$HARMONYOS_TOOLS`（根或直接指 `bin/`）→ PATH。
fn locate_harmony_tool(explicit: &Option<PathBuf>, name: &str) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.clone());
    }
    if let Some(root) = std::env::var_os("HARMONYOS_TOOLS").filter(|v| !v.is_empty()) {
        let root = PathBuf::from(root);
        for candidate in [root.join("bin").join(name), root.join(name)] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    exec::tool_path(name)
}

/// 缺 `hvigorw` 时的安装 / 配置建议。
fn hvigor_hint(inputs: &HarmonyInputs) -> String {
    let where_ = inputs.hvigorw.as_ref().map_or_else(
        || "$HARMONYOS_TOOLS（command-line-tools 根）/ PATH".to_string(),
        |path| path.display().to_string(),
    );
    format!(
        "未找到 hvigorw（查找：{where_}）：装 HarmonyOS command-line-tools 后设 \
         $HARMONYOS_TOOLS 指向其根（含 bin/hvigorw），或用 HarmonyInputs.hvigorw 显式指定"
    )
}

/// 缺 SDK 时的安装 / 配置建议。
fn sdk_hint(inputs: &AndroidInputs) -> String {
    let where_ = inputs.sdk_root.as_ref().map_or_else(
        || "$ANDROID_HOME / $ANDROID_SDK_ROOT / ~/Android/Sdk".to_string(),
        |root| root.display().to_string(),
    );
    format!(
        "未找到可用的 Android SDK（根：{where_}）：装 commandline-tools 后执行 \
         `sdkmanager \"platform-tools\" \"build-tools;36.0.0\" \"platforms;android-36\"`，\
         或用 AndroidInputs.sdk_root 指向 SDK 根（也支持 $ANDROID_HOME / $ANDROID_SDK_ROOT）"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_keys_order_numerically() {
        assert!(version_key("36.1.0").unwrap() > version_key("36.0.0").unwrap());
        assert!(version_key("9.0.0").unwrap() < version_key("34.0.0").unwrap());
        assert_eq!(version_key("android-Baklava"), None);
    }

    #[test]
    fn missing_sdk_root_is_not_silently_ignored() {
        // 显式配置独占：指向不存在的 SDK 不得回退到环境变量 / 用户目录。
        let inputs = AndroidInputs {
            sdk_root: Some(PathBuf::from("/polypack-nonexistent-sdk")),
            ..AndroidInputs::default()
        };
        assert!(probe(&inputs).is_none());
        let err = resolve(&inputs).expect_err("缺 aapt2 必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0001);
        let BundleError::ToolMissing { hint, .. } = err else {
            panic!("期望 ToolMissing");
        };
        assert!(
            hint.contains("polypack-nonexistent-sdk"),
            "hint 须含配置路径：{hint}"
        );
    }

    #[test]
    fn missing_hvigorw_is_not_silently_ignored() {
        // 同上：显式 hvigorw 独占，不回退到 $HARMONYOS_TOOLS / PATH。
        let inputs = HarmonyInputs {
            hvigorw: Some(PathBuf::from("/polypack-nonexistent-hvigorw")),
            ..HarmonyInputs::default()
        };
        assert!(probe_harmony(&inputs).is_none());
        let err = resolve_harmony(&inputs).expect_err("缺 hvigorw 必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0001);
        let BundleError::ToolMissing { hint, .. } = err else {
            panic!("期望 ToolMissing");
        };
        assert!(
            hint.contains("polypack-nonexistent-hvigorw"),
            "hint 须含配置路径：{hint}"
        );
    }
}
