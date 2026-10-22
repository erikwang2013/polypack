//! T-M2-06 / T-M2-07 契约测试：Android 打包器（AAPT2 免 Gradle + bundletool 组装校验）。
//!
//! 真跑范围：`aapt2 compile` / `link`（二进制 + proto）→ 未签名 APK（zip 条目 + `aapt2 dump
//! badging` 断言）→ 二跑 sha256 一致 → ABI / 语言裁剪 → 模块 zip → bundletool `build-bundle`
//! / `validate` / `build-apks` → 有连接设备时的安装与冷启动冒烟。
//!
//! 工具无关性：Android SDK 不在 PATH（`aapt2` 在 `build-tools/<版本>/`），一律经
//! [`polypack_bundle::tools`] 显式解析；CI 三宿主可能没有 SDK → 打印 SKIP 后通过，绝不红。
//! 占位 `.so` 只走结构通路（APK / AAB 内 `lib/<abi>/` 布局与裁剪），不做加载或运行。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::error::{BundleError, codes};
use polypack_bundle::inputs::{
    AndroidInputs, BundleInputs, Icon, IconKind, NativeLib, ProductInfo,
};
use polypack_bundle::packager::{Packager, builtin, find};
use polypack_bundle::packagers::android::{AndroidAab, derive_version_code};
use polypack_bundle::tools::{self, AndroidTools};
use polypack_bundle::zipwrite;
use polypack_targets::{Host, PackageSpec, TargetSpec};
use sha2::{Digest, Sha256};

const PKG: &str = "com.polypack.paxnotes";

/// 冒烟样本的 Activity（纯框架 API，无 AndroidX：javac 只需 android.jar 上类路径）。
const ACTIVITY_JAVA: &str = r#"package com.polypack.paxnotes;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;

public class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView view = new TextView(this);
        view.setText("pax-notes");
        setContentView(view);
    }
}
"#;

/// 冒烟样本清单：`uses-sdk` / 版本号都留给 `aapt2 link` 的旗标注入。
const LAUNCHER_MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.polypack.paxnotes">
  <application android:label="@string/app_name" android:hasCode="true">
    <activity android:name="com.polypack.paxnotes.MainActivity" android:exported="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
  </application>
</manifest>
"#;

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

fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("android", 1, &[Host::Win, Host::Linux, Host::Macos]);
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "android-aapt2".to_string(),
    };
    spec
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("polypack-android-{tag}-{}", std::process::id()));
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

/// Android 打包输入（显式 sdk_root，解析结果与测试内的 `sdk()` 一致）。
fn android_inputs(tools: &AndroidTools) -> AndroidInputs {
    AndroidInputs {
        sdk_root: Some(tools.sdk_root.clone()),
        native_libs: Vec::new(),
        ..AndroidInputs::default()
    }
}

/// 占位 `.so`（只走结构通路：`lib/<abi>/` 布局与裁剪；不做加载 / 运行）。
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
        assets: Some(dist),
        native_libs: vec![fake_lib(base, "arm64-v8a")],
        ..android_inputs(tools)
    });
    bundle
}

/// 打包产物三件（未签名 APK / AAB / APK 集）的路径。
fn artifacts(inputs: &BundleInputs) -> (PathBuf, PathBuf, PathBuf) {
    let out = &inputs.out_dir;
    (
        out.join("pax-notes-1.2.3-unsigned.apk"),
        out.join("pax-notes-1.2.3.aab"),
        out.join("pax-notes-1.2.3.apks"),
    )
}

fn sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|err| panic!("读 {}：{err}", path.display()));
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

/// JDK 里 java 同目录的工具（javac / jar / keytool），回退 PATH。
fn jdk_tool(tools: &AndroidTools, name: &str) -> Option<PathBuf> {
    tools
        .java
        .as_ref()
        .and_then(|java| java.parent().map(|dir| dir.join(name)))
        .filter(|path| path.is_file())
        .or_else(|| polypack_bundle::exec::tool_path(name))
}

