//! T-M7-11 穿戴契约测试：**Wear OS（`wearos.apk`）**。
//!
//! 四段断言由 [`common::wear_contract`] 统一驱动（注册表↔金样 / 真打包 /
//! `validate_wear_manifest` + `aapt2 dump badging` 校验器 / 产物结构↔金样），
//! 金样 `tests/fixtures/wearos.apk.json`。
//!
//! ## 新增穿戴目标：复制本文件
//!
//! 1. 改文件头目标名与 `TARGET` / `GOLDEN` 两个常量；
//! 2. 改 `product()`（产品元数据）与 `prepare()`（工程夹具；工具链缺席时返回 `None`，
//!    骨架会打印 SKIP-with-print，不红也不伪造真跑——本文件按 Android SDK 演示）；
//! 3. 改 `structure()`：调用目标自己的 `check_*_structure` / `validate_*`，返回要锁进
//!    金样的稳定字段（不要放工具链版本相关的条目全量清单）；
//! 4. 手写最小金样（schema 见 `tests/common/wear_contract.rs` 头注释），再
//!    `POLYPACK_UPDATE_GOLDEN=1 cargo test -p poly-bundle --test contract_m7_<目标>`
//!    补齐 `structure` 并 review git diff。
//!
//! 本机 / CI 无 Android SDK（aapt2 / android.jar）时 SKIP-with-print：mock 模式仍会跑
//! 注册表↔金样段；有工具的宿主上真打包并比对金样。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::inputs::{AndroidInputs, BundleInputs, Icon, IconKind, NativeLib, ProductInfo};
use poly_bundle::packager::Artifact;
use poly_bundle::{tools, zipwrite};
use poly_targets::TargetSpec;
use serde_json::{json, Value};

use common::wear_contract::{self, Contract};

const TARGET: &str = "wearos";
const GOLDEN: &str = "wearos.apk.json";
const PKG: &str = "com.polypack.wearnotes";
const WATCH_FEATURE: &str =
    "<uses-feature android:name=\"android.hardware.type.watch\" android:required=\"true\" />";

fn product() -> ProductInfo {
    ProductInfo {
        name: "Wear Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: PKG.to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Wear 示例笔记应用".to_string(),
        homepage: None,
    }
}

/// 本机 Android SDK 候选根（环境变量 → 开发机落点 → 用户目录惯例）。
fn sdk_root() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from))
        .collect();
    roots.push(PathBuf::from("/home/component/Android/sdk"));
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Android/Sdk"));
    }
    roots
        .into_iter()
        .find(|root| root.join("build-tools").is_dir())
}

/// 解析出的工具链；缺 SDK / aapt2 / android.jar 即返回 `None`（骨架打印 SKIP）。
fn sdk() -> Option<tools::AndroidTools> {
    let root = sdk_root()?;
    tools::probe(&AndroidInputs {
        sdk_root: Some(root),
        ..AndroidInputs::default()
    })
}

/// 占位 `.so`（只走结构通路：`lib/<abi>/` 布局；不做加载 / 运行）。
fn fake_lib(base: &Path, abi: &str) -> NativeLib {
    let dir = base.join("jniLibs").join(abi);
    std::fs::create_dir_all(&dir).expect("建 jniLibs");
    let path = dir.join("libwearcore.so");
    std::fs::write(&path, format!("placeholder .so for {abi}")).expect("写占位 .so");
    NativeLib {
        abi: abi.to_string(),
        path,
    }
}

/// 夹具：assets + 图标 + arm64 占位库；工具链缺席 → `None`。
fn prepare(root: &Path) -> Option<BundleInputs> {
    let tools = sdk()?;
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist");
    std::fs::write(dist.join("index.html"), "wear bundle").expect("写 assets");
    std::fs::write(dist.join("assets/app.js"), "console.log('wear')").expect("写 assets");

    let mut inputs = BundleInputs::new(product(), root.join("out"));
    std::fs::create_dir_all(&inputs.out_dir).expect("建输出目录");
    inputs.icons = vec![Icon {
        path: common::fixture("pax-notes-256.png"),
        kind: IconKind::Png,
        size: Some(256),
    }];
    inputs.android = Some(AndroidInputs {
        sdk_root: Some(tools.sdk_root),
        assets: Some(dist),
        native_libs: vec![fake_lib(root, "arm64-v8a")],
        ..AndroidInputs::default()
    });
    Some(inputs)
}

