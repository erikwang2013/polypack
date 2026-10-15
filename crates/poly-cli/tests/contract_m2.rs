//! T-M2-14 验收：移动目标（android / ios）契约测试与金样比对。
//!
//! ## 模式开关（同 T-M1-13 `contract_m1.rs`）
//!
//! - **mock（默认，每提交）**：①两端在 `poly-targets` 内建注册表中的声明（格式 / 打包器 /
//!   校验器 / 签名引用 / 扩展名）与金样逐字段一致；②金样自检（`artifacts[*]` 的 format /
//!   extension / entries 形态，空条目即失败——防金样空转）；③直连 `poly_bundle` 公开 API：
//!   打包器 id / format 与金样一致，iOS 的计划含骨架 + archive / exportArchive 链。
//!   漂移即失败并打印 `- 声明 / + 金样` diff。
//! - **real（`POLY_CONTRACT_REAL=1`，每夜）**：`poly init` → `poly build --target <t>` →
//!   产物（`.aab` / `.ipa`）zip 条目 ↔ 金样 `entries` 比对；条目缺失即失败并给出 diff，
//!   产物未产出（打包器 / SDK 未接线）打印 NOTE 后返回，绝不红。
//!
//! 金样入库：`crates/poly-cli/tests/fixtures/m2/<target>.artifact.json`（M1 金样的同形 schema，
//! 见 `contract_m1.rs` 顶部）。移动端不声明 `bare-product`：iOS 核心按目标 triple 交叉编译，
//! 与 `poly build` 快照的宿主 triple 不同域。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use poly_targets::{Registry, TargetSpec};

/// 移动目标（M2 两端）。
const TARGETS: [&str; 2] = ["android", "ios"];

// ---------------------------------------------------------------- 模式与金样

/// 真实模式开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 读取某端金样。
fn load_golden(target: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m2")
        .join(format!("{target}.artifact.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取金样 {} 失败：{err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("金样 {} 不是合法 JSON：{err}", path.display()))
}

/// 内建注册表中的目标（断言为 L1）。
fn registry_spec(id: &str) -> TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 L1 目标 {id}"));
    assert_eq!(spec.tier, 1, "{id} 应为 L1（tier 1）");
    spec
}

/// 打包格式 → 产物扩展名（目录形态的格式为 None）。
fn extension_of(format: &str) -> Option<&'static str> {
    match format {
        "aab" => Some("aab"),
        "ipa" => Some("ipa"),
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
    }
    // 声明格式（artifacts[0]）的扩展名必须可由格式推导（real 侧按它找产物）。
    let declared = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
    assert_eq!(
        extension_of(declared),
        artifacts(&golden)[0]["extension"].as_str(),
        "{target} 金样声明格式的扩展名映射不可推导"
    );
}

/// mock：打包器在 `poly_bundle` 内以金样声明的（目标 id，格式）注册，且 id / format 一致。
fn assert_packager_registered(target: &str) {
    let golden = load_golden(target);
    let format = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
    let packager = poly_bundle::find(target, format)
        .unwrap_or_else(|| panic!("poly-bundle 缺少 {target} 的 {format} 打包器"));
    assert_eq!(packager.id(), target, "{target} 打包器 id 应等于目标 id");
    assert_eq!(
        packager.format(),
        format,
        "{target} 打包器 format 与金样不一致"
    );
}

