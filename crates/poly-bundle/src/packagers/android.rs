//! Android 打包器（T-M2-06 / T-M2-07）：AAPT2 免 Gradle 直出未签名 AAB 与 APK。
//!
//! 链路：`aapt2 compile`（每个 res 目录一条）→ `aapt2 link`（二进制 APK）与 `aapt2 link
//! --proto-format`（AAB 模块源）→ 补 `lib/<abi>` / `assets` / `classes.dex` → `zipalign`
//! → 模块 zip → bundletool `build-bundle` / `validate` / `build-apks`。全程不依赖 Gradle。
//!
//! TV 变体（T-M4-04，`android-tv` / `fire-tv`，format `apk`）复用同一条链：差异只有生成清单
//! 的 Leanback 三行（`android.software.leanback` / `android.hardware.touchscreen` / banner，
//! 见 `generated_manifest`）与产物落点（未签名 APK）——不另起第二套 Android 打包。
//!
//! AAOS 变体（T-M6-11，`android-automotive`，format `apk`）同链复用：清单只加
//! `android.hardware.type.automotive` 一行，产物落点同 TV（未签名 APK）——车厂渠道的占位
//! 只发生在发布侧（poly-publish `oem-channel`），打包链保持与 Android 完全同源。
//!
//! 签名不在本任务范围内（T-M2-09 接入），产物一律是**未签名**件；bundletool 缺席时如实降级
//! （模块 zip + 未签名 APK，notes 写明未做 bundletool 步骤），绝不伪造 bundletool 结果。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{AndroidInputs, BundleInputs, IconKind, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager, ScriptFile};
use crate::tools::{self, AndroidTools};
use crate::zipwrite;

/// AAB 打包器（`TargetSpec.package.format = "aab"`，builder `android-aapt2`）。
pub struct AndroidAab;

/// TV 变体打包器（`android-tv` / `fire-tv`；format `apk`，builder `android-aapt2`）。
///
/// 复用 [`AndroidAab`] 的整条内部链（T-M4-04）：差异只有生成清单的 Leanback 三行与最终
/// 产物落点（未签名 APK；AAB / APK 集作为链中间产物照常落盘并进 notes）。字段即目标 id。
pub struct AndroidTv(pub &'static str);

/// AAOS 变体打包器（`android-automotive`；format `apk`，builder `android-aapt2`）。
///
/// 复用 [`AndroidAab`] 的整条内部链（T-M6-11）：差异只有生成清单的
/// `android.hardware.type.automotive` 行与产物落点（未签名 APK，同 TV）。
pub struct AndroidAutomotive;

/// 打包口径：AAB 目标与 TV / AAOS 变体共用全部内部步骤，差异集中在此。
#[derive(Clone, Copy)]
struct Flavor {
    /// 事件 / 产物 / 错误归属（`Packager::id()`）。
    id: &'static str,
    /// 打包格式（`aab` / `apk`）。
    format: &'static str,
    /// TV 清单（Leanback 三行 + TV banner 引用）。
    tv: bool,
    /// AAOS 清单（`android.hardware.type.automotive`）。
    automotive: bool,
}

/// AAB 口径。
const AAB_FLAVOR: Flavor = Flavor {
    id: ANDROID,
    format: AAB,
    tv: false,
    automotive: false,
};

/// TV 口径（两目标的差异只有 id）。
const fn tv_flavor(id: &'static str) -> Flavor {
    Flavor {
        id,
        format: APK,
        tv: true,
        automotive: false,
    }
}

/// AAOS 口径（T-M6-11）。
const AUTOMOTIVE_FLAVOR: Flavor = Flavor {
    id: ANDROID_AUTOMOTIVE,
    format: APK,
    tv: false,
    automotive: true,
};

const ANDROID: &str = "android";
const ANDROID_AUTOMOTIVE: &str = "android-automotive";
const AAB: &str = "aab";
const APK: &str = "apk";
/// AAB 模块 zip 里清单的落点（bundletool 契约：根级 `manifest/AndroidManifest.xml`）。
const MODULE_MANIFEST: &str = "manifest/AndroidManifest.xml";
/// ABI 的固定呈现顺序（未显式指定 `abi_filters` 时的裁剪基准）。
const ABI_ORDER: &[&str] = &["arm64-v8a", "armeabi-v7a", "x86_64", "x86"];
/// bundletool 缺位时的如实降级说明。
const BUNDLETOOL_HINT: &str = "AAB 校验与 APK 集需要 bundletool-all.jar 与 java\
     （AndroidInputs.bundletool / $POLYPACK_BUNDLETOOL / PATH / ~/.cache/polypack/tools/）";
/// bundletool 的 `BundleConfig.json`：ABI / 密度 / 语言三条 split 维度。
const BUNDLE_CONFIG: &str = concat!(
    "{\"optimizations\":{\"splitsConfig\":{\"splitDimension\":[",
    "{\"value\":\"ABI\",\"negate\":false},",
    "{\"value\":\"SCREEN_DENSITY\",\"negate\":false},",
    "{\"value\":\"LANGUAGE\",\"negate\":false}]}},",
    "\"compression\":{\"uncompressedGlob\":[]}}\n",
);

impl Packager for AndroidAab {
    fn id(&self) -> &'static str {
        ANDROID
    }

    fn format(&self) -> &'static str {
        AAB
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        AndroidAab::chain_plan(inputs, spec, AAB_FLAVOR)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        AndroidAab::chain_package(inputs, spec, AAB_FLAVOR)
    }
}

impl Packager for AndroidTv {
    fn id(&self) -> &'static str {
        self.0
    }

    fn format(&self) -> &'static str {
        APK
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        AndroidAab::chain_plan(inputs, spec, tv_flavor(self.0))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        AndroidAab::chain_package(inputs, spec, tv_flavor(self.0))
    }
}

