//! 小程序族打包器（T-M3-06）：微信 / 支付宝 / 抖音走 Taro 编译（`taro build --type`），
//! 快应用到「编译工程」为止（Taro 4 无 quickapp 平台插件；`.rpk` 由 T-M3-12 hap-toolkit 环节收口）。
//!
//! 产物为目录 `<out_dir>/miniapp/`：`dist/`（Taro 编译出的代码包本体）+ devtools 描述符
//! （`miniprogramRoot` 归一到 `./dist/`）——即「开发者工具打开该目录即可预览」的形态。
//! 编译前先跑宿主 API 白名单预检（T-M3-05 的 `bin/check-api.mjs`，越界调用构建期拒绝）；
//! 体积 / 预算门禁（T-M3-08）挂在同一条命令链上。
//!
//! wasm 沙箱出口（T-M3-07 的 `<工程根>/bridge/wasm-bindgen/` 四文件）在场即整目录拷进
//! 随包根（Taro 目标 `dist/`、快应用 `src/`），加载契约见出口内的 `polypack-loader.js`；
//! 缺席不报错（无 wasm 出口的工程照常打包）。
//!
//! 宿主（Taro 工程）解析：`<host_project>/ui`（poly 工程的 ui/ 即 Taro 工程）→ `<host_project>`
//! 自身 → `<host_project>/ui/adapters/taro/host`（本仓适配器布局：宿主骨架落包内 `host/`）。
//! Taro 工程判据 = `config/index.js` + `src/app.config.js` 齐备；缺宿主 / 缺工具链一律
//! [`BundleError::MissingInput`] / [`BundleError::ToolMissing`]（CLI 侧按环境性缺失跳过，
//! 不产出半成品包——devtools 本机大概率缺席，编译产物齐备 + 结构断言是本地真跑的重点）。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, version_parts};
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// 产物目录名（`<out_dir>/miniapp/`）。
const MINIAPP_DIR: &str = "miniapp";

/// 代码包目录名（Taro `outputRoot` 落点）。
const DIST_DIR: &str = "dist";

/// wasm 沙箱出口（T-M3-07）在随包根下的相对路径（与工程根侧同名，引用口径统一）。
const BRIDGE_REL: &str = "bridge/wasm-bindgen";

/// 页面路径（app.config 的 pages 清单与结构断言的锚点）。
const PAGE: &str = "pages/index/index";

/// 快应用图标占位（32×32 纯色 PNG，`#f8fafc` 与 tokens 背景同色）。
///
/// 内联字节（非外部资源）：manifest 的 `icon` 须有实体文件，工程在任意宿主生成都自包含。
/// 品牌图标（真实素材与商店提审规格）归 T-M3-12（可复用 T-M1-14 的图标派生管线出图）。
const QUICKAPP_LOGO_PNG: [u8; 98] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 32, 0, 0, 0, 32, 8, 2,
    0, 0, 0, 252, 24, 237, 163, 0, 0, 0, 41, 73, 68, 65, 84, 120, 218, 237, 205, 177, 9, 0, 0, 8,
    3, 176, 254, 127, 177, 136, 131, 87, 56, 8, 129, 236, 73, 245, 156, 138, 64, 32, 16, 8, 4, 2,
    129, 64, 32, 248, 18, 44, 166, 1, 184, 166, 242, 169, 218, 246, 0, 0, 0, 0, 73, 69, 78, 68,
    174, 66, 96, 130,
];

/// 宿主工程候选（相对 `miniapp_host`；空串 = 宿主根自身）。
const HOST_CANDIDATES: [&str; 3] = ["ui", "", "ui/adapters/taro/host"];

/// 页面 bundle 里必须出现的装配标记（T-M3-05 示例页的语义锚点）。
///
/// 只取 ASCII 标记：Taro / terser 把非 ASCII 字面量转义成 `\uXXXX` 序列
/// （真跑实证：示例页的中文按钮文案在产物里只剩转义序列，原文匹配为零），中文标记不可断言。
/// `poly build --target` 是示例页版本行的前缀（目标名在运行时拼接）。
const PAGE_MARKERS: [&str; 4] = ["PolyPack App", "counter", "reset", "poly build --target"];

/// 应用 bundle 里必须出现的装配自检标记（host/src/app.js）。
const APP_MARKER: &str = "POLYPACK_TARO";

