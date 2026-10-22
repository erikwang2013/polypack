//! T-M8-05 验收：同一套 provider 契约跑在内建目标与 wasm 参考插件上。
//!
//! 内置侧是 Tizen `.wgt`（确定性 zip，无需外部工具即可真跑）；插件侧是
//! `polypack-plugin` 的 `target-spec/v1` 参考组件。两边共用 [`PackageInput`] 与同一
//! `describe` / `package` 断言；格式字节差异显式记录（见文末测试）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use polypack_bundle::packagers::tizen::TizenWgt;
use polypack_bundle::provider::BuiltinProvider;
use polypack_core::PluginGrant;
use polypack_plugin::{PluginProvider, ResourceLimits, Sandbox};
use polypack_targets::provider::{PackageInput, ProviderRegistry, TargetProvider};
use polypack_targets::{Host, TargetSpec};

const REFERENCE: &[u8] =
    include_bytes!("../../polypack-plugin/tests/fixtures/reference-plugin.wasm");

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-provider-contract-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("临时目录可建");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "index.html".to_string(),
            b"<!doctype html><html><body>hi</body></html>".to_vec(),
        ),
        ("app.js".to_string(), b"console.log(1);".to_vec()),
    ]
}

fn grant() -> PluginGrant {
    PluginGrant {
        id: "reference".to_string(),
        path: "reference-plugin.wasm".to_string(),
        fs_read: Vec::new(),
        fs_write: Vec::new(),
        net: Vec::new(),
        sri: None,
        signature: None,
        trust_root: None,
    }
}

/// 同一个注册表：内建 Tizen provider + wasm 参考插件 provider。
fn registry(root: &std::path::Path) -> (ProviderRegistry, TargetSpec, TargetSpec) {
    let mut registry = ProviderRegistry::new();
    let builtin_spec = polypack_targets::builtin::builtin()
        .into_iter()
        .find(|spec| spec.id == "tizen-tv")
        .expect("内建 tizen-tv");
    registry
        .register(Box::new(
            BuiltinProvider::new(builtin_spec.clone(), Box::new(TizenWgt)).expect("内建 provider"),
        ))
        .expect("注册内建");

    let sandbox = Sandbox::new(ResourceLimits::default()).expect("沙箱可建");
    let plugin = sandbox
        .load(REFERENCE, "reference", None, root, &grant())
        .expect("参考插件可加载");
    let probe = polypack_plugin::PackageInput {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        target: "linux".to_string(),
        entry: "index.html".to_string(),
        files: Vec::new(),
        triple: "wasm32-unknown-unknown".to_string(),
        lib: Vec::new(),
    };
    let provider = PluginProvider::load(sandbox, plugin, &probe).expect("插件 describe");
    let plugin_spec = provider.describe().expect("describe");
    registry.register(Box::new(provider)).expect("注册插件");
    (registry, builtin_spec, plugin_spec)
}

/// 两类 provider 共用的完整契约断言。
fn assert_contract(registry: &ProviderRegistry, spec: &TargetSpec) {
    spec.validate().expect("provider 声明的目标必须合法");
    let input_files = files();
    let input = PackageInput {
        name: "Pax Notes",
        version: "1.2.3",
        target: &spec.id,
        entry: "index.html",
        files: &input_files,
        triple: "wasm32-unknown-unknown",
        lib: b"CORE",
        host: None,
    };
    let provider = registry
        .get(&spec.id, &spec.package.format)
        .unwrap_or_else(|| panic!("未注册（{}, {}）", spec.id, spec.package.format));
    assert_eq!(provider.describe().expect("describe"), *spec);

    let first = provider.package(&input).expect("package");
    assert_eq!(first.target, spec.id);
    assert_eq!(first.format, spec.package.format);
    assert!(first.bytes > 0, "产物不得为空");
    assert_eq!(first.bytes, first.content.len() as u64);
    assert!(!first.directory);

    let second = provider.package(&input).expect("二跑 package");
    assert_eq!(first.content, second.content, "同输入二跑必须字节一致");
}

#[test]
fn builtin_and_wasm_providers_share_one_contract() {
    let scratch = Scratch::new();
    let (registry, builtin_spec, plugin_spec) = registry(&scratch.0);
    assert_contract(&registry, &builtin_spec);
    assert_contract(&registry, &plugin_spec);

    // 插件 describe 与 target-spec/v1 规格一致（参考组件声明值）
    assert_eq!(plugin_spec.id, "linux-reference");
    assert_eq!(plugin_spec.tier, 3);
    assert_eq!(plugin_spec.runtime, polypack_targets::Runtime::Wasm);
    assert_eq!(plugin_spec.bridge, polypack_targets::Bridge::WasmBindgen);
    assert_eq!(plugin_spec.package.format, "reference-bin");
    assert_eq!(plugin_spec.package.builder, "reference-plugin");
    assert_eq!(plugin_spec.hosts, [Host::Linux, Host::Macos, Host::Win]);
}

#[test]
fn package_output_is_comparable_and_format_difference_is_documented() {
    let scratch = Scratch::new();
    let (registry, builtin_spec, plugin_spec) = registry(&scratch.0);
    let input_files = files();
    let input = PackageInput {
        name: "Pax Notes",
        version: "1.2.3",
        target: &plugin_spec.id,
        entry: "index.html",
        files: &input_files,
        triple: "wasm32-unknown-unknown",
        lib: b"CORE",
        host: None,
    };
    let plugin = registry
        .get(&plugin_spec.id, &plugin_spec.package.format)
        .expect("插件 provider")
        .package(&input)
        .expect("插件打包");
    // 参考插件契约：`<文件名>\n<内容>\n` 依序拼接，再追加核心库字节。
    assert_eq!(
        plugin.content,
        b"index.html\n<!doctype html><html><body>hi</body></html>\napp.js\nconsole.log(1);\nCORE"
    );

    // 内置侧消费**同一份 UI 字节**产出确定性 zip：格式不同（`.wgt` zip vs 裸拼接），
    // 字节不可直接比较；两边各自的产物基线与二跑确定性由本文件与
    // `polypack-bundle/tests/provider.rs`（统一入口 vs 直接打包器 + WGT 基线 SHA-256）锁定。
    let builtin_input = PackageInput {
        target: &builtin_spec.id,
        ..input
    };
    let builtin = registry
        .get(&builtin_spec.id, &builtin_spec.package.format)
        .expect("内建 provider")
        .package(&builtin_input)
        .expect("内建打包");
    assert!(
        builtin.content.starts_with(b"PK\x03\x04"),
        "`.wgt` 是 zip 容器（参考插件的 reference-bin 是裸拼接，格式差异已记录）"
    );
}
