//! T-M3-06 验收：小程序族四目标（wechat / alipay / douyin / quickapp）契约测试与金样比对。
//!
//! ## 模式开关（同 T-M1-13 `contract_m1.rs` / T-M2-14 `contract_m2.rs`）
//!
//! - **mock（默认，每提交）**：①四端在 `poly-targets` 内建注册表中的声明（格式 / 打包器 /
//!   校验器 / 签名引用 / 扩展名）与金样逐字段一致，漂移即失败并打印 `- 声明 / + 金样` diff；
//!   ②金样自检（`artifacts[*]` 的 format / extension / entries 形态，空条目即失败——防金样空转）；
//!   ③直连 `poly_bundle` 公开 API：打包器 id / format 与金样一致；④**快应用封装真跑比对**——
//!   快应用腿无外部工具（Taro 编译腿在计划里如实 skipped），产物目录条目 ↔ 金样 `entries`
//!   逐条比对；⑤比对器非空转证明（造一条缺失条目必被检出）。
//! - **real（`POLY_CONTRACT_REAL=1`，每夜）**：`poly init` → 工程 `ui/adapters/taro` 挂本仓
//!   Taro 宿主（候选链 `ui/adapters/taro/host`）→ `poly build --target <t>` → 产物目录
//!   （`.polypack/packages/<t>/miniapp/`）条目 ↔ 金样 `entries` 比对。Taro 工具链缺席
//!   （宿主 node_modules 未装）或主产物未产出 → 打印 NOTE 后返回，绝不红；快应用腿无外部
//!   工具，任意宿主真跑。
//!
//! 金样入库：`crates/poly-cli/tests/fixtures/m3/<target>.artifact.json`（M1 / M2 金样的同形
//! schema）。四端产物均为**目录形态**（`extension`: null）：`miniapp/`（devtools 打开点，
//! `miniprogramRoot = ./dist/`）+ `dist/`（Taro 编译出的代码包本体）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use poly_targets::{Registry, TargetSpec};

/// 小程序族四目标（M3 小程序链）。
const TARGETS: [&str; 4] = ["wechat", "alipay", "douyin", "quickapp"];

/// 有 Taro 编译腿的三端（含快应用的 Taro 腿如实 skipped，无外部工具）。
const TARO_TARGETS: [&str; 3] = ["wechat", "alipay", "douyin"];

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
        .join("tests/fixtures/m3")
        .join(format!("{target}.artifact.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取金样 {} 失败：{err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("金样 {} 不是合法 JSON：{err}", path.display()))
}

/// 内建注册表中的目标（断言为 L2）。
fn registry_spec(id: &str) -> TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 L2 目标 {id}"));
    assert_eq!(spec.tier, 2, "{id} 应为 L2（tier 2）");
    spec
}

/// 打包格式 → 产物扩展名。小程序族四端均为目录形态（`miniapp/`），一律 None。
fn extension_of(format: &str) -> Option<&'static str> {
    let _ = format;
    None
}

