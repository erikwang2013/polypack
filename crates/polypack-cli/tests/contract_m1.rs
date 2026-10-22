//! T-M1-13 验收：四端（Windows / macOS / Linux / Web）契约测试与二跑缓存契约。
//!
//! ## 模式开关
//!
//! - **mock（默认，每次提交）**：不依赖任何打包器真实产物，断言
//!   ①四端目标在 `polypack-targets` 内建注册表中以 L1 声明存在，且格式 / 打包器 / 校验器 / 签名
//!   与金样逐字段一致；②直连 `polypack_bundle` 公开 API：`for_spec` → `plan()`（打包器 id / format
//!   与计划产物落点扩展名，含 linux 的第二格式 deb 与 256 PNG 前置）、`tauri::bare_product`
//!   （macOS 缺 icns 必拒 + `.app` 布局命中金样条目）；③金样产物描述通过 [`validate_artifact`] 自检；
//!   ④校验器对篡改样本必失败（`validator_rejects_tampered_golden`，证明断言非空转）。
//!   任意宿主（CI 三宿主同一命令）必须全绿。
//! - **real（`POLYPACK_CONTRACT_REAL=1`，每夜）**：在真实链路上执行 `polypack init` → `polypack build` →
//!   裸产物快照（`.polypack/artifacts/<target>/build-<millis>/{core,dist,packages}`，T-M1-03）与金样
//!   `bare-product` 条目比对；package 阶段已接线，产物落 `<root>/.polypack/packages/<target>/`
//!   并随快照复制（`crates/polypack-cli/src/build.rs::snapshot_artifacts`），故 msi / dmg / appimage /
//!   deb / static-site 一律「有则深检、无则 NOTE 跳过」（缺的是工具链：AppImage 单文件需
//!   appimagetool 与 runtime；deb 链路在临时工程内把声明改成 deb 后重跑，见
//!   `linux_deb_end_to_end`；web 静态站在 `packages/web/site/`）。宿主 /
//!   动词 / 工具链 / 打包器检查器未就绪时打印 `SKIP` / `NOTE` 后返回，绝不红。
//!
//! ## 金样（`tests/fixtures/m1/<platform>.artifact.json`，打包器矩阵的产物契约单源）
//!
//! ```text
//! { target, builder, signing, verifiers,          # 与 polypack-targets 内建声明逐字段一致
//!   artifacts: [ { format, extension,             # extension = null 表示目录形态（如静态站）
//!                  entries,                       # 产物内部必备结构：'*' 通配任意字符，
//!                  manifest_keys? } ] }           # 末尾 '/' 表示目录；manifest_keys = 清单必备键
//! ```
//!
//! `entries` 的事实来源：`polypack-bundle` T-M1-03 裸产物布局（`crates/polypack-bundle/src/tauri.rs`）
//! 与各打包器输出（T-M1-05…08）；单文件安装器（msi / dmg / appimage）的条目指**安装后落盘结构**，
//! 跨宿主深检需打包器附带检查器。
//!
//! `artifacts[0]` 恒为该端的打包格式（`polypack-targets` 声明比对用）；`format = "bare-product"` 的
//! 追加条目描述真实链路**当前**终点 —— `polypack build` 的产物快照，与打包格式无关：桌面端
//! （windows / macos / linux）为 `dist + core/*`，web 端无 `core`（任务的 `CompileStage` 在
//! 无宿主原生桌面目标时跳过，`crates/polypack-core/src/pipeline.rs`）。
//!
//! ## 与 polypack-bundle 的接线
//!
//! `polypack-cli` 已声明 `polypack-bundle` 依赖，mock 侧直连公开 API（`for_spec` / `Packager::plan`）；
//! real 侧仍走 CLI 契约（`CARGO_BIN_EXE_polypack`）驱动的 `polypack init` / `polypack build`。**升级点**：
//! real 深检现在按产物落盘情况自动生效（`packages/<target>/` 有则断言结构）；单文件安装器
//! （msi / dmg / appimage）的内部结构需打包器附带检查器，届时在 [`artifact_entries`] 内补
//! 「解包 / 挂载」分支即可，契约与金样无需改动。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use polypack_targets::{Registry, TargetSpec};

/// 四端目标（T-M1-11 的 L1 声明即为此四项）。
const PLATFORMS: [&str; 4] = ["windows", "macos", "linux", "web"];

// ---------------------------------------------------------------- 模式与 CLI

/// 真实模式开关：`POLYPACK_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

/// 按 JSON Lines 解析输出（`polypack … --json` 契约）。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("行不是合法 JSON：{line}（{err}）"))
        })
        .collect()
}

/// 当前测试函数名（拼临时目录名用）：并行用例不得共用工作目录。
fn test_label() -> String {
    std::thread::current()
        .name()
        .unwrap_or("contract")
        .replace("::", "-")
}

/// CLI 是否已注册某动词（查 `polypack --help`）；未落盘时调用方清晰 SKIP。
fn cli_has_verb(verb: &str) -> bool {
    let out = polypack(&["--help"], &std::env::temp_dir());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.trim_start().starts_with(verb))
}

// ---------------------------------------------------------------- 金样与注册表

/// 读取某端金样。
fn load_golden(platform: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m1")
        .join(format!("{platform}.artifact.json"));
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

fn value_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("金样字段 {key} 应为字符串：{value}"))
}

fn value_str_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("金样字段 {key} 应为数组：{value}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("{key} 元素应为字符串：{item}"))
                .to_string()
        })
        .collect()
}

