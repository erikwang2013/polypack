//! AUTOSAR 打包器单元测试（`autosar.rs` 的子模块，见文件末尾 `#[path]` 声明）。

use super::*;
use crate::embedded::EmbeddedInputs;
use crate::inputs::{BundleInputs, ProductInfo};
use poly_targets::{PackageSpec, Registry};

fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Pax Notes".to_string(),
        homepage: None,
    }
}

fn spec(format: &str) -> TargetSpec {
    let mut spec = Registry::builtin().get("autosar").unwrap().clone();
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "autosar-adaptive".to_string(),
    };
    spec
}

fn fixture() -> (TempDir, BundleInputs) {
    let guard = TempDir::new("poly-autosar-pkg");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let header = guard.path().join("polypack_engine.h");
    std::fs::write(&header, "/* generated */\n").unwrap();
    let engine = guard.path().join("poly-core");
    std::fs::create_dir_all(&engine).unwrap();
    std::fs::write(
        engine.join("Cargo.toml"),
        "[package]\nname = \"poly-core\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(EmbeddedInputs {
        header,
        symbols: vec![
            "poly_engine_abi_version".to_string(),
            "poly_engine_cache_key".to_string(),
            "poly_engine_string_free".to_string(),
        ],
        engine_crate: engine,
        host_triple: "x86_64-unknown-linux-gnu".to_string(),
        qnx: None,
        qnx_triple: None,
        no_std: false,
    });
    (guard, inputs)
}

#[test]
fn arxml_maps_every_symbol_to_service_method_and_port() {
    let (_guard, inputs) = fixture();
    let embedded = inputs.embedded.as_ref().unwrap();
    let xml = arxml_text(&inputs, embedded);
    assert_eq!(blocks(&xml, "SERVICE-INTERFACE").len(), 3);
    assert_eq!(blocks(&xml, "PROVIDED-SERVICE-INSTANCE").len(), 3);
    for symbol in &embedded.symbols {
        assert!(xml.contains(&format!("<SHORT-NAME>{symbol}</SHORT-NAME>")));
        assert!(xml.contains(&format!("<SHORT-NAME>{symbol}Port</SHORT-NAME>")));
        assert!(xml.contains(&format!("/{PACKAGE}/{symbol}</PROVIDED-INTERFACE-TREF>")));
    }
}

#[test]
fn plan_emits_staticlib_command_and_schema() {
    let (_guard, inputs) = fixture();
    let plan = AutosarAdaptive.plan(&inputs, &spec(FORMAT)).unwrap();
    assert!(
        plan.scripts
            .iter()
            .any(|script| script.path.ends_with("pax-notes.arxml"))
    );
    assert!(
        plan.scripts
            .iter()
            .any(|script| script.path.ends_with(SCHEMA_FILE))
    );
    let command = plan.commands.first().expect("cargo rustc 命令");
    assert_eq!(command.program, "cargo");
    assert!(command.args.iter().any(|arg| arg == "rustc"));
    assert!(command.args.iter().any(|arg| arg == "staticlib"));
    assert!(
        !command
            .args
            .iter()
            .any(|arg| arg == "--no-default-features")
    );
}

#[test]
fn no_std_adds_no_default_features() {
    let (_guard, mut inputs) = fixture();
    inputs.embedded.as_mut().unwrap().no_std = true;
    let plan = AutosarAdaptive.plan(&inputs, &spec(FORMAT)).unwrap();
    assert!(
        plan.commands[0]
            .args
            .iter()
            .any(|arg| arg == "--no-default-features")
    );
}

#[test]
fn structure_check_accepts_fixture_and_rejects_port_drift() {
    let (_guard, inputs) = fixture();
    let parent = arxml_parent(&inputs);
    std::fs::create_dir_all(&parent).unwrap();
    let embedded = inputs.embedded.as_ref().unwrap();
    let xml = arxml_text(&inputs, embedded);
    std::fs::write(parent.join("pax-notes.arxml"), &xml).unwrap();
    std::fs::write(parent.join(SCHEMA_FILE), SCHEMA).unwrap();
    let structure = check_arxml(&inputs.out_dir).expect("结构检查应过");
    assert_eq!(structure.services, embedded.symbols);
    assert_eq!(structure.methods, embedded.symbols);
    assert_eq!(structure.ports.len(), embedded.symbols.len());

    std::fs::write(
        parent.join("pax-notes.arxml"),
        xml.replace("poly_engine_cache_keyPort", "poly_engine_cache_key_out"),
    )
    .unwrap();
    assert!(check_arxml(&inputs.out_dir).is_err(), "端口漂移必须失败");
}

#[test]
fn xmllint_validates_generated_arxml_when_present() {
    let Some(tool) = xmllint_tool() else {
        eprintln!("SKIP: xmllint 不在场（schema 校验由结构断言兜底）");
        return;
    };
    let (_guard, inputs) = fixture();
    let parent = arxml_parent(&inputs);
    std::fs::create_dir_all(&parent).unwrap();
    let embedded = inputs.embedded.as_ref().unwrap();
    let arxml = parent.join("pax-notes.arxml");
    let xsd = parent.join(SCHEMA_FILE);
    std::fs::write(&arxml, arxml_text(&inputs, embedded)).unwrap();
    std::fs::write(&xsd, SCHEMA).unwrap();
    let mut command = xmllint_command(&arxml, &xsd);
    command.program = tool.display().to_string();
    exec::run(&command).expect("生成 ARXML 应通过嵌入子集 XSD");
}

#[test]
fn rejects_other_formats() {
    let (_guard, inputs) = fixture();
    let err = AutosarAdaptive
        .plan(&inputs, &spec("image"))
        .expect_err("非 arxml 格式应拒绝");
    assert!(matches!(err, BundleError::InvalidInput { .. }), "{err:?}");
}

/// 测试用临时目录（唯一名；避免引入新依赖）。
struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}