/// 可启动样本输入：Java Activity（javac + jar）→ `.jar` 作为 dex 输入（d8 由打包器执行），
/// 配「LAUNCHER activity」清单；缺 javac / jar 时返回 None（打印原因，冒烟退回仅安装）。
fn launcher_inputs(base: &Path, tools: &AndroidTools) -> Option<BundleInputs> {
    let (Some(javac), Some(jar_tool)) = (jdk_tool(tools, "javac"), jdk_tool(tools, "jar")) else {
        eprintln!("SKIP：缺 javac / jar（JDK），可启动样本不可构造");
        return None;
    };
    let src_dir = base.join("java/src/com/polypack/paxnotes");
    std::fs::create_dir_all(&src_dir).expect("建 java 源码目录");
    std::fs::write(src_dir.join("MainActivity.java"), ACTIVITY_JAVA).expect("写 Activity");
    let classes = base.join("java/classes");
    std::fs::create_dir_all(&classes).expect("建 classes 目录");
    let compiled = Command::new(&javac)
        .arg("-classpath")
        .arg(&tools.android_jar)
        .arg("-d")
        .arg(&classes)
        .arg(src_dir.join("MainActivity.java"))
        .output()
        .expect("跑 javac");
    if !compiled.status.success() {
        eprintln!(
            "SKIP：javac 编译样本失败：{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        return None;
    }
    let classes_jar = base.join("java/classes.jar");
    let packed = Command::new(&jar_tool)
        .arg("cf")
        .arg(&classes_jar)
        .arg("-C")
        .arg(&classes)
        .arg(".")
        .output()
        .expect("跑 jar");
    if !packed.status.success() {
        eprintln!(
            "SKIP：jar 打包样本失败：{}",
            String::from_utf8_lossy(&packed.stderr)
        );
        return None;
    }

    let manifest = base.join("java/AndroidManifest.xml");
    std::fs::write(&manifest, LAUNCHER_MANIFEST).expect("写样本清单");
    let mut bundle = inputs(base, tools);
    let android = bundle.android.as_mut().expect("android 输入");
    android.manifest = Some(manifest);
    android.dex_inputs = vec![classes_jar];
    // 两种设备 ABI 都带上（模拟器 x86_64 / 真机 arm64-v8a），占位库不会被加载
    android.abi_filters = vec!["arm64-v8a".to_string(), "x86_64".to_string()];
    android.native_libs = vec![fake_lib(base, "arm64-v8a"), fake_lib(base, "x86_64")];
    Some(bundle)
}

fn adb(args: &[&str]) -> std::process::Output {
    Command::new("adb").args(args).output().expect("跑 adb")
}

/// `adb devices` 里有就绪设备（`device` 状态；`unauthorized` / `offline` 不算）。
fn device_connected() -> bool {
    let Ok(output) = Command::new("adb").arg("devices").output() else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .any(|line| line.trim_end().ends_with("\tdevice"))
}

/// 设备**就绪**（已完成启动且 `package` 服务在线）：连上了 `adb` 但没启动完 / system_server
/// 崩掉的模拟器会把产物问题与设备问题混在一起，此时如实 SKIP（阻塞（设备）），不误判产物。
fn device_ready() -> bool {
    let booted = Command::new("adb")
        .args(["shell", "getprop", "sys.boot_completed"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "1");
    if !booted {
        return false;
    }
    Command::new("adb")
        .args(["shell", "pm", "path", "android"])
        .output()
        .is_ok_and(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("package:")
        })
}

/// 设备不可用（仍在启动 / adb 传输断开 / system_server 消失）：与产物好坏无关的信号。
fn device_unusable(output: &str) -> bool {
    [
        "Broken pipe",
        "Can't find service",
        "device offline",
        "device not found",
        "still booting",
    ]
    .iter()
    .any(|signal| output.contains(signal))
}

/// 调试签名（`adb install` 不接受未签名 APK）：`~/.android/debug.keystore` 优先，否则现场生成。
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

/// 调试密钥库（Android 工具默认凭据 android / android）。
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
fn registry_exposes_android_packager() {
    let all: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    assert!(
        all.contains(&("android".to_string(), "aab".to_string())),
        "缺 android/aab：{all:?}"
    );
    assert!(find("android", "aab").is_some());
    assert!(find("android", "apk").is_none());
    // GrapheneOS 复用普通 APK 口径（无 TV / 车机 / Wear 清单注入）。
    let graphene = find("grapheneos", "apk").expect("GrapheneOS APK 打包器");
    assert_eq!(graphene.id(), "grapheneos");
    assert!(find("grapheneos", "aab").is_none());
}

#[test]
fn format_mismatch_is_rejected() {
    let base = scratch("guard");
    let mut bundle = BundleInputs::new(product(), base.join("out"));
    bundle.android = Some(AndroidInputs::default());
    let err = AndroidAab.plan(&bundle, &spec("apk")).unwrap_err();
    assert_eq!(err.code(), codes::E_BND_0003);
    assert!(matches!(
        err,
        BundleError::InvalidInput {
            field: "spec.package.format",
            ..
        }
    ));
}

#[test]
fn version_code_derivation() {
    assert_eq!(derive_version_code("1.2.3", None), 10203);
    assert_eq!(derive_version_code("1.2.3-beta.1", None), 10203);
    assert_eq!(derive_version_code("1.2.3", Some(7)), 7);
}

#[test]
fn missing_sdk_reports_tool_missing() {
    let base = scratch("no-sdk");
    let mut bundle = BundleInputs::new(product(), base.join("out"));
    bundle.android = Some(AndroidInputs {
        sdk_root: Some(base.join("no-such-sdk")),
        ..AndroidInputs::default()
    });
    let err = AndroidAab.plan(&bundle, &spec("aab")).unwrap_err();
    assert_eq!(err.code(), codes::E_BND_0001);
    assert!(matches!(err, BundleError::ToolMissing { .. }));
}

// ---------------------------------------------------------------- 计划契约（需 SDK）

#[test]
fn plan_shape_covers_aapt2_and_bundletool() {
    let Some(tools) = sdk() else { return };
    let base = scratch("plan");
    let mut bundle = inputs(&base, &tools);
    let android = bundle.android.as_mut().expect("android 输入");
    android.locales = vec!["zh".to_string(), "en".to_string()];
    android.version_code = Some(123);
    android.min_sdk = 24;
    android.target_sdk = 0;

    let plan = AndroidAab
        .plan(&bundle, &spec("aab"))
        .expect("计划（不执行外部工具）");

    // 生成的清单：包名 / 版本 / uses-sdk / application（图标有 256 PNG → 引用 mipmap）。
    let manifest = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("AndroidManifest.xml"))
        .expect("清单脚本");
    for field in [
        "package=\"com.polypack.paxnotes\"",
        "android:versionCode=\"123\"",
        "android:versionName=\"1.2.3\"",
        "android:minSdkVersion=\"24\"",
        "android:targetSdkVersion=\"",
        "android:hasCode=\"false\"",
        "android:icon=\"@mipmap/ic_launcher\"",
        "android:label=\"@string/app_name\"",
    ] {
        assert!(
            manifest.contents.contains(field),
            "清单缺 {field:?}：{}",
            manifest.contents
        );
    }
    // 生成的 res：app_name（图标是二进制，由 package() 拷贝，不进脚本）。
    assert!(
        plan.scripts
            .iter()
            .any(|script| script.path.ends_with("values/strings.xml")
                && script.contents.contains("Pax Notes")),
        "缺生成的 strings.xml：{:?}",
        plan.scripts.iter().map(|s| &s.path).collect::<Vec<_>>()
    );
    // BundleConfig.json（bundletool 契约）。
    let config = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("BundleConfig.json"))
        .expect("BundleConfig");
    assert!(config.contents.contains("\"splitDimension\""));
    assert!(config.contents.contains("\"ABI\"") && config.contents.contains("\"LANGUAGE\""));

    // 命令：compile → 二进制 link → proto link →（有 bundletool 时）三条 java -jar。
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();
    // 程序名带平台后缀（windows 上 aapt2.exe）：按配置的工具路径断言，不用字面 "aapt2"。
    let aapt2 = tools.aapt2.display().to_string();
    // compile 带 cwd（源路径嵌进 proto 资源表，相对路径才跨构建目录一致）→ 行尾是 `)`。
    assert!(
        lines[0].contains(&format!("{aapt2} compile --dir generated-res"))
            && lines[0].ends_with("generated-res.zip)"),
        "{lines:?}"
    );
    assert!(lines[1].contains(&format!("{aapt2} link -o")) && !lines[1].contains("--proto-format"));
    assert!(lines[2].contains(&format!("{aapt2} link --proto-format -o")));
    for line in &lines[1..3] {
        assert!(line.contains("--manifest"), "{line}");
        assert!(line.contains("--min-sdk-version 24"), "{line}");
        assert!(
            line.contains(&format!("--target-sdk-version {}", tools.platform_api)),
            "target_sdk=0 须取 SDK 最新平台：{line}"
        );
        assert!(line.contains("--version-code 123"), "{line}");
        assert!(line.contains("--version-name 1.2.3"), "{line}");
        // 语言裁剪：aapt2 的开关是 `-c`（可重复；该版本没有 `--config`）。
        assert!(line.contains("-c zh") && line.contains("-c en"), "{line}");
    }
    assert!(lines[1].contains(&tools.android_jar.display().to_string()));

    if let (Some(_), Some(java)) = (&tools.bundletool, &tools.java) {
        assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
        assert_eq!(lines.len(), 6, "aapt2 三条 + bundletool 三条：{lines:?}");
        // 同 aapt2：程序名带平台后缀（windows 上 java.exe），按解析出的 java 路径断言。
        let java = java.display().to_string();
        assert!(lines[3].contains(&format!("{java} -jar")) && lines[3].contains("build-bundle"));
        assert!(lines[3].contains("--modules=") && lines[3].contains("--config="));
        assert!(lines[3].ends_with("--overwrite"), "{}", lines[3]);
        assert!(lines[4].contains("validate --bundle="), "{}", lines[4]);
        assert!(
            lines[5].contains("build-apks") && lines[5].ends_with("--overwrite"),
            "{}",
            lines[5]
        );
    } else {
        assert!(
            plan.skipped.iter().any(|s| s.tool == "bundletool"),
            "{:?}",
            plan.skipped
        );
    }
}