impl Packager for AndroidAutomotive {
    fn id(&self) -> &'static str {
        ANDROID_AUTOMOTIVE
    }

    fn format(&self) -> &'static str {
        APK
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        AndroidAab::chain_plan(inputs, spec, AUTOMOTIVE_FLAVOR)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        AndroidAab::chain_package(inputs, spec, AUTOMOTIVE_FLAVOR)
    }
}

impl AndroidAab {
    /// 口径参数化的计划体：`Packager::plan` 与 TV 变体（[`AndroidTv`]）共用同一实现。
    fn chain_plan(
        inputs: &BundleInputs,
        spec: &TargetSpec,
        flavor: Flavor,
    ) -> Result<BundlePlan, BundleError> {
        check_format(spec, flavor.format)?;
        // 生成的清单与 res 直接内插产品元数据：先过白名单校验（无 XML / 路径元字符）。
        inputs.product.validate(flavor.id)?;
        let android = android_inputs(inputs);
        let tools = tools::resolve(&android)?;
        let stage = stage(inputs, &android, &tools);
        if requires_d8(&android) && tools.d8.is_none() {
            return Err(missing_d8());
        }

        let mut plan = BundlePlan::new();
        if android.manifest.is_none() {
            let has_icon = icon_mipmap(inputs, &android).is_some();
            let package = android_package(&inputs.product.identifier)?;
            let manifest = generated_manifest(
                &inputs.product,
                &package,
                &android,
                &stage,
                has_icon,
                flavor,
            );
            plan = plan.with_script(stage.manifest.clone(), manifest);
        }
        for script in generated_res(&inputs.product, &android, &stage) {
            plan = plan.with_script(script.path, script.contents);
        }
        plan = plan.with_script(stage.config.clone(), BUNDLE_CONFIG);
        for command in aapt2_commands(&stage, &android, &tools) {
            plan = plan.with_command(command);
        }
        match (&tools.bundletool, &tools.java) {
            (Some(jar), Some(java)) => {
                for command in bundletool_commands(java, jar, &stage) {
                    plan = plan.with_command(command);
                }
            }
            _ => plan = plan.skipping("bundletool", BUNDLETOOL_HINT),
        }
        Ok(plan)
    }

    /// 口径参数化的打包体：落盘脚本 → 执行命令 → 汇总产物（与 `chain_plan` 同口径）。
    fn chain_package(
        inputs: &BundleInputs,
        spec: &TargetSpec,
        flavor: Flavor,
    ) -> Result<Artifact, BundleError> {
        let plan = AndroidAab::chain_plan(inputs, spec, flavor)?;
        let android = android_inputs(inputs);
        let tools = tools::resolve(&android)?;
        let stage = stage(inputs, &android, &tools);

        // 落盘：生成的清单 / res 与 BundleConfig（图标 PNG 是二进制，走拷贝而非脚本）。
        let _ = std::fs::remove_dir_all(&stage.root);
        fsutil::write_scripts(&plan)?;
        if let Some((bucket, icon)) = icon_mipmap(inputs, &android) {
            let target = stage
                .generated_res
                .join(format!("mipmap-{bucket}/ic_launcher.png"));
            fsutil::copy_file(&icon, &target, false)?;
        }

        // 1) aapt2：compile（每个 res 目录一条）+ 二进制 link + proto link（AAB 模块源）。
        //    逐条检查 stderr：aapt2 的告警不进退出码（实测前缀是 `warn:`，不是 `warning:`），
        //    却可能悄悄丢资源（如 `warn: removing resource …:string/x without required default
        //    value`）——如实记进产物说明，不靠「不看就没错」。
        let mut notes = Vec::new();
        if flavor.automotive {
            notes.push(match android.manifest.is_some() {
                true => {
                    "AAOS 清单：使用输入清单，未注入 android.hardware.type.automotive".to_string()
                }
                false => "AAOS 清单：android.hardware.type.automotive required=true".to_string(),
            });
        }
        if flavor.tv {
            match (android.manifest.is_some(), has_tv_banner(&android)) {
                (true, _) => notes.push(
                    "TV 清单：使用输入清单，未注入 Leanback uses-feature / banner".to_string(),
                ),
                (false, true) => notes.push(
                    "TV 清单：Leanback uses-feature + android:banner=\"@drawable/tv_banner\""
                        .to_string(),
                ),
                (false, false) => notes.push(
                    "TV 清单：Leanback uses-feature；未提供 drawable*/tv_banner.*，\
                     android:banner 不写（不伪造资源）"
                        .to_string(),
                ),
            }
        }
        for command in aapt2_commands(&stage, &android, &tools) {
            let output = exec::run(&command)?;
            for line in String::from_utf8_lossy(&output.stderr).lines() {
                if line.contains("warn") {
                    notes.push(format!("aapt2 告警：{}", line.trim()));
                }
            }
        }

        // 2) dex：无 Java 侧时不产 dex（正常）；`.jar` 交给 d8。
        let dex = dex_entries(&android, &stage, &tools)?;
        let (apk_added, module_added) = added_entries(inputs, &android, &dex)?;

        // 3) 未签名 APK：aapt2 的二进制 APK 补 lib / assets / dex，再 zipalign。
        notes.push(format!("未签名 APK：{}", stage.apk.display()));
        let package = android_package(&inputs.product.identifier)?;
        if package != inputs.product.identifier {
            notes.push(format!(
                "应用 id：{} → {package}（Android 要求合法 Java 包名，非法字符已折叠为 `_`）",
                inputs.product.identifier
            ));
        }
        zipwrite::extend_zip(
            &stage.linked,
            &[],
            &["META-INF/"],
            &apk_added,
            &stage.assembled,
        )?;
        if let Some(zipalign) = &tools.zipalign {
            // 参数顺序：`<align>` 之前必须是全部选项（`zipalign [-f] [-p] <align> in out`）。
            // 把 `-f` 写在 `<align>` 之后，Linux 的 glibc getopt 会重排、侥幸通过，而
            // macOS/BSD getopt 在首个非选项处停止解析 → usage + exit 2（实测：
            // `POSIXLY_CORRECT=1 zipalign -p 4 -f …` 在本机可复现同一条 usage）。
            exec::run(&with_source_date(Command::new(
                path_arg(zipalign),
                [
                    "-p".to_string(),
                    "-f".to_string(),
                    "4".to_string(),
                    path_arg(&stage.assembled),
                    path_arg(&stage.apk),
                ],
            )))?;
        } else {
            fsutil::copy_file(&stage.assembled, &stage.apk, false)?;
            notes.push("未 zipalign（build-tools 内缺 zipalign）：APK 未对齐".to_string());
        }
        notes.extend(apk_notes(&stage.apk, &tools));

        // 4) AAB：proto APK → 模块 zip → bundletool 组装 / 校验 / APK 集。
        build_module(&stage, &module_added)?;
        let mut artifact_path = stage.module.clone();
        match (&tools.bundletool, &tools.java) {
            (Some(jar), Some(java)) => {
                let [build, validate, apks] = bundletool_commands(java, jar, &stage);
                exec::run(&build)?;
                let validated = exec::run(&validate)?;
                let stdout = String::from_utf8_lossy(&validated.stdout);
                let line = stdout
                    .lines()
                    .find(|line| line.contains("App Bundle information"))
                    .unwrap_or("（stdout 无 `App Bundle information` 行）");
                notes.push(format!("bundletool validate：{}", line.trim()));
                exec::run(&apks)?;
                notes.push(format!("未签名 APK 集：{}", stage.apks.display()));
                artifact_path = stage.aab.clone();
            }
            _ => notes.push(format!(
                "未执行 bundletool：降级产物为 AAB 模块 zip（{}）——{BUNDLETOOL_HINT}",
                stage.module.display()
            )),
        }
        if flavor.format == APK {
            // TV / AAOS 产物 = 未签名 APK（签名阶段按 `<kebab>-<version>-unsigned.apk` 定位）；
            // AAB / APK 集留在盘上作链证据，notes 已逐条列出。
            artifact_path = stage.apk.clone();
        }
        fsutil::file_artifact(flavor.id, flavor.format, &artifact_path, notes)
    }
}