/// 打包格式 → 产物扩展名（`static-site` 为目录形态，无扩展名）。
fn extension_of(format: &str) -> Option<&'static str> {
    match format {
        "msi" => Some("msi"),
        "dmg" => Some("dmg"),
        "appimage" => Some("appimage"),
        "deb" => Some("deb"),
        _ => None,
    }
}

/// 金样中某 format 的产物契约。
fn artifact_of(golden: &serde_json::Value, format: &str) -> serde_json::Value {
    golden["artifacts"]
        .as_array()
        .expect("金样 artifacts 数组")
        .iter()
        .find(|artifact| artifact["format"] == format)
        .unwrap_or_else(|| panic!("金样缺少 {format} 契约"))
        .clone()
}

/// mock 核心断言：内建目标声明与金样逐字段一致（任一侧漂移即红）。
fn assert_registry_matches_golden(platform: &str, golden: &serde_json::Value) {
    let spec = registry_spec(platform);
    let primary = &golden["artifacts"][0];
    assert_eq!(
        spec.package.format,
        value_str(primary, "format"),
        "{platform} 打包格式与金样不一致"
    );
    assert_eq!(
        spec.package.builder,
        value_str(golden, "builder"),
        "{platform} 打包器与金样不一致"
    );
    assert_eq!(
        spec.verify,
        value_str_list(golden, "verifiers"),
        "{platform} 校验器清单与金样不一致"
    );
    assert_eq!(
        spec.signing,
        golden["signing"].as_str().map(str::to_string),
        "{platform} 签名档案与金样不一致"
    );
    assert_eq!(
        extension_of(&spec.package.format),
        primary["extension"].as_str(),
        "{platform} 格式 → 扩展名映射与金样不一致"
    );
}

// ---------------------------------------------------------------- 产物结构校验器

