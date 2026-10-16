//! T-M3-06 小程序族打包器契约测试：真跑 Taro 编译（微信 / 支付宝 / 抖音）+ 快应用编译工程。
//!
//! 全部在本机真跑（不 mock）：宿主工程 = 本仓 `ui/adapters/taro/host`（`poly init` 工程的
//! ui/ 即 Taro 工程时走同一候选链）。Taro 工具链缺席（宿主 node_modules 未装）时
//! SKIP-with-print——与 devtools 预览腿同一纪律：真跑优先，缺席如实跳过，绝不伪造。
//!
//! 三个 Taro 目标在**同一用例内顺序**真跑：Taro 编译的 webpack 缓存在宿主目录内，
//! 并行跑会互踩；产物落在各自 temp 子目录，用例结束即回收。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::packager::ArtifactKind;
use poly_bundle::{BundleInputs, ProductInfo};
use poly_targets::Registry;

// ---------------------------------------------------------------- 夹具

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "poly-bundle-miniapp-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        Self(dir)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 仓库根（`crates/poly-bundle` 的上两级）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("仓库根可解析")
}

/// 本仓 Taro 宿主工程（`ui/adapters/taro/host`）。
fn host_project() -> PathBuf {
    repo_root().join("ui/adapters/taro/host")
}

/// 产品元数据（与 `poly init` 模板同形）。
fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "契约测试合成输入".to_string(),
        homepage: None,
    }
}

/// 打包输入：宿主交给候选链解析（`miniapp_host` = 仓库根 → 命中 `ui/adapters/taro/host`）。
fn inputs(t: &Temp) -> BundleInputs {
    let mut inputs = BundleInputs::new(product(), t.join("out"));
    inputs.miniapp_host = Some(repo_root());
    inputs
}

/// 内建注册表里的目标（L2 小程序族；规格漂移在这里即红）。
fn spec(id: &str) -> poly_targets::TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 {id}"));
    assert_eq!(spec.tier, 2, "{id} 应为 L2（tier 2）");
    assert!(
        spec.ui.iter().any(|ui| ui == "taro"),
        "{id} 的 UI 适配器应为 taro：{:?}",
        spec.ui
    );
    spec
}

// ---------------------------------------------------------------- 真跑

