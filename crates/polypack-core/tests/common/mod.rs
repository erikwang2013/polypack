//! 测试夹具助手：最小合法 `TargetSpec` 构造与 polypack.toml 夹具读取。

#![allow(dead_code)] // 各测试二进制只取用其中一部分助手

use std::path::{Path, PathBuf};

use polypack_targets::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

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

/// 写一个最小 bin crate（零依赖、离线可编译；独立 workspace 以免被上层 Cargo.toml 吸附）。
pub fn write_min_crate(root: &Path, dir: &str, name: &str) {
    let crate_dir = root.join(dir);
    std::fs::create_dir_all(crate_dir.join("src")).expect("创建 fixture 目录");
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
        ),
    )
    .expect("写 fixture Cargo.toml");
    std::fs::write(
        crate_dir.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", env!(\"CARGO_PKG_NAME\"));\n}\n",
    )
    .expect("写 fixture main.rs");
}

/// 读取 `tests/fixtures` 下的夹具（行尾归一化为 LF）。
pub fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取夹具 {} 失败: {err}", path.display()))
        // Windows 检出（autocrlf）是 CRLF：测试按 `\n` 字面量做替换与断言，先归一化
        .replace("\r\n", "\n")
}
