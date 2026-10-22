//! T-M4-05 验收：Android TV / Fire TV（`android-tv` / `fire-tv`）契约测试与金样比对。
//!
//! ## 模式开关（同 T-M1-13 `contract_m1.rs` / T-M2-14 `contract_m2.rs`）
//!
//! - **mock（默认，每提交）**：①两端在 `polypack-targets` 内建注册表中的声明（格式 / 打包器 /
//!   校验器 / 签名引用 / 推送渠道 / 扩展名）与金样逐字段一致；②金样自检（`artifacts[*]` 的
//!   format / extension / entries 形态，空条目即失败——防金样空转）；③直连 `polypack_bundle`
//!   公开 API：打包器 id / format 与金样一致，TV 计划链含 Leanback 两行（`android.software.
//!   leanback` required / `android.hardware.touchscreen` not-required；banner 仅在输入 res
//!   确有 `drawable*/tv_banner.*` 时引用）；④「Fire TV 复用同一 APK 链」的声明对拍（两目标
//!   format / builder / signing / verifiers 逐项相等，渠道各自独立）。漂移即失败并打印
//!   `- 声明 / + 金样` diff。
//! - **real（`POLYPACK_CONTRACT_REAL=1`，每夜）**：`polypack init` → `polypack build --target <t>` →
//!   产物（未签名 `.apk` / 模块 zip）zip 条目 ↔ 金样 `entries` 比对 + `aapt2 dump badging`
//!   的 TV 轨道清单断言（Leanback 两行）；产物与日志留档在临时工程（CI 上传），主产物未产出
//!   （打包器 / SDK 未接线）打印 NOTE 后返回，绝不红。
//!
//! 金样入库：`crates/polypack-cli/tests/fixtures/m4/<target>.artifact.json`（M1 / M2 金样的同形
//! schema + `publish` 渠道字段）。主产物 `name` 按名锚定（`<kebab>-<version>-unsigned.apk`）：
//! 链中途的 `linked.apk` / `proto.apk` / `assembled.apk` 与快照副本同扩展名，只按 mtime 取件
//! 会误中中间件（T-M2-14 实证的坑，同 `base.zip` 的锚定理由）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use polypack_targets::{Registry, TargetSpec};

/// TV 目标（M4 两枚，ROADMAP §3.4）。
const TARGETS: [&str; 2] = ["android-tv", "fire-tv"];

// ---------------------------------------------------------------- 模式与金样

/// 真实模式开关：`POLYPACK_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 读取某端金样。
fn load_golden(target: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m4")
        .join(format!("{target}.artifact.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取金样 {} 失败：{err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("金样 {} 不是合法 JSON：{err}", path.display()))
}

/// 内建注册表中的目标（断言为 L3）。
fn registry_spec(id: &str) -> TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 TV 目标 {id}"));
    assert_eq!(spec.tier, 3, "{id} 应为 L3（ROADMAP §3.4 电视与大屏）");
    spec
}

/// 打包格式 → 产物扩展名（目录形态的格式为 None）。
fn extension_of(format: &str) -> Option<&'static str> {
    match format {
        "apk" => Some("apk"),
        _ => None,
    }
}

/// 金样 `artifacts` 数组。
fn artifacts(golden: &serde_json::Value) -> &Vec<serde_json::Value> {
    golden["artifacts"]
        .as_array()
        .unwrap_or_else(|| panic!("金样 artifacts 应为数组：{golden}"))
}

/// 金样中某 format 的产物契约。
fn artifact_of<'a>(golden: &'a serde_json::Value, format: &str) -> &'a serde_json::Value {
    artifacts(golden)
        .iter()
        .find(|artifact| artifact["format"] == format)
        .unwrap_or_else(|| panic!("金样缺少 {format} 契约"))
}

/// 金样字符串数组字段。
fn value_str_list(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("应为数组：{value}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("元素应为字符串：{item}"))
                .to_string()
        })
        .collect()
}

// ---------------------------------------------------------------- mock 契约

