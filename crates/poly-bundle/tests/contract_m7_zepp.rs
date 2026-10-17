//! T-M7-11 穿戴契约测试：**Zepp OS（`zepp.zab`）**。
//!
//! 四段断言由 [`common::wear_contract`] 统一驱动（注册表↔金样 / 真打包 /
//! `check_zepp_structure` 校验器 / 产物结构↔金样），金样 `tests/fixtures/zepp.zab.json`。
//!
//! ## 新增穿戴目标：复制本文件
//!
//! 1. 改文件头目标名与 `TARGET` / `GOLDEN` 两个常量；
//! 2. 改 `product()`（产品元数据）与 `prepare()`（工程夹具：本目标 SDK 的目录形态）；
//! 3. 改 `structure()`：调用目标自己的 `check_*_structure`，返回要锁进金样的字段；
//! 4. 手写最小金样（schema 见 `tests/common/wear_contract.rs` 头注释），再
//!    `POLYPACK_UPDATE_GOLDEN=1 cargo test -p poly-bundle --test contract_m7_<目标>`
//!    补齐 `structure` 并 review git diff。
//!
//! Zepp 打包器不依赖 `zeus`（官方 CLI 只是校验 / 安装腿，缺席时 SKIP-with-print），
//! 所以本契约在 mock 模式（每提交）即真打包。

mod common;

use std::path::Path;

use poly_bundle::inputs::{BundleInputs, ProductInfo};
use poly_bundle::packager::Artifact;
use poly_bundle::{check_zepp_structure, zipwrite};
use poly_targets::{Runtime, TargetSpec};
use serde_json::{json, Value};

use common::wear_contract::{self, Contract};

const TARGET: &str = "zepp";
const GOLDEN: &str = "zepp.zab.json";

/// Zepp OS 工程夹具（`<host>/zepp/`）：官方 `app.json` + `app.js`。
const ZEPP_APP_JSON: &str = r#"{
  "app": {
    "appName": "pax-notes",
    "appId": 1001,
    "version": { "code": 12, "name": "1.2.3" },
    "appType": "app"
  },
  "targets": { "default": { "platforms": [{ "deviceSource": "default" }] } },
  "configVersion": "v2"
}"#;

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

/// 夹具：Zepp OS 工程；无外部工具依赖，总是返回输入。
fn prepare(root: &Path) -> Option<BundleInputs> {
    wear_contract::write(&root.join("zepp/app.json"), ZEPP_APP_JSON.as_bytes());
    wear_contract::write(&root.join("zepp/app.js"), b"App({});\n");
    let mut inputs = BundleInputs::new(product(), root.join("out"));
    inputs.miniapp_host = Some(root.to_path_buf());
    Some(inputs)
}

/// 校验器（`check_zepp_structure`）+ 结构快照（金样逐字段比对）。
fn structure(_inputs: &BundleInputs, artifact: &Artifact, _spec: &TargetSpec) -> Value {
    let checked = check_zepp_structure(&artifact.path).expect("zepp 产物必须过结构检查");
    assert!(checked.bytes > 0, "产物体积应为正");
    assert_eq!(checked.entries.len(), 2, "{:?}", checked.entries);
    assert!(checked.entries.iter().any(|entry| entry == "manifest.json"));
    assert_eq!(
        checked.zpk_name,
        format!("{}.zpk", checked.md5),
        "zpk 名取内容 md5"
    );
    assert_eq!(checked.md5.len(), 32);
    assert!(!checked.has_app_side, "基础夹具不带 app-side");

    let notes = artifact.notes.join("\n");
    assert!(
        notes.contains("核心不直接运行、能力由 Zepp JS API 侧实现"),
        "notes 必须明示运行口径：{notes}"
    );
    assert!(
        notes.contains("zeus") && notes.contains("SKIP-with-print"),
        "官方 CLI 缺席必须如实打印 SKIP：{notes}"
    );

    json!({
        "configVersion": checked.config_version,
        "appName": checked.app_name,
        "appId": checked.app_id,
        "versionCode": checked.version_code,
        "versionName": checked.version_name,
        "zpkNameShape": "<md5>.zpk",
        "md5Length": checked.md5.len(),
        "hasAppSide": checked.has_app_side,
        "deviceEntries": checked.device_entries,
    })
}

#[test]
fn zepp_contract() {
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .expect("内建注册表缺少 zepp")
        .clone();
    assert_eq!(spec.runtime, Runtime::None, "核心不运行在 Zepp OS 上");
    assert_eq!(spec.tier, 6);
    wear_contract::run(Contract {
        target: TARGET,
        golden: GOLDEN,
        prepare,
        structure,
    });
}

// ---------------------------------------------------------------- 契约补充（T-M7-08 验收）

/// 确定性：同输入二跑字节一致（`bundleInfo.timeStamp` 固定，zpk 名取内容 md5）。
#[test]
fn zepp_packaging_is_deterministic() {
    let root = std::env::temp_dir().join(format!("poly-wear-zepp-det-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("临时目录可建");
    let inputs = prepare(&root).expect("Zepp 夹具无外部依赖");
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .unwrap()
        .clone();
    let packager = poly_bundle::for_spec(&spec).expect("内建 zepp 打包器");

    let first = packager.package(&inputs, &spec).expect("一跑");
    let again = packager.package(&inputs, &spec).expect("二跑");
    assert_eq!(
        std::fs::read(&first.path).unwrap(),
        std::fs::read(&again.path).unwrap(),
        "同输入二跑必须字节一致"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 可选 app-side 腿：`app-side/` 存在时入 zpk 并出现在结构检查里。
#[test]
fn zepp_app_side_leg_is_optional() {
    let root = std::env::temp_dir().join(format!("poly-wear-zepp-side-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("临时目录可建");
    let inputs = prepare(&root).expect("Zepp 夹具无外部依赖");
    wear_contract::write(&root.join("zepp/app-side/index.js"), b"AppSide({});\n");
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .unwrap()
        .clone();
    let packager = poly_bundle::for_spec(&spec).expect("内建 zepp 打包器");
    let artifact = packager.package(&inputs, &spec).expect("带 app-side 打包");
    let checked = check_zepp_structure(&artifact.path).unwrap();
    assert!(checked.has_app_side);
    assert!(artifact.notes.join("\n").contains("app-side.zip"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 反例：外层清单缺 `zpks` 必须被结构检查拦下（防校验器空转）。
#[test]
fn zepp_checker_rejects_empty_zpks() {
    let dir = std::env::temp_dir().join(format!("poly-wear-zepp-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    wear_contract::write(
        &dir.join("manifest.json"),
        br#"{"configVersion":"v2","zpks":[]}"#,
    );
    let bad = dir.with_extension("zab");
    zipwrite::write_zip(&poly_bundle::webapp::collect_files(&dir).unwrap(), &bad).unwrap();
    assert!(check_zepp_structure(&bad).is_err(), "空 zpks 必须硬失败");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&bad);
}
