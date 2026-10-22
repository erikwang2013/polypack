//! T-M4-04 契约测试：TV 变体打包器（`android-tv` / `fire-tv`，复用 M2 的 AAPT2 + bundletool 链）。
//!
//! 契约面：注册表（两目标 `apk`）→ 口径守卫（`format` 必须 `apk`）→ 计划（Leanback 三行 +
//! banner 仅在输入 res 确有 `drawable*/tv_banner.*` 时引用）→ 真打包（`aapt2 dump badging`
//! 断言 Leanback / touchscreen 行、bundletool validate、apksigner verify）→ 有 TV 设备
//! （声明 `android.software.leanback`）时安装 + 冷启动冒烟。
//!
//! T-M4-05 真跑腿（同文件）：TV 设备上以 RN 宿主走完内核派生焦点导航走查（启动 → 遍历 →
//! 激活 → Back 返回），执行器 `ui/adapters/native/device-walks/run-walk.mjs`（期望现场从
//! 内核推导），日志归档 `target/tv-smoke-logs/`。
//!
//! 工具与设备无关性：Android SDK / adb / 设备都可能缺席（CI 三宿主）→ 打印 SKIP 后通过，
//! 绝不红，也绝不伪造真跑；非 TV 设备装不上 leanback required=true 的 APK → 按「阻塞（设备）」
//! 如实 SKIP，不误判产物。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::error::codes;
use polypack_bundle::inputs::{
    AndroidInputs, BundleInputs, Icon, IconKind, NativeLib, ProductInfo,
};
use polypack_bundle::packager::{Packager, builtin, find};
use polypack_bundle::packagers::android::AndroidTv;
use polypack_bundle::tools::{self, AndroidTools};
use polypack_bundle::zipwrite;
use polypack_targets::{Host, PackageSpec, TargetSpec};

const PKG: &str = "com.polypack.paxnotes";

/// 冒烟样本的 Activity（与 M2 相同：纯框架 API，javac 只需 android.jar 上类路径）。
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

/// 冒烟样本清单：`uses-sdk` / 版本号留给 `aapt2 link` 旗标注入；带 Leanback 两行（TV 应用形态）。
const TV_LAUNCHER_MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.polypack.paxnotes">
  <uses-feature android:name="android.software.leanback" android:required="true" />
  <uses-feature android:name="android.hardware.touchscreen" android:required="false" />
  <application android:label="@string/app_name" android:hasCode="true">
    <activity android:name="com.polypack.paxnotes.MainActivity" android:exported="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LEANBACK_LAUNCHER" />
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

/// TV 目标声明（tier 3，format `apk`）。
fn spec_tv(id: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(id, 3, &[Host::Win, Host::Linux, Host::Macos]);
    spec.package = PackageSpec {
        format: "apk".to_string(),
        builder: "android-aapt2".to_string(),
    };
    spec
}

fn scratch(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("polypack-android-tv-{tag}-{}", std::process::id()));
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

fn android_inputs(tools: &AndroidTools) -> AndroidInputs {
    AndroidInputs {
        sdk_root: Some(tools.sdk_root.clone()),
        native_libs: Vec::new(),
        ..AndroidInputs::default()
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
        assets: Some(dist),
        native_libs: vec![fake_lib(base, "arm64-v8a")],
        ..android_inputs(tools)
    });
    bundle
}