/// 注册表声明 ↔ 金样逐字段比对：漂移即失败，diff 为「- 注册表声明 / + 金样」。
fn assert_registry_matches_golden(target: &str) {
    let spec = registry_spec(target);
    let golden = load_golden(target);
    let primary = &artifacts(&golden)[0];
    let declared = [
        (
            "format",
            serde_json::json!(spec.package.format),
            primary["format"].clone(),
        ),
        (
            "builder",
            serde_json::json!(spec.package.builder),
            golden["builder"].clone(),
        ),
        (
            "signing",
            match &spec.signing {
                Some(signing) => serde_json::json!(signing),
                None => serde_json::Value::Null,
            },
            golden["signing"].clone(),
        ),
        (
            "verifiers",
            serde_json::json!(spec.verify),
            golden["verifiers"].clone(),
        ),
        (
            "publish",
            serde_json::json!(spec.publish),
            golden["publish"].clone(),
        ),
        (
            "extension",
            match extension_of(&spec.package.format) {
                Some(extension) => serde_json::json!(extension),
                None => serde_json::Value::Null,
            },
            primary["extension"].clone(),
        ),
    ];
    let mut diff = String::new();
    for (key, want, have) in declared {
        if want != have {
            diff.push_str(&format!("- {key} = {want}\n+ {key} = {have}\n"));
        }
    }
    assert!(
        diff.is_empty(),
        "{target} 注册表声明与金样漂移（- 注册表 / + 金样）：\n{diff}"
    );
}

/// 金样自检：形态合法且非空转（每个产物须有 format 与 entries；声明格式的扩展名映射可推导）。
fn assert_golden_shape(target: &str) {
    let golden = load_golden(target);
    assert_eq!(golden["target"], target, "金样 target 字段应为 {target}");
    for artifact in artifacts(&golden) {
        let format = artifact["format"]
            .as_str()
            .unwrap_or_else(|| panic!("{target} 金样产物缺 format：{artifact}"));
        assert!(
            !value_str_list(&artifact["entries"]).is_empty(),
            "{target} 金样 {format} 无 entries（金样空转）"
        );
        assert!(
            artifact["extension"].is_null() || artifact["extension"].is_string(),
            "{target} 金样 {format} 的 extension 应为字符串或 null：{artifact}"
        );
        // `name`（可选）是文件形态产物的**按名锚定**依据：给了就必须是非空字符串，
        // 否则「锚定」会静默退化成按扩展名取最新（中间件抢位的老毛病）。
        assert!(
            artifact
                .get("name")
                .is_none_or(|name| name.as_str().is_some_and(|name| !name.is_empty())),
            "{target} 金样 {format} 的 name 应为非空字符串：{artifact}"
        );
    }
    // 声明格式（artifacts[0]）的扩展名必须可由格式推导（real 侧按它找产物）。
    let declared = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
    assert_eq!(
        extension_of(declared),
        artifacts(&golden)[0]["extension"].as_str(),
        "{target} 金样声明格式的扩展名映射不可推导"
    );
}

/// mock：打包器在 `polypack_bundle` 内以金样声明的（目标 id，格式）注册，且 id / format 一致。
fn assert_packager_registered(target: &str) {
    let golden = load_golden(target);
    let format = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
    let packager = polypack_bundle::find(target, format)
        .unwrap_or_else(|| panic!("polypack-bundle 缺少 {target} 的 {format} 打包器"));
    assert_eq!(packager.id(), target, "{target} 打包器 id 应等于目标 id");
    assert_eq!(
        packager.format(),
        format,
        "{target} 打包器 format 与金样不一致"
    );
}

/// mock：「Fire TV 复用同一 APK 链，渠道指向亚马逊应用商店」（T-M4-04 交付物原文）——
/// 两目标的格式 / 打包器 / 签名 / 校验器 / 宿主 / 运行时 / 桥 / UI 逐项同源，渠道各自独立。
#[test]
fn tv_targets_share_one_apk_chain_and_keep_their_own_channels() {
    let android_tv = registry_spec("android-tv");
    let fire_tv = registry_spec("fire-tv");
    let chain = |spec: &TargetSpec| {
        serde_json::json!({
            "format": spec.package.format,
            "builder": spec.package.builder,
            "signing": spec.signing,
            "verify": spec.verify,
            "hosts": spec.hosts,
            "runtime": spec.runtime,
            "bridge": spec.bridge,
            "ui": spec.ui,
        })
    };
    assert_eq!(
        chain(&android_tv),
        chain(&fire_tv),
        "Fire TV 必须复用 Android TV 同一条 APK 链（逐项同源）"
    );

    // 渠道（publish）分别指向金样声明的商店，且互不相同。
    for (spec, want) in [
        (&android_tv, "google-play-tv"),
        (&fire_tv, "amazon-appstore"),
    ] {
        assert_eq!(spec.publish, vec![want.to_string()], "{} 渠道", spec.id);
        assert_eq!(
            spec.publish,
            value_str_list(&load_golden(&spec.id)["publish"]),
            "{} 渠道与金样不一致",
            spec.id
        );
    }

    // notes 如实标注 ROADMAP §3.4 的两条口径（Leanback 清单 / 复用同一链 + 亚马逊渠道）。
    let notes = android_tv.notes.clone().unwrap_or_default();
    assert!(
        notes.contains("Leanback"),
        "android-tv notes 应点明 Leanback：{notes}"
    );
    let fire_notes = fire_tv.notes.clone().unwrap_or_default();
    assert!(
        fire_notes.contains("复用") && fire_notes.contains("亚马逊"),
        "fire-tv notes 应点明复用链与亚马逊渠道：{fire_notes}"
    );
    assert!(
        !android_tv.experimental && !fire_tv.experimental,
        "两目标均非 experimental"
    );
}

