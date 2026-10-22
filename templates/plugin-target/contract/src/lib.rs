//! target-spec/v1 宿主侧契约：用 wasmtime 加载组件，调用 `describe` / `package`。

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store};

mod bindings {
    wasmtime::component::bindgen!({ path: "../plugin/wit", world: "polypack-target" });
}

use bindings::{BuildInputs, CoreLib, PolypackTarget, PolypackTargetImports, UiBundle};

/// 与 `plugin/src/lib.rs` 的 `PLUGIN_ID` 保持一致（`polypack plugin new` 同步替换）。
pub const PLUGIN_ID: &str = "__polypack_plugin_id__";

/// 模板默认产物格式；自定义 `describe` / `package` 后同步本常量。
///
/// 这是契约测试唯一需要随插件修改的断言来源；忘了同步会让 `cargo test` 以 101 失败。
pub const EXPECTED_FORMAT: &str = "example-bin";

/// 模板默认产物字节（`verify` 的输入是 index.html / app.js / CORE 三个固定片段）。
///
/// 自定义 `package` 的输出格式后，把这里替换为新的期望字节。
pub const EXPECTED_PACKAGE: &[u8] = b"index.html\n<html>\napp.js\nJS\nCORE";

struct Host;

// `types` 接口只有数据类型，无宿主函数；bindgen 仍要求空实现。
impl bindings::polypack::target_spec::types::Host for Host {}

impl PolypackTargetImports for Host {
    fn inputs(&mut self) -> BuildInputs {
        BuildInputs {
            name: String::from("demo"),
            version: String::from("1.2.3"),
            target: String::from("linux"),
        }
    }
}

/// 契约校验结果（断言留给调用方，便于测试与 CLI 复用）。
pub struct Report {
    /// 插件声明的接口版本。
    pub spec_version: String,
    /// 插件 id。
    pub id: String,
    /// 打包产物格式。
    pub format: String,
    /// 插件返回的产物字节。
    pub bytes: Vec<u8>,
    /// 制品摘要（SRI 口径 `blake3-<hex>`）。
    pub digest: String,
}

/// 加载组件、调用 describe / package，返回原始结果。
pub fn verify(wasm: &[u8]) -> Result<Report, String> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).map_err(|err| format!("engine 初始化失败: {err}"))?;
    let component = Component::from_binary(&engine, wasm)
        .map_err(|err| format!("不是可用的组件（先跑 ./build.sh）: {err}"))?;
    let mut linker = Linker::new(&engine);
    PolypackTarget::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)
        .map_err(|err| format!("linker 初始化失败: {err}"))?;
    let mut store = Store::new(&engine, Host);
    let plugin = PolypackTarget::instantiate(&mut store, &component, &linker)
        .map_err(|err| format!("实例化失败: {err}"))?;

    let spec = plugin
        .call_describe(&mut store)
        .map_err(|err| format!("describe 调用失败: {err}"))?;

    let bundle = UiBundle {
        entry: String::from("index.html"),
        files: vec![
            (String::from("index.html"), b"<html>".to_vec()),
            (String::from("app.js"), b"JS".to_vec()),
        ],
    };
    let lib = CoreLib {
        triple: String::from("wasm32-unknown-unknown"),
        bytes: b"CORE".to_vec(),
    };
    let artifact = plugin
        .call_package(&mut store, &bundle, &lib)
        .map_err(|err| format!("package 调用失败: {err}"))?
        .map_err(|err| format!("插件返回错误 {}: {}", err.code, err.message))?;

    Ok(Report {
        spec_version: spec.spec_version,
        id: spec.id,
        format: artifact.format,
        bytes: artifact.bytes,
        digest: format!("blake3-{}", blake3::hash(wasm).to_hex()),
    })
}
