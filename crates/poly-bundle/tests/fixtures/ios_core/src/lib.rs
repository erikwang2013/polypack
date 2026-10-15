//! T-M2-14 真跑夹具：最小核心静态库（导出面仅一个函数，供 FFI 头与切片链接冒烟）。

/// 供 C ABI 头声明与链接冒烟的最小导出（真核心的导出面由 T-M2-01 生成器产出）。
#[unsafe(no_mangle)]
pub extern "C" fn pax_add(a: i32, b: i32) -> i32 {
    a + b
}
