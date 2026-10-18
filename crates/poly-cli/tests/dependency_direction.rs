//! T-M0-01 验收：workspace 依赖方向约束（ROADMAP §4.1）。
//!
//! 通过 `cargo metadata` 读取真实依赖图，断言每条 crate 间依赖都落在白名单内：
//! 叶层（targets / toolchain / cache）不依赖任何 poly-* crate；cli 是唯一聚合入口。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// 允许的依赖边：crate → 其被允许的 poly-* 依赖集合。
const ALLOWED: &[(&str, &[&str])] = &[
    (
        "poly-cli",
        &[
            "poly-core",
            "poly-targets",
            "poly-toolchain",
            "poly-bundle",
            "poly-sign",
            "poly-publish",
            "poly-cache",
            "poly-plugin",
        ],
    ),
    (
        "poly-core",
        &["poly-targets", "poly-toolchain", "poly-cache"],
    ),
    ("poly-bundle", &["poly-core", "poly-targets"]),
    ("poly-sign", &["poly-core"]),
    ("poly-publish", &["poly-core"]),
    // T-M8-01：插件宿主需要 JobSpec / PluginGrant / Diagnostic 等核心类型；
    // 方向仍是 core ← plugin（core 不反向依赖插件），无环。
    ("poly-plugin", &["poly-core", "poly-targets"]),
    ("poly-targets", &[]),
    ("poly-toolchain", &[]),
    ("poly-cache", &[]),
];

#[test]
fn workspace_dependency_direction() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/poly-cli 上溯两级为 workspace 根")
        .to_path_buf();

    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .expect("cargo metadata 可执行");
    assert!(output.status.success(), "cargo metadata 失败: {output:?}");

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata 输出为合法 JSON");

    let allowed: BTreeMap<&str, &[&str]> = ALLOWED.iter().copied().collect();

    let packages = metadata["packages"].as_array().expect("packages 数组");
    let mut checked = 0usize;
    for pkg in packages {
        let name = pkg["name"].as_str().expect("crate 名");
        let Some(perms) = allowed.get(name) else {
            continue; // 非 workspace 成员（理论上 --no-deps 下不会出现）
        };
        for dep in pkg["dependencies"].as_array().expect("dependencies 数组") {
            let dep_name = dep["name"].as_str().expect("依赖名");
            if !allowed.contains_key(dep_name) {
                continue; // 外部依赖交给 cargo-deny（T-M0-11）
            }
            assert!(
                perms.contains(&dep_name),
                "{name} → {dep_name} 违反依赖方向约束（ROADMAP §4.1）；\
                 允许的依赖集: {perms:?}"
            );
            checked += 1;
        }
    }

    assert!(
        checked >= 3,
        "至少应检查到若干 poly-* 间的依赖边（实际 {checked}），依赖图可能被误读"
    );
}
