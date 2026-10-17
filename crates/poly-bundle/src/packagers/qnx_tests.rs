//! QNX 打包器单元测试（`qnx.rs` 的子模块，见文件末尾 `#[path]` 声明）。

use super::*;
use crate::embedded::QnxEnv;
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
    let mut spec = Registry::builtin().get("qnx").unwrap().clone();
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "qnx-sdp".to_string(),
    };
    spec
}

fn inputs(with_sdp: bool) -> (TempDir, BundleInputs) {
    let guard = TempDir::new("poly-qnx-pkg");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let header = guard.path().join("polypack_engine.h");
    std::fs::write(&header, "int32_t poly_engine_abi_version(void);\n").unwrap();
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
        symbols: vec!["poly_engine_abi_version".to_string()],
        engine_crate: engine,
        host_triple: "x86_64-unknown-linux-gnu".to_string(),
        qnx: with_sdp.then(|| QnxEnv {
            root: guard.path().join("sdp"),
            host: guard.path().join("sdp/host/linux/x86_64"),
            target: guard.path().join("sdp/target/qnx7"),
        }),
        qnx_triple: Some("aarch64-unknown-nto-qnx710".to_string()),
        no_std: true,
        prebuilt: Vec::new(),
    });
    (guard, inputs)
}

#[test]
fn plan_generates_buildfile_and_cross_command() {
    let (_guard, inputs) = inputs(true);
    let plan = QnxImage.plan(&inputs, &spec(FORMAT_IMAGE)).unwrap();
    let buildfile = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("pax-notes.build"))
        .expect("buildfile 脚本");
    for marker in [
        "[image=",
        "[virtual=",
        "lib/",
        ".a",
        "include/polypack_engine.h",
    ] {
        assert!(buildfile.contents.contains(marker), "缺 {marker}");
    }
    let manifest = plan
        .scripts
        .iter()
        .find(|script| script.path.ends_with("manifest.json"))
        .unwrap();
    assert!(manifest.contents.contains("poly_engine_abi_version"));
    let command = plan.commands.first().expect("cargo 命令");
    assert_eq!(command.program, "cargo");
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == "aarch64-unknown-nto-qnx710")
    );
    assert!(command.env.iter().any(|(key, _)| key == "QNX_HOST"));
    assert!(
        command
            .env
            .iter()
            .any(|(key, value)| key == "PATH" && value.contains("usr/bin"))
    );
}

#[test]
fn plan_without_sdp_prints_skip_and_has_no_commands() {
    let (_guard, inputs) = inputs(false);
    let plan = QnxImage.plan(&inputs, &spec(FORMAT_IMAGE)).unwrap();
    assert!(plan.commands.is_empty(), "无 SDP 不得执行交叉编译");
    assert!(
        plan.skipped
            .iter()
            .any(|skip| skip.reason.contains("QNX SDP")),
        "SKIP 注记须点名 QNX SDP：{:?}",
        plan.skipped
    );
    let err = QnxImage.package(&inputs, &spec(FORMAT_IMAGE)).unwrap_err();
    assert!(matches!(err, BundleError::ToolMissing { .. }), "{err:?}");
}

#[test]
fn structure_check_accepts_generated_layout_and_rejects_drift() {
    let (_guard, inputs) = inputs(true);
    std::fs::create_dir_all(inputs.out_dir.join("qnx")).unwrap();
    std::fs::create_dir_all(inputs.out_dir.join("lib")).unwrap();
    std::fs::create_dir_all(inputs.out_dir.join("include")).unwrap();
    std::fs::write(
        inputs.out_dir.join("qnx/pax-notes.build"),
        buildfile_text(&inputs),
    )
    .unwrap();
    let embedded = inputs.embedded.as_ref().unwrap();
    std::fs::write(
        inputs.out_dir.join("qnx/manifest.json"),
        manifest_text(&inputs, embedded, "aarch64-unknown-nto-qnx710").unwrap(),
    )
    .unwrap();
    std::fs::write(inputs.out_dir.join("lib").join(LIBRARY), b"archive").unwrap();
    std::fs::write(inputs.out_dir.join("include/polypack_engine.h"), b"header").unwrap();
    let structure = check_qnx_structure(&inputs.out_dir).expect("结构检查应过");
    assert_eq!(structure.triple, "aarch64-unknown-nto-qnx710");
    assert_eq!(structure.symbols, vec!["poly_engine_abi_version"]);

    std::fs::remove_file(inputs.out_dir.join("lib").join(LIBRARY)).unwrap();
    assert!(
        check_qnx_structure(&inputs.out_dir).is_err(),
        "库缺失必须失败"
    );
}

#[test]
fn rejects_format_outside_image_library() {
    let (_guard, inputs) = inputs(true);
    let err = QnxImage
        .plan(&inputs, &spec("arxml"))
        .expect_err("非 image 格式应拒绝");
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
