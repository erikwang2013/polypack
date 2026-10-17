//! T-M7-11 穿戴契约测试：**Fitbit OS（`fitbit.fba`）**。
//!
//! 四段断言由 [`common::wear_contract`] 统一驱动（注册表↔金样 / 真打包 /
//! `check_fitbit_structure` 校验器 / 产物结构↔金样），金样 `tests/fixtures/fitbit.fba.json`。
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
//! Fitbit 打包器不依赖外部 CLI（`fitbit-build` 只是校验腿，缺席时 SKIP-with-print），
//! 所以本契约在 mock 模式（每提交）即真打包。

mod common;

use std::path::Path;

use poly_bundle::inputs::{BundleInputs, ProductInfo};
use poly_bundle::packager::Artifact;
use poly_bundle::{check_fitbit_structure, zipwrite};
use poly_targets::{Runtime, TargetSpec};
use serde_json::{Value, json};

use common::wear_contract::{self, Contract};

const TARGET: &str = "fitbit";
const GOLDEN: &str = "fitbit.fba.json";
const APP_UUID: &str = "12345678-9abc-def0-1234-56789abcdef0";

/// Fitbit SDK 工程夹具（`<host>/fitbit/`）：官方 `config.fitbit` + `app/index.js` + 图标。
const FITBIT_PACKAGE_JSON: &str = r#"{
  "name": "pax-notes-fitbit",
  "version": "1.2.3",
  "config": {
    "fitbit": {
      "appUUID": "12345678-9abc-def0-1234-56789abcdef0",
      "appDisplayName": "Pax Notes",
      "appType": "app",
      "requestedPermissions": ["access_activity"],
      "buildTargets": ["atlas", "vulcan"]
    }
  }
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

/// 夹具：Fitbit SDK 工程；无外部工具依赖，总是返回输入。
fn prepare(root: &Path) -> Option<BundleInputs> {
    wear_contract::write(
        &root.join("fitbit/package.json"),
        FITBIT_PACKAGE_JSON.as_bytes(),
    );
    wear_contract::write(&root.join("fitbit/app/index.js"), b"console.log('pax');\n");
    wear_contract::write(&root.join("fitbit/resources/icon.png"), b"fake-png");
    let mut inputs = BundleInputs::new(product(), root.join("out"));
    inputs.miniapp_host = Some(root.to_path_buf());
    Some(inputs)
}

/// 校验器（`check_fitbit_structure`）+ 结构快照（金样逐字段比对）。
fn structure(_inputs: &BundleInputs, artifact: &Artifact, _spec: &TargetSpec) -> Value {
    let checked = check_fitbit_structure(&artifact.path).expect("fitbit 产物必须过结构检查");
    assert!(checked.bytes > 0, "产物体积应为正");
    assert_eq!(checked.app_id, APP_UUID);
    assert_eq!(checked.device_main, "app/index.js");
    assert!(!checked.companion, "基础夹具不带 companion");
    assert!(
        checked.build_id.len() == 18 && checked.build_id.starts_with("0x0"),
        "buildId 形态（0x0 + 15 hex）：{}",
        checked.build_id
    );

    let manifest: Value = serde_json::from_str(
        &zipwrite::entry_text(&artifact.path, "manifest.json")
            .expect("读应用清单")
            .expect("应用清单存在"),
    )
    .expect("应用清单为合法 JSON");
    let device = zipwrite::entry_bytes(&artifact.path, "device-atlas.zip")
        .expect("读设备组件")
        .expect("device-atlas.zip 存在");
    let device_entries = zipwrite::bytes_entry_names(&device, &artifact.path).expect("读设备条目");

    let notes = artifact.notes.join("\n");
    assert!(
        notes.contains("核心不直接运行、能力由 JS 侧实现"),
        "notes 必须明示运行口径：{notes}"
    );
    assert!(
        notes.contains("fitbit-build") && notes.contains("SKIP-with-print"),
        "官方 CLI 缺席必须如实打印 SKIP：{notes}"
    );

    json!({
        "appId": checked.app_id,
        "buildIdShape": "0x0+15hex",
        "families": checked.families,
        "companion": checked.companion,
        "deviceMain": checked.device_main,
        "entries": checked.entries,
        "manifestVersion": manifest["manifestVersion"],
        "deviceApi": manifest["sdkVersion"]["deviceApi"],
        "deviceEntries": device_entries,
    })
}

#[test]
fn fitbit_contract() {
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .expect("内建注册表缺少 fitbit")
        .clone();
    assert_eq!(spec.runtime, Runtime::None, "核心不运行在 Fitbit 上");
    assert_eq!(spec.tier, 6);
    wear_contract::run(Contract {
        target: TARGET,
        golden: GOLDEN,
        prepare,
        structure,
    });
}

// ---------------------------------------------------------------- 契约补充（T-M7-07 验收）

/// 确定性：同输入二跑字节一致（buildId / bundleDate 固定，金样可复现）。
#[test]
fn fitbit_packaging_is_deterministic() {
    let root = std::env::temp_dir().join(format!("poly-wear-fitbit-det-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("临时目录可建");
    let inputs = prepare(&root).expect("Fitbit 夹具无外部依赖");
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .unwrap()
        .clone();
    let packager = poly_bundle::for_spec(&spec).expect("内建 fitbit 打包器");

    let first = packager.package(&inputs, &spec).expect("一跑");
    let again = packager.package(&inputs, &spec).expect("二跑");
    assert_eq!(
        std::fs::read(&first.path).unwrap(),
        std::fs::read(&again.path).unwrap(),
        "同输入二跑必须字节一致"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 可选伴生组件腿：`companion/index.js` 存在时入包并出现在结构检查里。
#[test]
fn fitbit_companion_leg_is_optional() {
    let root =
        std::env::temp_dir().join(format!("poly-wear-fitbit-companion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("临时目录可建");
    let inputs = prepare(&root).expect("Fitbit 夹具无外部依赖");
    wear_contract::write(
        &root.join("fitbit/companion/index.js"),
        b"console.log('companion');\n",
    );
    let spec = poly_targets::Registry::builtin()
        .get(TARGET)
        .unwrap()
        .clone();
    let packager = poly_bundle::for_spec(&spec).expect("内建 fitbit 打包器");
    let artifact = packager.package(&inputs, &spec).expect("带 companion 打包");
    let checked = check_fitbit_structure(&artifact.path).unwrap();
    assert!(checked.companion);
    assert!(checked.entries.iter().any(|entry| entry == "companion.zip"));
    assert!(artifact.notes.join("\n").contains("companion.zip"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 反例：应用清单版本漂移必须被结构检查拦下（防校验器空转）。
#[test]
fn fitbit_checker_rejects_manifest_version_drift() {
    let dir = std::env::temp_dir().join(format!("poly-wear-fitbit-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    wear_contract::write(
        &dir.join("manifest.json"),
        br#"{"appId":"12345678-9abc-def0-1234-56789abcdef0","buildId":"0x0abc"}"#,
    );
    let bad = dir.with_extension("fba");
    zipwrite::write_zip(&poly_bundle::webapp::collect_files(&dir).unwrap(), &bad).unwrap();
    assert!(
        check_fitbit_structure(&bad).is_err(),
        "manifestVersion != 6 必须硬失败"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&bad);
}
