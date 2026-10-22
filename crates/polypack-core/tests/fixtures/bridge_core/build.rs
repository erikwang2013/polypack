//! 由 UDL 生成 Rust 侧脚手架（FFI 导出符号），与绑定同一份接口定义。

fn main() {
    uniffi::generate_scaffolding("src/pax.udl").expect("由 UDL 生成脚手架");
}
