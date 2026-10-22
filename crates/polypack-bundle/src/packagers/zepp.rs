//! Zepp OS 小程序打包器（T-M7-08）：`.zab` 外层 = `manifest.json` + `<md5>.zpk`，
//! zpk 内含 `device.zip`（+ 可选 `app-side.zip`），与官方 zpm 的打包形态同构。核心不直接
//! 运行（`runtime = none`）：设备侧是 Zepp JS API 的应用树，能力由 JS 侧实现。
//!
//! 输入口径：`BundleInputs.miniapp_host` 下的 `zepp/`（或宿主根自身）须有 `app.json` +
//! `app.js`。打包器不做 qjsc 字节码编译（官方 zeus build 的编译腿），设备树按工程原样入包；
//! `zeus` 与模拟器是**校验 / 安装腿**：缺席 SKIP-with-print，结构断言真跑。
//!
//! 确定性：`bundleInfo.timeStamp` 固定 0（官方用当前时间），zpk 名取内容 md5——同输入二跑
//! 字节一致。

use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use polypack_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::{BundleInputs, version_parts};
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp;
use crate::zipwrite;

/// 目标 id / 产物格式（与 `builtin.rs` 的 `zepp` 声明逐字一致）。
const ID: &str = "zepp";
const FORMAT: &str = "zab";

/// 打包暂存目录（`<out_dir>/zepp/`）。
const WORK_DIR: &str = "zepp";

/// 设备树复制时丢弃的顶层项（工程元数据 / 官方编译落点；设备包只带运行期树）。
const SKIP_TOP: [&str; 8] = [
    "app-side",
    "node_modules",
    "dist",
    "package.json",
    "_package.json",
    "jsconfig.json",
    ".git",
    ".DS_Store",
];

/// Zepp OS 小程序打包器。
pub struct Zepp;

impl Packager for Zepp {
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
        let config = ZeppConfig::load(&project, inputs)?;
        let work = inputs.out_dir.join(WORK_DIR);
        let _ = std::fs::remove_dir_all(&work);
        let package_dir = work.join("package");
        std::fs::create_dir_all(&package_dir).map_err(io_err(&package_dir))?;

        let device_tree = work.join("device");
        copy_device_tree(&project, &device_tree)?;
        let device_zip = work.join("device.zip");
        zipwrite::write_zip(&webapp::collect_files(&device_tree)?, &device_zip)?;

        let app_side = project.join("app-side");
        let has_app_side = app_side.is_dir();
        let app_side_zip = work.join("app-side.zip");
        if has_app_side {
            let tree = work.join("app-side");
            fsutil::copy_tree(&app_side, &tree)?;
            zipwrite::write_zip(&webapp::collect_files(&tree)?, &app_side_zip)?;
        }

        let zpk_dir = work.join("zpk");
        std::fs::create_dir_all(&zpk_dir).map_err(io_err(&zpk_dir))?;
        fsutil::copy_file(&device_zip, &zpk_dir.join("device.zip"), false)?;
        if has_app_side {
            fsutil::copy_file(&app_side_zip, &zpk_dir.join("app-side.zip"), false)?;
        }
        let zpk = work.join("app.zpk");
        zipwrite::write_zip(&webapp::collect_files(&zpk_dir)?, &zpk)?;
        let zpk_bytes = std::fs::read(&zpk).map_err(io_err(&zpk))?;
        let md5 = format!("{:x}", Md5::digest(&zpk_bytes));
        let zpk_name = format!("{md5}.zpk");
        fsutil::copy_file(&zpk, &package_dir.join(&zpk_name), false)?;
        fsutil::write_text(
            &package_dir.join("manifest.json"),
            &zab_manifest(&config, &zpk_name, &md5),
        )?;

        let package = inputs.out_dir.join(format!(
            "{}-{}.zab",
            inputs.product.kebab_name(),
            inputs.product.version
        ));
        zipwrite::write_zip(&webapp::collect_files(&package_dir)?, &package)?;
        let structure = check_zepp_structure(&package)?;

        let notes = vec![
            format!(
                "产物：{}（{} 条目；manifest.json + {}，zpk 内 device.zip{}）",
                package.display(),
                structure.entries.len(),
                structure.zpk_name,
                if structure.has_app_side {
                    " + app-side.zip"
                } else {
                    ""
                }
            ),
            format!(
                "应用：{}（appId {}，版本 {} / {}，configVersion {}），zpk md5={}",
                structure.app_name,
                structure.app_id,
                structure.version_code,
                structure.version_name,
                structure.config_version,
                structure.md5
            ),
            "核心不直接运行、能力由 Zepp JS API 侧实现：runtime = none，设备树按工程原样入包\
             （官方 zeus build 的 qjsc 字节码编译腿不在本项目）"
                .to_string(),
            "zeus / 模拟器缺席：开发者工具校验与模拟器安装腿 SKIP-with-print，内建结构断言\
             （外层清单 + zpk md5 + device.zip 内 app.json）真跑；签名 / 提审归 Zepp 商店平台侧"
                .to_string(),
        ];
        fsutil::file_artifact(ID, FORMAT, &package, notes)
    }
}

