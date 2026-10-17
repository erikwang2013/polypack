//! 生成引擎层 C 头文件（T-M6-02）：
//! `cargo run -p poly-core --example gen_engine_header -- <out-dir>`。
//!
//! `out-dir` 缺省为 `crates/poly-core/include/`；头文件内容只由
//! `poly_core::engine::ffi` 的接口清单决定。

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/include").to_string());
    let path = poly_core::engine::header::write_header(std::path::Path::new(&dir))
        .expect("写引擎头文件失败");
    println!("{}", path.display());
}
