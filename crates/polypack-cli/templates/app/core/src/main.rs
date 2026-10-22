//! 示例应用核心（M1 形态：宿主可执行，启动输出应用版本后退出）。
//!
//! M1-03 的 Tauri 捆绑阶段落位后，本 crate 作为宿主入口被注入 WebView 资源与前沿端产物。

fn main() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}
