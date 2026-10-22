//! T-M0-01 验收：workspace 依赖方向约束（ROADMAP §4.1）。
//!
//! 通过 `cargo metadata` 读取真实依赖图，断言每条 crate 间依赖都落在白名单内：
//! 叶层（targets / toolchain / cache）不依赖任何 polypack-* crate；cli 是唯一聚合入口。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// 允许的依赖边：crate → 其被允许的 polypack-* 依赖集合。
const ALLOWED: &[(&str, &[&str])] = &[
    (
        "polypack-cli",
        &[
            "polypack-core",
            "polypack-targets",
            "polypack-toolchain",
            "polypack-bundle",
            "polypack-sign",
            "polypack-publish",
            "polypack-cache",
            "polypack-plugin",
        ],
    ),
    (
        "polypack-core",
        &["polypack-targets", "polypack-toolchain", "polypack-cache"],
    ),
    ("polypack-bundle", &["polypack-core", "polypack-targets"]),
    ("polypack-sign", &["polypack-core"]),
    ("polypack-publish", &["polypack-core"]),
    // T-M8-01：插件宿主需要 JobSpec / PluginGrant / Diagnostic 等核心类型；
    // T-M8-05：插件实现统一 `TargetProvider`（polypack-targets）。
    // T-M8-03：验签复用 polypack-sign（GpgSigner / ArtifactVerifier / CredentialRef 解析），
    // 不重造密码学。方向仍是 core ← sign ← plugin，无环；仍不得依赖 polypack-bundle。
    ("polypack-plugin", &["polypack-core", "polypack-sign", "polypack-targets"]),
    ("polypack-targets", &[]),
    ("polypack-toolchain", &[]),
    ("polypack-cache", &[]),
];

#[test]
fn workspace_dependency_direction() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/polypack-cli 上溯两级为 workspace 根")
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
        "至少应检查到若干 polypack-* 间的依赖边（实际 {checked}），依赖图可能被误读"
    );
}
