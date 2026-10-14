//! 测试夹具助手：最小合法 `TargetSpec` 构造与 polypack.toml 夹具读取。

#![allow(dead_code)] // 各测试二进制只取用其中一部分助手

use std::path::PathBuf;

use poly_targets::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 最小合法目标（L1、仅 Linux 宿主、无签名）。
pub fn spec(id: &str) -> TargetSpec {
    spec_with(id, 1, vec![Host::Linux], None)
}

/// 按需覆盖 tier / 宿主 / 签名档案的最小合法目标（`validate` 通过）。
pub fn spec_with(id: &str, tier: u8, hosts: Vec<Host>, signing: Option<&str>) -> TargetSpec {
    TargetSpec {
        id: id.to_string(),
        tier,
        hosts,
        runtime: Runtime::None,
        bridge: Bridge::None,
        ui: vec!["react-dom".to_string()],
        package: PackageSpec {
            format: "tar.gz".to_string(),
            builder: "test-builder".to_string(),
        },
        signing: signing.map(str::to_string),
        verify: vec!["test-verify".to_string()],
        publish: vec!["test-store".to_string()],
        experimental: false,
        notes: None,
    }
}

/// 读取 `tests/fixtures` 下的夹具。
pub fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取夹具 {} 失败: {err}", path.display()))
}