/// 校验器：清单校验器已在打桩阶段真跑；这里用 `aapt2 dump badging` 复核产物清单，
/// 并核对计划面仍复用同一条 Android 链（3 条 aapt2：compile + link + link --proto）。
fn structure(inputs: &BundleInputs, artifact: &Artifact, spec: &TargetSpec) -> Value {
    let android = inputs
        .android
        .as_ref()
        .expect("Wear 夹具必须带 AndroidInputs");
    let tools = tools::probe(android).expect("prepare 已确认工具链在场");
    let output = Command::new(&tools.aapt2)
        .args(["dump", "badging"])
        .arg(&artifact.path)
        .output()
        .expect("跑 aapt2 dump badging");
    assert!(
        output.status.success(),
        "aapt2 dump badging 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let badging = String::from_utf8_lossy(&output.stdout);
    assert!(
        badging.contains("uses-feature: name='android.hardware.type.watch'"),
        "{badging}"
    );
    assert!(badging.contains("minSdkVersion:'26'"), "{badging}");
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("android.hardware.type.watch")),
        "{:?}",
        artifact.notes
    );

    let names = zipwrite::entry_names(&artifact.path).expect("读 APK 条目");
    assert!(
        names
            .iter()
            .any(|name| name == "lib/arm64-v8a/libwearcore.so"),
        "{names:?}"
    );

    let plan = poly_bundle::for_spec(spec)
        .expect("内建 wearos 打包器")
        .plan(inputs, spec)
        .expect("Wear 计划");
    let manifest = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("AndroidManifest.xml"))
        .expect("计划须含生成的清单")
        .contents
        .clone();
    assert!(manifest.contains(WATCH_FEATURE), "{manifest}");
    assert!(
        manifest.contains("android:minSdkVersion=\"26\""),
        "默认 minSdk 24 应被抬到 Wear 下限 26：{manifest}"
    );
    assert!(
        !manifest.contains("leanback") && !manifest.contains("android.hardware.type.automotive"),
        "Wear 不叠加 TV / 车机清单行：{manifest}"
    );
    let aapt2_commands = plan
        .commands
        .iter()
        .filter(|command| command.program.contains("aapt2"))
        .count();
    assert_eq!(
        aapt2_commands, 3,
        "aapt2 命令数（compile + link + link --proto-format）"
    );

    json!({
        "watchFeature": true,
        "minSdk": 26,
        "hasNativeLib": true,
        "aapt2Commands": aapt2_commands,
    })
}

#[test]
fn wearos_contract() {
    wear_contract::run(Contract {
        target: TARGET,
        golden: GOLDEN,
        prepare,
        structure,
    });
}

// ---------------------------------------------------------------- 契约补充（T-M7-02 验收）

/// 清单校验器的无 SDK 单测兜底：watch 特性必须 required=true，minSdk 必须 ≥ 26。
#[test]
fn wear_manifest_validator_requires_watch_feature_and_min_sdk() {
    use poly_bundle::packagers::android::{validate_wear_manifest, WEAR_MIN_API};

    let ok = format!(
        "<manifest><uses-sdk android:minSdkVersion=\"26\" />\n  {WATCH_FEATURE}\n</manifest>"
    );
    assert_eq!(validate_wear_manifest(&ok), Ok(()), "最小合法清单应通过");

    let missing = "<manifest><uses-sdk android:minSdkVersion=\"26\" /></manifest>";
    assert!(validate_wear_manifest(missing).is_err());

    // required=false 不算声明（Wear 应用必须硬性要求 watch 特性）。
    let optional = "<manifest><uses-feature android:name=\"android.hardware.type.watch\" \
                    android:required=\"false\" /><uses-sdk android:minSdkVersion=\"26\" /></manifest>";
    assert!(validate_wear_manifest(optional).is_err());

    let low =
        format!("<manifest><uses-sdk android:minSdkVersion=\"24\" />{WATCH_FEATURE}</manifest>");
    let reason = validate_wear_manifest(&low).unwrap_err();
    assert!(reason.contains("24") && reason.contains("26"), "{reason}");
    assert_eq!(WEAR_MIN_API, 26, "Wear OS 2.0 / Android 8.0 下限");
}

/// 口径守卫：Wear 打包器只认 `apk`，传入别的格式即报错（不静默按别家格式出包）。
#[test]
fn wearos_format_mismatch_is_rejected() {
    use poly_bundle::packager::{find, Packager};
    use poly_targets::{Host, PackageSpec};

    let mut spec = TargetSpec::new_stub(TARGET, 6, &[Host::Win, Host::Linux, Host::Macos]);
    spec.package = PackageSpec {
        format: "aab".to_string(),
        builder: "android-aapt2".to_string(),
    };
    let mut bundle = BundleInputs::new(product(), std::env::temp_dir().join("poly-wear-guard"));
    bundle.android = Some(AndroidInputs::default());
    let error = find(TARGET, "apk")
        .expect("内建 wearos/apk")
        .plan(&bundle, &spec);
    assert!(error.is_err(), "format=aab 必须被 Wear 打包器拒绝");
}