/// 单目标差异表——与 `crates/poly-targets/src/builtin.rs` 的 L2 小程序族声明逐一对照。
struct MiniSpec {
    /// 目标 id（`TargetSpec.id`）。
    id: &'static str,
    /// 产物格式（`TargetSpec.package.format`）。
    format: &'static str,
    /// `taro build --type` 值（快应用无 Taro 平台插件 → `None`）。
    taro_type: Option<&'static str>,
    /// 宿主侧 devtools 描述符源文件名（读出来做 `miniprogramRoot` 归一；快应用 `None`）。
    descriptor_source: Option<&'static str>,
    /// 产物根落地的 devtools 描述符文件名（工具实际读的名字；快应用 `None`）。
    descriptor_target: Option<&'static str>,
    /// 开发者工具名（notes 里的预览指引）。
    devtools: &'static str,
    /// 页面标记语言扩展名（Taro 编译产物结构断言的必需文件面）。
    markup: &'static str,
    /// 页面样式扩展名（应用级样式文件 `app.<style>`，Taro 把页面样式上提到这一层）。
    style: &'static str,
}

impl MiniSpec {
    /// `taro build --type` 值（仅 Taro 承载三目标调用）。
    fn taro_type(&self) -> &'static str {
        self.taro_type
            .unwrap_or_else(|| panic!("目标 {} 无 Taro 编译腿", self.id))
    }
}

const WECHAT: MiniSpec = MiniSpec {
    id: "wechat",
    format: "weapp",
    taro_type: Some("weapp"),
    descriptor_source: Some("project.config.json"),
    descriptor_target: Some("project.config.json"),
    devtools: "微信开发者工具",
    markup: "wxml",
    style: "wxss",
};

const ALIPAY: MiniSpec = MiniSpec {
    id: "alipay",
    format: "alipay",
    taro_type: Some("alipay"),
    descriptor_source: Some("mini.project.json"),
    descriptor_target: Some("mini.project.json"),
    devtools: "支付宝小程序开发者工具",
    markup: "axml",
    style: "acss",
};

// 抖音：Taro 的 tt 平台约定源名是 `project.tt.json`（宿主侧），但会把它重写为
// `dist/project.config.json` 落进产物——devtools 实际读的名字因此是后者（真跑实证）。
const DOUYIN: MiniSpec = MiniSpec {
    id: "douyin",
    format: "tt",
    taro_type: Some("tt"),
    descriptor_source: Some("project.tt.json"),
    descriptor_target: Some("project.config.json"),
    devtools: "抖音开发者工具",
    markup: "ttml",
    style: "ttss",
};

const QUICKAPP: MiniSpec = MiniSpec {
    id: "quickapp",
    format: "quickapp",
    taro_type: None,
    descriptor_source: None,
    descriptor_target: None,
    devtools: "快应用调试器（hap-toolkit）",
    markup: "ux",
    style: "",
};

/// 微信小程序代码包（`weapp`）。
pub struct WechatMini;
/// 支付宝小程序代码包（`alipay`）。
pub struct AlipayMini;
/// 抖音小程序代码包（`tt`）。
pub struct DouyinMini;
/// 快应用编译工程（`quickapp`；`.rpk` 由 T-M3-12 hap-toolkit 收口）。
pub struct QuickApp;

impl Packager for WechatMini {
    fn id(&self) -> &'static str {
        WECHAT.id
    }

    fn format(&self) -> &'static str {
        WECHAT.format
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_taro(inputs, spec, &WECHAT)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_taro(inputs, spec, &WECHAT)
    }
}

impl Packager for AlipayMini {
    fn id(&self) -> &'static str {
        ALIPAY.id
    }

    fn format(&self) -> &'static str {
        ALIPAY.format
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_taro(inputs, spec, &ALIPAY)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_taro(inputs, spec, &ALIPAY)
    }
}

impl Packager for DouyinMini {
    fn id(&self) -> &'static str {
        DOUYIN.id
    }

    fn format(&self) -> &'static str {
        DOUYIN.format
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_taro(inputs, spec, &DOUYIN)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_taro(inputs, spec, &DOUYIN)
    }
}

impl Packager for QuickApp {
    fn id(&self) -> &'static str {
        QUICKAPP.id
    }

    fn format(&self) -> &'static str {
        QUICKAPP.format
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, &QUICKAPP)?;
        inputs.product.validate(QUICKAPP.id)?;
        // SKIP-with-print：计划里如实记，不伪装成编译过
        Ok(BundlePlan::new().skipping(
            "taro",
            "Taro 4 无 quickapp 平台插件（npm 未发布 @tarojs/plugin-platform-quickapp）：\
             快应用编译腿跳过，本目标直接组装 hap-toolkit 就绪的编译工程（.rpk 归 T-M3-12）",
        ))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_quickapp(inputs, spec)
    }
}

// ---------------------------------------------------------------- Taro 三目标

