//! T-M7-02 契约测试：Wear OS 变体打包器（`wearos`，复用 M2 的 AAPT2 + bundletool +
//! apksigner 链，不新增第二套 Android 打包实现）。
//!
//! 契约面：注册表（`wearos` / `apk`）→ 口径守卫（format 必须 `apk`）→ 清单校验器
//! （`android.hardware.type.watch` required=true + minSdkVersion ≥ 26，生成件 / 输入件同门）
//! → 计划（复用同一 Android 链，生成的清单带 watch 特性且不叠加 Leanback / automotive）
//! → 真打包（结构断言 + `aapt2 dump badging` 的 watch 特性与 sdkVersion 行；bundletool
//! 在场时 validate 真跑）→ apksigner 取证。设备安装腿归 `ui/smoke/wear/run.mjs`（T-M7-03）。
//!
//! 工具无关性：Android SDK / bundletool / apksigner 都可能缺席（CI 三宿主）→ 打印
//! SKIP 后通过，绝不红，也绝不伪造真跑。清单校验器另有无 SDK 单测兜底。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::error::codes;
use polypack_bundle::inputs::{AndroidInputs, BundleInputs, Icon, IconKind, NativeLib, ProductInfo};
use polypack_bundle::packager::{Packager, builtin, find};
use polypack_bundle::packagers::android::{AndroidWear, WEAR_MIN_API, validate_wear_manifest};
use polypack_bundle::tools::{self, AndroidTools};
use polypack_bundle::zipwrite;
use polypack_targets::{Host, PackageSpec, TargetSpec};

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

