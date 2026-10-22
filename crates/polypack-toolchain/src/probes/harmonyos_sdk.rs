//! HarmonyOS SDK 探测：`$HARMONYOS_TOOLS`（command-line-tools 根）/ `$DEVECO_SDK_HOME` /
//! `$POLYPACK_OHOS_SDK`（显式覆盖）。
//!
//! 三种安装形态指向的层级不同（命令行工具根 / DevEco 的 sdk 目录 / 直接指到
//! `…/sdk/default/openharmony`），探测逐个候选层级试。命中判据 = 目录下有
//! `native/llvm/bin`（OHOS clang）与 `native/sysroot`（napi 头 + libc）——那是交叉编译
//! NAPI `.so` 的两件必需品，只有 IDE 外壳不算命中。

use std::path::{Path, PathBuf};

use crate::probe::{Probe, ProbeResult};

use super::{found_result, missing_result};

/// HarmonyOS SDK 探测器。
pub struct HarmonyOsSdk;

impl Probe for HarmonyOsSdk {
    fn id(&self) -> &'static str {
        "harmonyos-sdk"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["harmonyos"]
    }

    fn locate(&self) -> ProbeResult {
        match sdk_dir() {
            Some(dir) => {
                let version = sdk_version(&dir);
                found_result(self.id(), self.required_for(), dir, version)
            }
            None => missing_result(
                self.id(),
                self.required_for(),
                "未找到 HarmonyOS SDK：装 command-line-tools 后设 $HARMONYOS_TOOLS 指向其根\
                 （或 DevEco Studio 的 $DEVECO_SDK_HOME），也可用 $POLYPACK_OHOS_SDK 直接指向 \
                 …/sdk/default/openharmony；`polypack bridge ohos` 的 .so 交叉编译需要其中的 \
                 native/llvm 与 native/sysroot",
            ),
        }
    }
}

/// 解析 SDK 根（= 含 `native/` 的 openharmony 目录）。`pub(crate)` 供桥编译链同口径复用。
pub(crate) fn sdk_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for key in [
        "POLYPACK_OHOS_SDK",
        "DEVECO_SDK_HOME",
        "HARMONYOS_TOOLS",
        "OHOS_SDK_HOME",
    ] {
        let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let root = PathBuf::from(value);
        // 逐个层级的落点：命令行工具根（…/command-line-tools）→ …/sdk → …/sdk/default/openharmony；
        // DevEco 的 sdk 目录 → …/default/openharmony；以及直接指到 openharmony 的形态
        for nested in [
            "",
            "sdk",
            "sdk/default/openharmony",
            "default/openharmony",
            "openharmony",
        ] {
            candidates.push(root.join(nested));
        }
    }
    candidates.into_iter().find(|dir| is_sdk(dir))
}

/// 命中判据：OHOS 交叉编译工具链（clang）与 sysroot 都在场。
fn is_sdk(dir: &Path) -> bool {
    dir.join("native/llvm/bin").is_dir() && dir.join("native/sysroot").is_dir()
}

/// 版本：`<sdk>/../sdk-pkg.json` 的 `displayName（API <n>）`；读不到回退
/// `native/oh-uni-package.json` 的 `version`。两处都没有返回 `None`。
fn sdk_version(sdk: &Path) -> Option<String> {
    if let Some(text) = sdk
        .parent()
        .and_then(|default| std::fs::read_to_string(default.join("sdk-pkg.json")).ok())
    {
        let display = json_string(&text, "displayName");
        let api = json_string(&text, "apiVersion");
        match (display, api) {
            (Some(display), Some(api)) => return Some(format!("{display}（API {api}）")),
            (Some(display), None) => return Some(display),
            _ => {}
        }
    }
    json_string(
        &std::fs::read_to_string(sdk.join("native/oh-uni-package.json")).ok()?,
        "version",
    )
}

/// 扁平 JSON 里取一个字符串字段（本探测只读自家 SDK 描述文件的两个平铺键）。
fn json_string(text: &str, key: &str) -> Option<String> {
    let rest = text.split(&format!("\"{key}\"")).nth(1)?;
    let value = rest.split(':').nth(1)?;
    let value = value.trim_start().strip_prefix('"')?;
    Some(value.split('"').next()?.to_string())
}