#[test]
fn bundletool_absent_degrades_without_faking() {
    let Some(tools) = sdk() else { return };
    let base = scratch("no-bundletool");
    let mut bundle = inputs(&base, &tools);
    // 显式配置指向不存在的 jar：视为未配置（不硬失败，也不伪造 bundletool 结果）。
    bundle.android.as_mut().expect("android 输入").bundletool = Some(base.join("missing.jar"));

    let plan = AndroidAab
        .plan(&bundle, &spec("aab"))
        .expect("降级计划仍可用");
    assert!(plan.skipped.iter().any(|s| s.tool == "bundletool"));
    assert!(
        !plan.commands.iter().any(|c| c.line().contains("-jar")),
        "缺 bundletool 时不得编排 java 命令"
    );

    let artifact = AndroidAab
        .package(&bundle, &spec("aab"))
        .expect("降级打包仍产出结构件");
    assert_eq!(
        (artifact.target.as_str(), artifact.format.as_str()),
        ("android", "aab")
    );
    assert!(
        artifact.path.ends_with("base.zip"),
        "{}",
        artifact.path.display()
    );
    let notes = artifact.notes.join("\n");
    assert!(notes.contains("未执行 bundletool："), "{notes}");
    // 结构降级断言：模块 zip 的 AAB 布局仍在（清单移入 manifest/、资源表为 resources.pb）。
    let names = zipwrite::entry_names(&artifact.path).expect("读模块 zip");
    for want in [
        "manifest/AndroidManifest.xml",
        "resources.pb",
        "lib/arm64-v8a/libpaxcore.so",
    ] {
        assert!(
            names.contains(&want.to_string()),
            "模块 zip 缺 {want}：{names:?}"
        );
    }
    // 未签名 APK 一并产出（notes 里如实标注路径）。
    let (apk, _, apks) = artifacts(&bundle);
    assert!(apk.is_file(), "降级路径仍应产出未签名 APK");
    assert!(!apks.exists(), "缺 bundletool 不得出现 .apks");
}

