//! 把 wit-bindgen 生成的 core wasm 封装为组件模型（等价 `wasm-tools component new`）。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(input), Some(output)) => (input, output),
        _ => return Err("用法: polypack-componentize <input.wasm> <output.wasm>".into()),
    };
    let module = std::fs::read(&input)?;
    let component = wit_component::ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    std::fs::write(&output, component)?;
    Ok(())
}