/// 计划：白名单预检 → Taro 编译（产物直落本目标的 `dist/`）。
fn plan_taro(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    mini: &MiniSpec,
) -> Result<BundlePlan, BundleError> {
    check_format(spec, mini)?;
    let host = resolve_host(inputs, mini)?;
    let taro = taro_cli(&host, mini)?;
    let guard = apiguard_cli(&host, mini)?;

    Ok(BundlePlan::new()
        // 1. 宿主 API 白名单预检（越界调用构建期拒绝，非零退出即中止本目标）
        .with_command(
            Command::new(
                "node",
                [
                    guard.display().to_string(),
                    host.join("src").display().to_string(),
                ],
            )
            .in_dir(&host),
        )
        // 2. Taro 编译：平台经 --type 切换，产物目录经 POLYPACK_OUTPUT_ROOT 注入
        .with_command(
            Command::new(
                taro.display().to_string(),
                ["build", "--type", mini.taro_type()],
            )
            .in_dir(&host)
            .with_env(
                "POLYPACK_OUTPUT_ROOT",
                dist_dir(inputs).display().to_string(),
            ),
        ))
}

/// 打包：跑计划 → 结构断言 → 描述符归一 → 目录产物。
fn package_taro(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    mini: &MiniSpec,
) -> Result<Artifact, BundleError> {
    let plan = plan_taro(inputs, spec, mini)?;
    let host = resolve_host(inputs, mini)?;
    let dir = miniapp_dir(inputs);
    let dist = dist_dir(inputs);
    // 与 web 的 site/ 同口径：整份替换，不留上一轮残留
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;

    exec::run_all(&plan.commands)?;
    assert_code_package(&dist, mini)?;
    let bridge_note = stage_wasm_bridge(inputs, &dist, mini)?;

    let mut notes = vec![
        format!(
            "代码包：{}（taro build --type {}）",
            dist.display(),
            mini.taro_type()
        ),
        // devtools 预览腿：本机无官方工具链（Linux 宿主），如实标注而非伪造
        format!(
            "预览：{}打开 {}（miniprogramRoot=./dist/）——本机无 devtools，预览腿 SKIP-with-print",
            mini.devtools,
            dir.display()
        ),
        "编译前已过宿主 API 白名单预检（@polypack/adapter-taro/bin/check-api.mjs）".to_string(),
    ];
    if let Some(note) = bridge_note {
        notes.push(note);
    }
    if mini.descriptor_target.is_some() {
        notes.push(write_descriptor(&dir, &host, mini)?);
    }
    for skipped in &plan.skipped {
        notes.push(format!("跳过：{}（{}）", skipped.tool, skipped.reason));
    }

    fsutil::dir_artifact(mini.id, mini.format, &dir, notes)
}

/// 代码包结构断言（devtools 缺席时的替代证据）：应用级四件套 + 页面三件套齐备，
/// 路由 / 装配标记 / 示例页标记在产物里。
///
/// 与真跑输出对齐的两个事实（T-M3-06 实证）：页面**没有**独立样式文件——Taro 把页面样式
/// 上提到应用级 `app.<style>`（断言它即可）；页面节点树由运行时序列化进 Taro 模板引擎，
/// 静态页面标记只有一行 base 导入 + `taro_tmpl` 桥接（`id="counter"` 之类的节点锚点
/// 不落在标记文件里，桥接行即结构证据）。
fn assert_code_package(dist: &Path, mini: &MiniSpec) -> Result<(), BundleError> {
    let required = [
        "app.json".to_string(),
        "app.js".to_string(),
        format!("app.{}", mini.style),
        format!("base.{}", mini.markup),
        format!("{PAGE}.js"),
        format!("{PAGE}.json"),
        format!("{PAGE}.{}", mini.markup),
    ];
    for rel in &required {
        if !dist.join(rel).is_file() {
            return Err(BundleError::InvalidInput {
                packager: mini.id,
                field: "dist(代码包)",
                value: rel.clone(),
                reason: format!(
                    "Taro 编译产物缺 {rel}——编译未产出完整代码包（{}）",
                    dist.display()
                ),
            });
        }
    }

    let app_json = read_text(&dist.join("app.json"))?;
    if !app_json.contains(PAGE) {
        return Err(BundleError::InvalidInput {
            packager: mini.id,
            field: "dist(app.json)",
            value: PAGE.to_string(),
            reason: "代码包路由清单不含装配页（app.config 的 pages 声明未进产物）".to_string(),
        });
    }
    let app_js = read_text(&dist.join("app.js"))?;
    if !app_js.contains(APP_MARKER) {
        return Err(BundleError::InvalidInput {
            packager: mini.id,
            field: "dist(app.js)",
            value: APP_MARKER.to_string(),
            reason: "app bundle 缺装配自检标记（宿主 app.js 的装配链未进产物）".to_string(),
        });
    }
    let page_js = read_text(&dist.join(format!("{PAGE}.js")))?;
    for marker in PAGE_MARKERS {
        if !page_js.contains(marker) {
            return Err(BundleError::InvalidInput {
                packager: mini.id,
                field: "dist(页面 bundle)",
                value: marker.to_string(),
                reason: "页面 bundle 缺示例页标记（装配层页面未进产物）".to_string(),
            });
        }
    }
    let markup = read_text(&dist.join(format!("{PAGE}.{}", mini.markup)))?;
    let base_import = format!("base.{}", mini.markup);
    for anchor in [base_import.as_str(), "taro_tmpl"] {
        if !markup.contains(anchor) {
            return Err(BundleError::InvalidInput {
                packager: mini.id,
                field: "dist(页面标记)",
                value: anchor.to_string(),
                reason: "页面标记缺模板桥接（base 导入 / taro_tmpl 模板未进产物）".to_string(),
            });
        }
    }
    Ok(())
}