/// 可复用产物结构校验器：输入**产物描述 JSON + 平台**，对照该端金样；
/// 返回全部违例（空 = 通过），便于一次报出所有结构缺陷。
///
/// 产物描述形态（mock 自检与 real 模式产物共用同一形态）：
/// `{ target, builder, format, extension, verifiers, entries, manifest_keys? }`。
fn validate_artifact(description: &serde_json::Value, platform: &str) -> Result<(), Vec<String>> {
    let golden = load_golden(platform);
    let mut errors: Vec<String> = Vec::new();

    if description["target"] != golden["target"] {
        errors.push(format!(
            "目标不符：描述 {} ≠ 金样 {}",
            description["target"], golden["target"]
        ));
    }
    if description["builder"] != golden["builder"] {
        errors.push(format!(
            "打包器不符：描述 {} ≠ 金样 {}",
            description["builder"], golden["builder"]
        ));
    }

    let artifacts = golden["artifacts"].as_array().expect("金样 artifacts 数组");
    let Some(expected) = artifacts
        .iter()
        .find(|artifact| artifact["format"] == description["format"])
    else {
        return Err(vec![format!("金样未声明 format {}", description["format"])]);
    };

    if description["extension"] != expected["extension"] {
        errors.push(format!(
            "扩展名不符：描述 {} ≠ 金样 {}",
            description["extension"], expected["extension"]
        ));
    }

    let declared = value_str_list(&golden, "verifiers");
    let actual: Vec<String> = description["verifiers"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for verifier in &declared {
        if !actual.contains(verifier) {
            errors.push(format!("缺少契约校验器 {verifier}"));
        }
    }

    match description["entries"].as_array() {
        Some(entries) => {
            for pattern in expected["entries"].as_array().expect("金样 entries 数组") {
                let pattern = pattern.as_str().expect("金样 entries 元素为字符串");
                let matched = entries.iter().any(|entry| {
                    entry
                        .as_str()
                        .is_some_and(|entry| entry_matches(pattern, entry))
                });
                if !matched {
                    errors.push(format!("缺少契约条目 {pattern}"));
                }
            }
        }
        None => errors.push("描述缺少 entries（产物内部清单）".to_string()),
    }

    if let Some(keys) = expected
        .get("manifest_keys")
        .and_then(|value| value.as_array())
    {
        let actual = description
            .get("manifest_keys")
            .and_then(|value| value.as_array());
        for key in keys {
            let key = key.as_str().expect("金样 manifest_keys 元素为字符串");
            if actual.is_none_or(|items| !items.iter().any(|item| item.as_str() == Some(key))) {
                errors.push(format!("清单缺少必备键 {key}"));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// 契约条目匹配：末尾 `/` 为目录（匹配目录自身或其后代），否则为 `*` 通配 glob。
fn entry_matches(pattern: &str, entry: &str) -> bool {
    if let Some(dir) = pattern.strip_suffix('/') {
        return entry == dir.trim_end_matches('/') || entry.starts_with(dir);
    }
    glob_match(pattern, entry)
}

/// 极简 glob：`*` 匹配任意字符（含 `/`），其余字符字面匹配。
fn glob_match(pattern: &str, entry: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == entry;
    }
    let mut rest = entry;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            if index == parts.len() - 1 {
                return true; // 尾随 '*' 吞掉剩余
            }
            continue; // 前导 / 连续的 '*' 不锚定
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

/// 由金样条目构造自检描述（金样 ↔ 校验器 一致性；不含任何发明字段）。
fn golden_description(
    golden: &serde_json::Value,
    artifact: &serde_json::Value,
) -> serde_json::Value {
    let mut description = serde_json::json!({
        "target": golden["target"],
        "builder": golden["builder"],
        "format": artifact["format"],
        "extension": artifact["extension"],
        "verifiers": golden["verifiers"],
        "entries": artifact["entries"],
    });
    if let Some(keys) = artifact.get("manifest_keys") {
        description["manifest_keys"] = keys.clone();
    }
    description
}

// ---------------------------------------------------------------- 四端契约

/// mock 契约（任意宿主恒跑）+ real 追加链路（`POLYPACK_CONTRACT_REAL=1`）。
fn contract_platform(platform: &str) {
    let golden = load_golden(platform);
    assert_registry_matches_golden(platform, &golden);
    for artifact in golden["artifacts"].as_array().expect("金样 artifacts 数组") {
        let description = golden_description(&golden, artifact);
        validate_artifact(&description, platform)
            .unwrap_or_else(|errors| panic!("{platform} 金样自检失败：{errors:?}"));
    }
    if real_mode() {
        real_artifact_check(platform, &golden);
    }
}

/// Windows（MSI / WiX）契约：注册表 L1 声明 + 金样校验；real 模式补跑真实打包。
#[test]
fn contract_windows() {
    contract_platform("windows");
}

/// macOS（DMG）契约：注册表 L1 声明 + 金样校验；real 模式补跑真实打包。
#[test]
fn contract_macos() {
    contract_platform("macos");
}

/// Linux（AppImage / deb）契约：注册表 L1 声明 + 金样校验；real 链路见 `linux_deb_end_to_end`。
#[test]
fn contract_linux() {
    contract_platform("linux");
}

/// Web（静态站 + PWA）契约：注册表 L1 声明 + 金样校验；real 模式补跑真实打包。
#[test]
fn contract_web() {
    contract_platform("web");
}

/// CLI 契约：`polypack doctor --json` 报告含四端 L1 覆盖矩阵（T-M1-11 / T-M1-13）。
#[test]
fn doctor_json_declares_four_l1_targets() {
    let out = polypack(&["doctor", "--json"], Path::new(env!("CARGO_MANIFEST_DIR")));
    assert_eq!(out.status.code(), Some(0), "doctor 恒 0（报告不门控）");
    let lines = json_lines(&out);
    let report = &lines[0];
    assert_eq!(report["event"], "doctor", "首行是 doctor 报告对象");
    let targets = report["targets"].as_array().expect("报告含 L1 覆盖矩阵");
    for platform in PLATFORMS {
        let target = targets
            .iter()
            .find(|target| target["id"] == platform)
            .unwrap_or_else(|| panic!("doctor 报告缺少 L1 目标 {platform}"));
        assert!(
            target["ready"].is_boolean(),
            "{platform} 带 ready 字段：{target}"
        );
        assert!(
            target["missing"].is_array(),
            "{platform} 带 missing 字段：{target}"
        );
    }
}

/// 校验器非空转证明：四端金样自检通过，篡改扩展名 / 抽空清单必失败。
#[test]
fn validator_rejects_tampered_golden() {
    for platform in PLATFORMS {
        let golden = load_golden(platform);
        for artifact in golden["artifacts"].as_array().expect("金样 artifacts 数组") {
            let description = golden_description(&golden, artifact);
            validate_artifact(&description, platform)
                .unwrap_or_else(|errors| panic!("{platform} 金样自检失败：{errors:?}"));

            let mut tampered = description.clone();
            tampered["extension"] = serde_json::json!("bogus");
            assert!(
                validate_artifact(&tampered, platform).is_err(),
                "{platform} 扩展名篡改未被检出"
            );

            let mut tampered = description.clone();
            tampered["entries"] = serde_json::json!([]);
            assert!(
                validate_artifact(&tampered, platform).is_err(),
                "{platform} 清单抽空未被检出"
            );
        }
    }
}

/// mock 直连 polypack-bundle 公开 API：四端 `for_spec` → `plan()` 可生成，且计划中的产物落点
/// 扩展名与金样一致（`plan` 为纯函数：只读输入元数据，不写盘、不执行工具 —— 任意宿主可跑）。
#[test]
fn packager_plan_satisfies_golden_contract() {
    for platform in PLATFORMS {
        let spec = registry_spec(platform);
        let golden = load_golden(platform);
        let primary = &golden["artifacts"][0];
        let packager = polypack_bundle::for_spec(&spec).unwrap_or_else(|| {
            panic!(
                "polypack-bundle 缺少 {platform} 的 {} 打包器",
                spec.package.format
            )
        });
        assert_eq!(
            packager.id(),
            platform,
            "{platform} 打包器 id 应等于目标 id"
        );
        assert_eq!(
            packager.format(),
            value_str(primary, "format"),
            "{platform} 打包器 format 与金样不一致"
        );

        let inputs = plan_inputs(platform);
        let plan = packager
            .plan(&inputs, &spec)
            .unwrap_or_else(|err| panic!("{platform} plan() 失败：{err}"));
        assert!(
            !plan.scripts.is_empty() || !plan.commands.is_empty() || !plan.skipped.is_empty(),
            "{platform} plan() 空转：无脚本 / 命令 / 跳过项"
        );
        if let Some(extension) = primary["extension"].as_str() {
            let suffix = format!(".{}", extension.to_lowercase());
            let planned: Vec<String> = plan
                .scripts
                .iter()
                .map(|script| script.path.to_string_lossy().to_lowercase())
                .chain(
                    plan.commands
                        .iter()
                        .flat_map(|command| command.args.iter().map(|arg| arg.to_lowercase())),
                )
                .collect();
            assert!(
                planned.iter().any(|path| path.ends_with(&suffix)),
                "{platform} plan() 中无 *{suffix} 落点：{planned:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&inputs.out_dir);
    }
}

/// `plan()` 的合成输入：计划阶段只读输入元数据（不写盘、不执行工具），但 deb / msi 会枚举
/// 裸产物树、static-site 会枚举 `ui_dist` 内的 HTML，故给一份最小真实树与最小入口 HTML；
/// 图标按 macOS 布局的必需输入（`IconKind::Icns`，T-M1-03）给一枚占位 icns。
fn plan_inputs(platform: &str) -> polypack_bundle::BundleInputs {
    let out_dir = std::env::temp_dir().join(format!(
        "polypack-contract-plan-{}-{platform}-{}",
        test_label(),
        std::process::id()
    ));
    let bare_root = out_dir.join("bare");
    let executable = bare_root.join("usr/bin/polypack-app-core");
    let ui_dist = out_dir.join("dist");
    let icns = out_dir.join("icon.icns");
    let png256 = out_dir.join("icon-256.png");
    std::fs::create_dir_all(executable.parent().expect("可执行文件有父目录"))
        .expect("裸产物树可建");
    std::fs::write(&executable, b"").expect("裸产物可执行可写");
    std::fs::create_dir_all(&ui_dist).expect("UI 产物目录可建");
    std::fs::write(ui_dist.join("index.html"), "<!doctype html><html></html>")
        .expect("最小入口 HTML 可写");
    // 与 polypack-bundle/tests/tauri_layout.rs 同形（魔数 + 长度 + 哑负载）：布局只校验输入存在，不解析格式
    std::fs::write(&icns, b"icns\x00\x00\x00\x10PAX \x00\x00\x00\x00").expect("占位 icns 可写");
    std::fs::write(&png256, b"png").expect("占位 256 PNG 可写");

    let mut inputs = polypack_bundle::BundleInputs::new(
        polypack_bundle::ProductInfo {
            name: "polypack-app".to_string(),
            version: "0.1.0".to_string(),
            identifier: "com.polypack.app".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "契约测试合成输入".to_string(),
            homepage: None,
        },
        &out_dir,
    );
    inputs.core_binary = Some(executable.clone());
    inputs.bare = Some(polypack_bundle::BareProduct {
        root: bare_root,
        executable,
        host: polypack_targets::current_host().unwrap_or(polypack_targets::Host::Linux),
    });
    inputs.ui_dist = Some(ui_dist);
    inputs.icons.push(polypack_bundle::Icon {
        path: icns,
        kind: polypack_bundle::IconKind::Icns,
        size: None,
    });
    inputs.icons.push(polypack_bundle::Icon {
        path: png256,
        kind: polypack_bundle::IconKind::Png,
        size: Some(256),
    });
    inputs
}

/// mock 直连 polypack-bundle：linux 的第二打包格式 deb（金样 `artifacts[1]`，声明默认格式是
/// appimage，故按格式查找并把声明克隆成 deb 变体）。顺带锚定金样
/// `usr/share/icons/hicolor/256x256/apps/*` 的前提 —— 256 PNG 缺失必须响亮拒绝。
#[test]
fn deb_packager_plan_requires_png256_and_matches_golden() {
    let golden = load_golden("linux");
    let mut spec = registry_spec("linux");
    spec.package.format = "deb".to_string();
    let packager = polypack_bundle::find("linux", "deb").expect("polypack-bundle 内置 deb 打包器");
    let inputs = plan_inputs("linux-deb");

    // 无 PNG：响亮拒绝（金样 hicolor 条目的图标前提，图标由 T-M1-14 派生管线提供）
    let mut no_png = inputs.clone();
    no_png
        .icons
        .retain(|icon| icon.kind != polypack_bundle::IconKind::Png);
    assert!(
        matches!(
            packager.plan(&no_png, &spec),
            Err(polypack_bundle::BundleError::MissingInput {
                field: "icons(PNG)",
                ..
            })
        ),
        "deb 无 PNG 应报 MissingInput/icons(PNG)"
    );

    // 有 PNG 但非 256：同样拒绝（对齐金样 `usr/share/icons/hicolor/256x256/apps/*`）
    let mut png64 = inputs.clone();
    for icon in &mut png64.icons {
        if icon.kind == polypack_bundle::IconKind::Png {
            icon.size = Some(64);
        }
    }
    assert!(
        matches!(
            packager.plan(&png64, &spec),
            Err(polypack_bundle::BundleError::MissingInput {
                field: "icons(PNG 256)",
                ..
            })
        ),
        "deb 缺 256 PNG 应报 MissingInput/icons(PNG 256)"
    );

    let plan = packager
        .plan(&inputs, &spec)
        .unwrap_or_else(|err| panic!("deb plan() 失败：{err}"));
    let planned: Vec<String> = plan
        .scripts
        .iter()
        .map(|script| script.path.to_string_lossy().replace('\\', "/"))
        .chain(
            plan.commands
                .iter()
                .flat_map(|command| command.args.iter().cloned()),
        )
        .collect();
    assert!(
        planned.iter().any(|path| path.ends_with("DEBIAN/control")),
        "deb 计划缺少金样条目 DEBIAN/control：{planned:?}"
    );
    let deb_contract = artifact_of(&golden, "deb");
    let extension = value_str(&deb_contract, "extension");
    assert!(
        planned
            .iter()
            .any(|path| path.to_lowercase().ends_with(&format!(".{extension}"))),
        "deb 计划中无 *.{extension} 落点：{planned:?}"
    );
    let _ = std::fs::remove_dir_all(&inputs.out_dir);
}

/// mock 直连 T-M1-03 生产者（`polypack_bundle::tauri::bare_product`，宿主显式传参故任意宿主可跑）：
/// macOS `.app` 缺 icns **响亮拒绝**（不静默省略图标）；补齐后布局命中金样 dmg 的
/// `*.app/Contents/*` 条目 —— 金样 `*.icns` 条的 mock 级锚点（不依赖 macOS 打包机）。
#[test]
fn macos_bare_product_requires_icns_and_matches_golden_layout() {
    let golden = load_golden("macos");
    let mut inputs = plan_inputs("macos");

    let icns = inputs
        .icons
        .iter()
        .find(|icon| icon.kind == polypack_bundle::IconKind::Icns)
        .expect("plan_inputs 提供占位 icns")
        .clone();
    inputs
        .icons
        .retain(|icon| icon.kind != polypack_bundle::IconKind::Icns);
    let error = polypack_bundle::tauri::bare_product(&inputs, polypack_targets::Host::Macos)
        .expect_err("macOS 裸产物缺 icns 应报错");
    assert!(
        matches!(
            error,
            polypack_bundle::BundleError::MissingInput {
                field: "icons.icns",
                ..
            }
        ),
        "应报 MissingInput/icons.icns：{error:?}"
    );

    inputs.icons.push(icns);
    let bare = polypack_bundle::tauri::bare_product(&inputs, polypack_targets::Host::Macos)
        .expect("补齐 icns 后应产出 .app");
    let entries = directory_entries(&bare.root);
    for pattern in artifact_of(&golden, "dmg")["entries"]
        .as_array()
        .expect("金样 entries 数组")
    {
        let pattern = pattern.as_str().expect("金样 entries 元素为字符串");
        if pattern == "Applications" {
            continue; // DMG 暂存目录布局（打包阶段 T-M1-06），裸产物不含
        }
        assert!(
            entries.iter().any(|entry| entry_matches(pattern, entry)),
            "macOS 裸产物缺契约条目 {pattern}：{entries:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&inputs.out_dir);
}

/// 报告契约：缓存命中统计依赖的事件字段（`cached.key` / `cached.hit`，ROADMAP §4.2）。
#[test]
fn cached_event_contract_has_key_and_hit() {
    let event = polypack_core::BuildEvent::Cached {
        key: "blake3:0000".to_string(),
        hit: true,
    };
    let value = serde_json::to_value(&event).expect("BuildEvent 可序列化");
    assert_eq!(value["event"], "cached", "cached 事件 tag");
    assert!(value["key"].is_string(), "cached 事件含缓存键：{value}");
    assert_eq!(
        value["hit"],
        serde_json::json!(true),
        "cached 事件含命中标记：{value}"
    );
}

// ---------------------------------------------------------------- 二跑缓存（T-M1-09 × T-M1-13）

/// T-M1-13 验收（缓存部分）：同一输入二跑缓存命中率 ≥ 90%。
///
/// 默认 mock 模式 SKIP；真实链路需 `POLYPACK_CONTRACT_REAL=1` + Linux 宿主 + `cargo`。首跑由
/// `real_project` 完成 —— 它同时生成 `core/Cargo.lock`，缓存键自此稳定，故以第二 / 第三跑为
/// **同一输入基准**（首跑键与二跑不同，不参与命中率断言）。命中数取自事件流 `cached.hit`。
#[test]
fn second_run_cache_hit_rate_at_least_90pct() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    let spec = registry_spec("linux");
    let host = polypack_targets::current_host();
    if !host.is_some_and(|host| spec.hosts.contains(&host)) {
        eprintln!("SKIP：缓存二跑契约在 Linux 宿主执行，当前宿主 {host:?}");
        return;
    }
    if !cli_has_verb("init") || !cli_has_verb("build") {
        eprintln!("SKIP：`polypack init` / `polypack build` 未落盘（T-M1-01 / T-M1-11）");
        return;
    }
    if !cargo_available() {
        eprintln!("SKIP：宿主无 cargo，compile 支线无法执行（T-M1-01）");
        return;
    }

    let project = real_project("linux");
    let mut rates = Vec::new();
    for run in 2..=3 {
        let events = json_lines(&polypack(
            &["build", "--target", "linux", "--json"],
            &project.workdir,
        ));
        let (hits, total) = cache_hits(&events);
        assert!(
            total > 0,
            "第 {run} 跑事件流应含 cached 事件（契约见 cached_event_contract_has_key_and_hit）：{events:?}"
        );
        rates.push((run, hits, total));
    }
    for (run, hits, total) in rates {
        let rate = hits as f64 / total as f64;
        assert!(
            rate >= 0.9,
            "第 {run} 跑缓存命中率 {rate:.3} < 0.90（{hits} / {total}）；T-M1-13 验收"
        );
        eprintln!(
            "OK：第 {run} 跑缓存命中率 {:.0}%（{hits} / {total}）",
            rate * 100.0
        );
    }
    let _ = std::fs::remove_dir_all(&project.workdir);
}

/// 事件流中的缓存统计：（`hit = true` 数，`cached` 事件总数）。
fn cache_hits(events: &[serde_json::Value]) -> (usize, usize) {
    let cached: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event["event"] == "cached")
        .collect();
    let hits = cached
        .iter()
        .filter(|event| event["hit"] == serde_json::json!(true))
        .count();
    (hits, cached.len())
}

/// 宿主是否有 cargo（`polypack build` 的 compile 支线前提）。
fn cargo_available() -> bool {
    Command::new("cargo")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

// ---------------------------------------------------------------- real 模式链路

/// 临时工程：`polypack init` → 断言模板配置声明四端 → `polypack build`。
///
/// real 模式下 init / build 失败即真实失败（该模式显式开启，不做静默降级）。
struct RealProject {
    workdir: PathBuf,
}

fn real_project(target: &str) -> RealProject {
    let workdir = std::env::temp_dir().join(format!(
        "polypack-contract-{target}-{}-{}",
        test_label(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("临时工程目录可创建");
    let init = polypack(&["init"], &workdir);
    assert!(
        init.status.success(),
        "polypack init 失败（{}）：{}",
        init.status,
        String::from_utf8_lossy(&init.stderr)
    );
    // T-M1-11：init 模板必须声明四端 L1 目标，且直接通过 schema 校验。
    let config = polypack_core::PolypackConfig::load(&workdir.join("polypack.toml"))
        .unwrap_or_else(|err| panic!("init 产物 polypack.toml 应通过 schema 校验：{err}"));
    for platform in PLATFORMS {
        assert!(
            config.targets.contains_key(platform),
            "init 模板应声明目标 {platform}（T-M1-11）"
        );
    }
    let build = polypack(&["build", "--target", target, "--json"], &workdir);
    assert!(
        build.status.success(),
        "polypack build --target {target} 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );
    RealProject { workdir }
}

/// 真实模式逐端链路：init → build → 裸产物快照结构 → 金样 `bare-product` 比对 →
/// 打包格式产物「有则深检、无则 NOTE 跳过」。
///
/// package 阶段已接线：产物落 `<root>/.polypack/packages/<target>/`（web 端为静态站目录
/// `site/`），并随快照复制到 `packages/`。单文件安装器（msi / dmg）的内部结构深检需打包器
/// 附带检查器，缺失时只断言存在。宿主 / 动词未就绪一律 SKIP 后返回。
fn real_artifact_check(platform: &str, golden: &serde_json::Value) {
    let spec = registry_spec(platform);
    let host = polypack_targets::current_host();
    if !host.is_some_and(|host| spec.hosts.contains(&host)) {
        eprintln!(
            "SKIP {platform}：目标声明宿主 {:?}，当前宿主 {host:?}",
            spec.hosts
        );
        return;
    }
    if !cli_has_verb("init") || !cli_has_verb("build") {
        eprintln!("SKIP {platform}：`polypack init` / `polypack build` 未落盘（等待 T-M1-11 / T-M1-01）");
        return;
    }
    if !cargo_available() {
        eprintln!("SKIP {platform}：宿主无 cargo，compile 支线无法执行（T-M1-01）");
        return;
    }
    if platform == "linux" {
        eprintln!(
            "NOTE linux：真实链路（快照 + deb / AppImage 深检）在 linux_deb_end_to_end 执行，此处不重复构建"
        );
        return;
    }
    let project = real_project(platform);
    assert_bare_snapshot(platform, &project.workdir, golden);

    let package_contract = golden["artifacts"][0].clone();
    let format = value_str(&package_contract, "format").to_string();
    match extension_of(&format) {
        // 单文件安装器（msi / dmg）：发现即深检；内部结构需检查器时 packaged_check 报 NOTE。
        Some(extension) => match find_artifact(&project.workdir, extension) {
            Some(artifact) => {
                packaged_check(platform, golden, &package_contract, &artifact, &format)
            }
            None => eprintln!(
                "NOTE {platform}：未发现 .{extension} 产物（package 阶段已接线；缺宿主工具链时打包器如实跳过，落点 .polypack/packages/{platform}/）"
            ),
        },
        // 目录形态（web static-site）：按标记文件定位，已知布局 `.polypack/packages/web/site/`。
        None => match find_tree(&project.workdir, "integrity.json") {
            Some(site) => packaged_check(platform, golden, &package_contract, &site, &format),
            None => eprintln!(
                "NOTE {platform}：未定位到 static-site 产物（应含 index.html + integrity.json；已知布局 .polypack/packages/{platform}/site/）"
            ),
        },
    }
    let _ = std::fs::remove_dir_all(&project.workdir);
}

/// T-M1-07 / T-M1-13 真实端到端：`polypack init` 模板 → 构建 → 裸产物快照断言 → linux 产物
/// 「有则深检、无则 NOTE 跳过」。
///
/// package 阶段已接线，产物落 `<root>/.polypack/packages/linux/`：AppImage 单文件（需
/// appimagetool）或降级 AppDir 目录；deb 归档 `.deb` 或暂存树 `*-deb.stage/`（含
/// `DEBIAN/control`）。Deb 打包器默认不参与（内建声明 format = appimage），本测试在临时工程内
/// 改声明为 deb 后重跑一次，使真 deb 链路每次都跑；宿主缺 dpkg-deb 时打包器如实跳过，非红。
///
/// 默认 mock 模式直接 SKIP；`POLYPACK_CONTRACT_REAL=1` 且在 Linux 宿主执行。
#[test]
fn linux_deb_end_to_end() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }
    let spec = registry_spec("linux");
    let host = polypack_targets::current_host();
    if !host.is_some_and(|host| spec.hosts.contains(&host)) {
        eprintln!(
            "SKIP linux：目标声明宿主 {:?}，当前宿主 {host:?}",
            spec.hosts
        );
        return;
    }
    if !cli_has_verb("init") || !cli_has_verb("build") {
        eprintln!("SKIP linux：`polypack init` / `polypack build` 未落盘（等待 T-M1-11 / T-M1-01）");
        return;
    }
    if !cargo_available() {
        eprintln!("SKIP linux：宿主无 cargo，compile 支线无法执行（T-M1-01）");
        return;
    }

    let project = real_project("linux");
    let golden = load_golden("linux");

    // 真实链路当前终点（T-M1-03 快照 `{core, dist, packages}`）：裸产物契约。
    assert_bare_snapshot("linux", &project.workdir, &golden);

    // AppImage：有 appimagetool 即单文件；缺失时打包器如实降级为 AppDir 目录
    // （polypack-bundle linux.rs：`dir_artifact("appdir")`）。两者都按 appimage 金样条目深检。
    match find_artifact(&project.workdir, "appimage") {
        Some(appimage) => packaged_check(
            "linux",
            &golden,
            &artifact_of(&golden, "appimage"),
            &appimage,
            "appimage",
        ),
        None => match find_appdir(&project.workdir) {
            Some(appdir) => {
                eprintln!(
                    "NOTE linux：宿主缺 appimagetool，无 AppImage 单文件；深检降级产物 AppDir {}",
                    appdir.display()
                );
                packaged_check(
                    "linux",
                    &golden,
                    &artifact_of(&golden, "appimage"),
                    &appdir,
                    "appimage",
                );
            }
            None => eprintln!(
                "NOTE linux：未产出 AppImage / AppDir（package 阶段已接线，落点 .polypack/packages/linux/）"
            ),
        },
    }

    // deb 打包器默认不参与（内建声明为 appimage）：临时工程里把声明改成 deb 后重跑，走真 deb
    // 链路（T-M1-07）。快照断言已在上方完成，重跑只新增快照与 `packages/` 产物。
    declare_linux_deb(&project.workdir);

    // deb：优先 .deb 归档（`dpkg-deb` 内容清单 + 控制字段）；否则 DEBIAN 暂存树（同结构、
    // 同字段）；两者皆无 → NOTE（宿主缺 dpkg-deb 时打包器如实跳过，非红）。
    let deb_contract = artifact_of(&golden, "deb");
    match find_artifact(&project.workdir, "deb") {
        Some(deb) => {
            packaged_check("linux", &golden, &deb_contract, &deb, "deb");
            match Command::new("dpkg-deb")
                .arg("--field")
                .arg(&deb)
                .args(["Package", "Version", "Architecture"])
                .output()
            {
                // 能启动但读不到 control（缺成员即 exit 2）须响亮失败——`--contents` 只列数据段，
                // 控制段缺失不会被清单发现，只能靠这里兜住（实测：空 stdout 会顺手带出断言）。
                Ok(info) => {
                    assert!(
                        info.status.success(),
                        "dpkg-deb --field 失败（{}）：{}",
                        info.status,
                        String::from_utf8_lossy(&info.stderr)
                    );
                    assert_deb_control(
                        &String::from_utf8_lossy(&info.stdout),
                        &deb.display().to_string(),
                    );
                }
                Err(err) => eprintln!("SKIP linux deb 控制字段深检：dpkg-deb 不可用（{err}）"),
            }
        }
        None => match find_tree(&project.workdir, "DEBIAN/control") {
            Some(stage) => {
                let control = stage.join("DEBIAN/control");
                eprintln!(
                    "NOTE linux：无 .deb 归档，深检 deb 暂存树 {}",
                    stage.display()
                );
                packaged_check("linux", &golden, &deb_contract, &stage, "deb");
                assert_deb_control(
                    &std::fs::read_to_string(&control).unwrap_or_default(),
                    &control.display().to_string(),
                );
            }
            None => eprintln!(
                "NOTE linux：声明 deb 后仍未产出 .deb 归档与 DEBIAN 暂存树（宿主缺 dpkg-deb 时打包器如实跳过，落点 .polypack/packages/linux/）"
            ),
        },
    }
    let _ = std::fs::remove_dir_all(&project.workdir);
}

/// 把临时工程的 linux 打包格式改声明为 deb 并重跑构建（`polypack-targets` 内建默认 appimage，
/// 声明是格式矩阵的唯一开关）。模板漂移（找不到 `format = "appimage"`）时直接红，避免静默退化。
fn declare_linux_deb(workdir: &Path) {
    let toml = workdir.join("polypack.toml");
    let text = std::fs::read_to_string(&toml).expect("读取 polypack.toml");
    let swapped = text.replace("format = \"appimage\"", "format = \"deb\"");
    assert_ne!(
        swapped, text,
        "初始化模板未找到 linux 的 format = \"appimage\"（模板漂移？）"
    );
    std::fs::write(&toml, swapped).expect("写入 polypack.toml");
    eprintln!("NOTE linux：重跑构建（[targets.linux].format = appimage → deb）走真 deb 链路");
    let build = polypack(&["build", "--target", "linux", "--json"], workdir);
    assert!(
        build.status.success(),
        "deb 声明重跑 `polypack build --target linux` 失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );
}

/// deb 控制段必备字段（Debian Policy 5.3：Package / Version / Architecture）。
fn assert_deb_control(fields: &str, source: &str) {
    for key in ["Package", "Version", "Architecture"] {
        assert!(
            fields.contains(key),
            "{source} 的 deb control 缺少 {key} 字段：{fields}"
        );
    }
    eprintln!("OK linux：deb 控制字段（Package / Version / Architecture）来自 {source}");
}

// ---------------------------------------------------------------- real 模式工具

/// `polypack build` 的产物快照（写入方见 `crates/polypack-cli/src/build.rs::snapshot_artifacts`）：
/// `<root>/.polypack/artifacts/<target>/build-<millis>/{core,dist}`，取最新一份。
fn snapshot_of(workdir: &Path, target: &str) -> PathBuf {
    let root = workdir.join(".polypack").join("artifacts").join(target);
    let mut snapshots: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    snapshots.sort();
    snapshots.pop().unwrap_or_else(|| {
        panic!(
            "{target}：未发现构建快照（{}）—— polypack build 报成功却无产物",
            root.display()
        )
    })
}

/// 裸产物契约断言：快照内部清单对照金样 `bare-product` 条目。
fn assert_bare_snapshot(platform: &str, workdir: &Path, golden: &serde_json::Value) {
    let snapshot = snapshot_of(workdir, platform);
    let contract = artifact_of(golden, "bare-product");
    let description = real_description(golden, &contract, directory_entries(&snapshot), &snapshot);
    validate_artifact(&description, platform).unwrap_or_else(|errors| {
        panic!(
            "{platform} 裸产物快照结构断言失败（{}）：{errors:?}",
            snapshot.display()
        )
    });
    eprintln!("OK {platform}：裸产物快照 {} 通过契约", snapshot.display());
}

/// package 阶段产物深检（有则深检）：结构描述对照金样；打包器附带检查器缺失时只报存在。
fn packaged_check(
    platform: &str,
    golden: &serde_json::Value,
    artifact: &serde_json::Value,
    found: &Path,
    format: &str,
) {
    match artifact_entries(found, format) {
        Ok(entries) => {
            let description = real_description(golden, artifact, entries, found);
            validate_artifact(&description, platform).unwrap_or_else(|errors| {
                panic!(
                    "{platform} {format} 产物结构断言失败（{}）：{errors:?}",
                    found.display()
                )
            });
            eprintln!("OK {platform}：{format} 产物 {} 通过契约", found.display());
        }
        Err(skip) => eprintln!(
            "NOTE {platform}：{skip}；只断言产物存在（{}）",
            found.display()
        ),
    }
}

/// 实际产物的内部清单：目录（AppDir / 静态站 / deb 暂存树 / 快照）→ 递归相对路径；
/// deb → `dpkg-deb --contents`；单文件安装器（msi / dmg / appimage）→ 需打包器附带检查器，
/// 返回 skip 原因。
fn artifact_entries(artifact: &Path, format: &str) -> Result<Vec<String>, String> {
    if artifact.is_dir() {
        return Ok(directory_entries(artifact));
    }
    match format {
        "deb" => deb_entries(artifact),
        other => Err(format!(
            "{other} 为单文件安装器，内部结构深检需打包器附带检查器（解包 / 挂载）"
        )),
    }
}

/// deb 内容清单（`./` 前缀与尾斜杠归一化后的相对路径）。
/// 只列数据段（data.tar）：控制段不在 `--contents` 输出里（实测命中 0），故金样 entries
/// 不得声明 `DEBIAN/control`（恒不满足）；控制段存在性由 `--field` 三字段断言兜住。
fn deb_entries(deb: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("dpkg-deb")
        .arg("--contents")
        .arg(deb)
        .output()
        .map_err(|err| format!("dpkg-deb 不可用（{err}）"))?;
    if !output.status.success() {
        return Err(format!(
            "dpkg-deb --contents 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(5))
        .map(|path| path.trim_start_matches("./").to_string())
        .filter(|path| !path.is_empty())
        .collect())
}

/// 目录产物的递归相对清单（`/` 分隔，目录不带尾斜杠）。
fn directory_entries(root: &Path) -> Vec<String> {
    let mut stack = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            let rel = match path.strip_prefix(root) {
                Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if path.is_dir() {
                stack.push(path);
            }
            entries.push(rel);
        }
    }
    entries.sort();
    entries
}

/// 由真实产物构造校验器输入；产物清单里含 `manifest.webmanifest`（PWA 契约）时并入其实际键。
fn real_description(
    golden: &serde_json::Value,
    artifact: &serde_json::Value,
    entries: Vec<String>,
    found: &Path,
) -> serde_json::Value {
    let manifest = entries
        .iter()
        .find(|entry| entry.ends_with("manifest.webmanifest"))
        .map(|entry| found.join(entry));
    let mut description = serde_json::json!({
        "target": golden["target"],
        "builder": golden["builder"],
        "format": artifact["format"],
        "extension": artifact["extension"],
        "verifiers": golden["verifiers"],
        "entries": entries,
    });
    if let Some(path) = manifest
        && let Ok(text) = std::fs::read_to_string(path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(object) = value.as_object()
    {
        description["manifest_keys"] = serde_json::json!(object.keys().collect::<Vec<_>>());
    }
    description
}

/// 按契约扩展名递归发现产物（大小写不敏感：`.AppImage`）。
///
/// shortcut: 同名候选取最近写入的一份；多产物目标靠扩展名区分，打包器接线后随产物布局调整。
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
                .to_string();
            if name.to_lowercase().ends_with(&suffix) {
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

/// AppDir 目录（`*.AppDir`，appimagetool 缺失时打包器的降级产物）。
fn find_appdir(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_dir() {
                if path.extension().is_some_and(|ext| ext == "AppDir") {
                    found.push(path.clone());
                }
                stack.push(path);
            }
        }
    }
    found.sort();
    found.pop()
}

/// 按「相对标记路径」定位产物目录（`DEBIAN/control` → deb 暂存树，`integrity.json` →
/// web 静态站）：产物目录命名随打包器演进，按内容定位比按名字稳。取最浅者
/// （`packages/` 下的是原件，快照里的 `packages/` 是其副本）。
fn find_tree(root: &Path, marker: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            if child.path().is_dir() {
                stack.push(child.path());
            }
        }
        if dir.join(marker).is_file() {
            found.push(dir);
        }
    }
    found.into_iter().min_by_key(|dir| dir.components().count())
}
