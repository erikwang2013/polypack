//! T-M6-11 契约测试：AAOS 变体打包器（`android-automotive`，复用 M2 的 AAPT2 + bundletool +
//! apksigner 链，不新增第二套 Android 打包实现）。
//!
//! 契约面：注册表（`android-automotive` / `apk`）→ 口径守卫（format 必须 `apk`）→ 计划
//! （生成的清单带 `android.hardware.type.automotive`，不叠加 Leanback）→ 真打包（结构断言 +
//! `aapt2 dump badging` 的 automotive 特性行；bundletool 在场时 validate 真跑）→ 签名取证。
//!
//! 工具无关性：Android SDK / bundletool / apksigner 都可能缺席（CI 三宿主）→ 打印 SKIP 后
//! 通过，绝不红，也绝不伪造真跑。清单结构断言另有 `android.rs` 单元测试兜底（无需 SDK）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::error::codes;
use poly_bundle::inputs::{AndroidInputs, BundleInputs, Icon, IconKind, NativeLib, ProductInfo};
use poly_bundle::packager::{Packager, builtin, find};
use poly_bundle::packagers::android::AndroidAutomotive;
use poly_bundle::tools::{self, AndroidTools};
use poly_bundle::zipwrite;
use poly_targets::{Host, PackageSpec, TargetSpec};

const PKG: &str = "com.polypack.paxnotes";

fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: PKG.to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Pax 示例笔记应用".to_string(),
        homepage: None,
    }
}

/// AAOS 目标声明（tier 6，format `apk`；清单变体由打包器生成）。
fn spec_automotive() -> TargetSpec {
    let mut spec = TargetSpec::new_stub(
        "android-automotive",
        6,
        &[Host::Win, Host::Linux, Host::Macos],
    );
    spec.package = PackageSpec {
        format: "apk".to_string(),
        builder: "android-aapt2".to_string(),
    };
    spec
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poly-aaos-{tag}-{}", std::process::id()));
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
    let path = dir.join("libpaxcore.so");
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
    std::fs::write(dist.join("index.html"), "pax bundle").expect("写 assets");
    std::fs::write(dist.join("assets/app.js"), "console.log('pax')").expect("写 assets");

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
        .or_else(|| poly_bundle::exec::tool_path(name))
}

/// 调试签名（验收：AAOS APK 过 apksigner 取证；密钥库优先 `~/.android/debug.keystore`）。
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
fn registry_exposes_the_automotive_packager() {
    let all: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    assert!(
        all.contains(&("android-automotive".to_string(), "apk".to_string())),
        "缺 android-automotive/apk：{all:?}"
    );
    let packager = find("android-automotive", "apk").expect("find(android-automotive, apk) 应命中");
    assert_eq!(packager.id(), "android-automotive");
    assert_eq!(packager.format(), "apk");
    // M2 契约不受影响：android 仍是 aab、没有 apk 打包器。
    assert!(find("android", "aab").is_some());
    assert!(find("android", "apk").is_none());
}

#[test]
fn automotive_format_mismatch_is_rejected() {
    let base = scratch("guard");
    let mut bundle = BundleInputs::new(product(), base.join("out"));
    bundle.android = Some(AndroidInputs::default());
    // 口径守卫：AAOS 打包器只认 `apk`——传入别的口径即报错，不静默按别家格式出包。
    let mut spec = spec_automotive();
    spec.package.format = "aab".to_string();
    let err = AndroidAutomotive.plan(&bundle, &spec).unwrap_err();
    assert_eq!(err.code(), codes::E_BND_0003);
    assert!(
        matches!(
            err,
            poly_bundle::error::BundleError::InvalidInput {
                field: "spec.package.format",
                ..
            }
        ),
        "{err}"
    );
}

// ---------------------------------------------------------------- 计划与真打包（需 SDK）

#[test]
fn automotive_plan_reuses_the_android_chain_and_manifest_variant() {
    let Some(tools) = sdk() else { return };
    let base = scratch("plan");
    let bundle = inputs(&base, &tools);
    assert_eq!(AndroidAutomotive.id(), "android-automotive");
    assert_eq!(AndroidAutomotive.format(), "apk");

    let plan = AndroidAutomotive
        .plan(&bundle, &spec_automotive())
        .expect("AAOS 计划");
    let manifest = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("AndroidManifest.xml"))
        .expect("计划须含生成的清单")
        .contents
        .clone();
    assert!(
        manifest.contains(
            "<uses-feature android:name=\"android.hardware.type.automotive\" android:required=\"true\" />"
        ),
        "{manifest}"
    );
    assert!(
        !manifest.contains("leanback") && !manifest.contains("android:banner"),
        "AAOS 不叠加 TV 清单行：{manifest}"
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
}

#[test]
fn package_builds_an_automotive_apk_and_validates_it() {
    let Some(tools) = sdk() else { return };
    let base = scratch("pkg");
    let bundle = inputs(&base, &tools);
    let artifact = AndroidAutomotive
        .package(&bundle, &spec_automotive())
        .expect("打包 AAOS APK");
    assert_eq!(artifact.target, "android-automotive");
    assert_eq!(artifact.format, "apk");
    let apk = bundle.out_dir.join("pax-notes-1.2.3-unsigned.apk");
    let aab = bundle.out_dir.join("pax-notes-1.2.3.aab");
    let apks = bundle.out_dir.join("pax-notes-1.2.3.apks");
    assert_eq!(artifact.path, apk, "AAOS 产物 = 未签名 APK（同 TV）");
    assert!(artifact.bytes > 0, "产物体积应为正");

    // 结构断言（真跑，不依赖 bundletool）：lib/<abi> 进包 + 清单 automotive 特性行。
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names
            .iter()
            .any(|name| name == "lib/arm64-v8a/libpaxcore.so"),
        "{names:?}"
    );
    let badging = dump_badging(&tools, &apk);
    assert!(
        badging.contains("uses-feature: name='android.hardware.type.automotive'"),
        "{badging}"
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("android.hardware.type.automotive")),
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

    // apksigner 取证（工具 / JDK 缺席即 SKIP 该子断言）。
    if let Some(apksigner) = tools.apksigner.as_deref() {
        let signed = base.join("aaos-signed.apk");
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