/// mock：TV 计划链（`plan()` 只组装脚本与命令；Android SDK 缺席时打 NOTE——CI 三宿主可能
/// 都没有 SDK，装有 SDK 的宿主（nightly ubuntu）自然走真断言分支）。
#[test]
fn tv_plan_chain_carries_leanback_lines_or_note() {
    for target in ["android-tv", "fire-tv"] {
        let golden = load_golden(target);
        let format = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
        let spec = registry_spec(target);
        // 目录名按目标区分：real 用例的目录是 `polypack-contract-m4-tv-{target}-{pid}`，
        // 同进程并行跑，收尾的 remove_dir_all 不得互相踩。
        let dir = std::env::temp_dir().join(format!(
            "polypack-contract-m4-tv-plan-{target}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("输出目录可建");
        let inputs = polypack_bundle::BundleInputs::new(
            polypack_bundle::ProductInfo {
                name: "pax-notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "契约测试合成输入".to_string(),
                homepage: None,
            },
            &dir,
        );
        let packager = polypack_bundle::find(target, format).expect("TV 打包器");
        match packager.plan(&inputs, &spec) {
            Ok(plan) => {
                let mut blob: Vec<String> = Vec::new();
                for script in &plan.scripts {
                    blob.push(script.path.display().to_string());
                    blob.push(script.contents.clone());
                }
                blob.extend(plan.commands.iter().map(|command| command.line()));
                let blob = blob.join("\n");
                for marker in [
                    // Leanback 三行差异里的两行清单（banner 无输入 res 时如实省去，不伪造资源）
                    "android.software.leanback\" android:required=\"true\"",
                    "android.hardware.touchscreen\" android:required=\"false\"",
                    // 复用 M2 链：aapt2 link 二态（二进制 + proto）都在计划里
                    "aapt2",
                    "link",
                ] {
                    assert!(
                        blob.contains(marker),
                        "{target} TV 计划链缺 {marker}：\n{blob}"
                    );
                }
                assert!(
                    !blob.contains("android:banner"),
                    "{target} 无输入 res 时不得引用 banner：\n{blob}"
                );
            }
            Err(polypack_bundle::BundleError::ToolMissing { tool, .. }) => eprintln!(
                "NOTE {target}：宿主缺 {tool}，计划链断言在装有 Android SDK 的宿主（nightly ubuntu）执行"
            ),
            Err(err) => panic!("{target} plan() 失败：{err}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 两端 mock 契约（每提交，任意宿主）。
#[test]
fn tv_contract_mock_covers_both_targets() {
    for target in TARGETS {
        assert_registry_matches_golden(target);
        assert_golden_shape(target);
        assert_packager_registered(target);
    }
}

// ---------------------------------------------------------------- 条目比对（diff）

/// 极简 glob：`*` 匹配任意字符（含 `/`），其余字符字面匹配（同 `contract_m1.rs`）。
fn glob_match(pattern: &str, entry: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == entry;
    }
    let mut rest = entry;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            if index == parts.len() - 1 {
                return true;
            }
            continue;
        }
        if index == 0 {
            if !rest.starts_with(part) {
                return false;
            }
            rest = &rest[part.len()..];
        } else if index == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(pos) = rest.find(part) {
            rest = &rest[pos + part.len()..];
        } else {
            return false;
        }
    }
    rest.is_empty()
}

/// 金样条目 vs 实际清单：金样 `entries` 是**契约模式**（须至少命中一条实际条目），
/// 实际多出的条目不算违约（打包器可自由增补）。
fn unmatched_patterns(expected: &[String], actual: &[String]) -> Vec<String> {
    expected
        .iter()
        .filter(|pattern| !actual.iter().any(|entry| glob_match(pattern, entry)))
        .cloned()
        .collect()
}

/// 比对报告：`- <模式>（无匹配条目）` 为违约，`+ <条目>（金样未声明）` 为多出条目（仅提示）。
fn entries_report(expected: &[String], actual: &[String]) -> String {
    let mut report = String::new();
    for pattern in unmatched_patterns(expected, actual) {
        report.push_str(&format!("- {pattern}（无匹配条目）\n"));
    }
    for entry in actual {
        if !expected.iter().any(|pattern| glob_match(pattern, entry)) {
            report.push_str(&format!("+ {entry}（金样未声明）\n"));
        }
    }
    report
}

/// 比对器非空转证明：条目缺失必被检出且报告含该模式（diff 形态）。
#[test]
fn golden_drift_is_reported_with_diff() {
    let golden = load_golden("android-tv");
    let expected = value_str_list(&artifact_of(&golden, "apk")["entries"]);
    let full = vec![
        "AndroidManifest.xml".to_string(),
        "resources.arsc".to_string(),
    ];
    assert!(
        unmatched_patterns(&expected, &full).is_empty(),
        "合规清单不得报违约：{}",
        entries_report(&expected, &full)
    );

    // 实际清单换成缺清单文件的产物：`AndroidManifest.xml` 必被检出（半成品 APK 的形态）。
    let drifted = vec!["resources.arsc".to_string()];
    assert_eq!(
        unmatched_patterns(&expected, &drifted),
        vec!["AndroidManifest.xml".to_string()],
        "缺条目必被检出"
    );
    let report = entries_report(&expected, &drifted);
    assert!(
        report.contains("- AndroidManifest.xml（无匹配条目）"),
        "报告须给出 diff：\n{report}"
    );
}

// ---------------------------------------------------------------- real 模式

/// `polypack build` 的产物快照 / 打包落点内发现产物：金样声明了 `name` 即**按名锚定**
/// （同名多件取最新），否则按扩展名取最新一件（大小写不敏感）。
///
/// 按名锚定不是洁癖：链中间件（`linked.apk` / `proto.apk` / `assembled.apk`）与快照副本
/// 同扩展名，只看 mtime 会选中中间件（T-M2-14 在 module-zip 上的同款实证）。
fn find_artifact(root: &Path, extension: &str, name: Option<&str>) -> Option<PathBuf> {
    let suffix = format!(".{extension}");
    let mut stack = vec![root.to_path_buf()];
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let matched = match name {
                Some(name) => file_name.eq_ignore_ascii_case(name),
                None => file_name.to_lowercase().ends_with(&suffix),
            };
            if matched {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    files.pop()
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

/// TV 轨道清单断言（真产物）：`aapt2 dump badging` 须见 Leanback 两行。缺 SDK → NOTE 不红
/// （真实链路本就需要 SDK，正常走不到这里）。
fn assert_tv_track_manifest(apk: &Path) {
    let android = polypack_bundle::inputs::AndroidInputs::default();
    let Some(tools) = polypack_bundle::tools::probe(&android) else {
        eprintln!("NOTE：本机无 Android SDK，TV 轨道 badging 断言延后");
        return;
    };
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
    let badging = String::from_utf8_lossy(&output.stdout).into_owned();
    for marker in [
        "uses-feature: name='android.software.leanback'",
        "uses-feature-not-required: name='android.hardware.touchscreen'",
    ] {
        assert!(badging.contains(marker), "TV 产物缺 {marker}：\n{badging}");
    }
}

/// real：真实链路的产物条目 ↔ 金样 `artifacts[*]` 逐件比对 + TV 轨道清单断言；
/// 主产物未产出即 NOTE 返回。产物 / 日志留档在临时工程（CI 上传）。
fn real_artifact_check(target: &str) {
    let spec = registry_spec(target);
    let host = polypack_targets::current_host();
    if !host.is_some_and(|host| spec.hosts.contains(&host)) {
        eprintln!(
            "SKIP {target}：目标声明宿主 {:?}，当前宿主 {host:?}",
            spec.hosts
        );
        return;
    }
    let golden = load_golden(target);
    let primary = &artifacts(&golden)[0];
    let extension = primary["extension"]
        .as_str()
        .expect("声明格式须有扩展名")
        .to_string();

    let workdir = std::env::temp_dir().join(format!(
        "polypack-contract-m4-tv-{target}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("临时工程目录可建");
    for verb in ["init", "build"] {
        let out = polypack(&["--help"], &workdir);
        if !String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|line| line.trim_start().starts_with(verb))
        {
            eprintln!("SKIP {target}：`polypack {verb}` 未落盘（等待 M1 链路接线）");
            return;
        }
    }
    let init = polypack(&["init"], &workdir);
    assert!(
        init.status.success(),
        "polypack init 失败（{}）：{}",
        init.status,
        String::from_utf8_lossy(&init.stderr)
    );
    let build = polypack(&["build", "--target", target, "--json"], &workdir);
    let build_log = format!(
        "polypack build --target {target}（exit {}）\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status,
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(
        build.status.success(),
        "polypack build --target {target} 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );

    let mut log = format!(
        "T-M4-05 契约 real 模式：{target}\n工程：{}\n\n{build_log}\n",
        workdir.display()
    );
    for (index, artifact) in artifacts(&golden).iter().enumerate() {
        let format = artifact["format"].as_str().expect("声明格式").to_string();
        let found = artifact_file_entries(
            &workdir,
            &extension_of_artifact(artifact),
            artifact["name"].as_str(),
        );
        let Some((path, names)) = found else {
            if index == 0 {
                eprintln!(
                    "NOTE {target}：未发现 .{extension} 产物（落点 .polypack/packages/{target}/；\
                     打包器 / SDK 未接线时如实跳过，nightly 装齐工具链后走真比对）"
                );
                return;
            }
            // 主产物在而声明过的次产物缺 = 声明了却没给：硬失败，绝不静默。
            panic!(
                "{target} 金样声明了 {format} 产物但工程未产出（工程 {}）",
                workdir.display()
            );
        };
        let expected = value_str_list(&artifact["entries"]);
        assert!(
            unmatched_patterns(&expected, &names).is_empty(),
            "{target} {format} 产物 {} 与金样漂移（- 金样模式缺失 / + 产物多出条目）：\n{}",
            path.display(),
            entries_report(&expected, &names)
        );
        if format == "apk" {
            assert_tv_track_manifest(&path);
        }
        log.push_str(&format!(
            "OK {format}：{}（{} 条命中 {} 条模式）\n条目：{}\n",
            path.display(),
            names.len(),
            expected.len(),
            names.join(", ")
        ));
        eprintln!(
            "OK {target}：{format} 产物 {} 的 {} 条命中金样 {} 条模式",
            path.display(),
            names.len(),
            expected.len()
        );
    }
    // 日志归档（产物留档同 M2：不删临时工程，CI `留产物` 步上传）。
    let log_path = workdir.join("tv-contract-real.log");
    std::fs::write(&log_path, &log).expect("契约日志可写");
    eprintln!(
        "OK {target}：全部 {} 件产物命中金样，日志 {}（工程 {}）",
        artifacts(&golden).len(),
        log_path.display(),
        workdir.display()
    );
}

/// 金样 `extension` 字段（目录形态为空的场景在 TV 不存在——两件产物都是文件）。
fn extension_of_artifact(artifact: &serde_json::Value) -> String {
    artifact["extension"]
        .as_str()
        .unwrap_or_else(|| panic!("TV 金样产物须有扩展名：{artifact}"))
        .to_string()
}

/// 文件形态产物：按金样声明的文件名（缺省则按扩展名）定位，读 zip 条目名。
fn artifact_file_entries(
    root: &Path,
    extension: &str,
    name: Option<&str>,
) -> Option<(PathBuf, Vec<String>)> {
    let path = find_artifact(root, extension, name)?;
    let names = polypack_bundle::zipwrite::entry_names(&path)
        .unwrap_or_else(|err| panic!("{} 产物条目清单读取失败：{err}", path.display()));
    Some((path, names))
}

/// real：Android TV（ubuntu-latest 每夜）。
#[test]
fn contract_android_tv_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("android-tv");
}

/// real：Fire TV（ubuntu-latest 每夜；同一 APK 链，落点 `.polypack/packages/fire-tv/`）。
#[test]
fn contract_fire_tv_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("fire-tv");
}
