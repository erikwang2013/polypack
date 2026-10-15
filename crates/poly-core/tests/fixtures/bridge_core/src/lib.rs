//! 夹具核心库：实现 `pax.udl` 里的两个函数（UDL 是唯一接口来源）。

uniffi::include_scaffolding!("pax");

/// 打招呼。
pub fn greet(name: String) -> String {
    format!("hello, {name}")
}

/// 相加。
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