/// Wear 目标声明（tier 6，format `apk`；清单变体由打包器生成）。
fn spec_wear() -> TargetSpec {
    let mut spec = TargetSpec::new_stub("wearos", 6, &[Host::Win, Host::Linux, Host::Macos]);
    spec.package = PackageSpec {
        format: "apk".to_string(),
        builder: "android-aapt2".to_string(),
    };
    spec
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("polypack-wear-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
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

/// 解析出的工具链；缺 SDK / aapt2 / android.jar 即打印 SKIP 并返回 None。
fn sdk() -> Option<AndroidTools> {
    let Some(root) = sdk_root() else {
        eprintln!("SKIP：本机无 Android SDK（CI 三宿主可能都没有；有 SDK 的宿主走真跑分支）");
        return None;
    };
    let inputs = AndroidInputs {
        sdk_root: Some(root.clone()),
        ..AndroidInputs::default()
    };
    match tools::probe(&inputs) {
        Some(found) => Some(found),
        None => {
            eprintln!(
                "SKIP：{} 内缺 build-tools（aapt2）或 platforms/android-<NN>",
                root.display()
            );
            None
        }
    }
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

/// 最小输入：产品元数据 + 256 PNG 图标 + assets + 一个 arm64 占位库。
fn inputs(base: &Path, tools: &AndroidTools) -> BundleInputs {
    let dist = base.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist");
    std::fs::write(dist.join("index.html"), "wear bundle").expect("写 assets");
    std::fs::write(dist.join("assets/app.js"), "console.log('wear')").expect("写 assets");

    let mut bundle = BundleInputs::new(product(), base.join("out"));
    std::fs::create_dir_all(&bundle.out_dir).expect("建输出目录");
    bundle.icons = vec![Icon {
        path: common::fixture("pax-notes-256.png"),
        kind: IconKind::Png,
        size: Some(256),
    }];
    bundle.android = Some(AndroidInputs {
        sdk_root: Some(tools.sdk_root.clone()),
        assets: Some(dist),
        native_libs: vec![fake_lib(base, "arm64-v8a")],
        ..AndroidInputs::default()
    });
    bundle
}

/// `aapt2 dump badging` 的 stdout（aapt2 从 SDK 解析，不经 PATH）。
fn dump_badging(tools: &AndroidTools, apk: &Path) -> String {
    let output = Command::new(&tools.aapt2)
        .args(["dump", "badging"])
        .arg(apk)
        .output()
        .expect("跑 aapt2 dump badging");
    assert!(
        output.status.success(),
        "aapt2 dump badging 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// JDK 里 java 同目录的工具（keytool），回退 PATH。
fn jdk_tool(tools: &AndroidTools, name: &str) -> Option<PathBuf> {
    tools
        .java
        .as_ref()
        .and_then(|java| java.parent().map(|dir| dir.join(name)))
        .filter(|path| path.is_file())
        .or_else(|| polypack_bundle::exec::tool_path(name))
}

/// 调试签名（验收：Wear APK 过 apksigner 取证；密钥库优先 `~/.android/debug.keystore`）。
fn sign_debug(
    base: &Path,
    tools: &AndroidTools,
    unsigned: &Path,
    signed: &Path,
) -> Result<(), String> {
    let apksigner = tools
        .apksigner
        .as_deref()
        .ok_or_else(|| "build-tools 内缺 apksigner".to_string())?;
    let keystore = debug_keystore(base, tools)?;
    let output = Command::new(apksigner)
        .args(["sign", "--ks"])
        .arg(&keystore)
        .args([
            "--ks-pass",
            "pass:android",
            "--key-pass",
            "pass:android",
            "--out",
        ])
        .arg(signed)
        .arg(unsigned)
        .output()
        .map_err(|err| format!("跑 apksigner 失败：{err}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "apksigner 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// 调试密钥库（Android 工具默认凭据 android / android；无既有库则现场生成）。
fn debug_keystore(base: &Path, tools: &AndroidTools) -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("HOME") {
        let existing = PathBuf::from(home).join(".android/debug.keystore");
        if existing.is_file() {
            return Ok(existing);
        }
    }
    let keytool = jdk_tool(tools, "keytool").ok_or_else(|| "缺 keytool（JDK）".to_string())?;
    let keystore = base.join("debug.keystore");
    let output = Command::new(keytool)
        .args(["-genkeypair", "-keystore"])
        .arg(&keystore)
        .args([
            "-alias",
            "androiddebugkey",
            "-storepass",
            "android",
            "-keypass",
            "android",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-dname",
            "CN=Android Debug,O=Android,C=US",
        ])
        .output()
        .map_err(|err| format!("跑 keytool 失败：{err}"))?;
    if output.status.success() {
        return Ok(keystore);
    }
    Err(format!(
        "keytool 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    ))
}

// ---------------------------------------------------------------- 无需 SDK 的契约面

#[test]
fn registry_exposes_the_wearos_packager() {
    let all: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    assert!(
        all.contains(&("wearos".to_string(), "apk".to_string())),
        "缺 wearos/apk：{all:?}"
    );
    let packager = find("wearos", "apk").expect("find(wearos, apk) 应命中");
    assert_eq!(packager.id(), "wearos");
    assert_eq!(packager.format(), "apk");
    // M2 契约不受影响：android 仍是 aab、没有 apk 打包器。
    assert!(find("android", "aab").is_some());
    assert!(find("android", "apk").is_none());
}

#[test]
fn wearos_format_mismatch_is_rejected() {
    let base = scratch("guard");
    let mut bundle = BundleInputs::new(product(), base.join("out"));
    bundle.android = Some(AndroidInputs::default());
    // 口径守卫：Wear 打包器只认 `apk`——传入别的口径即报错，不静默按别家格式出包。
    let mut spec = spec_wear();
    spec.package.format = "aab".to_string();
    let err = AndroidWear.plan(&bundle, &spec).unwrap_err();
    assert_eq!(err.code(), codes::E_BND_0003);
    assert!(
        matches!(
            err,
            polypack_bundle::error::BundleError::InvalidInput {
                field: "spec.package.format",
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn wear_manifest_validator_requires_watch_feature_and_min_sdk() {
    let ok = format!(
        "<manifest><uses-sdk android:minSdkVersion=\"26\" />\n  {WATCH_FEATURE}\n</manifest>"
    );
    assert_eq!(validate_wear_manifest(&ok), Ok(()), "最小合法清单应通过");

    let missing = "<manifest><uses-sdk android:minSdkVersion=\"26\" /></manifest>";
    let reason = validate_wear_manifest(missing).unwrap_err();
    assert!(reason.contains("android.hardware.type.watch"), "{reason}");

    // required=false 不算声明（Wear 应用必须硬性要求 watch 特性）。
    let optional = "<manifest><uses-feature android:name=\"android.hardware.type.watch\" \
                    android:required=\"false\" /><uses-sdk android:minSdkVersion=\"26\" /></manifest>";
    assert!(validate_wear_manifest(optional).is_err());

    let low =
        format!("<manifest><uses-sdk android:minSdkVersion=\"24\" />{WATCH_FEATURE}</manifest>");
    let reason = validate_wear_manifest(&low).unwrap_err();
    assert!(reason.contains("24") && reason.contains("26"), "{reason}");

    let absent = format!("<manifest>{WATCH_FEATURE}</manifest>");
    let reason = validate_wear_manifest(&absent).unwrap_err();
    assert!(reason.contains("minSdkVersion"), "{reason}");

    let garbage =
        format!("<manifest><uses-sdk android:minSdkVersion=\"x\" />{WATCH_FEATURE}</manifest>");
    assert!(validate_wear_manifest(&garbage).is_err());
    assert_eq!(WEAR_MIN_API, 26, "Wear OS 2.0 / Android 8.0 下限");
}

// ---------------------------------------------------------------- 计划与真打包（需 SDK）

#[test]
fn wearos_plan_reuses_the_android_chain_and_watch_manifest() {
    let Some(tools) = sdk() else { return };
    let base = scratch("plan");
    let bundle = inputs(&base, &tools);
    assert_eq!(AndroidWear.id(), "wearos");
    assert_eq!(AndroidWear.format(), "apk");

    let plan = AndroidWear.plan(&bundle, &spec_wear()).expect("Wear 计划");
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
    // 链同源：aapt2 三步（compile ×1 + link 二进制 + link proto）都在计划里。
    let aapt2 = plan
        .commands
        .iter()
        .filter(|command| command.program.contains("aapt2"))
        .count();
    assert_eq!(
        aapt2, 3,
        "aapt2 命令数（compile + link + link --proto-format）"
    );
    if tools.bundletool.is_some() {
        assert!(
            plan.commands
                .iter()
                .any(|c| c.line().contains("build-bundle")),
            "bundletool 在场时计划须含 build-bundle"
        );
    } else {
        eprintln!("SKIP：bundletool 不在场——计划降级为模块 zip + 未签名 APK（结构断言照跑）");
    }

    // 输入清单同门：minSdk 24 被拒，26 放行且不再生成清单。
    let bad = base.join("bad-manifest.xml");
    std::fs::write(
        &bad,
        format!("<manifest><uses-sdk android:minSdkVersion=\"24\" />{WATCH_FEATURE}</manifest>"),
    )
    .expect("写低 minSdk 清单");
    let mut bad_bundle = BundleInputs::new(product(), base.join("out-bad"));
    std::fs::create_dir_all(&bad_bundle.out_dir).expect("建输出目录");
    bad_bundle.android = Some(AndroidInputs {
        sdk_root: Some(tools.sdk_root.clone()),
        manifest: Some(bad),
        ..AndroidInputs::default()
    });
    let err = AndroidWear.plan(&bad_bundle, &spec_wear()).unwrap_err();
    assert_eq!(err.code(), codes::E_BND_0003);
    assert!(
        matches!(
            err,
            polypack_bundle::error::BundleError::InvalidInput {
                field: "AndroidManifest.xml",
                ..
            }
        ),
        "{err}"
    );

    let good = base.join("good-manifest.xml");
    std::fs::write(
        &good,
        format!("<manifest><uses-sdk android:minSdkVersion=\"26\" />{WATCH_FEATURE}</manifest>"),
    )
    .expect("写合规清单");
    let mut good_bundle = BundleInputs::new(product(), base.join("out-good"));
    std::fs::create_dir_all(&good_bundle.out_dir).expect("建输出目录");
    good_bundle.android = Some(AndroidInputs {
        sdk_root: Some(tools.sdk_root.clone()),
        manifest: Some(good),
        ..AndroidInputs::default()
    });
    let plan = AndroidWear
        .plan(&good_bundle, &spec_wear())
        .expect("输入清单计划");
    assert!(
        !plan
            .scripts
            .iter()
            .any(|script| script.path.ends_with("AndroidManifest.xml")),
        "输入清单时不得再生成清单"
    );
}

#[test]
fn package_builds_a_wear_apk_and_validates_it() {
    let Some(tools) = sdk() else { return };
    let base = scratch("pkg");
    let bundle = inputs(&base, &tools);
    let artifact = AndroidWear
        .package(&bundle, &spec_wear())
        .expect("打包 Wear APK");
    assert_eq!(artifact.target, "wearos");
    assert_eq!(artifact.format, "apk");
    let apk = bundle.out_dir.join("wear-notes-1.2.3-unsigned.apk");
    let aab = bundle.out_dir.join("wear-notes-1.2.3.aab");
    let apks = bundle.out_dir.join("wear-notes-1.2.3.apks");
    assert_eq!(artifact.path, apk, "Wear 产物 = 未签名 APK（同 TV / AAOS）");
    assert!(artifact.bytes > 0, "产物体积应为正");

    // 结构断言（真跑，不依赖 bundletool）：lib/<abi> 进包 + 清单 watch 特性行。
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names
            .iter()
            .any(|name| name == "lib/arm64-v8a/libwearcore.so"),
        "{names:?}"
    );
    let badging = dump_badging(&tools, &apk);
    assert!(
        badging.contains("uses-feature: name='android.hardware.type.watch'"),
        "{badging}"
    );
    assert!(
        badging.contains("minSdkVersion:'26'"),
        "APK 清单的 minSdkVersion 应为 26：{badging}"
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("android.hardware.type.watch")),
        "{:?}",
        artifact.notes
    );

    // bundletool（在场）：validate 真跑 + AAB / APK 集落盘；缺席：如实降级说明。
    if tools.bundletool.is_some() {
        assert!(aab.is_file() && apks.is_file(), "缺 AAB / APK 集");
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.starts_with("bundletool validate：")),
            "{:?}",
            artifact.notes
        );
    } else {
        eprintln!("SKIP：bundletool 不在场——validate 未跑（结构断言与 APK 产物已真跑）");
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.contains("未执行 bundletool")),
            "{:?}",
            artifact.notes
        );
    }

    // apksigner 取证（工具 / JDK 缺席即 SKIP 该子断言）；设备安装腿在
    // `ui/smoke/wear/run.mjs --leg android`（adb 门 + 表单用例），此处不重复。
    if let Some(apksigner) = tools.apksigner.as_deref() {
        let signed = base.join("wear-signed.apk");
        match sign_debug(&base, &tools, &apk, &signed) {
            Ok(()) => {
                let verify = Command::new(apksigner)
                    .args(["verify", "--print-certs"])
                    .arg(&signed)
                    .output()
                    .expect("跑 apksigner verify");
                assert!(
                    verify.status.success(),
                    "apksigner verify 失败：{}",
                    String::from_utf8_lossy(&verify.stderr)
                );
            }
            Err(reason) => eprintln!("SKIP：调试签名不可用（{reason}），apksigner verify 未跑"),
        }
    }
}
