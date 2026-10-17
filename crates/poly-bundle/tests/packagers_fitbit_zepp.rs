//! T-M7-07 / T-M7-08 契约测试：Fitbit `.fba` 与 Zepp `.zab` 打包器真跑。
//!
//! 真跑面：构造官方 SDK 形态的最小工程 → 打包器出包 → 与 verify 腿**同一份**结构检查
//! （[`poly_bundle::check_fitbit_structure`] / [`poly_bundle::check_zepp_structure`]）过检 →
//! 条目 / 清单字段 ↔ 金样（`tests/fixtures/fitbit.fba.json` / `zepp.zab.json`）→ 二跑字节
//! 一致 → notes 明示「核心不直接运行、能力由 JS 侧实现」与官方 CLI / 模拟器 SKIP 口径。
//! `fitbit-build` / `zeus` 本机不可得：测试不调用官方 CLI，结构断言是本文件的真跑面。

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::{
    BundleInputs, ProductInfo, check_fitbit_structure, check_zepp_structure, zipwrite,
};
use poly_targets::{Registry, Runtime, TargetSpec};

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "poly-bundle-fitzep-{}-{tag}-{seq}",
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

fn spec(id: &str) -> TargetSpec {
    Registry::builtin()
        .get(id)
        .unwrap_or_else(|| panic!("内建注册表缺少 {id}"))
        .clone()
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("父目录可建");
    }
    std::fs::write(path, contents).expect("夹具文件可写");
}

fn golden(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取金样 {} 失败：{err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("金样 {name} 非法 JSON：{err}"))
}

fn golden_list(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .expect("金样应为数组")
        .iter()
        .map(|item| item.as_str().expect("元素应为字符串").to_string())
        .collect()
}

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

#[test]
fn fitbit_packager_builds_and_self_checks_the_fba() {
    let spec = spec("fitbit");
    assert_eq!(spec.tier, 6);
    assert_eq!(spec.runtime, Runtime::None, "核心不运行在 Fitbit 上");
    assert_eq!(spec.package.format, "fba");
    assert_eq!(spec.package.builder, "fitbit-build");
    assert_eq!(spec.verify, vec!["fitbit-structure-check"]);
    let spec_notes = spec.notes.as_deref().unwrap_or_default();
    assert!(
        spec_notes.contains("核心不直接运行、能力由 JS 侧实现"),
        "notes 必须明示运行口径：{spec_notes}"
    );

    let temp = Temp::new("fitbit");
    let host = temp.join("host");
    write(
        &host.join("fitbit/package.json"),
        FITBIT_PACKAGE_JSON.as_bytes(),
    );
    write(&host.join("fitbit/app/index.js"), b"console.log('pax');\n");
    write(&host.join("fitbit/resources/icon.png"), b"fake-png");

    let out = temp.join("out");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.miniapp_host = Some(host.clone());
    let packager = poly_bundle::for_spec(&spec).expect("fitbit 目标应有内建打包器");
    assert_eq!((packager.id(), packager.format()), ("fitbit", "fba"));

    let artifact = packager
        .package(&inputs, &spec)
        .expect("Fitbit 打包应真跑成功");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.fba"));
    assert_eq!(artifact.format, "fba");

    // 结构与金样（官方 SDK 6.1 的 appPackageManifest / componentManifest 形态）。
    let structure = check_fitbit_structure(&artifact.path).expect("产物必须过同一份结构检查");
    let golden = golden("fitbit.fba.json");
    assert_eq!(structure.entries, golden_list(&golden["entries"]));
    assert_eq!(structure.app_id, "12345678-9abc-def0-1234-56789abcdef0");
    assert_eq!(structure.families, golden_list(&golden["families"]));
    assert_eq!(structure.device_main, "app/index.js");
    assert!(!structure.companion);
    assert_eq!(
        structure.build_id.len(),
        18,
        "buildId = 0x0 + 15 hex：{}",
        structure.build_id
    );
    assert!(structure.build_id.starts_with("0x0"));

    let manifest: serde_json::Value = serde_json::from_str(
        &zipwrite::entry_text(&artifact.path, "manifest.json")
            .unwrap()
            .expect("应用清单存在"),
    )
    .unwrap();
    assert_eq!(
        manifest["manifestVersion"].as_u64().unwrap(),
        golden["appManifestVersion"].as_u64().unwrap()
    );
    assert_eq!(
        manifest["sdkVersion"]["deviceApi"].as_str().unwrap(),
        golden["deviceApi"].as_str().unwrap()
    );
    let device = zipwrite::entry_bytes(&artifact.path, "device-atlas.zip")
        .unwrap()
        .expect("device-atlas.zip 存在");
    assert_eq!(
        zipwrite::bytes_entry_names(&device, &artifact.path).unwrap(),
        golden_list(&golden["deviceEntries"]),
        "设备组件条目须与金样一致"
    );

    // 确定性：同输入二跑字节一致。
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(
        std::fs::read(&artifact.path).unwrap(),
        std::fs::read(&again.path).unwrap(),
        "同输入二跑必须字节一致"
    );

    // notes：运行口径 + 官方 CLI SKIP 口径如实随产物。
    let notes = artifact.notes.join("\n");
    assert!(
        notes.contains("核心不直接运行、能力由 JS 侧实现"),
        "{notes}"
    );
    assert!(notes.contains("fitbit-build"), "{notes}");
    assert!(notes.contains("SKIP-with-print"), "{notes}");

    // 伴生组件：官方 companion.zip 腿（可选）。
    write(
        &host.join("fitbit/companion/index.js"),
        b"console.log('companion');\n",
    );
    let with_companion = packager
        .package(&inputs, &spec)
        .expect("带 companion 打包应成功");
    let structure = check_fitbit_structure(&with_companion.path).unwrap();
    assert!(structure.companion);
    assert!(
        structure
            .entries
            .iter()
            .any(|entry| entry == "companion.zip")
    );
    assert!(with_companion.notes.join("\n").contains("companion.zip"));

    // 反例：清单版本漂移必须被结构检查拦下（防检查器空转）。
    let bad_dir = temp.join("bad");
    write(
        &bad_dir.join("manifest.json"),
        br#"{"appId":"12345678-9abc-def0-1234-56789abcdef0","buildId":"0x0abc"}"#,
    );
    let bad = temp.join("bad.fba");
    zipwrite::write_zip(&poly_bundle::webapp::collect_files(&bad_dir).unwrap(), &bad).unwrap();
    assert!(
        check_fitbit_structure(&bad).is_err(),
        "manifestVersion != 6 必须硬失败"
    );
}