/// 暂存目录内的全部落点（`plan()` 与 `package()` 共用的纯路径推导，不写盘）。
struct Stage {
    root: PathBuf,
    /// `aapt2 link --manifest` 实际使用的清单（生成件在暂存目录内，输入件在调用方给的路径）。
    manifest: PathBuf,
    /// 生成的 `res/`（无 `res` 输入时）。
    generated_res: PathBuf,
    /// （res 目录, `aapt2 compile` 输出）对：生成 res 在前，输入的 res 目录按给定顺序在后。
    compiled: Vec<(PathBuf, PathBuf)>,
    /// `aapt2 link` 的二进制 APK。
    linked: PathBuf,
    /// `aapt2 link --proto-format` 的 proto APK（AAB 模块源）。
    proto: PathBuf,
    /// d8 输出目录。
    dex_dir: PathBuf,
    /// 补完 lib / assets / dex 的 APK（zipalign 之前）。
    assembled: PathBuf,
    /// AAB 模块 zip。
    module: PathBuf,
    /// bundletool 的 `BundleConfig.json`。
    config: PathBuf,
    /// 最终未签名 APK。
    apk: PathBuf,
    /// 最终 AAB。
    aab: PathBuf,
    /// bundletool 产出的 APK 集。
    apks: PathBuf,
    /// 生效的 `versionCode`（显式值优先，否则由语义化版本派生）。
    version_code: u32,
    /// 生效的 `versionName`（产品语义化版本）。
    version_name: String,
    /// 生效的 `targetSdkVersion`（`0` → SDK 内最新平台）。
    target_sdk: u32,
}

/// 推导暂存与产物路径（只读输入与已解析的工具链）。
fn stage(inputs: &BundleInputs, android: &AndroidInputs, tools: &AndroidTools) -> Stage {
    let root = inputs
        .out_dir
        .join(format!("{}-android.stage", inputs.product.kebab_name()));
    let base = format!("{}-{}", inputs.product.kebab_name(), inputs.product.version);
    let generated_res = root.join("generated-res");
    let mut compiled = Vec::new();
    if android.res.is_empty() {
        compiled.push((generated_res.clone(), root.join("generated-res.zip")));
    }
    for (index, dir) in android.res.iter().enumerate() {
        compiled.push((dir.clone(), root.join(format!("res-{index}.zip"))));
    }
    Stage {
        manifest: android
            .manifest
            .clone()
            .unwrap_or_else(|| root.join("AndroidManifest.xml")),
        compiled,
        generated_res,
        linked: root.join("linked.apk"),
        proto: root.join("proto.apk"),
        dex_dir: root.join("dex"),
        assembled: root.join("assembled.apk"),
        module: root.join("base.zip"),
        config: root.join("BundleConfig.json"),
        apk: inputs.out_dir.join(format!("{base}-unsigned.apk")),
        aab: inputs.out_dir.join(format!("{base}.aab")),
        apks: inputs.out_dir.join(format!("{base}.apks")),
        version_code: derive_version_code(&inputs.product.version, android.version_code),
        version_name: inputs.product.version.clone(),
        target_sdk: target_sdk(android, tools),
        root,
    }
}