/// devtools 描述符：宿主侧源文件（[`MiniSpec::descriptor_source`]，如抖音的 `project.tt.json`）
/// 读出后 `miniprogramRoot` 一律归一到 `./dist/`，以 [`MiniSpec::descriptor_target`] 名落地
/// （devtools 实际读的名字）；宿主无源文件时按目标生成最小描述符。返回 notes 行的来源说明。
fn write_descriptor(dir: &Path, host: &Path, mini: &MiniSpec) -> Result<String, BundleError> {
    let name = mini
        .descriptor_target
        .expect("调用方已判定描述符在场（descriptor_target 为 Some）");
    let source = host.join(mini.descriptor_source.unwrap_or(name));
    let (value, origin) = if source.is_file() {
        let text = read_text(&source)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&text).map_err(|parse| BundleError::InvalidInput {
                packager: mini.id,
                field: "devtools 描述符",
                value: source.display().to_string(),
                reason: format!("JSON 解析失败：{parse}"),
            })?;
        value["miniprogramRoot"] = serde_json::Value::String("./dist/".to_string());
        (value, format!("源 {}", source.display()))
    } else {
        (
            serde_json::json!({
                "miniprogramRoot": "./dist/",
                "projectname": "polypack",
                "compileType": "miniprogram",
            }),
            "宿主无同名文件（按目标生成最小描述符）".to_string(),
        )
    };
    let text = serde_json::to_string_pretty(&value).map_err(|error| BundleError::Io {
        path: dir.join(name),
        source: std::io::Error::other(error),
    })?;
    fsutil::write_text(&dir.join(name), &format!("{text}\n"))?;
    Ok(format!("devtools 描述符：{name}（{origin}）"))
}

// ---------------------------------------------------------------- 快应用编译工程

