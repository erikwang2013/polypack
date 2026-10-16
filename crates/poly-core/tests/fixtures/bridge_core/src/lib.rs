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

#[cfg(test)]
mod tests {
    //! T-M3-01 的调用对拍基准：`tests/napi_real.rs` 在真跑模式下（①）真跑这组单元测试、
    //! （②）把同一组字面量喂给 NAPI 模块 `.so` 的 node 侧断言——两侧都真执行，值对值一致。

    #[test]
    fn greet_and_add_match_the_napi_expectations() {
        assert_eq!(super::greet("world".to_string()), "hello, world");
        assert_eq!(super::add(2, 3), 5);
    }
}
