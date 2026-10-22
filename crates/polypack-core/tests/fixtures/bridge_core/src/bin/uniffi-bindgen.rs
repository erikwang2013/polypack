//! uniffi 官方推荐的自托管生成器入口：`UniffiEngine::discover` 的第三顺位。
//! 版本与夹具的 uniffi 依赖同锁一处，生成结果与运行时不会版本错配。

fn main() {
    uniffi::uniffi_bindgen_main()
}