/// 打包：组装 hap-toolkit 就绪的编译工程（`src/manifest.json` + `src/app.ux` + 页面）。
///
/// shortcut: 工程清单取产品元数据 + 最小展示配置；快应用 API 面 / 安全区适配归 T-M3-12
/// （真 `hap build` + lint 校验后再补）。
fn package_quickapp(inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
    let plan = QuickApp.plan(inputs, spec)?;
    let product = &inputs.product;
    let dir = miniapp_dir(inputs);
    let src = dir.join("src");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(src.join("pages").join("index")).map_err(io_err(&src))?;

    // 快应用 versionCode：与 Android 同一条派生（主*10000+次*100+补）
    let version_code = crate::packagers::android::derive_version_code(&product.version, None);
    let (major, minor, patch) = version_parts(&product.version);
    // 必填字段面 = hap-toolkit 2.1.1 的 manifest schema（真跑实证：缺 icon / config /
    // router.entry 时 `hap build` 报 schema 错误；icon 路径相对 `src/`）。
    let manifest = serde_json::json!({
        "package": product.identifier,
        "name": product.name,
        "icon": "logo.png",
        "versionName": product.version,
        "versionCode": version_code,
        "minPlatformVersion": 1070,
        "features": [],
        "deviceTypeList": ["phone"],
        "config": { "designWidth": 750, "logLevel": "log" },
        "router": {
            "entry": "pages/index",
            "pages": { "pages/index": { "component": "index" } },
        },
        "display": {
            "titleBarBackgroundColor": "#f8fafc",
            "titleBarTextColor": "#0f172a",
            "titleBarText": product.name,
        },
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| BundleError::Io {
        path: src.join("manifest.json"),
        source: std::io::Error::other(error),
    })?;
    fsutil::write_text(&src.join("manifest.json"), &format!("{text}\n"))?;

    let app_ux = format!(
        "<script>\n\
         // PolyPack 编译工程入口（T-M3-06 生成的骨架：Taro 4 无 quickapp 平台插件，\n\
         // 快应用编译腿跳过——本工程即平台工具链（hap-toolkit）的输入，.rpk 归 T-M3-12）\n\
         export default {{\n\
         \x20 onCreate() {{\n\
         \x20   console.log('POLYPACK_QUICKAPP target=quickapp version={version}')\n\
         \x20 }},\n\
         }}\n\
         </script>\n",
        version = product.version
    );
    fsutil::write_text(&src.join("app.ux"), &app_ux)?;

    let page_ux = format!(
        "<!-- 示例页（与 Taro 侧 createExamplePage 语义同形：标题 / 版本行 / 计数按钮 / 重置） -->\n\
         <template>\n\
         \x20 <div class=\"card\" id=\"card\">\n\
         \x20   <text class=\"title\">PolyPack App</text>\n\
         \x20   <text class=\"muted\">版本 {major}.{minor}.{patch} · 由 PolyPack 构建（poly build --target quickapp）</text>\n\
         \x20   <input class=\"button\" type=\"button\" id=\"counter\" value=\"点击计数：{{{{clicks}}}}\" onclick=\"press\" />\n\
         \x20   <input class=\"button\" type=\"button\" id=\"reset\" value=\"重置\" onclick=\"reset\" />\n\
         \x20 </div>\n\
         </template>\n\
         <script>\n\
         export default {{\n\
         \x20 data() {{\n\
         \x20   return {{ clicks: 0 }}\n\
         \x20 }},\n\
         \x20 press() {{\n\
         \x20   this.clicks += 1\n\
         \x20 }},\n\
         \x20 reset() {{\n\
         \x20   this.clicks = 0\n\
         \x20 }},\n\
         }}\n\
         </script>\n\
         <style>\n\
         .card {{\n\
         \x20 flex-direction: column;\n\
         \x20 padding: 24px;\n\
         }}\n\
         .title {{\n\
         \x20 font-size: 32px;\n\
         \x20 font-weight: bold;\n\
         \x20 color: #0f172a;\n\
         }}\n\
         .muted {{\n\
         \x20 font-size: 24px;\n\
         \x20 color: #64748b;\n\
         }}\n\
         .button {{\n\
         \x20 margin-top: 16px;\n\
         \x20 background-color: #ea580c;\n\
         \x20 color: #ffffff;\n\
         \x20 border-radius: 12px;\n\
         }}\n\
         </style>\n"
    );
    fsutil::write_text(&src.join("pages").join("index").join("index.ux"), &page_ux)?;

    // manifest.icon 的实体文件（hap-toolkit 真跑实证：schema 要求 icon 字段，打包按 src/ 下取图）
    std::fs::write(src.join("logo.png"), QUICKAPP_LOGO_PNG)
        .map_err(io_err(&src.join("logo.png")))?;
    // 设备细分配置：hap-toolkit 的 device-type-plugin 按 deviceTypeList 找 `config-<device>.json`
    // 深合并进 manifest（真跑实证：缺席只 WARN，但如实补齐——空对象 = 无逐设备覆盖）
    fsutil::write_text(&src.join("config-phone.json"), "{}\n")?;

    // 工程元数据：hap-toolkit 的 `hap build` 从工程根读 package.json（init 模板同形）
    let package_json = format!(
        "{{\n  \"name\": \"{}\",\n  \"version\": \"{}\",\n  \"toolkitVersion\": \"2.1.1\",\n  \
         \"deviceTypeList\": [\"phone\"]\n}}\n",
        product.kebab_name(),
        product.version
    );
    fsutil::write_text(&dir.join("package.json"), &package_json)?;

    let bridge_note = stage_wasm_bridge(inputs, &src, &QUICKAPP)?;

    let mut notes = vec![
        format!(
            "编译工程：{}（hap-toolkit 就绪：src/manifest.json + src/app.ux + src/pages/index/index.ux）",
            dir.display()
        ),
        "Taro 编译腿跳过：Taro 4 无 quickapp 平台插件（npm 未发布 @tarojs/plugin-platform-quickapp）\
         ——快应用不走 Taro 编译，工程由本打包器组装"
            .to_string(),
        "manifest 必填面（icon / config / router.entry）与 src/config-phone.json 按 hap-toolkit \
         2.1.1 的 schema 补齐（真跑 `hap build` 实证）；icon 为 32×32 占位 PNG，品牌图标归 T-M3-12"
            .to_string(),
        "`.rpk` 与商店校验：T-M3-12（hap-toolkit build + lint 入 verify 阶段）".to_string(),
        format!(
            "快应用调试器 / 平台预览：本机无（R4：自动化止步提审）；工程清单 package={} versionCode={version_code}",
            product.identifier
        ),
    ];
    if let Some(note) = bridge_note {
        notes.push(note);
    }
    for skipped in &plan.skipped {
        notes.push(format!("跳过：{}（{}）", skipped.tool, skipped.reason));
    }

    fsutil::dir_artifact(QUICKAPP.id, QUICKAPP.format, &dir, notes)
}

// ---------------------------------------------------------------- 共用

/// 打包格式与打包器声明必须一致（防「按 weapp 目标产出 alipay 包」）。
fn check_format(spec: &TargetSpec, mini: &MiniSpec) -> Result<(), BundleError> {
    if spec.package.format == mini.format {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: mini.id,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!(
            "打包器要求格式 `{}`（polypack.toml 的 [targets.*].package.format）",
            mini.format
        ),
    })
}