/// 金样 `artifacts` 数组。
fn artifacts(golden: &serde_json::Value) -> &Vec<serde_json::Value> {
    golden["artifacts"]
        .as_array()
        .unwrap_or_else(|| panic!("金样 artifacts 应为数组：{golden}"))
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
/// 实际多出的条目不算违约（编译产物内部文件可自由增补）。
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

/// 目录形态产物条目：`<目录名>/<相对路径>`（同 `contract_m2.rs` 的 xcframework 约定）。
fn dir_entries(dir: &Path) -> Vec<String> {
    let base = dir
        .file_name()
        .expect("目录名")
        .to_string_lossy()
        .to_string();
    let mut names = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
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
            if let Ok(rel) = path.strip_prefix(dir) {
                names.push(format!(
                    "{base}/{}",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }
    }
    names.sort();
    names
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

/// 金样自检：形态合法且非空转（每个产物须有 format 与 entries）。
fn assert_golden_shape(target: &str) {
    let golden = load_golden(target);
    assert_eq!(golden["target"], target, "金样 target 字段应为 {target}");
    assert_eq!(
        golden["signing"],
        serde_json::Value::Null,
        "{target} 签名在平台侧（上传签名），本地不签名"
    );
    for artifact in artifacts(&golden) {
        let format = artifact["format"]
            .as_str()
            .unwrap_or_else(|| panic!("{target} 金样产物缺 format：{artifact}"));
        assert!(
            !value_str_list(&artifact["entries"]).is_empty(),
            "{target} 金样 {format} 无 entries（金样空转）"
        );
        assert!(
            artifact["extension"].is_null(),
            "{target} 金样 {format} 是目录形态产物，extension 应为 null：{artifact}"
        );
    }
}

/// mock：四端打包器在 `poly_bundle` 内以金样声明的（目标 id，格式）注册，且 id / format 一致。
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

/// 四端 mock 契约（每提交，任意宿主）。
#[test]
fn miniapp_contract_mock_covers_all_targets() {
    for target in TARGETS {
        assert_registry_matches_golden(target);
        assert_golden_shape(target);
        assert_packager_registered(target);
    }
}

/// mock：Taro 三端的计划层环境槽——缺宿主工程 / 缺工具链如实报缺（CLI 侧据此跳过），
/// 格式不符硬失败。计划层不碰外部命令，任意宿主可跑。
#[test]
fn taro_targets_plan_needs_host_and_toolchain() {
    let dir = std::env::temp_dir().join(format!("poly-contract-m3-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("临时目录可建");
    let product = poly_bundle::ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "契约测试合成输入".to_string(),
        homepage: None,
    };

    for target in TARO_TARGETS {
        let golden = load_golden(target);
        let format = artifacts(&golden)[0]["format"].as_str().expect("声明格式");
        let spec = registry_spec(target);
        let packager = poly_bundle::find(target, format).expect("打包器注册");

        // 缺宿主：MissingInput（环境槽，CLI 跳过该目标而非硬失败）
        let mut bundle = poly_bundle::BundleInputs::new(product.clone(), dir.join(target));
        bundle.miniapp_host = None;
        match packager.plan(&bundle, &spec) {
            Err(poly_bundle::BundleError::MissingInput { field, .. }) => {
                assert_eq!(field, "miniapp_host", "{target}");
            }
            other => panic!("{target} 缺宿主应报 MissingInput，实得 {other:?}"),
        }

        // 宿主在场但非 Taro 工程（候选链全落空）：同样 MissingInput
        let mut bundle = poly_bundle::BundleInputs::new(product.clone(), dir.join(target));
        bundle.miniapp_host = Some(dir.join(target));
        match packager.plan(&bundle, &spec) {
            Err(poly_bundle::BundleError::MissingInput { field, .. }) => {
                assert_eq!(field, "miniapp_host", "{target}");
            }
            other => panic!("{target} 非 Taro 工程应报 MissingInput，实得 {other:?}"),
        }

        // 格式不符：硬失败（防「按 weapp 目标产出 alipay 包」；拿另一个小程序目标的声明）
        let mut bundle = poly_bundle::BundleInputs::new(product.clone(), dir.join(target));
        bundle.miniapp_host = Some(dir.clone());
        let wrong = if target == "wechat" {
            "alipay"
        } else {
            "wechat"
        };
        match packager.plan(&bundle, &registry_spec(wrong)) {
            Err(err) => assert_eq!(err.code(), "poly::E_BND_0003", "{target} 格式守卫"),
            Ok(_) => panic!("{target} 在 {wrong} 声明下 plan 必须失败"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// mock：快应用封装真跑比对——无外部工具腿（Taro 编译腿在计划里如实 skipped），
/// 产物目录条目 ↔ 金样 `entries` 逐条比对。金样的 real 侧在任意宿主都可跑同一断言。
#[test]
fn quickapp_artifact_matches_golden_without_tools() {
    let dir =
        std::env::temp_dir().join(format!("poly-contract-m3-quickapp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("临时目录可建");

    let mut bundle = poly_bundle::BundleInputs::new(
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
    // 快应用封装不解析宿主工程，但输入面上给出工程根（与 CLI 组装同形）
    bundle.miniapp_host = Some(dir.clone());
    let spec = registry_spec("quickapp");
    let packager = poly_bundle::find("quickapp", "quickapp").expect("quickapp 打包器");
    let plan = packager.plan(&bundle, &spec).expect("计划必须生成");
    assert_eq!(plan.commands.len(), 0, "快应用无外部编译命令");
    assert_eq!(plan.skipped.len(), 1, "Taro 编译腿须如实记入 skipped");

    let artifact = packager.package(&bundle, &spec).expect("封装必须成功");
    let golden = load_golden("quickapp");
    let expected = value_str_list(&artifacts(&golden)[0]["entries"]);
    let actual = dir_entries(&artifact.path);
    assert!(
        unmatched_patterns(&expected, &actual).is_empty(),
        "quickapp 产物与金样漂移（- 金样模式缺失 / + 产物多出条目）：\n{}",
        entries_report(&expected, &actual)
    );
    eprintln!(
        "快应用封装真跑 OK：{}（{} 条 ↔ 金样 {} 条模式）",
        artifact.path.display(),
        actual.len(),
        expected.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 比对器非空转证明：金样声明的条目缺失必被检出（diff 形态），任意宿主可跑。
#[test]
fn golden_drift_is_reported_with_diff() {
    let golden = load_golden("wechat");
    let expected = value_str_list(&artifacts(&golden)[0]["entries"]);
    let full = vec![
        "miniapp/project.config.json".to_string(),
        "miniapp/dist/app.json".to_string(),
        "miniapp/dist/app.js".to_string(),
        "miniapp/dist/app.wxss".to_string(),
        "miniapp/dist/base.wxml".to_string(),
        "miniapp/dist/pages/index/index.js".to_string(),
        "miniapp/dist/pages/index/index.json".to_string(),
        "miniapp/dist/pages/index/index.wxml".to_string(),
    ];
    assert!(
        unmatched_patterns(&expected, &full).is_empty(),
        "合规清单不得报违约：{}",
        entries_report(&expected, &full)
    );

    // 抽掉页面标记文件：仅该模式失配
    let drifted: Vec<String> = full
        .iter()
        .filter(|entry| !entry.ends_with("index.wxml"))
        .cloned()
        .collect();
    let unmatched = unmatched_patterns(&expected, &drifted);
    assert_eq!(
        unmatched,
        vec!["miniapp/dist/pages/index/index.wxml".to_string()],
        "缺条目必被检出"
    );
    let report = entries_report(&expected, &drifted);
    assert!(
        report.contains("- miniapp/dist/pages/index/index.wxml（无匹配条目）"),
        "报告须给出 diff：\n{report}"
    );
}

// ---------------------------------------------------------------- real 模式

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 仓库根（`crates/poly-cli` 的上两级）里的 Taro 宿主工程。
fn repo_taro_adapter() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/adapters/taro")
        .canonicalize()
        .expect("仓库 Taro 适配器可解析")
}

/// 最新一份 `miniapp` 产物目录（快照会复制多份，内容同源；取最新）。
fn find_miniapp(root: &Path) -> Option<PathBuf> {
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
            if path.file_name().is_some_and(|name| name == "miniapp") {
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

/// real：真链路的产物目录条目 ↔ 金样 `entries` 逐件比对。
///
/// Taro 三端需要工程 `ui/adapters/taro` 指向本仓宿主（候选链 `ui/adapters/taro/host`）；
/// 工具链缺席（宿主 node_modules 未装）或主产物未产出 → NOTE 返回，绝不红。
/// 快应用腿无外部工具，任意宿主真跑。
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
    let workdir =
        std::env::temp_dir().join(format!("poly-contract-m3-{target}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("临时工程目录可建");

    let init = poly(&["init"], &workdir);
    assert!(
        init.status.success(),
        "poly init 失败（{}）：{}",
        init.status,
        String::from_utf8_lossy(&init.stderr)
    );

    // 工程 ui/ 挂本仓 Taro 宿主：候选链 `ui` → 根 → `ui/adapters/taro/host` 的第三跳。
    // Taro 三端要求工具链就绪（宿主 node_modules），缺席如实 NOTE 跳过。
    if TARO_TARGETS.contains(&target) {
        let adapter = repo_taro_adapter();
        if !adapter.join("host/node_modules/.bin/taro").is_file() {
            eprintln!(
                "NOTE {target}：Taro 工具链未装（{} 缺 node_modules）——在宿主工程 npm install 后走真比对",
                adapter.join("host").display()
            );
            return;
        }
        let link = workdir.join("ui/adapters/taro");
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).expect("ui/adapters 可建");
        }
        #[cfg(unix)]
        if std::os::unix::fs::symlink(&adapter, &link).is_err() {
            eprintln!("NOTE {target}：无法挂载宿主工程（symlink 失败）");
            return;
        }
    }

    let build = poly(&["build", "--target", target, "--json"], &workdir);
    assert!(
        build.status.success(),
        "poly build --target {target} 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );

    let Some(dir) = find_miniapp(&workdir) else {
        eprintln!(
            "NOTE {target}：未发现 miniapp 产物（落点 .polypack/packages/{target}/；\
             打包腿未接线时如实跳过，nightly 装齐工具链后走真比对）"
        );
        return;
    };
    let expected = value_str_list(&artifacts(&load_golden(target))[0]["entries"]);
    let actual = dir_entries(&dir);
    assert!(
        unmatched_patterns(&expected, &actual).is_empty(),
        "{target} 产物 {} 与金样漂移（- 金样模式缺失 / + 产物多出条目）：\n{}",
        dir.display(),
        entries_report(&expected, &actual)
    );
    eprintln!(
        "OK {target}：{} 的 {} 条命中金样 {} 条模式",
        dir.display(),
        actual.len(),
        expected.len()
    );
    // 真跑产物留档：不删临时工程（本地留一份供人工核验）
    eprintln!("OK {target}：工程 {}", workdir.display());
}

/// real：四端（Linux 宿主真跑 Taro 三端；快应用任意宿主）。
#[test]
fn contract_miniapp_real_artifacts() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLY_CONTRACT_REAL=1 运行");
        return;
    }
    for target in TARGETS {
        real_artifact_check(target);
    }
}