/// aapt2 命令序列：每个 res 目录一条 `compile` → 二进制 `link` → proto `link`。
fn aapt2_commands(stage: &Stage, android: &AndroidInputs, tools: &AndroidTools) -> Vec<Command> {
    let aapt2 = path_arg(&tools.aapt2);
    // 二进制与 proto 两态只差 `--proto-format` 与输出路径。
    let link = |out: &Path, proto: bool| {
        let mut args: Vec<String> = vec!["link".to_string()];
        if proto {
            args.push("--proto-format".to_string());
        }
        args.extend([
            "-o".to_string(),
            path_arg(out),
            "--manifest".to_string(),
            path_arg(&stage.manifest),
            "-I".to_string(),
            path_arg(&tools.android_jar),
            "--min-sdk-version".to_string(),
            android.min_sdk.to_string(),
            "--target-sdk-version".to_string(),
            stage.target_sdk.to_string(),
            "--version-code".to_string(),
            stage.version_code.to_string(),
            "--version-name".to_string(),
            stage.version_name.clone(),
        ]);
        // 语言裁剪：aapt2 的开关是 `-c`（可重复；该版本没有 `--config`）。
        for locale in &android.locales {
            args.push("-c".to_string());
            args.push(locale.clone());
        }
        for (_, zip) in &stage.compiled {
            args.push(path_arg(zip));
        }
        with_source_date(Command::new(aapt2.clone(), args))
    };

    let mut commands = Vec::new();
    for (dir, zip) in &stage.compiled {
        // compile 的源路径会嵌进 proto 资源表（实测：绝对路径使 AAB 随构建目录漂移），
        // 故 cwd 落在 res 的父目录、`--dir` 只给目录名——嵌入路径与绝对构建目录无关。
        let parent = dir.parent().unwrap_or_else(|| Path::new("."));
        let name = dir
            .file_name()
            .map_or_else(|| path_arg(dir), |n| n.to_string_lossy().into_owned());
        commands.push(with_source_date(
            Command::new(
                aapt2.clone(),
                [
                    "compile".to_string(),
                    "--dir".to_string(),
                    name,
                    "-o".to_string(),
                    path_arg(zip),
                ],
            )
            .in_dir(parent),
        ));
    }
    commands.push(link(&stage.linked, false));
    commands.push(link(&stage.proto, true));
    commands
}

/// bundletool 三步（`java -jar`）：组装 AAB → 校验 → 生成 APK 集。
fn bundletool_commands(java: &Path, jar: &Path, stage: &Stage) -> [Command; 3] {
    let java = path_arg(java);
    let jar = path_arg(jar);
    let aab = path_arg(&stage.aab);
    let run = |args: Vec<String>| with_source_date(Command::new(java.clone(), args));
    [
        run(vec![
            "-jar".to_string(),
            jar.clone(),
            "build-bundle".to_string(),
            format!("--modules={}", path_arg(&stage.module)),
            format!("--output={aab}"),
            format!("--config={}", path_arg(&stage.config)),
            "--overwrite".to_string(),
        ]),
        run(vec![
            "-jar".to_string(),
            jar.clone(),
            "validate".to_string(),
            format!("--bundle={aab}"),
        ]),
        run(vec![
            "-jar".to_string(),
            jar,
            "build-apks".to_string(),
            format!("--bundle={aab}"),
            format!("--output={}", path_arg(&stage.apks)),
            "--overwrite".to_string(),
        ]),
    ]
}

/// 可复现构建：`SOURCE_DATE_EPOCH` 存在时注入（aapt2 / java / d8 / zipalign），未设置则不注入。
fn with_source_date(mut command: Command) -> Command {
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        command = command.with_env("SOURCE_DATE_EPOCH", epoch);
    }
    command
}

/// 最小清单（未给 `manifest` 输入时）：包名 / 版本 / `uses-sdk` / `application`。
///
/// `@string/app_name` 由生成的 `res/values/strings.xml`（无 `res` 输入时）或调用方的 res 提供；
/// `hasCode` 按有无 dex 输入取真；图标仅在同时生成 mipmap 时才引用。
/// TV 变体（`flavor.tv`）另加 Leanback 三行（T-M4-04 / ROADMAP §3.4）：
/// `android.software.leanback` required=true、`android.hardware.touchscreen` required=false、
/// `android:banner`（仅在输入 res 确有 `drawable*/tv_banner.*` 时引用——缺则省去，不伪造资源）。
/// AAOS 变体（`flavor.automotive`）只加 `android.hardware.type.automotive` required=true
/// （T-M6-11 / ROADMAP §3.7）。
fn generated_manifest(
    product: &ProductInfo,
    package: &str,
    android: &AndroidInputs,
    stage: &Stage,
    has_icon: bool,
    flavor: Flavor,
) -> String {
    let has_code = !android.dex_inputs.is_empty();
    let icon = if has_icon {
        " android:icon=\"@mipmap/ic_launcher\""
    } else {
        ""
    };
    let banner = if flavor.tv && has_tv_banner(android) {
        " android:banner=\"@drawable/tv_banner\""
    } else {
        ""
    };
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str(&format!(
        "<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\" package=\"{}\" \
         android:versionCode=\"{}\" android:versionName=\"{}\">\n",
        package, stage.version_code, product.version
    ));
    out.push_str(&format!(
        "  <uses-sdk android:minSdkVersion=\"{}\" android:targetSdkVersion=\"{}\" />\n",
        android.min_sdk, stage.target_sdk
    ));
    if flavor.tv {
        out.push_str(
            "  <uses-feature android:name=\"android.software.leanback\" android:required=\"true\" />\n",
        );
        out.push_str(
            "  <uses-feature android:name=\"android.hardware.touchscreen\" android:required=\"false\" />\n",
        );
    }
    if flavor.automotive {
        out.push_str(
            "  <uses-feature android:name=\"android.hardware.type.automotive\" android:required=\"true\" />\n",
        );
    }
    out.push_str(&format!(
        "  <application android:label=\"@string/app_name\" android:hasCode=\"{has_code}\"{icon}{banner} />\n"
    ));
    out.push_str("</manifest>\n");
    out
}

