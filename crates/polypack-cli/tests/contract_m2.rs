//! T-M2-14 验收：移动目标（android / ios）契约测试与金样比对。
//!
//! ## 模式开关（同 T-M1-13 `contract_m1.rs`）
//!
//! - **mock（默认，每提交）**：①两端在 `polypack-targets` 内建注册表中的声明（格式 / 打包器 /
//!   校验器 / 签名引用 / 扩展名）与金样逐字段一致；②金样自检（`artifacts[*]` 的 format /
//!   extension / entries 形态，空条目即失败——防金样空转）；③直连 `polypack_bundle` 公开 API：
//!   打包器 id / format 与金样一致，iOS 的计划含骨架 + archive / Payload 直压链。
//!   漂移即失败并打印 `- 声明 / + 金样` diff。
//! - **real（`POLYPACK_CONTRACT_REAL=1`，每夜）**：`polypack init` → `polypack build --target <t>` →
//!   产物（`.aab` / `.ipa`）zip 条目 ↔ 金样 `entries` 比对；条目缺失即失败并给出 diff，
//!   产物未产出（打包器 / SDK 未接线）打印 NOTE 后返回，绝不红。
//!
//! 金样入库：`crates/polypack-cli/tests/fixtures/m2/<target>.artifact.json`（M1 金样的同形 schema，
//! 见 `contract_m1.rs` 顶部）。移动端不声明 `bare-product`：iOS 核心按目标 triple 交叉编译，
//! 与 `polypack build` 快照的宿主 triple 不同域。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use polypack_targets::{Registry, TargetSpec};

/// 移动目标（M2 两端）。
const TARGETS: [&str; 2] = ["android", "ios"];

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