#[test]
fn zepp_packager_builds_and_self_checks_the_zab() {
    let spec = spec("zepp");
    assert_eq!(spec.tier, 6);
    assert_eq!(spec.runtime, Runtime::None, "核心不运行在 Zepp OS 上");
    assert_eq!(spec.package.format, "zab");
    assert_eq!(spec.package.builder, "zeus");
    assert_eq!(spec.verify, vec!["zepp-structure-check"]);
    let spec_notes = spec.notes.as_deref().unwrap_or_default();
    assert!(
        spec_notes.contains("核心不直接运行、能力由 Zepp JS API 侧实现"),
        "notes 必须明示运行口径：{spec_notes}"
    );

    let temp = Temp::new("zepp");
    let host = temp.join("host");
    write(&host.join("zepp/app.json"), ZEPP_APP_JSON.as_bytes());
    write(&host.join("zepp/app.js"), b"App({});\n");

    let out = temp.join("out");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.miniapp_host = Some(host.clone());
    let packager = poly_bundle::for_spec(&spec).expect("zepp 目标应有内建打包器");
    assert_eq!((packager.id(), packager.format()), ("zepp", "zab"));

    let artifact = packager
        .package(&inputs, &spec)
        .expect("Zepp 打包应真跑成功");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.zab"));
    assert_eq!(artifact.format, "zab");

    // 结构与金样（官方 zpm：外层 manifest + <md5>.zpk → device.zip）。
    let structure = check_zepp_structure(&artifact.path).expect("产物必须过同一份结构检查");
    let golden = golden("zepp.zab.json");
    assert_eq!(structure.entries.len(), 2);
    assert!(
        structure
            .entries
            .iter()
            .any(|entry| entry == "manifest.json")
    );
    assert_eq!(
        structure.config_version,
        golden["configVersion"].as_str().unwrap()
    );
    assert_eq!(structure.app_name, golden["appName"].as_str().unwrap());
    assert_eq!(structure.app_id, golden["appId"].as_u64().unwrap());
    assert_eq!(
        structure.version_code,
        golden["versionCode"].as_u64().unwrap()
    );
    assert_eq!(
        structure.version_name,
        golden["versionName"].as_str().unwrap()
    );
    assert_eq!(structure.zpk_name, format!("{}.zpk", structure.md5));
    assert_eq!(structure.md5.len(), 32, "zpk 名取内容 md5");
    assert!(!structure.has_app_side);
    assert_eq!(
        structure.device_entries,
        golden_list(&golden["deviceEntries"]),
        "device.zip 条目须与金样一致"
    );

    // 确定性：同输入二跑字节一致。
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(
        std::fs::read(&artifact.path).unwrap(),
        std::fs::read(&again.path).unwrap(),
        "同输入二跑必须字节一致"
    );

    // notes：运行口径 + zeus / 模拟器 SKIP 口径如实随产物。
    let notes = artifact.notes.join("\n");
    assert!(
        notes.contains("核心不直接运行、能力由 Zepp JS API 侧实现"),
        "{notes}"
    );
    assert!(notes.contains("zeus"), "{notes}");
    assert!(notes.contains("SKIP-with-print"), "{notes}");

    // 可选 app-side 腿。
    write(&host.join("zepp/app-side/index.js"), b"AppSide({});\n");
    let with_side = packager
        .package(&inputs, &spec)
        .expect("带 app-side 打包应成功");
    let structure = check_zepp_structure(&with_side.path).unwrap();
    assert!(structure.has_app_side);
    assert!(with_side.notes.join("\n").contains("app-side.zip"));

    // 反例：外层清单缺 zpks 必须被结构检查拦下（防检查器空转）。
    let bad_dir = temp.join("bad");
    write(
        &bad_dir.join("manifest.json"),
        br#"{"configVersion":"v2","zpks":[]}"#,
    );
    let bad = temp.join("bad.zab");
    zipwrite::write_zip(&poly_bundle::webapp::collect_files(&bad_dir).unwrap(), &bad).unwrap();
    assert!(check_zepp_structure(&bad).is_err(), "空 zpks 必须硬失败");
}