fn check_plan(inputs: &BundleInputs, spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format != FORMAT {
        return Err(BundleError::InvalidInput {
            packager: ID,
            field: "spec.package.format",
            value: spec.package.format.clone(),
            reason: format!("打包器要求格式 `{FORMAT}`"),
        });
    }
    project_root(inputs)?;
    inputs.product.validate(ID)
}

/// Zepp OS 工程根：`<miniapp_host>/zepp` 优先，否则宿主根自身（须有 app.json + app.js）。
fn project_root(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    let host = inputs
        .miniapp_host
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: ID,
            field: "miniapp_host",
        })?;
    let nested = host.join("zepp");
    let root = if is_project(&nested) {
        nested
    } else if is_project(host) {
        host.to_path_buf()
    } else {
        // 工程本身缺席 = 输入缺失（环境槽 SKIP-with-print）；`--target all` 不应硬失败。
        return Err(BundleError::MissingInput {
            packager: ID,
            field: "zepp.project",
        });
    };
    Ok(root)
}

fn is_project(dir: &Path) -> bool {
    dir.join("app.json").is_file() && dir.join("app.js").is_file()
}

/// `app.json` 的清单面（官方 zpm 从 app.json 取 version / appType / configVersion；
/// `appName` / `appId` 只做工程合法性校验，设备侧以原样入包的 `app.json` 为准）。
struct ZeppConfig {
    app_type: String,
    ext_type: Option<String>,
    version_code: u64,
    version_name: String,
    config_version: String,
    platforms: Vec<serde_json::Value>,
}