/// 宿主工程解析：候选序见 [`HOST_CANDIDATES`]。
fn resolve_host(inputs: &BundleInputs, mini: &MiniSpec) -> Result<PathBuf, BundleError> {
    let root = inputs
        .miniapp_host
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: mini.id,
            field: "miniapp_host",
        })?;
    for rel in HOST_CANDIDATES {
        let dir = if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel)
        };
        if is_taro_project(&dir) {
            return Ok(dir);
        }
    }
    Err(BundleError::MissingInput {
        packager: mini.id,
        field: "miniapp_host",
    })
}

/// Taro 工程判据（Taro 4）：编译配置 + 应用配置齐备。
///
/// shortcut: v0 只认 JS 形态的 `config/index.js` / `src/app.config.js`；TS 工程后续放宽。
fn is_taro_project(dir: &Path) -> bool {
    dir.join("config/index.js").is_file() && dir.join("src/app.config.js").is_file()
}

/// Taro CLI（宿主依赖树内 `node_modules/.bin/taro`；缺席即缺工具链，不静默降级）。
fn taro_cli(host: &Path, mini: &MiniSpec) -> Result<PathBuf, BundleError> {
    let cli = host.join("node_modules").join(".bin").join("taro");
    if cli.is_file() {
        return Ok(cli);
    }
    Err(BundleError::ToolMissing {
        packager: mini.id,
        tool: "taro".to_string(),
        hint: format!(
            "在 {} 执行 npm install（Taro 4.3 工具链含 @tarojs/cli 与三平台插件）",
            host.display()
        ),
    })
}

/// 白名单预检 CLI：宿主依赖树里的 @polypack/adapter-taro，或宿主在适配器包内时的同级 `bin/`。
fn apiguard_cli(host: &Path, mini: &MiniSpec) -> Result<PathBuf, BundleError> {
    let candidates = [
        host.join("node_modules")
            .join("@polypack")
            .join("adapter-taro")
            .join("bin")
            .join("check-api.mjs"),
        host.join("..").join("bin").join("check-api.mjs"),
    ];
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| BundleError::ToolMissing {
            packager: mini.id,
            tool: "check-api.mjs".to_string(),
            hint: "宿主依赖须含 @polypack/adapter-taro（npm install）——白名单预检缺席即不打包"
                .to_string(),
        })
}

/// 产物目录（`<out_dir>/miniapp/`）。
fn miniapp_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(MINIAPP_DIR)
}

/// 代码包目录（Taro `outputRoot` 的注入值）。
fn dist_dir(inputs: &BundleInputs) -> PathBuf {
    miniapp_dir(inputs).join(DIST_DIR)
}

/// wasm 沙箱出口（T-M3-07 的四个随包文件）随代码包分发：`miniapp_bridge` 目录整拷到
/// `dest_root`（Taro 目标 = `dist/`、快应用 = `src/`，都在各自平台的随包根内）。
///
/// 目录缺席 = 工程没跑过 `poly bridge wasm`（或本无 wasm 接口）——不属错误，notes 也不占行；
/// 在场则两个不变式文件必须齐备，缺一即拒（半成品出口目录不许静默进包）。打包期不解析
/// wasm / 胶水内容：白名单与导入面核对是 loader 的加载期闸（T-M3-07），不在这里重复。
fn stage_wasm_bridge(
    inputs: &BundleInputs,
    dest_root: &Path,
    mini: &MiniSpec,
) -> Result<Option<String>, BundleError> {
    let Some(src) = inputs.miniapp_bridge.as_deref() else {
        return Ok(None);
    };
    if !src.is_dir() {
        return Ok(None);
    }
    for invariant in ["manifest.json", "polypack-loader.js"] {
        if !src.join(invariant).is_file() {
            return Err(BundleError::InvalidInput {
                packager: mini.id,
                field: "miniapp_bridge",
                value: src.join(invariant).display().to_string(),
                reason: format!(
                    "wasm 出口目录在场但缺 {invariant}——半成品不许随包，重跑 poly bridge wasm"
                ),
            });
        }
    }
    let dest = dest_root.join(BRIDGE_REL);
    fsutil::copy_tree(src, &dest)?;
    // 只是拷，不重编：随包字节 = 出口目录当前字节（T-M3-08 的裁剪时机在出口层，
    // 「出口目录生成后、随包拷贝前」——打包器不做二次处理）
    let bytes = fsutil::tree_bytes(&dest)?;
    let wasm = match wasm_leaf_bytes(&dest)? {
        Some(wasm) => format!("，其中 wasm {wasm} 字节"),
        None => String::new(),
    };
    Ok(Some(format!(
        "wasm 沙箱出口随包：{BRIDGE_REL}/（{bytes} 字节{wasm}；随包字节即出口层 wasm-opt 裁剪后字节；\
         加载契约 polypack-loader.js 的 load(bytes)）"
    )))
}