/// 输入 res 里是否存在 TV banner（`drawable*/tv_banner.*`——banner 是 Leanback 的显式资源，
/// 通道与密度桶由 aapt2 编译期决定，这里只看顶层 `drawable*` 目录里是否有 `tv_banner.*`）。
fn has_tv_banner(android: &AndroidInputs) -> bool {
    android.res.iter().any(|root| {
        let Ok(entries) = std::fs::read_dir(root) else {
            return false;
        };
        entries.flatten().any(|dir| {
            dir.file_name().to_string_lossy().starts_with("drawable")
                && std::fs::read_dir(dir.path()).is_ok_and(|files| {
                    files
                        .flatten()
                        .any(|file| file.file_name().to_string_lossy().starts_with("tv_banner."))
                })
        })
    })
}

/// 生成的 `res/`（无 `res` 输入时）：`values/strings.xml` 的 `app_name`。
///
/// 图标是二进制（PNG），不走脚本文本，由 `package()` 拷进 `mipmap-<密度>/`。
fn generated_res(product: &ProductInfo, android: &AndroidInputs, stage: &Stage) -> Vec<ScriptFile> {
    if !android.res.is_empty() {
        return Vec::new();
    }
    vec![ScriptFile {
        path: stage.generated_res.join("values/strings.xml"),
        contents: format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
             <string name=\"app_name\">{}</string>\n</resources>\n",
            product.name
        ),
    }]
}

/// 应用 id → `<manifest package>`：Android 要求**合法 Java 包名**，而 PolyPack 的反向域名
/// 标识允许连字符（`com.polypack.pax-notes`，macOS / deb / iOS 均合法），aapt2 会直接拒绝
/// （`not a valid Android package name`）。这里逐段折叠：保留 `[A-Za-z0-9_]`，其余字符 → `_`，
/// 数字开头的段补 `_`；映射结果与原名不同时写进产物 notes（如实披露，不静默改名）。
fn android_package(identifier: &str) -> Result<String, BundleError> {
    let mut segments = Vec::new();
    for segment in identifier.split('.') {
        if segment.is_empty() {
            return Err(unmappable_package(identifier));
        }
        let mut mapped = String::with_capacity(segment.len() + 1);
        for (index, ch) in segment.char_indices() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                if index == 0 && ch.is_ascii_digit() {
                    mapped.push('_');
                }
                mapped.push(ch);
            } else {
                mapped.push('_');
            }
        }
        segments.push(mapped);
    }
    if segments.len() < 2 {
        return Err(unmappable_package(identifier));
    }
    Ok(segments.join("."))
}

/// 无法映射成 Java 包名（分段数 < 2 或含空段）时的错误。
fn unmappable_package(identifier: &str) -> BundleError {
    BundleError::InvalidInput {
        packager: ANDROID,
        field: "product.identifier",
        value: identifier.to_string(),
        reason: "Android 应用 id 须至少两段（如 `com.example.app`）且不含空段".to_string(),
    }
}

/// mipmap 图标的密度桶（256 → xxxhdpi、128 → xhdpi、48 → mdpi；更小的尺寸不成桶即跳过）。
fn density_bucket(size: Option<u32>) -> Option<&'static str> {
    match size? {
        size if size >= 192 => Some("xxxhdpi"),
        size if size >= 144 => Some("xxhdpi"),
        size if size >= 96 => Some("xhdpi"),
        size if size >= 72 => Some("hdpi"),
        size if size >= 48 => Some("mdpi"),
        _ => None,
    }
}

/// 生成 mipmap 的 PNG 图标与密度桶（仅无 `res` 输入时；取第一枚可成桶的 PNG）。
fn icon_mipmap(inputs: &BundleInputs, android: &AndroidInputs) -> Option<(&'static str, PathBuf)> {
    if !android.res.is_empty() {
        return None;
    }
    inputs
        .icons
        .iter()
        .filter(|icon| icon.kind == IconKind::Png)
        .find_map(|icon| density_bucket(icon.size).map(|bucket| (bucket, icon.path.clone())))
}

/// 入选原生库的 ABI：显式 `abi_filters` 优先；否则取 `native_libs` 出现的 ABI，
/// 固定顺序 arm64-v8a → armeabi-v7a → x86_64 → x86（表外 ABI 按字典序在后）。
fn selected_abis(android: &AndroidInputs) -> Vec<String> {
    if !android.abi_filters.is_empty() {
        return android.abi_filters.clone();
    }
    let mut present: Vec<String> = android
        .native_libs
        .iter()
        .map(|lib| lib.abi.clone())
        .collect();
    present.sort();
    present.dedup();
    present.sort_by_key(|abi| {
        ABI_ORDER
            .iter()
            .position(|known| known == &abi.as_str())
            .unwrap_or(ABI_ORDER.len())
    });
    present
}