/// 自带 res 的输入：`values/strings.xml`（`@string/app_name`）+ `drawable-xhdpi/tv_banner.png`。
fn inputs_with_banner_res(base: &Path, tools: &AndroidTools) -> BundleInputs {
    let res = base.join("res");
    std::fs::create_dir_all(res.join("values")).expect("建 res/values");
    std::fs::create_dir_all(res.join("drawable-xhdpi")).expect("建 res/drawable-xhdpi");
    std::fs::write(
        res.join("values/strings.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
         <string name=\"app_name\">Pax Notes</string>\n</resources>\n",
    )
    .expect("写 strings.xml");
    std::fs::copy(
        common::fixture("pax-notes-128.png"),
        res.join("drawable-xhdpi/tv_banner.png"),
    )
    .expect("写 banner");
    let mut bundle = inputs(base, tools);
    bundle.android.as_mut().expect("android 输入").res = vec![res];
    bundle
}

/// 可启动 TV 样本：Java Activity（javac + jar）→ `.jar` 作 dex 输入；清单带 Leanback。
///
/// `abis` 是目标设备声明的 ABI（[`device_abis`]）——样本自该设备派生，硬编码会让
/// 明明可装的样本以 `INSTALL_FAILED_NO_MATCHING_ABIS` 假红（如实测：TV 模拟器是 32 位 x86）。
fn tv_launcher_inputs(base: &Path, tools: &AndroidTools, abis: &[String]) -> Option<BundleInputs> {
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
    std::fs::write(&manifest, TV_LAUNCHER_MANIFEST).expect("写样本清单");
    let mut bundle = inputs(base, tools);
    let android = bundle.android.as_mut().expect("android 输入");
    android.manifest = Some(manifest);
    android.dex_inputs = vec![classes_jar];
    // ABI 从目标设备派生（占位库不会被加载，只求安装期 ABI 匹配）
    android.abi_filters = abis.to_vec();
    android.native_libs = abis.iter().map(|abi| fake_lib(base, abi)).collect();
    Some(bundle)
}

fn artifacts(inputs: &BundleInputs) -> (PathBuf, PathBuf, PathBuf) {
    let out = &inputs.out_dir;
    (
        out.join("pax-notes-1.2.3-unsigned.apk"),
        out.join("pax-notes-1.2.3.aab"),
        out.join("pax-notes-1.2.3.apks"),
    )
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

/// 设备声明 `android.software.leanback`（TV 轨道安装门：非 TV 设备装不上 leanback 清单）。
fn device_has_leanback() -> bool {
    Command::new("adb")
        .args(["shell", "pm", "list", "features"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("android.software.leanback"))
}

/// 设备声明的 ABI（`ro.product.cpu.abilist`，按序）；adb 失败或为空 → 空表（调用方 SKIP）。
///
/// 样本 ABI 一律自目标设备派生：TV 模拟器多为 32 位 x86（`amati_x86`），硬编码
/// `arm64-v8a` / `x86_64` 会让可装样本报 `INSTALL_FAILED_NO_MATCHING_ABIS` 假红。
fn device_abis() -> Vec<String> {
    Command::new("adb")
        .args(["shell", "getprop", "ro.product.cpu.abilist"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .split(',')
                .map(|abi| abi.trim().to_string())
                .filter(|abi| !abi.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 设备**就绪**（已完成启动且 `package` 服务在线）：与产物好坏无关的信号，不满足即 SKIP。
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
fn registry_exposes_tv_packagers() {
    let all: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    for id in ["android-tv", "fire-tv"] {
        assert!(
            all.contains(&(id.to_string(), "apk".to_string())),
            "缺 {id}/apk：{all:?}"
        );
        let packager = find(id, "apk").unwrap_or_else(|| panic!("find({id}, apk) 应命中"));
        assert_eq!(packager.id(), id);
        assert_eq!(packager.format(), "apk");
    }
    // M2 契约不受影响：android 仍是 aab、没有 apk 打包器。
    assert!(find("android", "aab").is_some());
    assert!(find("android", "apk").is_none());
}

#[test]
fn tv_format_mismatch_is_rejected() {
    let base = scratch("guard");
    let mut bundle = BundleInputs::new(product(), base.join("out"));
    bundle.android = Some(AndroidInputs::default());
    for id in ["android-tv", "fire-tv"] {
        // 口径守卫：TV 打包器只认 `apk`——传入别的口径（如 aab 目标的）即报错，不静默按别家格式出包。
        let mut spec = spec_tv(id);
        spec.package.format = "aab".to_string();
        let err = AndroidTv(id).plan(&bundle, &spec).unwrap_err();
        assert_eq!(err.code(), codes::E_BND_0003, "{id}");
        assert!(
            matches!(
                err,
                polypack_bundle::error::BundleError::InvalidInput {
                    field: "spec.package.format",
                    ..
                }
            ),
            "{id}"
        );
    }
}

// ---------------------------------------------------------------- 计划与真打包（需 SDK）

#[test]
fn tv_plan_carries_leanback_and_optional_banner() {
    let Some(tools) = sdk() else { return };
    let base = scratch("tv-plan");
    let bundle = inputs(&base, &tools);
    let packager = AndroidTv("fire-tv");
    assert_eq!(packager.id(), "fire-tv");
    assert_eq!(packager.format(), "apk");

    let manifest_of = |plan: &polypack_bundle::packager::BundlePlan| -> String {
        plan.scripts
            .iter()
            .find(|script| script.path.ends_with("AndroidManifest.xml"))
            .expect("计划须含生成的清单")
            .contents
            .clone()
    };

    // 无 res 输入（生成 res）：Leanback 两行在，banner 不写（不伪造资源）。
    let plan = packager
        .plan(&bundle, &spec_tv("fire-tv"))
        .expect("TV 计划");
    let manifest = manifest_of(&plan);
    assert!(
        manifest.contains(
            "<uses-feature android:name=\"android.software.leanback\" android:required=\"true\" />"
        ),
        "{manifest}"
    );
    assert!(
        manifest.contains(
            "<uses-feature android:name=\"android.hardware.touchscreen\" android:required=\"false\" />"
        ),
        "{manifest}"
    );
    assert!(!manifest.contains("android:banner"), "{manifest}");
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
    }

    // 输入 res 带 drawable*/tv_banner.* → banner 引用出现。
    let with_res = inputs_with_banner_res(&base, &tools);
    let plan = packager
        .plan(&with_res, &spec_tv("fire-tv"))
        .expect("TV 计划（带 banner）");
    let manifest = manifest_of(&plan);
    assert!(
        manifest.contains("android:banner=\"@drawable/tv_banner\""),
        "{manifest}"
    );
}

#[test]
fn package_builds_tv_apk_with_leanback_manifest() {
    let Some(tools) = sdk() else { return };
    let base = scratch("tv-pkg");
    let bundle = inputs_with_banner_res(&base, &tools);
    let artifact = AndroidTv("android-tv")
        .package(&bundle, &spec_tv("android-tv"))
        .expect("打包 TV APK");
    assert_eq!(artifact.target, "android-tv");
    assert_eq!(artifact.format, "apk");
    let (apk, aab, apks) = artifacts(&bundle);
    assert_eq!(artifact.path, apk, "TV 产物 = 未签名 APK");
    assert!(artifact.bytes > 0, "产物体积应为正");

    // 结构：lib/<abi> 与 banner 资源进包。
    let names = zipwrite::entry_names(&apk).expect("读 APK");
    assert!(
        names
            .iter()
            .any(|name| name == "lib/arm64-v8a/libpaxcore.so"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains("tv_banner")),
        "{names:?}"
    );

    // Leanback / TV 轨道清单断言（aapt2 dump badging）。
    let badging = dump_badging(&tools, &apk);
    assert!(
        badging.contains("uses-feature: name='android.software.leanback'"),
        "{badging}"
    );
    assert!(
        badging.contains("uses-feature-not-required: name='android.hardware.touchscreen'"),
        "{badging}"
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("android:banner=\"@drawable/tv_banner\"")),
        "{:?}",
        artifact.notes
    );

    // bundletool（在场）：validate 行 + AAB / APK 集落盘；缺席：如实降级说明。
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
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.contains("未执行 bundletool")),
            "{:?}",
            artifact.notes
        );
    }

    // apksigner verify（acceptance：TV APK 过签名链取证；工具缺席即 SKIP 该子断言）。
    if let Some(apksigner) = tools.apksigner.as_deref() {
        let signed = base.join("tv-signed.apk");
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

// ---------------------------------------------------------------- 设备冒烟（需 SDK + TV 设备）

#[test]
fn tv_install_smoke_on_device() {
    let Some(tools) = sdk() else { return };
    if common::skip_without("adb") {
        return;
    }
    if !device_connected() {
        eprintln!(
            "SKIP：无连接设备 / 模拟器（阻塞（设备））——起好 TV AVD 后本测试走 install + 冷启动真跑"
        );
        return;
    }
    // TV 轨道安装门：leanback required=true 的 APK 在非 TV 设备上装不上——如实 SKIP，不误判产物。
    if !device_has_leanback() {
        eprintln!("SKIP：所连设备未声明 android.software.leanback（非 TV 设备）（阻塞（设备））");
        return;
    }
    let abis = device_abis();
    if abis.is_empty() {
        eprintln!("SKIP：读不到设备 ABI（ro.product.cpu.abilist 为空）（阻塞（设备））");
        return;
    }
    eprintln!("设备 ABI：{}", abis.join(", "));
    let base = scratch("tv-smoke");
    let Some(bundle) = tv_launcher_inputs(&base, &tools, &abis) else {
        return;
    };
    let artifact = AndroidTv("android-tv")
        .package(&bundle, &spec_tv("android-tv"))
        .expect("打包 TV 样本");
    eprintln!("产物说明：\n{}", artifact.notes.join("\n"));

    let unsigned = artifact.path.clone();
    let signed = base.join("tv-smoke-signed.apk");
    if let Err(reason) = sign_debug(&base, &tools, &unsigned, &signed) {
        eprintln!("SKIP：调试签名不可用（{reason}）；未签名 APK 无法安装（签名归 T-M2-09）");
        return;
    }
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
        "adb install 失败（Leanback 清单在 TV 设备上应可安装）：{install_out}"
    );

    // 冷启动：LEANBACK_LAUNCHER activity 起得来，且进程存活（随后清场）。
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

/// 所连设备序列号（`adb devices` 首台 `device` 状态；`unauthorized` / `offline` 不算）。
fn device_serial() -> Option<String> {
    let out = Command::new("adb").arg("devices").output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .find_map(|line| line.trim_end().strip_suffix("\tdevice").map(str::to_string))
}

/// RN 宿主 TV 冒烟包（release = 内嵌 JS bundle，无 Metro 依赖）：`POLYPACK_TV_HOST_APK`
/// 优先，否则取宿主工程默认出包路径。TV 形态 = 编译前 `host/app.json` 的
/// `polypack.pointer="none"`（无指针；焦点环由 capabilities 声明驱动渲染）。
fn host_apk() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("POLYPACK_TV_HOST_APK") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../ui/adapters/native/host/android/app/build/outputs/apk/release/app-release.apk",
    );
    path.is_file().then_some(path)
}

/// 走查日志归档目录（CI 收集用）：`POLYPACK_TV_SMOKE_LOGS` > `CARGO_TARGET_DIR/tv-smoke-logs`
/// > `<仓库根>/target/tv-smoke-logs`。
fn smoke_logs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("POLYPACK_TV_SMOKE_LOGS") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    );
    base.join("tv-smoke-logs")
}