/// 出口目录内 `.wasm` 文件的字节数（T-M3-08 体积报告；T-M3-07 四文件形态下恰一个）。
fn wasm_leaf_bytes(dir: &Path) -> Result<Option<u64>, BundleError> {
    let mut total: Option<u64> = None;
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wasm") {
            let len = entry.metadata().map_err(io_err(&path))?.len();
            total = Some(total.unwrap_or(0) + len);
        }
    }
    Ok(total)
}

/// 读文本（错误带路径）。
fn read_text(path: &Path) -> Result<String, BundleError> {
    std::fs::read_to_string(path).map_err(io_err(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_targets::{Bridge, Host, PackageSpec, Runtime};

    fn spec(id: &str, format: &str) -> TargetSpec {
        let mut spec = TargetSpec::new_stub(id, 2, &[Host::Linux]);
        spec.runtime = Runtime::Wasm;
        spec.bridge = Bridge::WasmBindgen;
        spec.ui = vec!["taro".to_string()];
        spec.package = PackageSpec {
            format: format.to_string(),
            builder: format!("taro-{id}"),
        };
        spec
    }

    fn inputs(dir: &Path) -> BundleInputs {
        let mut inputs = BundleInputs::new(
            crate::inputs::ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "契约测试合成输入".to_string(),
                homepage: None,
            },
            dir,
        );
        inputs.miniapp_host = Some(dir.to_path_buf());
        inputs
    }

    #[test]
    fn format_guard_rejects_other_formats() {
        let dir = std::env::temp_dir().join(format!("poly-miniapp-fmt-{}", std::process::id()));
        let bundle = inputs(&dir);
        let err = WechatMini
            .plan(&bundle, &spec("wechat", "alipay"))
            .expect_err("格式不符必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
        assert_eq!(WechatMini.id(), "wechat");
        assert_eq!(WechatMini.format(), "weapp");
        assert_eq!(QuickApp.id(), "quickapp");
        assert_eq!(QuickApp.format(), "quickapp");
    }

    #[test]
    fn host_and_toolchain_gaps_are_environment_slots() {
        let dir = std::env::temp_dir().join(format!("poly-miniapp-host-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录可建");

        // 缺宿主：MissingInput（CLI 侧跳过，不产出半成品包）
        let mut bundle = inputs(&dir);
        bundle.miniapp_host = None;
        let err = WechatMini
            .plan(&bundle, &spec("wechat", "weapp"))
            .expect_err("缺宿主必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0002);

        // 宿主在场但无 Taro 工程形态（候选链全落空）：同样 MissingInput
        let err = AlipayMini
            .plan(&inputs(&dir), &spec("alipay", "alipay"))
            .expect_err("无 Taro 工程形态必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0002);

        // 候选链第二跳：宿主根自身即 Taro 工程（`ui` 子目录缺席）
        std::fs::create_dir_all(dir.join("config")).expect("建 config");
        std::fs::create_dir_all(dir.join("src")).expect("建 src");
        fsutil::write_text(&dir.join("config/index.js"), "x").expect("写 config");
        fsutil::write_text(&dir.join("src/app.config.js"), "x").expect("写 app.config");
        let err = WechatMini
            .plan(&inputs(&dir), &spec("wechat", "weapp"))
            .expect_err("缺 taro CLI 必须报错");
        match err {
            BundleError::ToolMissing { tool, hint, .. } => {
                assert_eq!(tool, "taro");
                assert!(hint.contains("npm install"), "提示须给安装动作：{hint}");
            }
            other => panic!("期望 ToolMissing，实得 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quickapp_plan_records_the_skipped_taro_leg() {
        let dir = std::env::temp_dir().join(format!("poly-quickapp-plan-{}", std::process::id()));
        let bundle = inputs(&dir);
        let plan = QuickApp
            .plan(&bundle, &spec("quickapp", "quickapp"))
            .expect("计划必须生成");
        assert!(plan.commands.is_empty(), "快应用无外部编译命令");
        assert_eq!(plan.skipped.len(), 1, "Taro 编译腿须如实记入 skipped");
        assert!(
            plan.skipped[0].reason.contains("plugin-platform-quickapp"),
            "跳过原因须点名缺的平台插件：{}",
            plan.skipped[0].reason
        );
    }

    #[test]
    fn quickapp_package_builds_the_engineering_project() {
        let dir = std::env::temp_dir().join(format!("poly-quickapp-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let artifact = QuickApp
            .package(&inputs(&dir), &spec("quickapp", "quickapp"))
            .expect("快应用工程必须产出");
        assert_eq!(artifact.format, "quickapp");
        assert_eq!(artifact.path, dir.join("miniapp"));
        let src = artifact.path.join("src");
        for rel in [
            "manifest.json",
            "app.ux",
            "pages/index/index.ux",
            "logo.png",
            "config-phone.json",
        ] {
            assert!(src.join(rel).is_file(), "编译工程缺 src/{rel}");
        }
        assert!(
            artifact.path.join("package.json").is_file(),
            "缺工程 package.json"
        );
        let manifest: serde_json::Value =
            serde_json::from_str(&read_text(&src.join("manifest.json")).expect("读 manifest"))
                .expect("manifest 须为合法 JSON");
        assert_eq!(manifest["package"], "com.polypack.paxnotes");
        assert_eq!(
            manifest["router"]["pages"]["pages/index"]["component"],
            "index"
        );
        // hap-toolkit 2.1.1 schema 的必填面（真跑 `hap build` 实证缺失即报 schema 错误）
        assert_eq!(manifest["icon"], "logo.png");
        assert_eq!(manifest["router"]["entry"], "pages/index");
        assert_eq!(manifest["config"]["designWidth"], 750);
        assert_eq!(
            manifest["versionCode"], 10203,
            "versionCode 派生应与 Android 同源（1.2.3 → 1*10000+2*100+3）"
        );
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.contains("hap-toolkit")),
            "notes 须点明 hap-toolkit：{:?}",
            artifact.notes
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 合成 wasm 出口四文件（打包器只做整目录拷贝分发，打包期不解析内容）。
    fn seed_bridge(dir: &Path) {
        std::fs::create_dir_all(dir).expect("建 wasm 出口目录");
        for (name, bytes) in [
            ("manifest.json", &b"{\"schema\":\"wasm-sandbox/v1\"}\n"[..]),
            ("polypack-loader.js", &b"// loader\n"[..]),
            ("pax_core_wasm.js", &b"// glue\n"[..]),
            ("pax_core_wasm_bg.wasm", &[0x00, 0x61, 0x73, 0x6d][..]),
        ] {
            std::fs::write(dir.join(name), bytes).expect("写出口文件");
        }
    }

    #[test]
    fn wasm_bridge_is_carried_into_the_code_package() {
        let dir = std::env::temp_dir().join(format!("poly-quickapp-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = dir.join(BRIDGE_REL);
        seed_bridge(&bridge);
        let mut bundle = inputs(&dir);
        bundle.miniapp_bridge = Some(bridge);
        let artifact = QuickApp
            .package(&bundle, &spec("quickapp", "quickapp"))
            .expect("wasm 出口在场必须随包");
        for rel in [
            "manifest.json",
            "polypack-loader.js",
            "pax_core_wasm.js",
            "pax_core_wasm_bg.wasm",
        ] {
            assert!(
                artifact
                    .path
                    .join("src")
                    .join(BRIDGE_REL)
                    .join(rel)
                    .is_file(),
                "随包缺 src/{BRIDGE_REL}/{rel}"
            );
        }
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.contains("wasm 沙箱出口随包")),
            "notes 须记随包出口：{:?}",
            artifact.notes
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn half_written_bridge_dir_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("poly-quickapp-bridge-half-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = dir.join(BRIDGE_REL);
        std::fs::create_dir_all(&bridge).expect("建 wasm 出口目录");
        fsutil::write_text(&bridge.join("manifest.json"), "{}\n").expect("写 manifest");
        let mut bundle = inputs(&dir);
        bundle.miniapp_bridge = Some(bridge);
        let err = QuickApp
            .package(&bundle, &spec("quickapp", "quickapp"))
            .expect_err("缺 loader 的半成品出口目录必须拒绝");
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
        assert!(
            err.to_string().contains("polypack-loader.js"),
            "报文须点名缺的文件：{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