/// `lib/<abi>/<文件名>` 条目（APK 与 AAB 模块同布局）。
fn lib_entries(android: &AndroidInputs) -> Result<Vec<(String, PathBuf)>, BundleError> {
    let abis = selected_abis(android);
    let mut entries = Vec::new();
    for lib in &android.native_libs {
        // ABI 目录名进产物路径：只允许 [a-z0-9_-]（防路径穿越进 APK / AAB）。
        if lib.abi.is_empty()
            || !lib
                .abi
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        {
            return Err(BundleError::InvalidInput {
                packager: ANDROID,
                field: "android.native_libs",
                value: lib.abi.clone(),
                reason: "ABI 目录名只允许 [a-z0-9_-]（如 arm64-v8a）".to_string(),
            });
        }
        if abis.contains(&lib.abi) {
            entries.push((
                format!(
                    "lib/{}/{}",
                    lib.abi,
                    file_name(&lib.path, "android.native_libs")?
                ),
                lib.path.clone(),
            ));
        }
    }
    Ok(entries)
}

/// `assets/` 条目：显式 `assets` 目录优先，缺省用 UI 产物（RN bundle 等）。
fn asset_entries(
    inputs: &BundleInputs,
    android: &AndroidInputs,
) -> Result<Vec<(String, PathBuf)>, BundleError> {
    let Some(root) = android.assets.as_deref().or(inputs.ui_dist.as_deref()) else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    collect_tree(root, "assets", &mut entries)?;
    Ok(entries)
}

/// 递归收集目录树为 `前缀/相对路径` 条目（符号链接不入包：assets 由构建产出，无链接语义）。
fn collect_tree(
    root: &Path,
    prefix: &str,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(root).map_err(io_err(root))? {
        let entry = entry.map_err(io_err(root))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_err(&path))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            collect_tree(&path, &format!("{prefix}/{name}"), out)?;
        } else if file_type.is_file() {
            out.push((format!("{prefix}/{name}"), path));
        }
    }
    Ok(())
}

/// dex 条目：`*.dex` 直接入包；`*.jar` 交给 d8 转 `classes.dex`（d8 缺失即 ToolMissing）。
fn dex_entries(
    android: &AndroidInputs,
    stage: &Stage,
    tools: &AndroidTools,
) -> Result<Vec<(String, PathBuf)>, BundleError> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut jars: Vec<&Path> = Vec::new();
    for input in &android.dex_inputs {
        if is_dex(input) {
            entries.push((file_name(input, "android.dex_inputs")?, input.clone()));
        } else {
            jars.push(input);
        }
    }
    if !jars.is_empty() {
        let d8 = tools.d8.as_deref().ok_or_else(missing_d8)?;
        std::fs::create_dir_all(&stage.dex_dir).map_err(io_err(&stage.dex_dir))?;
        let mut args = vec![
            "--min-api".to_string(),
            android.min_sdk.to_string(),
            "--lib".to_string(),
            path_arg(&tools.android_jar),
            "--output".to_string(),
            path_arg(&stage.dex_dir),
        ];
        args.extend(jars.iter().map(|jar| path_arg(jar)));
        if let Err(err) = exec::run(&with_source_date(Command::new(path_arg(d8), args))) {
            // d8 由 java 驱动，老 JDK（< 17）的失败表现是 java 类加载栈（sun.misc.Launcher…），
            // 用户读不出修复动作：探测 `java -version`，把「需要 JDK 17+」作为修复提示
            // （`hint`，经 poly-cli 接到 Diagnostic）。探测不到（无 java / 输出不可解析）不遮蔽原错误。
            let java = path_arg(tools.java.as_deref().unwrap_or(Path::new("java")));
            let version = exec::run(&Command::new(java, ["-version"]))
                .map(|out| String::from_utf8_lossy(&out.stderr).into_owned());
            return Err(match version.ok().and_then(|v| old_jdk_hint(&v)) {
                Some(hint) => err.with_hint(hint),
                None => err,
            });
        }
        for entry in std::fs::read_dir(&stage.dex_dir).map_err(io_err(&stage.dex_dir))? {
            let path = entry.map_err(io_err(&stage.dex_dir))?.path();
            if is_dex(&path) {
                entries.push((file_name(&path, "android.dex_inputs")?, path));
            }
        }
    }
    // 同名 dex（如两个 classes.dex）会让产物出现重复条目 → 明确拒绝，不静默丢件。
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(BundleError::InvalidInput {
            packager: ANDROID,
            field: "android.dex_inputs",
            value: entries
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            reason: "dex 条目重名（APK 内多 dex 须为 classes.dex / classes2.dex …）".to_string(),
        });
    }
    Ok(entries)
}

/// `java -version`（stderr）里的主版本 < 17 → 修复提示，否则 `None`。
///
/// 文件名后缀随平台（windows 上 java.exe——类加载栈同样出在老 JDK 上）；老式 `1.x` 命名
/// （`"1.8.0_402"` → 8）与 JEP 223（`"17.0.10"` → 17）都认。
fn old_jdk_hint(version_output: &str) -> Option<String> {
    let (_, rest) = version_output.split_once('"')?;
    let (version, _) = rest.split_once('"')?;
    let mut parts = version.split(['.', '_', '-']);
    let first = parts.next()?;
    let major: u32 = if first == "1" {
        parts.next()?.parse().ok()?
    } else {
        first.parse().ok()?
    };
    (major < 17).then(|| {
        format!(
            "检测到 JDK {major}（Android 构建链需要 JDK 17+）：d8 由 java 驱动，\
             老 JDK 在类加载阶段就失败。安装 / 切换 JDK 17+（如 Temurin 17）后重试"
        )
    })
}

/// （zip 内路径, 源文件）对。
type Entry = (String, PathBuf);

/// APK 与 AAB 模块的追加条目（dex 在 APK 根、模块在 `dex/` 下；原生库与 assets 同布局）。
fn added_entries(
    inputs: &BundleInputs,
    android: &AndroidInputs,
    dex: &[Entry],
) -> Result<(Vec<Entry>, Vec<Entry>), BundleError> {
    let libs = lib_entries(android)?;
    let assets = asset_entries(inputs, android)?;
    let mut apk: Vec<(String, PathBuf)> = dex.to_vec();
    apk.extend(libs.iter().cloned());
    apk.extend(assets.iter().cloned());
    let mut module: Vec<(String, PathBuf)> = dex
        .iter()
        .map(|(name, path)| (format!("dex/{name}"), path.clone()))
        .collect();
    module.extend(libs);
    module.extend(assets);
    Ok((apk, module))
}

