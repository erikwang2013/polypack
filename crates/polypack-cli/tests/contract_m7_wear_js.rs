//! T-M7-07 / T-M7-08 契约测试（CI 默认真跑）：Fitbit `.fba` 与 Zepp `.zab` 的
//! **打包 → 产物结构断言 → `polypack verify` 声明校验器**全链路。
//!
//! 打包器与 verify 腿共用 `polypack_bundle::check_fitbit_structure` / `check_zepp_structure`
//! （与 `polypack build` 的 verify 阶段同源）；本测试用真实内建打包器产出官方形态的包，
//! 落到 `<project>/.polypack/packages/<target>/`，再跑 `polypack verify --target <target> --json`，
//! 断言 `verified` 事件与结构 / SKIP-with-print 证据。`fitbit-build` / `zeus` / 模拟器
//! 本机不可得：不做假调用，SKIP 句必须如实出现（结构断言是真跑面）。

mod common;

use std::path::{Path, PathBuf};

use polypack_bundle::{BundleInputs, ProductInfo, check_fitbit_structure, check_zepp_structure};
use polypack_targets::Registry;

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

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("父目录可建");
    }
    std::fs::write(path, contents).expect("夹具文件可写");
}

/// `polypack verify --json` 的 JSON Lines 里是否有指定 checker 的 `verified` 事件。
fn verified_event(output: &std::process::Output, checker: &str) -> bool {
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        event["event"] == "verified" && event["checker"] == checker && event["ok"] == true
    })
}

/// 打包 → 落暂存目录 → `polypack verify` 一目标；返回（产物路径，verify 输出）。
fn package_and_verify(root: &Path, target: &str) -> (PathBuf, std::process::Output) {
    let spec = Registry::builtin()
        .get(target)
        .unwrap_or_else(|| panic!("内建注册表缺少 {target}"))
        .clone();
    let out = root.join(".polypack/packages").join(target);
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.miniapp_host = Some(root.to_path_buf());
    let packager =
        polypack_bundle::for_spec(&spec).unwrap_or_else(|| panic!("{target} 目标应有内建打包器"));
    let artifact = packager
        .package(&inputs, &spec)
        .unwrap_or_else(|err| panic!("{target} 打包应真跑成功：{err}"));
    assert!(
        artifact.path.is_file(),
        "{} 产物应落盘",
        artifact.path.display()
    );

    let output = common::polypack(&["verify", "--target", target, "--json"], root);
    assert!(
        output.status.success(),
        "polypack verify --target {target} 应通过：{}",
        common::combined(&output)
    );
    (artifact.path, output)
}

#[test]
fn fitbit_and_zepp_package_verify_contract() {
    let root = common::init_project("m7-fitbit-zepp");

    // Fitbit SDK 工程（官方 `config.fitbit` 面）。
    write(
        &root.join("fitbit/package.json"),
        br#"{
  "name": "pax-notes-fitbit",
  "version": "1.2.3",
  "config": {
    "fitbit": {
      "appUUID": "12345678-9abc-def0-1234-56789abcdef0",
      "appDisplayName": "Pax Notes",
      "requestedPermissions": ["access_activity"]
    }
  }
}"#,
    );
    write(&root.join("fitbit/app/index.js"), b"console.log('pax');\n");
    write(&root.join("fitbit/resources/icon.png"), b"fake-png");

    // Zepp OS 工程（官方 `app.json` 面）。
    write(
        &root.join("zepp/app.json"),
        br#"{
  "app": {
    "appName": "pax-notes",
    "appId": 1001,
    "version": { "code": 12, "name": "1.2.3" }
  },
  "configVersion": "v2"
}"#,
    );
    write(&root.join("zepp/app.js"), b"App({});\n");

    // Fitbit：结构与金样形态一致（条目 / appId / 家族 / 入口），verify 腿真跑。
    let (fba, fitbit_verify) = package_and_verify(&root, "fitbit");
    let fitbit_output = common::combined(&fitbit_verify);
    let fitbit = check_fitbit_structure(&fba).expect("fitbit 产物必须过结构检查");
    assert_eq!(
        fitbit.entries,
        ["device-atlas.zip", "device-vulcan.zip", "manifest.json"]
    );
    assert_eq!(fitbit.families, ["atlas", "vulcan"]);
    assert_eq!(fitbit.app_id, "12345678-9abc-def0-1234-56789abcdef0");
    assert_eq!(fitbit.device_main, "app/index.js");
    assert!(
        fitbit_output.contains("fitbit-structure-check"),
        "{fitbit_output}"
    );
    assert!(fitbit_output.contains("结构自检"), "{fitbit_output}");
    assert!(
        fitbit_output.contains("fitbit-build") && fitbit_output.contains("SKIP-with-print"),
        "官方 CLI 缺席必须如实打印 SKIP：{fitbit_output}"
    );
    assert!(
        verified_event(&fitbit_verify, "fitbit-structure-check"),
        "verify 必须发出 fitbit-structure-check 的 verified 事件"
    );

    // Zepp：zpk 以内容 md5 命名，verify 腿真跑；模拟器安装腿 SKIP-with-print。
    let (zab, zepp_verify) = package_and_verify(&root, "zepp");
    let zepp_output = common::combined(&zepp_verify);
    let zepp = check_zepp_structure(&zab).expect("zepp 产物必须过结构检查");
    assert_eq!(zepp.zpk_name, format!("{}.zpk", zepp.md5));
    assert_eq!((zepp.app_name.as_str(), zepp.app_id), ("pax-notes", 1001));
    assert_eq!(zepp.version_code, 12);
    assert!(
        zepp_output.contains("zepp-structure-check"),
        "{zepp_output}"
    );
    assert!(zepp_output.contains("结构自检"), "{zepp_output}");
    assert!(
        zepp_output.contains("模拟器安装腿 SKIP-with-print"),
        "{zepp_output}"
    );
    assert!(
        verified_event(&zepp_verify, "zepp-structure-check"),
        "verify 必须发出 zepp-structure-check 的 verified 事件"
    );

    let _ = std::fs::remove_dir_all(&root);
}
