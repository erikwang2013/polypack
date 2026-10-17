//! 测试公共设施：CI 三宿主上工具缺失时的**显式跳过**与仓库内夹具。
//!
//! 原则：打包工具（dpkg-deb / rsvg-convert / magick / desktop-file-validate …）不保证
//! 每个宿主都装；缺失时测试打印原因后**以通过结束**，绝不红，也绝不伪造真跑。需要真跑
//! 的宿主（CI ubuntu 腿）装齐工具后自然走真跑分支。
#![allow(dead_code)]

pub mod wear_contract;

use std::path::{Path, PathBuf};

/// 工具是否在 PATH（复用 crate 自身探测，避免第二份 PATH 逻辑）。
pub fn has(tool: &str) -> bool {
    poly_bundle::exec::tool_present(tool)
}

/// 宿主缺少该工具 → 打印 SKIP 原因并返回 true（调用方随即 `return`）。
pub fn skip_without(tool: &str) -> bool {
    if has(tool) {
        return false;
    }
    eprintln!("SKIP：宿主缺少 {tool}（该断言需真机执行；工具就绪的宿主机上会走真跑分支）");
    true
}

/// 任一工具缺失即跳过。
pub fn skip_without_any(tools: &[&str]) -> bool {
    let missing: Vec<&str> = tools.iter().copied().filter(|t| !has(t)).collect();
    if missing.is_empty() {
        return false;
    }
    eprintln!("SKIP：宿主缺少 {}（该断言需真机执行）", missing.join(" / "));
    true
}

/// 工具**存在**时跳过：用于「缺工具须如实报错」类断言，避免在装齐工具的宿主上误跑真打包。
pub fn skip_if_present(tool: &str) -> bool {
    if !has(tool) {
        return false;
    }
    eprintln!("SKIP：宿主已有 {tool}，本断言针对缺工具路径（真跑由对应宿主覆盖）");
    true
}

/// 非 Unix 宿主跳过：符号链接、POSIX shell 夹具等 Unix 语义（`fsutil::symlink` 在
/// 非 Unix 上会如实报 ToolMissing，而非静默降级）。
pub fn skip_unless_unix(what: &str) -> bool {
    if cfg!(unix) {
        return false;
    }
    eprintln!("SKIP：非 Unix 宿主不支持 {what}");
    true
}

/// 仓库内提交的测试夹具（`tests/fixtures/`）。
///
/// 素材由 T-M1-14 派生管线自 `docs/assets/pax-mark.svg` 生成一次并入库，使打包器测试
/// **不依赖宿主工具**（rsvg-convert / magick）；派生管线自身仍按工具就绪情况真跑。
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 仓库内 Pax 品牌 SVG（派生管线的单一源）。
pub fn brand_svg() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/assets/pax-mark.svg")
}