/// AAB 模块 zip：proto APK 的清单移到 `manifest/`、丢 `META-INF`，再补 dex / lib / assets。
fn build_module(stage: &Stage, added: &[(String, PathBuf)]) -> Result<(), BundleError> {
    zipwrite::extend_zip(
        &stage.proto,
        &[("AndroidManifest.xml", MODULE_MANIFEST)],
        &["META-INF/"],
        added,
        &stage.module,
    )
}

/// APK 结构断言（notes 即事件流）：zip 条目清单 + `aapt2 dump badging` 的清单 / ABI / 语言行。
fn apk_notes(apk: &Path, tools: &AndroidTools) -> Vec<String> {
    let mut notes = Vec::new();
    match zipwrite::entry_names(apk) {
        Ok(names) => notes.push(format!("APK 结构：{}", names.join(", "))),
        Err(err) => notes.push(format!("APK 结构读取失败：{err}")),
    }
    let dump = Command::new(
        path_arg(&tools.aapt2),
        ["dump".to_string(), "badging".to_string(), path_arg(apk)],
    );
    if let Ok(output) = exec::run(&dump) {
        let text = String::from_utf8_lossy(&output.stdout);
        for key in ["package:", "native-code:", "locales:"] {
            if let Some(line) = text.lines().find(|line| line.starts_with(key)) {
                notes.push(format!("aapt2 dump badging：{line}"));
            }
        }
    }
    notes
}

/// `versionCode` 派生：`主*10000 + 次*100 + 补`（如 `1.2.3` → 10203），显式值优先。
pub fn derive_version_code(version: &str, explicit: Option<u32>) -> u32 {
    if let Some(code) = explicit {
        return code;
    }
    let (major, minor, patch) = crate::inputs::version_parts(version);
    // Google Play 的 versionCode 上限 2_100_000_000（越界的语义化版本夹取到上限）。
    u32::try_from(major * 10_000 + minor * 100 + patch)
        .unwrap_or(2_100_000_000)
        .min(2_100_000_000)
}

/// 生效的 `targetSdkVersion`（`0` → SDK 内最新平台）。
fn target_sdk(android: &AndroidInputs, tools: &AndroidTools) -> u32 {
    if android.target_sdk != 0 {
        return android.target_sdk;
    }
    tools.platform_api
}

/// 打包格式与打包器声明必须一致（防「按 apk 目标产出 aab」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ANDROID,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// Android 打包输入（`None` → 按产品元数据 + 图标 + UI 产物派生）。
fn android_inputs(inputs: &BundleInputs) -> AndroidInputs {
    inputs.android.clone().unwrap_or_default()
}

/// 是否有 `.jar` 输入（d8 只在此时必需）。
fn requires_d8(android: &AndroidInputs) -> bool {
    android.dex_inputs.iter().any(|input| !is_dex(input))
}

/// 扩展名是否为 `.dex`。
fn is_dex(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dex"))
}

/// 缺 d8 时的错误（`.jar` 必须经 d8 才能变 dex）。
fn missing_d8() -> BundleError {
    BundleError::ToolMissing {
        packager: ANDROID,
        tool: "d8".to_string(),
        hint: "Android SDK build-tools 自带 d8（`.jar` → `classes.dex`）；也可直接提供 `.dex` 输入"
            .to_string(),
    }
}