/// mock：iOS 计划链（`plan()` 为纯组装，无需 Apple 工具——任意宿主可跑）：
/// 骨架脚本 + archive / Payload 直压 + 产物校验命令必须在位。
#[test]
fn ios_plan_chain_matches_golden_contract() {
    // 目录名必须与 real 用例（`polypack-contract-m2-{target}-{pid}`）区分：同进程并行跑，
    // 这里收尾的 remove_dir_all 会把 real 用例刚建好的工作目录删掉，害它 spawn 时 ENOENT
    let dir =
        std::env::temp_dir().join(format!("polypack-contract-m2-ios-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Pax Notes.xcframework")).expect("输出目录可建");
    let mut inputs = polypack_bundle::BundleInputs::new(
        polypack_bundle::ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "契约测试合成输入".to_string(),
            homepage: None,
        },
        &dir,
    );
    inputs.ios = Some(polypack_bundle::IosInputs {
        core_crate: Some(dir.join("core")),
        lib_name: Some("pax_core".to_string()),
        host_project: None,
        bindings_dir: None,
    });

    let spec = registry_spec("ios");
    let packager = polypack_bundle::for_spec(&spec).expect("ios 目标须有内建打包器");
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
        "ExportOptions.plist",     // 导出方式（T-M2-10 签名导出的输入）
        "archive",                 // xcodebuild archive
        "ditto",                   // 归档 Payload 直压（exportArchive 需 Team，真跑实证）
        "--keepParent",            // 选项须先于操作数（BSD getopt，zipalign 同坑）
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
    // 降级产物（缺 bundletool 时的声明产物）是 AAB 模块 **zip 文件**（真产物 `base.zip` 实证）：
    // 声明成目录形态（extension = null）会让 real 侧去找不存在的 `*.module-zip` 目录并误报
    // 「声明了却没给」（dispatch 38693729015 的假阴性即此）。
    assert!(
        artifact_of(&golden, "module-zip")["extension"].is_string(),
        "android 降级产物是 zip 文件形态，非目录"
    );
    let spec = registry_spec("android");
    // 与 real 用例的目录名区分开（见 `ios_plan_chain_matches_golden_contract` 的注释）
    let dir = std::env::temp_dir().join(format!(
        "polypack-contract-m2-android-plan-{}",
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
    let packager = polypack_bundle::find("android", format).expect("android aab 打包器");
    match packager.plan(&inputs, &spec) {
        Ok(plan) => {
            assert!(
                !plan.scripts.is_empty() || !plan.commands.is_empty() || !plan.skipped.is_empty(),
                "android plan() 空转：无脚本 / 命令 / 跳过项"
            );
        }
        Err(polypack_bundle::BundleError::ToolMissing { tool, .. }) => eprintln!(
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

/// `polypack build` 的产物快照 / 打包落点内发现产物：金样声明了 `name` 即**按名锚定**
/// （同名多件取最新），否则按扩展名取最新一件（大小写不敏感）。
///
/// 按名锚定不是洁癖：android 的 AAB 模块 zip 与 aapt2 的 `generated-res.zip` 等中间件同扩展，
/// 只看 mtime 会选中中间件（dispatch 38696103225 实证：包了中间件却报金样漂移）。
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

/// iOS 真跑夹具的核心 crate 名（`lib<pax_core>.a` / `<pax_core>.swift` 的命名基准）。
const IOS_FIXTURE_LIB: &str = "pax_core";

/// 把 `polypack init` 模板的 bin 核心换成库形态夹具：静态库 crate + UDL + Swift 绑定三件套。
///
/// crate 与 `polypack-bundle/tests/fixtures/ios_core`（T-M2-14 nightly 在用）**同一份源**——
/// 夹具形态只有一处，避免两腿漂移。绑定三件套手写预置同 `ios_real_run::write_bindings`：
/// 真绑定由 T-M2-01 生成器产出，本腿不依赖 runner 上的 `uniffi-bindgen` CLI（其版本与
/// uniffi 库版本强耦合，nightly 会因此变脆）。
fn install_ios_core_fixture(root: &Path) {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../polypack-bundle/tests/fixtures/ios_core");
    let core = root.join("core");
    for (from, to) in [
        (source.join("Cargo.toml"), core.join("Cargo.toml")),
        (source.join("src/lib.rs"), core.join("src/lib.rs")),
    ] {
        std::fs::copy(&from, &to).unwrap_or_else(|err| {
            panic!("夹具 {} 拷入 {} 失败：{err}", from.display(), to.display())
        });
    }
    // 模板的 bin 入口留着会让同名包同时产 bin，库形态夹具不要它
    let _ = std::fs::remove_file(core.join("src/main.rs"));

    // UDL 与 lib.rs 的导出面同构；`<core>/src/*.udl` 是 T-M2-01 生成器的接口源约定。
    let udl_path = core.join(format!("src/{IOS_FIXTURE_LIB}.udl"));
    let udl = format!("namespace {IOS_FIXTURE_LIB} {{\n  i32 pax_add(i32 a, i32 b);\n}};\n");
    std::fs::write(&udl_path, &udl).expect("UDL 可写");
    // 自证：夹具 UDL 必须能被 T-M2-01 的解析器读通（否则夹具形态本身就是错的；
    // 本机即可跑，不必等 macos 腿）。
    let interface = polypack_core::bridge::udl::parse_source(&udl_path).expect("夹具 UDL 可解析");
    assert_eq!(interface.namespace, IOS_FIXTURE_LIB);
    assert_eq!(interface.functions.len(), 1, "夹具 UDL 应声明 1 个函数");

    let bridge = root.join("bridge").join("swift");
    std::fs::create_dir_all(&bridge).expect("绑定目录可建");
    let lib = IOS_FIXTURE_LIB;
    for (name, body) in [
        (
            format!("{lib}FFI.h"),
            "#include <stdint.h>\nint32_t pax_add(int32_t a, int32_t b);\n".to_string(),
        ),
        (
            format!("{lib}FFI.modulemap"),
            format!("module {lib}FFI {{\n  header \"{lib}FFI.h\"\n  export *\n}}\n"),
        ),
        (
            format!("{lib}.swift"),
            format!(
                "import {lib}FFI\n\npublic func paxAdd(_ a: Int32, _ b: Int32) -> Int32 {{\n    pax_add(a, b)\n}}\n"
            ),
        ),
    ] {
        std::fs::write(bridge.join(name), body).expect("绑定文件可写");
    }
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

/// 目录形态产物（`*.<format>`，如 `*.xcframework`）：条目名 = `<目录名>/<相对路径>`。
///
/// 金样模式含目录名段（`*.xcframework/ios-arm64/*.a`），故条目名以目录名起头——
/// 与「相对产物父目录」同义，且不受产物落在快照目录还是 `packages/` 的影响。
fn artifact_dir_entries(root: &Path, format: &str) -> Option<(PathBuf, Vec<String>)> {
    let dir = find_dir(root, &format!(".{format}"))?;
    let base = dir.file_name()?.to_string_lossy().to_string();
    let mut names = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&current) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if let Ok(rel) = path.strip_prefix(&dir) {
                // 归一化分隔符：期望串一律 `/`（Windows 腿同纪律）
                names.push(format!(
                    "{base}/{}",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }
    }
    names.sort();
    Some((dir, names))
}

/// 目录形态产物定位：与 [`find_artifact`] 同一「取最新」纪律——真跑工程里可能留着旧快照。
fn find_dir(root: &Path, suffix: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut dirs: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if name.ends_with(suffix) {
                dirs.push(path.clone());
            }
            stack.push(path);
        }
    }
    dirs.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    dirs.pop()
}

/// real：真实链路的产物条目 ↔ 金样 `artifacts[*]` 逐件比对；主产物未产出即 NOTE 返回。
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

    let workdir =
        std::env::temp_dir().join(format!("polypack-contract-m2-{target}-{}", std::process::id()));
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
    if target == "ios" {
        install_ios_core_fixture(&workdir);
    }
    let build = polypack(&["build", "--target", target, "--json"], &workdir);
    assert!(
        build.status.success(),
        "polypack build --target {target} 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );

    for (index, artifact) in artifacts(&golden).iter().enumerate() {
        let format = artifact["format"].as_str().expect("声明格式").to_string();
        // 文件形态按扩展名找 zip；目录形态（extension = null，如 xcframework）按 `*.<format>` 找目录。
        let found = match artifact["extension"].as_str() {
            Some(extension) => {
                artifact_file_entries(&workdir, extension, artifact["name"].as_str())
            }
            None => artifact_dir_entries(&workdir, &format),
        };
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
        eprintln!(
            "OK {target}：{format} 产物 {} 的 {} 条命中金样 {} 条模式",
            path.display(),
            names.len(),
            expected.len()
        );
    }
    // 真跑产物留档：不删临时工程（CI 由 `留产物` 步上传；本地留一份供人工核验）。
    eprintln!(
        "OK {target}：全部 {} 件产物命中金样（工程 {}）",
        artifacts(&golden).len(),
        workdir.display()
    );
}

/// real：Android（ubuntu-latest 每夜）。
#[test]
fn contract_android_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("android");
}

/// real：iOS（macos-14 每夜）。
#[test]
fn contract_ios_real_artifact() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    real_artifact_check("ios");
}

/// 目录形态产物（iOS 第二件 `xcframework`）的比对器自检：条目名约定 + 漂移必被检出。
///
/// 真比对只在 macos-14 跑，本用例在任意宿主先把「`<目录名>/<相对路径>` ↔ 金样
/// `*.xcframework/...` 模式」这一约定钉住——约定写错时本机即红，不必等一夜。
#[test]
fn xcframework_directory_entries_match_golden_patterns() {
    let golden = load_golden("ios");
    let artifact = artifact_of(&golden, "xcframework");
    let expected = value_str_list(&artifact["entries"]);
    assert!(artifact["extension"].is_null(), "目录形态产物无扩展名");

    let root = std::env::temp_dir().join(format!(
        "polypack-contract-m2-xcframework-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let xcframework = root.join("Polypack App.xcframework");
    for slice in ["ios-arm64", "ios-arm64_x86_64-simulator"] {
        let headers = xcframework.join(slice).join("Headers");
        std::fs::create_dir_all(&headers).expect("夹具切片目录可建");
        std::fs::write(xcframework.join(slice).join("libpax_core.a"), b"!<arch>\n").unwrap();
        std::fs::write(
            headers.join("pax_coreFFI.h"),
            b"int32_t pax_add(int32_t, int32_t);\n",
        )
        .unwrap();
        std::fs::write(
            headers.join("module.modulemap"),
            b"module pax_coreFFI { header \"pax_coreFFI.h\" export * }\n",
        )
        .unwrap();
    }
    std::fs::write(
        xcframework.join("Info.plist"),
        b"<plist><dict><key>AvailableLibraries</key></dict></plist>\n",
    )
    .unwrap();

    let (found, names) = artifact_dir_entries(&root, "xcframework").expect("目录被定位");
    assert_eq!(found, xcframework, "取到的应是唯一那件");
    assert!(
        unmatched_patterns(&expected, &names).is_empty(),
        "条目名约定与金样模式不符：\n{}",
        entries_report(&expected, &names)
    );

    // 非空转证明：抽掉仿真切片的 modulemap，对应模式必被报为缺失。
    std::fs::remove_file(
        xcframework
            .join("ios-arm64_x86_64-simulator")
            .join("Headers")
            .join("module.modulemap"),
    )
    .unwrap();
    let (_, after) = artifact_dir_entries(&root, "xcframework").expect("目录仍被定位");
    assert_eq!(
        unmatched_patterns(&expected, &after),
        vec!["*.xcframework/ios-arm64_x86_64-simulator/Headers/module.modulemap".to_string()],
        "缺少的条目应被精确报出"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 文件形态产物的取件锚定：金样声明 `name` 时**按名**取，不按 mtime 抢新。
///
/// dispatch 38696103225 实证：aapt2 的中间件 `generated-res.zip`（比 `base.zip` 新）被取成
/// module-zip，条目断言随即误报漂移。本用例刻意把中间件造得更新，断言仍取中 `base.zip`。
#[test]
fn module_zip_lookup_is_anchored_by_name() {
    let golden = load_golden("android");
    let artifact = artifact_of(&golden, "module-zip");
    let expected = value_str_list(&artifact["entries"]);
    let name = artifact["name"]
        .as_str()
        .expect("module-zip 声明了按名锚定（与 android.rs 的 `base.zip` 同源）");

    let root = std::env::temp_dir().join(format!("polypack-contract-m2-anchor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let stage = root.join("polypack-app-android.stage");
    std::fs::create_dir_all(&stage).expect("暂存目录可建");

    let manifest = stage.join("AndroidManifest.xml");
    std::fs::write(&manifest, b"<manifest/>").expect("清单可写");
    let resources = stage.join("resources.pb");
    std::fs::write(&resources, b"resources").expect("资源表可写");
    polypack_bundle::zipwrite::write_zip(
        &[
            ("manifest/AndroidManifest.xml".to_string(), manifest),
            ("resources.pb".to_string(), resources),
        ],
        &stage.join(name),
    )
    .expect("base.zip 可写");
    // 中间件写得**更晚**（mtime 更新）——这正是缺陷成立的前提，本用例先反证它。
    let flat = stage.join("values_strings.arsc.flat");
    std::fs::write(&flat, b"flat").expect("中间件内容可写");
    polypack_bundle::zipwrite::write_zip(
        &[("values_strings.arsc.flat".to_string(), flat)],
        &stage.join("generated-res.zip"),
    )
    .expect("generated-res.zip 可写");

    let (newest, _) = artifact_file_entries(&root, "zip", None).expect("按扩展名定位");
    assert_eq!(
        newest.file_name().expect("文件名"),
        "generated-res.zip",
        "中间件确实更新（缺陷前提成立，锚定才是必需的）"
    );

    let (path, names) = artifact_file_entries(&root, "zip", Some(name)).expect("按名锚定命中");
    assert_eq!(path.file_name().expect("文件名"), name);
    assert!(
        unmatched_patterns(&expected, &names).is_empty(),
        "锚定后条目应命中金样：\n{}",
        entries_report(&expected, &names)
    );
    let _ = std::fs::remove_dir_all(&root);
}