/// 真跑一目标：计划链 → Taro 编译 → 代码包结构断言 → devtools 描述符。
///
/// 返回产物目录（调用方断言 notes 等）。
fn real_compile_one(
    id: &str,
    format: &str,
    markup: &str,
    style: &str,
    descriptor: &str,
    devtools: &str,
) {
    let t = Temp::new(id);
    let bundle = inputs(&t);
    let spec = spec(id);
    assert_eq!(spec.package.format, format, "{id} 声明格式");
    let packager = poly_bundle::find(id, format)
        .unwrap_or_else(|| panic!("poly-bundle 缺少 {id} 的 {format} 打包器"));
    assert_eq!(packager.id(), id);

    // 计划链：白名单预检 + Taro 编译（产物经 POLYPACK_OUTPUT_ROOT 注入本目标的 out/miniapp/dist）
    let plan = packager
        .plan(&bundle, &spec)
        .unwrap_or_else(|err| panic!("{id} plan() 失败：{err}"));
    assert_eq!(
        plan.commands.len(),
        2,
        "{id} 计划链应为预检 + 编译：{plan:?}"
    );
    assert_eq!(
        plan.commands[0].program, "node",
        "{id} 第一步须是白名单预检"
    );
    assert!(
        plan.commands[0]
            .args
            .iter()
            .any(|arg| arg.ends_with("check-api.mjs")),
        "{id} 预检须走 @polypack/adapter-taro 的 CLI：{:?}",
        plan.commands[0]
    );
    let compile = &plan.commands[1];
    assert!(
        compile.program.ends_with("taro"),
        "{id} 编译腿须走宿主 node_modules 里的 taro：{:?}",
        compile
    );
    assert_eq!(
        compile.args,
        vec![
            "build".to_string(),
            "--type".to_string(),
            format.to_string()
        ],
        "{id} 编译参数（tt 的平台名与目标 id 不同，金样锚定）"
    );
    let expected_dist = t.join("out/miniapp/dist");
    assert!(
        compile
            .env
            .iter()
            .any(|(key, value)| key == "POLYPACK_OUTPUT_ROOT" && Path::new(value) == expected_dist),
        "{id} 产物目录须经环境变量注入：{:?}",
        compile.env
    );

    let artifact = packager
        .package(&bundle, &spec)
        .unwrap_or_else(|err| panic!("{id} package() 真跑失败：{err}"));
    assert_eq!(artifact.kind, ArtifactKind::Directory);
    assert_eq!(artifact.target, id);
    assert_eq!(artifact.format, format);
    assert_eq!(artifact.path, t.join("out/miniapp"));
    assert!(artifact.bytes > 0, "目录产物须有体积");

    // 代码包结构：应用级四件套 + 页面三件套（Taro 真跑形态，见 miniapp.rs 的断言注记）
    let dist = artifact.path.join("dist");
    for rel in [
        "app.json".to_string(),
        "app.js".to_string(),
        format!("app.{style}"),
        format!("base.{markup}"),
        "pages/index/index.js".to_string(),
        "pages/index/index.json".to_string(),
        format!("pages/index/index.{markup}"),
    ] {
        assert!(dist.join(&rel).is_file(), "{id} 代码包缺 {rel}");
    }

    // 路由 / 装配标记 / 示例页标记（ASCII——中文在产物里是 \uXXXX 转义）
    let app_json = std::fs::read_to_string(dist.join("app.json")).expect("读 app.json");
    assert!(
        app_json.contains("pages/index/index"),
        "{id} app.json 须含装配页路由：{app_json}"
    );
    let app_js = std::fs::read_to_string(dist.join("app.js")).expect("读 app.js");
    assert!(
        app_js.contains("POLYPACK_TARO"),
        "{id} app bundle 缺装配自检标记"
    );
    let page_js =
        std::fs::read_to_string(dist.join("pages/index/index.js")).expect("读页面 bundle");
    for marker in ["PolyPack App", "counter", "reset", "poly build --target"] {
        assert!(page_js.contains(marker), "{id} 页面 bundle 缺标记 {marker}");
    }
    let page_markup = std::fs::read_to_string(dist.join(format!("pages/index/index.{markup}")))
        .expect("读页面标记");
    assert!(
        page_markup.contains(&format!("base.{markup}")) && page_markup.contains("taro_tmpl"),
        "{id} 页面标记缺模板桥接：{page_markup}"
    );

    // devtools 描述符：产物根落在 devtools 实际读的名字上，miniprogramRoot 归一到 ./dist/
    let descriptor_path = artifact.path.join(descriptor);
    assert!(
        descriptor_path.is_file(),
        "{id} 产物根缺 devtools 描述符 {descriptor}"
    );
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&descriptor_path).expect("读描述符"))
            .expect("描述符须为合法 JSON");
    assert_eq!(
        value["miniprogramRoot"], "./dist/",
        "{id} 描述符 miniprogramRoot 须归一：{value}"
    );

    // notes：预览指引 + devtools 缺席如实标注（SKIP-with-print）
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains(devtools) && note.contains("SKIP-with-print")),
        "{id} notes 须给 devtools 预览指引与 SKIP 注记：{:?}",
        artifact.notes
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("check-api.mjs")),
        "{id} notes 须记白名单预检：{:?}",
        artifact.notes
    );
    eprintln!("真跑 OK {id}：{}", artifact.path.display());
}

/// Taro 三目标真跑：产物结构 + 描述符 + notes 全链断言。
///
/// 宿主 node_modules 缺席（工具链未装）时 SKIP-with-print：本用例真跑的是 Taro 编译本身，
/// 装好依赖的宿主（本地 / 装齐工具链的 CI 腿）自然走真跑分支。
#[test]
fn taro_targets_compile_real_code_packages() {
    let host = host_project();
    if !host.join("node_modules/.bin/taro").is_file() {
        eprintln!(
            "SKIP：Taro 工具链未装（{} 缺 node_modules）——在宿主工程 npm install 后走真跑分支",
            host.display()
        );
        return;
    }
    // 三个 Taro 目标顺序真跑：webpack 缓存同宿主目录，并行会互踩
    real_compile_one(
        "wechat",
        "weapp",
        "wxml",
        "wxss",
        "project.config.json",
        "微信开发者工具",
    );
    real_compile_one(
        "alipay",
        "alipay",
        "axml",
        "acss",
        "mini.project.json",
        "支付宝小程序开发者工具",
    );
    // 抖音：宿主源名 project.tt.json，devtools 实际读 project.config.json（真跑实证）
    real_compile_one(
        "douyin",
        "tt",
        "ttml",
        "ttss",
        "project.config.json",
        "抖音开发者工具",
    );
}