/// 条目用的文件名（入参路径必须带文件名）。
fn file_name(path: &Path, field: &'static str) -> Result<String, BundleError> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| BundleError::InvalidInput {
            packager: ANDROID,
            field,
            value: path.display().to_string(),
            reason: "路径缺少文件名".to_string(),
        })
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_jdk_failures_get_a_fix_hint_not_a_stack() {
        // 样本取自真实 `java -version` 输出（stderr）：老式 1.x 命名与 JEP 223 都要解析出来。
        let java8 =
            "java version \"1.8.0_402\"\nJava(TM) SE Runtime Environment (build 1.8.0_402-b06)\n";
        let hint = old_jdk_hint(java8).expect("JDK 8 需要提示");
        assert!(hint.contains("JDK 8") && hint.contains("JDK 17+"), "{hint}");
        assert!(
            old_jdk_hint("openjdk version \"11.0.22\" 2024-01-16\n")
                .expect("JDK 11 需要提示")
                .contains("JDK 11")
        );
        // ≥ 17 与不可解析的输出：不给提示（不遮蔽 d8 自己的错误）。
        assert!(old_jdk_hint("openjdk version \"17.0.10\" 2024-01-16\n").is_none());
        assert!(old_jdk_hint("openjdk version \"18.0.2.1\" 2022-08-18\n").is_none());
        assert!(old_jdk_hint("no version here").is_none());
    }

    #[test]
    fn version_code_is_derived_from_semver() {
        assert_eq!(derive_version_code("1.2.3", None), 10203);
        assert_eq!(derive_version_code("2.0.0", None), 20000);
        assert_eq!(derive_version_code("1.2.3-beta.1", None), 10203);
        assert_eq!(derive_version_code("1.2.3", Some(7)), 7);
        assert!(derive_version_code("999999.0.0", None) <= 2_100_000_000);
    }

    #[test]
    fn app_id_maps_to_a_java_package() {
        // 连字符（poly-cli 的 `com.polypack.<kebab>` 约定）折叠为 `_`，其余原样。
        assert_eq!(
            android_package("com.polypack.pax-notes").expect("可映射"),
            "com.polypack.pax_notes"
        );
        assert_eq!(
            android_package("com.polypack.paxnotes").expect("可映射"),
            "com.polypack.paxnotes"
        );
        assert_eq!(
            android_package("com.1pax.app").expect("数字段可补 `_`"),
            "com._1pax.app"
        );
        // 单段 / 空段无法成为 Java 包名：如实报错，不猜。
        assert!(android_package("paxnotes").is_err());
        assert!(android_package("com..app").is_err());
    }

    #[test]
    fn density_buckets_follow_the_spec() {
        assert_eq!(density_bucket(Some(256)), Some("xxxhdpi"));
        assert_eq!(density_bucket(Some(128)), Some("xhdpi"));
        assert_eq!(density_bucket(Some(48)), Some("mdpi"));
        assert_eq!(density_bucket(Some(16)), None);
        assert_eq!(density_bucket(None), None);
    }

    #[test]
    fn abi_order_is_fixed_when_filters_are_empty() {
        let android = AndroidInputs {
            native_libs: vec![
                crate::inputs::NativeLib {
                    abi: "x86_64".to_string(),
                    path: PathBuf::from("x86_64/libpax.so"),
                },
                crate::inputs::NativeLib {
                    abi: "arm64-v8a".to_string(),
                    path: PathBuf::from("arm64-v8a/libpax.so"),
                },
            ],
            ..AndroidInputs::default()
        };
        assert_eq!(selected_abis(&android), ["arm64-v8a", "x86_64"]);
        let filtered = AndroidInputs {
            abi_filters: vec!["x86_64".to_string()],
            ..android
        };
        assert_eq!(selected_abis(&filtered), ["x86_64"]);
    }

    /// TV 清单断言用的 Stage（纯路径推导，不触碰文件系统）。
    fn tv_stage() -> Stage {
        let root = PathBuf::from("stage");
        Stage {
            manifest: root.join("AndroidManifest.xml"),
            compiled: Vec::new(),
            generated_res: root.join("generated-res"),
            linked: root.join("linked.apk"),
            proto: root.join("proto.apk"),
            dex_dir: root.join("dex"),
            assembled: root.join("assembled.apk"),
            module: root.join("base.zip"),
            config: root.join("BundleConfig.json"),
            apk: root.join("out-unsigned.apk"),
            aab: root.join("out.aab"),
            apks: root.join("out.apks"),
            version_code: 10203,
            version_name: "1.2.3".to_string(),
            target_sdk: 36,
            root,
        }
    }

    fn tv_product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用".to_string(),
            homepage: None,
        }
    }

    #[test]
    fn tv_manifest_carries_leanback_lines_and_optional_banner() {
        let stage = tv_stage();
        let product = tv_product();
        let package = android_package(&product.identifier).expect("可映射");

        // 无 res 输入（生成 res）：Leanback 两行在、banner 不写（不伪造资源）；AAB 口径不受影响。
        let android = AndroidInputs::default();
        let tv = generated_manifest(
            &product,
            &package,
            &android,
            &stage,
            false,
            tv_flavor("android-tv"),
        );
        assert!(
            tv.contains(
                "<uses-feature android:name=\"android.software.leanback\" android:required=\"true\" />"
            ),
            "{tv}"
        );
        assert!(
            tv.contains(
                "<uses-feature android:name=\"android.hardware.touchscreen\" android:required=\"false\" />"
            ),
            "{tv}"
        );
        assert!(!tv.contains("android:banner"), "{tv}");
        let aab = generated_manifest(&product, &package, &android, &stage, false, AAB_FLAVOR);
        assert!(
            !aab.contains("leanback") && !aab.contains("touchscreen"),
            "{aab}"
        );

        // 输入 res 里画了 drawable*/tv_banner.*：banner 引用出现；无 tv_banner 的 res 仍不写。
        let base = std::env::temp_dir().join(format!("poly-tv-banner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let drawable = base.join("res/drawable-xhdpi");
        std::fs::create_dir_all(&drawable).expect("建 drawable");
        std::fs::write(drawable.join("tv_banner.png"), b"png").expect("写 banner");
        let with_banner = AndroidInputs {
            res: vec![base.join("res")],
            ..AndroidInputs::default()
        };
        assert!(has_tv_banner(&with_banner));
        let manifest = generated_manifest(
            &product,
            &package,
            &with_banner,
            &stage,
            false,
            tv_flavor("fire-tv"),
        );
        assert!(
            manifest.contains("android:banner=\"@drawable/tv_banner\""),
            "{manifest}"
        );

        let elsewhere = base.join("res2/drawable");
        std::fs::create_dir_all(&elsewhere).expect("建 drawable");
        std::fs::write(elsewhere.join("ic_launcher.png"), b"png").expect("写图标");
        let no_banner = AndroidInputs {
            res: vec![base.join("res2")],
            ..AndroidInputs::default()
        };
        assert!(!has_tv_banner(&no_banner));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn automotive_manifest_carries_the_automotive_feature_only() {
        // 结构断言不依赖 SDK：AAOS 清单变体由同一 `generated_manifest` 生成（T-M6-11）。
        let stage = tv_stage();
        let product = tv_product();
        let package = android_package(&product.identifier).expect("可映射");
        let manifest = generated_manifest(
            &product,
            &package,
            &AndroidInputs::default(),
            &stage,
            false,
            AUTOMOTIVE_FLAVOR,
        );
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
        // AAB / TV 口径不受影响。
        let aab = generated_manifest(
            &product,
            &package,
            &AndroidInputs::default(),
            &stage,
            false,
            AAB_FLAVOR,
        );
        assert!(!aab.contains("automotive"), "{aab}");
    }
}