/// mock：iOS 计划链（`plan()` 为纯组装，无需 Apple 工具——任意宿主可跑）：
/// 骨架脚本 + archive / exportArchive + 产物校验命令必须在位。
#[test]
fn ios_plan_chain_matches_golden_contract() {
    // 目录名必须与 real 用例（`poly-contract-m2-{target}-{pid}`）区分：同进程并行跑，
    // 这里收尾的 remove_dir_all 会把 real 用例刚建好的工作目录删掉，害它 spawn 时 ENOENT
    let dir =
        std::env::temp_dir().join(format!("poly-contract-m2-ios-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Pax Notes.xcframework")).expect("输出目录可建");
    let mut inputs = poly_bundle::BundleInputs::new(
        poly_bundle::ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "契约测试合成输入".to_string(),
            homepage: None,
        },
        &dir,
    );
    inputs.ios = Some(poly_bundle::IosInputs {
        core_crate: Some(dir.join("core")),
        lib_name: Some("pax_core".to_string()),
        host_project: None,
        bindings_dir: None,
    });

    let spec = registry_spec("ios");
    let packager = poly_bundle::for_spec(&spec).expect("ios 目标须有内建打包器");
    let plan = packager
        .plan(&inputs, &spec)
        .unwrap_or_else(|err| panic!("ios plan() 失败：{err}"));

    // 脚本以「路径 + 内容」入 blob：`project.pbxproj` 只出现在路径里（pbxproj 正文不含自身文件名）。
    let mut blob: Vec<String> = Vec::new();
    for script in &plan.scripts {
        blob.push(script.path.display().to_string());
        blob.push(script.contents.clone());
    }
    blob.extend(plan.commands.iter().map(|command| command.line()));
    let blob = blob.join("\n");
    for marker in [
        "project.pbxproj",         // 工程骨架
        "xcscheme",                // 共享 scheme（裸 CI 无自动生成）
        "ExportOptions.plist",     // 导出方式（exportArchive 的必需输入）
        "archive",                 // xcodebuild archive
        "-exportArchive",          // xcodebuild -exportArchive
        "CODE_SIGNING_ALLOWED=NO", // 未签名归档（签名位由 T-M2-10 接入）
    ] {
        assert!(blob.contains(marker), "iOS 计划链缺 {marker}：\n{blob}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// mock：android 计划在 Android SDK 就绪时直连公开 API 断言（缺 aapt2 → NOTE，不红）。
#[test]
fn android_plan_or_note_without_sdk() {
    let golden = load_golden("android");
    let format = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
    let spec = registry_spec("android");
    // 与 real 用例的目录名区分开（见 `ios_plan_chain_matches_golden_contract` 的注释）
    let dir = std::env::temp_dir().join(format!(
        "poly-contract-m2-android-plan-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("输出目录可建");
    let inputs = poly_bundle::BundleInputs::new(
        poly_bundle::ProductInfo {
            name: "pax-notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "契约测试合成输入".to_string(),
            homepage: None,
        },
        &dir,
    );
    let packager = poly_bundle::find("android", format).expect("android aab 打包器");
    match packager.plan(&inputs, &spec) {
        Ok(plan) => {
            assert!(
                !plan.scripts.is_empty() || !plan.commands.is_empty() || !plan.skipped.is_empty(),
                "android plan() 空转：无脚本 / 命令 / 跳过项"
            );
        }
        Err(poly_bundle::BundleError::ToolMissing { tool, .. }) => eprintln!(
            "NOTE android：宿主缺 {tool}，计划链断言在装有 Android SDK 的宿主（nightly ubuntu）执行"
        ),
        Err(err) => panic!("android plan() 失败：{err}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 两端 mock 契约（每提交，任意宿主）。
#[test]
fn mobile_contract_mock_covers_both_targets() {
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
/// 实际多出的条目不算违约（打包器可自由增补，如 `_CodeSignature/`）。
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
    let golden = load_golden("ios");
    let expected = value_str_list(&artifact_of(&golden, "ipa")["entries"]);
    let full = vec![
        "Payload/Pax Notes.app/Info.plist".to_string(),
        "Payload/Pax Notes.app/Pax Notes".to_string(),
    ];
    assert!(
        unmatched_patterns(&expected, &full).is_empty(),
        "合规清单不得报违约：{}",
        entries_report(&expected, &full)
    );

    // 实际清单换成一个 Info.plist 之外的条目：仅 `Payload/*.app/Info.plist` 失配。
    let drifted = vec!["Payload/Pax Notes.app/Pax Notes".to_string()];
    let unmatched = unmatched_patterns(&expected, &drifted);
    assert_eq!(
        unmatched,
        vec!["Payload/*.app/Info.plist".to_string()],
        "缺条目必被检出"
    );
    let report = entries_report(&expected, &drifted);
    assert!(
        report.contains("- Payload/*.app/Info.plist（无匹配条目）"),
        "报告须给出 diff：\n{report}"
    );
}

// ---------------------------------------------------------------- real 模式

/// `poly build` 的产物快照 / 打包落点内按扩展名发现产物（大小写不敏感）。
fn find_artifact(root: &Path, extension: &str) -> Option<PathBuf> {
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
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if name.ends_with(&suffix) {
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

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// real：真实链路的产物 zip 条目 ↔ 金样 `entries` 比对；产物未产出即 NOTE 返回。
fn real_artifact_check(target: &str) {
    let spec = registry_spec(target);
    let host = poly_targets::current_host();
    if !host.is_some_and(|host| spec.hosts.contains(&host)) {
        eprintln!(
            "SKIP {target}：目标声明宿主 {:?}，当前宿主 {host:?}",
            spec.hosts
        );
        return;
    }
    let golden = load_golden(target);
    let primary = &artifacts(&golden)[0];
    let format = primary["format"].as_str().expect("声明格式").to_string();
    let extension = primary["extension"]
        .as_str()
        .expect("声明格式须有扩展名")
        .to_string();

    let workdir =
        std::env::temp_dir().join(format!("poly-contract-m2-{target}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("临时工程目录可建");
    for verb in ["init", "build"] {
        let out = poly(&["--help"], &workdir);
        if !String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|line| line.trim_start().starts_with(verb))
        {
            eprintln!("SKIP {target}：`poly {verb}` 未落盘（等待 M1 链路接线）");
            return;
        }
    }
    let init = poly(&["init"], &workdir);
    assert!(
        init.status.success(),
        "poly init 失败（{}）：{}",
        init.status,
        String::from_utf8_lossy(&init.stderr)
    );
    let build = poly(&["build", "--target", target, "--json"], &workdir);
    assert!(
        build.status.success(),
        "poly build --target {target} 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );

    let Some(artifact) = find_artifact(&workdir, &extension) else {
        eprintln!(
            "NOTE {target}：未发现 .{extension} 产物（落点 .polypack/packages/{target}/；\
             打包器 / SDK 未接线时如实跳过，nightly 装齐工具链后走真比对）"
        );
        return;
    };

    let names = poly_bundle::zipwrite::entry_names(&artifact)
        .unwrap_or_else(|err| panic!("{target} 产物条目清单读取失败：{err}"));
    let expected = value_str_list(&primary["entries"]);
    let unmatched = unmatched_patterns(&expected, &names);
    assert!(
        unmatched.is_empty(),
        "{target} {format} 产物 {} 与金样漂移（- 金样模式缺失 / + 产物多出条目）：\n{}",
        artifact.display(),
        entries_report(&expected, &names)
    );
    // 真跑产物留档：不删临时工程（CI 由 `留产物` 步上传；本地留一份供人工核验）。
    eprintln!(
        "OK {target}：{format} 产物 {} 的 {} 条命中金样 {} 条模式（工程 {}）",
        artifact.display(),
        names.len(),
        expected.len(),
        workdir.display()
    );
}

/// real：Android（ubuntu-latest 每夜）。
#[test]
fn contract_android_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLY_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("android");
}

/// real：iOS（macos-14 每夜）。
#[test]
fn contract_ios_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLY_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("ios");
}