// ---------------------------------------------------------------- 真跑（需 SDK）

#[test]
fn default_inputs_build_like_the_cli() {
    // CLI 目前不填 `BundleInputs.android`（`polypack-cli` 不在本任务改动范围），即走
    // `AndroidInputs::default()`：无清单 / 无 res / 无图标 / 无 assets —— 这条路径必须能出包。
    // 应用 id 用 CLI 的派生形态（`com.polypack.<kebab>`，带连字符），锁住 aapt2 的包名约束。
    let Some(tools) = sdk() else { return };
    let base = scratch("defaults");
    let bundle = BundleInputs::new(
        ProductInfo {
            identifier: "com.polypack.polypack-app".to_string(),
            ..product()
        },
        base.join("out"),
    );
    std::fs::create_dir_all(&bundle.out_dir).expect("建输出目录");
    assert!(bundle.android.is_none(), "默认构造不应自带 android 输入");

    let artifact = AndroidAab
        .package(&bundle, &spec("aab"))
        .expect("默认输入打包");
    assert_eq!(
        (artifact.target.as_str(), artifact.format.as_str()),
        ("android", "aab")
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("com.polypack.polypack_app")),
        "id 映射须如实写进 notes：{:?}",
        artifact.notes
    );

    let (apk, _, _) = artifacts(&bundle);
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names.contains(&"AndroidManifest.xml".to_string()),
        "{names:?}"
    );
    assert!(names.contains(&"resources.arsc".to_string()), "{names:?}");
    let badging = dump_badging(&tools, &apk);
    // `-` 折叠为 `_`（aapt2 只接受合法 Java 包名）。
    assert!(
        badging.contains("package: name='com.polypack.polypack_app'"),
        "{badging}"
    );
    assert!(badging.contains("versionCode='10203'"), "{badging}");
    // 生成的 strings.xml 真的进了资源表（label 由 `@string/app_name` 解析）。
    assert!(
        badging.contains("application-label:'Pax Notes'"),
        "{badging}"
    );
}