impl ZeppConfig {
    fn load(project: &Path, inputs: &BundleInputs) -> Result<Self, BundleError> {
        let path = project.join("app.json");
        let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        let root: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| bad("app.json", &path, format!("非法 JSON：{err}")))?;
        let app = root
            .get("app")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| bad("app.json", &path, "缺 app 对象".to_string()))?;
        if app
            .get("appName")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(bad("app.appName", &path, "须为非空字符串".to_string()));
        }
        if app
            .get("appId")
            .and_then(serde_json::Value::as_u64)
            .is_none()
        {
            return Err(bad("app.appId", &path, "须为整数".to_string()));
        }
        let version = app.get("version").and_then(serde_json::Value::as_object);
        let parts = version_parts(&inputs.product.version);
        let version_code = version
            .and_then(|value| value.get("code"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(parts.0);
        let version_name = version
            .and_then(|value| value.get("name"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&inputs.product.version)
            .to_string();
        let mut platforms = Vec::new();
        if let Some(targets) = root.get("targets").and_then(serde_json::Value::as_object) {
            for target in targets.values() {
                if let Some(items) = target
                    .get("platforms")
                    .and_then(serde_json::Value::as_array)
                {
                    platforms.extend(items.iter().cloned());
                }
            }
        }
        if platforms.is_empty() {
            platforms.push(serde_json::json!({ "deviceSource": "default" }));
        }
        Ok(Self {
            app_type: app
                .get("appType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("app")
                .to_string(),
            ext_type: app
                .get("extType")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            version_code,
            version_name,
            config_version: root
                .get("configVersion")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("v2")
                .to_string(),
            platforms,
        })
    }
}

/// 外层 `.zab` 清单（官方 zpm `f()`：configVersion / zpks / bundleInfo）。
fn zab_manifest(config: &ZeppConfig, zpk_name: &str, md5: &str) -> String {
    let mut zpk = serde_json::json!({
        "name": zpk_name,
        "version": { "code": config.version_code, "name": config.version_name.clone() },
        "appType": config.app_type.clone(),
        "platforms": config.platforms.clone(),
        "md5": md5,
    });
    if let Some(ext_type) = &config.ext_type {
        zpk["extType"] = serde_json::json!(ext_type);
    }
    pretty(serde_json::json!({
        "configVersion": config.config_version.clone(),
        "zpks": [zpk],
        "bundleInfo": { "zpm": "1.0.0", "timeStamp": 0 },
    }))
}

/// 设备树：整棵工程去掉工程元数据与 `app-side/`（后者单独进 zpk）。
fn copy_device_tree(src: &Path, dst: &Path) -> Result<(), BundleError> {
    std::fs::create_dir_all(dst).map_err(io_err(dst))?;
    for entry in std::fs::read_dir(src).map_err(io_err(src))? {
        let entry = entry.map_err(io_err(src))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP_TOP.contains(&name.as_str()) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type().map_err(io_err(&from))?.is_dir() {
            fsutil::copy_tree(&from, &to)?;
        } else {
            fsutil::copy_file(&from, &to, false)?;
        }
    }
    Ok(())
}

fn pretty(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value)
        .map(|text| format!("{text}\n"))
        .unwrap_or_else(|_| value.to_string())
}

/// 结构自检（打包器自检与 verify 腿同一实现）：外层 manifest → zpk md5 → device.zip app.json。
pub fn check_zepp_structure(package: &Path) -> Result<ZeppStructure, BundleError> {
    let bad = |reason: String| BundleError::InvalidInput {
        packager: ID,
        field: "zab(结构自检)",
        value: package.display().to_string(),
        reason,
    };
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    let text = zipwrite::entry_text(package, "manifest.json")?
        .ok_or_else(|| bad("缺 manifest.json（外层清单必须落在 .zab 根）".to_string()))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| bad(format!("manifest.json 非法 JSON：{err}")))?;
    let config_version = manifest["configVersion"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad("缺 configVersion".to_string()))?
        .to_string();
    let zpks = manifest["zpks"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| bad("zpks 须为非空数组".to_string()))?;
    let zpk = &zpks[0];
    let zpk_name = zpk["name"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad("zpks[0].name 缺失".to_string()))?
        .to_string();
    let md5 = zpk["md5"]
        .as_str()
        .filter(|value| value.len() == 32 && value.chars().all(|ch| ch.is_ascii_hexdigit()))
        .ok_or_else(|| bad("zpks[0].md5 须为 32 位十六进制".to_string()))?
        .to_string();
    let version_name = zpk["version"]["name"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad("zpks[0].version.name 缺失".to_string()))?
        .to_string();
    let version_code = zpk["version"]["code"]
        .as_u64()
        .ok_or_else(|| bad("zpks[0].version.code 须为整数".to_string()))?;
    if zpk["platforms"]
        .as_array()
        .is_none_or(|items| items.is_empty())
    {
        return Err(bad("zpks[0].platforms 须为非空数组".to_string()));
    }
    if zpk_name != format!("{md5}.zpk") {
        return Err(bad(format!(
            "zpk 名 {zpk_name} 与 md5 {md5} 不同源（官方 zpm 以内容 md5 命名）"
        )));
    }
    if !entries.iter().any(|entry| entry == &zpk_name) {
        return Err(bad(format!("zpk {zpk_name} 不在 .zab 内")));
    }
    let zpk_bytes = zipwrite::entry_bytes(package, &zpk_name)?
        .ok_or_else(|| bad(format!("zpk {zpk_name} 读不出字节")))?;
    let actual = format!("{:x}", Md5::digest(&zpk_bytes));
    if actual != md5 {
        return Err(bad(format!("zpk md5 不一致：清单 {md5} / 实际 {actual}")));
    }
    let zpk_entries = zipwrite::bytes_entry_names(&zpk_bytes, package)?;
    if !zpk_entries.iter().any(|entry| entry == "device.zip") {
        return Err(bad("zpk 内缺 device.zip".to_string()));
    }
    let has_app_side = zpk_entries.iter().any(|entry| entry == "app-side.zip");
    let device_bytes = zipwrite::bytes_entry(&zpk_bytes, "device.zip", package)?
        .ok_or_else(|| bad("device.zip 读不出字节".to_string()))?;
    let device_entries = zipwrite::bytes_entry_names(&device_bytes, package)?;
    let app_text = zipwrite::bytes_entry(&device_bytes, "app.json", package)?
        .and_then(|raw| String::from_utf8(raw).ok())
        .ok_or_else(|| bad("device.zip 缺 app.json".to_string()))?;
    let app: serde_json::Value = serde_json::from_str(&app_text)
        .map_err(|err| bad(format!("device.zip 的 app.json 非法：{err}")))?;
    let app_name = app["app"]["appName"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| bad("device.zip app.json 缺 app.appName".to_string()))?
        .to_string();
    let app_id = app["app"]["appId"]
        .as_u64()
        .ok_or_else(|| bad("device.zip app.json 缺 app.appId（整数）".to_string()))?;
    let bytes = std::fs::metadata(package)
        .map_err(|source| BundleError::Io {
            path: package.to_path_buf(),
            source,
        })?
        .len();
    if bytes == 0 {
        return Err(bad("包体积为 0".to_string()));
    }
    Ok(ZeppStructure {
        entries,
        config_version,
        app_name,
        app_id,
        version_code,
        version_name,
        zpk_name,
        md5,
        has_app_side,
        device_entries,
        bytes,
    })
}

/// 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeppStructure {
    /// `.zab` 条目名（排序）。
    pub entries: Vec<String>,
    /// 外层 `configVersion`。
    pub config_version: String,
    /// 设备侧 `app.json` 的 `app.appName`。
    pub app_name: String,
    /// 设备侧 `app.json` 的 `app.appId`。
    pub app_id: u64,
    /// zpk 版本号（整数）。
    pub version_code: u64,
    /// zpk 版本名。
    pub version_name: String,
    /// zpk 文件名（`<md5>.zpk`）。
    pub zpk_name: String,
    /// zpk 内容 md5。
    pub md5: String,
    /// zpk 是否随包 `app-side.zip`。
    pub has_app_side: bool,
    /// device.zip 条目名（排序）。
    pub device_entries: Vec<String>,
    /// 包体积（字节）。
    pub bytes: u64,
}

fn bad(field: &'static str, path: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field,
        value: path.display().to_string(),
        reason,
    }
}