/// 抖音描述符的源/目标名差异（真跑实证锚定）：宿主 `project.tt.json` → 产物
/// `project.config.json`，源里的 appid 保留、miniprogramRoot 归一。
#[test]
fn douyin_descriptor_is_written_under_the_devtools_name() {
    let host = host_project();
    if !host.join("node_modules/.bin/taro").is_file() {
        eprintln!("SKIP：Taro 工具链未装——本用例断言真跑产物，见上一条的安装指引");
        return;
    }
    let t = Temp::new("tt-descriptor");
    let bundle = inputs(&t);
    let spec = spec("douyin");
    let packager = poly_bundle::find("douyin", "tt").expect("douyin tt 打包器");
    let artifact = packager
        .package(&bundle, &spec)
        .unwrap_or_else(|err| panic!("douyin 真跑失败：{err}"));
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(artifact.path.join("project.config.json")).expect("读描述符"),
    )
    .expect("描述符须为合法 JSON");
    assert_eq!(
        value["appid"], "tta0000000000000001",
        "宿主源的 appid 须保留"
    );
    assert_eq!(value["miniprogramRoot"], "./dist/");
    assert!(
        !artifact.path.join("project.tt.json").exists(),
        "宿主源名不得落在产物根（devtools 读的是 project.config.json）"
    );
}

/// 快应用编译工程：不依赖 Taro（无编译腿），任意宿主真跑——hap-toolkit 就绪布局。
#[test]
fn quickapp_engineering_project_is_hap_toolkit_ready() {
    let t = Temp::new("quickapp");
    let bundle = inputs(&t);
    let spec = spec("quickapp");
    let packager = poly_bundle::find("quickapp", "quickapp").expect("quickapp 打包器");
    let plan = packager.plan(&bundle, &spec).expect("快应用计划必须生成");
    assert!(plan.commands.is_empty(), "快应用无外部编译命令");
    assert_eq!(plan.skipped.len(), 1, "Taro 编译腿须如实记入 skipped");

    let artifact = packager
        .package(&bundle, &spec)
        .expect("快应用编译工程必须产出");
    assert_eq!(artifact.format, "quickapp");
    assert_eq!(artifact.path, t.join("out/miniapp"));
    for rel in [
        "package.json",
        "src/manifest.json",
        "src/app.ux",
        "src/pages/index/index.ux",
        "src/logo.png",
        "src/config-phone.json",
    ] {
        assert!(artifact.path.join(rel).is_file(), "编译工程缺 {rel}");
    }
    // hap-toolkit 布局（2.1.1 grep 实证）：manifest 在 src/ 下，工程根 package.json 带 toolkitVersion
    let engineering: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(artifact.path.join("package.json")).expect("读工程清单"),
    )
    .expect("工程 package.json 须为合法 JSON");
    assert_eq!(engineering["toolkitVersion"], "2.1.1");
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(artifact.path.join("src/manifest.json")).expect("读 manifest"),
    )
    .expect("manifest 须为合法 JSON");
    assert_eq!(
        manifest["router"]["pages"]["pages/index"]["component"],
        "index"
    );
    assert_eq!(manifest["versionCode"], 10203);
    assert_eq!(manifest["icon"], "logo.png", "hap-toolkit schema 必填面");
    assert_eq!(manifest["router"]["entry"], "pages/index");
    assert!(manifest["config"]["designWidth"].is_number());
    assert!(
        artifact.notes.iter().any(|note| note.contains("T-M3-12")),
        "notes 须点明 .rpk 收口归 T-M3-12：{:?}",
        artifact.notes
    );
}