#[test]
fn package_builds_unsigned_apk_and_aab() {
    let Some(tools) = sdk() else { return };
    let base = scratch("build");
    let bundle = inputs(&base, &tools);
    let artifact = AndroidAab.package(&bundle, &spec("aab")).expect("打包");
    assert_eq!(
        (artifact.target.as_str(), artifact.format.as_str()),
        ("android", "aab")
    );
    assert!(artifact.bytes > 0, "产物为空");

    let notes = artifact.notes.join("\n");
    // T-M2-06 验收：AAPT2 link 零警告（真跑断言，缺 SDK 的宿主在 sdk() 处已跳过）。
    assert!(
        !notes.lines().any(|line| line.starts_with("aapt2 警告：")),
        "AAPT2 有警告：{notes}"
    );
    let (apk, aab, apks) = artifacts(&bundle);
    assert!(apk.is_file(), "未签名 APK 缺失：{}", apk.display());

    // 结构断言（notes 即事件流）：清单 / 资源表 / 原生库 / 资源 / assets。
    // assets 目录的**内容**整体映射到 APK `assets/` 下并保留相对层级（`dist/assets/app.js`
    // → `assets/assets/app.js`，与 Tauri 的 web 资源落位一致）。
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    for want in [
        "AndroidManifest.xml",
        "resources.arsc",
        "res/mipmap-xxxhdpi-v4/ic_launcher.png",
        "lib/arm64-v8a/libpaxcore.so",
        "assets/index.html",
        "assets/assets/app.js",
    ] {
        assert!(
            names.contains(&want.to_string()),
            "APK 缺 {want}：{names:?}"
        );
        assert!(notes.contains(want), "notes 结构断言缺 {want}：{notes}");
    }

    // 清单 / 版本 / ABI（aapt2 dump badging 真跑）。
    let badging = dump_badging(&tools, &apk);
    assert!(
        badging.contains(&format!(
            "package: name='{PKG}' versionCode='10203' versionName='1.2.3'"
        )),
        "{badging}"
    );
    assert!(badging.contains("native-code: 'arm64-v8a'"), "{badging}");
    assert!(notes.contains("aapt2 dump badging：package:"), "{notes}");

    // zipalign 的**结果**断言（不信命令本身）：参数顺序错了 Linux 也能侥幸通过，只有
    // 校验能抓住——见 android.rs 里 `-p -f 4` 顺序的注释（macOS getopt 不容忍另一种写法）。
    if let Some(zipalign) = &tools.zipalign {
        let check = Command::new(zipalign)
            .args(["-c", "-p", "4"])
            .arg(&apk)
            .output()
            .expect("跑 zipalign -c");
        assert!(
            check.status.success(),
            "APK 未按 4 字节 + 页对齐（zipalign -c -p 4）：{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    if tools.bundletool.is_some() && tools.java.is_some() {
        assert!(
            artifact.path.ends_with("pax-notes-1.2.3.aab"),
            "{}",
            artifact.path.display()
        );
        assert!(aab.is_file());
        assert!(
            notes.contains("bundletool validate：App Bundle information"),
            "{notes}"
        );
        assert!(apks.is_file(), "APK 集缺失");
        assert!(std::fs::metadata(&apks).expect("APK 集元数据").len() > 0);
        assert!(notes.contains("未签名 APK 集："), "{notes}");
    } else {
        assert!(notes.contains("未执行 bundletool："), "{notes}");
    }
}

/// 参与可复现比对的产物清单：未签名 APK 恒在；bundletool + java 在场时加 AAB 与 APK 集，
/// 缺席时加降级产物（模块 zip = 声明产物）。只列**真实存在**的件。
fn watched(apk: &Path, aab: &Path, apks: &Path, declared: &Path, full: bool) -> Vec<PathBuf> {
    let mut out = vec![apk.to_path_buf()];
    if full {
        out.push(aab.to_path_buf());
        out.push(apks.to_path_buf());
    } else {
        out.push(declared.to_path_buf());
    }
    out
}

/// 产物的 `(文件名, sha256)` 清单（按名排序，便于整体比较）。
fn digests(paths: &[PathBuf]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = paths
        .iter()
        .filter(|path| path.is_file())
        .map(|path| {
            (
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                sha256(path),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn second_run_is_byte_identical() {
    let Some(tools) = sdk() else { return };
    // AAB / APK 集取决于 bundletool + java：缺席时打包器如实降级为「模块 zip + 未签名
    // APK」（见 android.rs），本用例只比对**真实产出物**，不对降级环境说谎
    // （CI 的 ubuntu 腿与 mobile-contract android 列正是这一环境）。
    let full = tools.bundletool.is_some() && tools.java.is_some();
    let base = scratch("repro");
    let bundle = inputs(&base, &tools);
    let (apk, aab, apks) = artifacts(&bundle);

    let first_artifact = AndroidAab.package(&bundle, &spec("aab")).expect("首跑");
    let watch = watched(&apk, &aab, &apks, &first_artifact.path, full);
    assert!(apk.is_file(), "未签名 APK 缺失：{}", apk.display());
    assert_eq!(
        digests(&watch).len(),
        if full { 3 } else { 2 },
        "产物件数与 bundletool 可用性不符（full={full}）：{:?}",
        digests(&watch)
    );
    let first = digests(&watch);

    // 二跑：同一工程同一命令（输出目录整份重建）。
    let _ = std::fs::remove_dir_all(&bundle.out_dir);
    std::fs::create_dir_all(&bundle.out_dir).expect("重建输出目录");
    AndroidAab.package(&bundle, &spec("aab")).expect("二跑");
    let second = digests(&watch);

    for (name, sha) in &first {
        eprintln!("首跑 {name} sha256 = {sha}");
    }
    assert_eq!(first, second, "同目录二跑必须逐字节一致（full={full}）");

    // 跨构建目录：aapt2 会把资源**源路径**嵌进 proto 资源表（AAB），故 compile 走
    // cwd + 相对目录名（见 android.rs）；换个绝对路径重建必须仍字节一致，
    // 否则 `polypack verify --reproducible` 在不同工作目录下会假红。
    let other = scratch("repro-elsewhere");
    let moved = inputs(&other, &tools);
    let moved_artifact = AndroidAab.package(&moved, &spec("aab")).expect("异地重建");
    let (moved_apk, moved_aab, moved_apks) = artifacts(&moved);
    let moved_watch = watched(
        &moved_apk,
        &moved_aab,
        &moved_apks,
        &moved_artifact.path,
        full,
    );
    assert_eq!(
        first,
        digests(&moved_watch),
        "换构建目录后必须逐字节一致（full={full}）"
    );
}

#[test]
fn abi_and_locale_filters_apply() {
    let Some(tools) = sdk() else { return };
    let base = scratch("filters");
    let mut bundle = inputs(&base, &tools);
    // 带语言资源的手写 res（语言裁剪要真资源才看得见）。
    let res = base.join("res");
    for (dir, label) in [
        ("values", "Pax Notes"),
        ("values-zh", "帕克斯笔记"),
        ("values-fr", "Pax Notes FR"),
    ] {
        let dir = res.join(dir);
        std::fs::create_dir_all(&dir).expect("建 res 目录");
        std::fs::write(
            dir.join("strings.xml"),
            format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
                 <string name=\"app_name\">{label}</string>\n</resources>\n"
            ),
        )
        .expect("写 strings");
    }
    let android = bundle.android.as_mut().expect("android 输入");
    android.res = vec![res.clone()];
    android.native_libs = vec![fake_lib(&base, "arm64-v8a"), fake_lib(&base, "x86_64")];
    android.abi_filters = vec!["x86_64".to_string()];
    android.locales = vec!["zh".to_string()];

    let plan = AndroidAab.plan(&bundle, &spec("aab")).expect("计划");
    assert!(
        plan.commands[0].line().ends_with("res-0.zip)"),
        "{}",
        plan.commands[0].line()
    );
    // compile 走 cwd = res 父目录 + 目录名（源路径嵌进 proto 资源表，绝对路径会让 AAB
    // 随构建目录漂移）；故断言 cwd 与目录名，而非绝对路径。
    let res_parent = res.parent().expect("res 父目录").display().to_string();
    assert!(
        plan.commands[0].line().contains(&res_parent)
            && plan.commands[0].line().contains("--dir res "),
        "输入的 res 目录须进 compile（cwd + 目录名）：{}",
        plan.commands[0].line()
    );
    assert!(
        plan.commands[1].line().contains("-c zh"),
        "{}",
        plan.commands[1].line()
    );

    let artifact = AndroidAab.package(&bundle, &spec("aab")).expect("打包");
    assert_eq!(artifact.format, "aab");
    let (apk, _, _) = artifacts(&bundle);
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names.contains(&"lib/x86_64/libpaxcore.so".to_string()),
        "被选中的 ABI 必须入包：{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("arm64-v8a")),
        "被裁掉的 ABI 不得入包：{names:?}"
    );
    let badging = dump_badging(&tools, &apk);
    assert!(badging.contains("'zh'"), "语言裁剪后应保留 zh：{badging}");
    assert!(!badging.contains("'fr'"), "语言裁剪后不应有 fr：{badging}");
    assert!(badging.contains("native-code: 'x86_64'"), "{badging}");
}

#[test]
fn aapt2_diagnostics_reach_the_notes() {
    let Some(tools) = sdk() else { return };
    let base = scratch("warn");
    let mut bundle = inputs(&base, &tools);
    // `hello` 只有 en 变体、没有默认值：aapt2 打 `warn: removing resource …`（退出码仍 0）。
    // 默认值那份必须留着——生成的清单要 `string/app_name`，缺了是 error 而非 warn。
    let res = base.join("res");
    std::fs::create_dir_all(res.join("values")).expect("建 res 目录");
    std::fs::create_dir_all(res.join("values-en")).expect("建 res 目录");
    std::fs::write(
        res.join("values/strings.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
         <string name=\"app_name\">Pax Notes</string>\n</resources>\n",
    )
    .expect("写 strings");
    std::fs::write(
        res.join("values-en/strings.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
         <string name=\"app_name\">Pax Notes</string>\n  \
         <string name=\"hello\">Hello</string>\n</resources>\n",
    )
    .expect("写 strings");
    bundle.android.as_mut().expect("android 输入").res = vec![res];

    let artifact = AndroidAab.package(&bundle, &spec("aab")).expect("打包");
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.starts_with("aapt2 告警：") && note.contains("hello")),
        "aapt2 的告警必须进产物说明（否则「零警告」是假绿）：{:?}",
        artifact.notes
    );
}

#[test]
fn jar_dex_inputs_become_classes_dex() {
    let Some(tools) = sdk() else { return };
    let base = scratch("dex");
    let Some(bundle) = launcher_inputs(&base, &tools) else {
        return;
    };
    let artifact = AndroidAab.package(&bundle, &spec("aab")).expect("打包样本");
    assert_eq!(artifact.format, "aab");
    let (apk, _, _) = artifacts(&bundle);
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names.contains(&"classes.dex".to_string()),
        "`.jar` 输入须经 d8 变成 classes.dex：{names:?}"
    );
    let badging = dump_badging(&tools, &apk);
    assert!(
        badging.contains("launchable-activity: name='com.polypack.paxnotes.MainActivity'"),
        "{badging}"
    );
}

#[test]
fn install_smoke_on_device() {
    let Some(tools) = sdk() else { return };
    if common::skip_without("adb") {
        return;
    }
    if !device_connected() {
        eprintln!(
            "SKIP：无连接设备 / 模拟器（阻塞（设备））——起好 AVD 后本测试走 install + 冷启动真跑"
        );
        return;
    }
    let base = scratch("smoke");
    let Some(bundle) = launcher_inputs(&base, &tools) else {
        return;
    };
    let artifact = AndroidAab.package(&bundle, &spec("aab")).expect("打包样本");
    eprintln!("产物说明：\n{}", artifact.notes.join("\n"));

    let unsigned = artifacts(&bundle).0;
    let signed = base.join("smoke-signed.apk");
    if let Err(reason) = sign_debug(&base, &tools, &unsigned, &signed) {
        eprintln!("SKIP：调试签名不可用（{reason}）；未签名 APK 无法安装（签名归 T-M2-09）");
        return;
    }
    // 设备就绪门放在打包与签名之后：核对设备健康（启动完成 + package 服务在线）与 install 之间
    // 的窗口尽量短——模拟器崩掉 / 仍在启动时无法判定产物好坏，如实 SKIP 而不误判。
    if !device_ready() {
        eprintln!("SKIP：设备尚未就绪（未启动完或 system_server 已崩）（阻塞（设备））");
        return;
    }

    let installed = adb(&["install", "-r", &signed.display().to_string()]);
    let install_out = format!(
        "{}{}",
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr)
    );
    if device_unusable(&install_out) {
        eprintln!(
            "SKIP：设备在使用中掉线（阻塞（设备））：{}",
            install_out.trim()
        );
        return;
    }
    assert!(
        installed.status.success(),
        "adb install 失败：{install_out}"
    );

    // 冷启动：LAUNCHER activity 起得来，且进程存活（随后清场）。
    let start = adb(&[
        "shell",
        "am",
        "start",
        "-W",
        "-n",
        &format!("{PKG}/.MainActivity"),
    ]);
    let stdout = String::from_utf8_lossy(&start.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&start.stderr).into_owned();
    eprintln!("am start -W：{}", stdout.trim());
    if device_unusable(&format!("{stdout}{stderr}")) {
        eprintln!(
            "SKIP：设备在冷启动途中掉线（阻塞（设备））：{}",
            stderr.trim()
        );
        return;
    }
    assert!(start.status.success(), "am start 失败：{stderr}");
    assert!(
        !stdout.contains("Error") && !stderr.contains("Error"),
        "冷启动报错：{stdout} / {stderr}"
    );
    let pid = adb(&["shell", "pidof", "-s", PKG]);
    assert!(
        !String::from_utf8_lossy(&pid.stdout).trim().is_empty(),
        "冷启动后进程未存活"
    );
    eprintln!(
        "冷启动成功：{} 进程 {}",
        PKG,
        String::from_utf8_lossy(&pid.stdout).trim()
    );

    let _ = adb(&["shell", "am", "force-stop", PKG]);
    let _ = adb(&["uninstall", PKG]);
}