/// T-M4-05 焦点导航冒烟（真跑腿）：TV 设备上装载 RN 宿主（`@polypack/adapter-native` 示例页），
/// 走完内核派生走查——启动 → 方向键遍历 → 激活 → Back 返回。期望值由执行器现场从焦点内核
/// 推导（`device-walks/run-walk.mjs` → `walks.mjs`），不读手写值；全程仅 D-pad / OK / Back
/// 键事件（无指针），逐键对拍 `POLYPACK_KEY` 通道与界面「点击计数」文本，日志归档。
///
/// 前置（缺席即 SKIP，绝不红）：adb + node、TV 设备就绪、宿主 release 包（见 `host_apk`）。
#[test]
fn tv_focus_nav_smoke_on_device() {
    if common::skip_without("adb") || common::skip_without("node") {
        return;
    }
    let Some(serial) = device_serial() else {
        eprintln!(
            "SKIP：无连接设备 / 模拟器（阻塞（设备））——起好 TV AVD 后本测试走 D-pad 走查真跑"
        );
        return;
    };
    if !device_has_leanback() {
        eprintln!("SKIP：所连设备未声明 android.software.leanback（非 TV 设备）（阻塞（设备））");
        return;
    }
    if !device_ready() {
        eprintln!("SKIP：设备尚未就绪（未启动完或 system_server 已崩）（阻塞（设备））");
        return;
    }
    let Some(apk) = host_apk() else {
        eprintln!(
            "SKIP：无 RN 宿主 release 包（阻塞（宿主构建））——先同步 ui/adapters/native/host/node_modules，\
             设 host/app.json polypack.pointer=\"none\"（TV 无指针形态），再 \
             `cd ui/adapters/native/host/android && ./gradlew assembleRelease -PreactNativeArchitectures=<设备 ABI>`；\
             或用 POLYPACK_TV_HOST_APK 指定产物"
        );
        return;
    };
    let runner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/adapters/native/device-walks/run-walk.mjs");
    assert!(runner.is_file(), "缺走查执行器：{}", runner.display());
    let logs = smoke_logs_dir();
    let run = Command::new("node")
        .arg(&runner)
        .arg("--serial")
        .arg(&serial)
        .arg("--apk")
        .arg(&apk)
        .arg("--logs")
        .arg(&logs)
        .output()
        .expect("跑走查执行器");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    eprintln!("{stdout}{stderr}");
    assert!(
        run.status.success(),
        "走查失败（定位与现场见上方输出与 {}）",
        logs.display()
    );
    assert!(
        stdout.contains("无指针完成核心任务：PASS"),
        "走查输出缺「无指针完成核心任务」判定"
    );
}
