//! FreeBSD pkg 打包器单测（T-M5-06）：暂存树 / manifest 同源生成与结构检查硬失败。

use super::*;
use poly_targets::{Bridge, Host, PackageSpec, Runtime};

fn spec(id: &str, format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(id, 5, &[Host::Linux]);
    spec.runtime = Runtime::Native;
    spec.bridge = Bridge::TauriIpc;
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "freebsd-pkg".to_string(),
    };
    spec
}

fn inputs(tag: &str) -> (PathBuf, BundleInputs) {
    let root = std::env::temp_dir().join(format!("poly-bundle-pkg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bare = root.join("bare");
    fsutil::write_text(&bare.join("usr/bin/pax-notes"), "#!/bin/sh\nexit 0\n").unwrap();
    fsutil::write_text(&bare.join("usr/lib/pax-notes/app.js"), "console.log(1)").unwrap();
    fsutil::write_text(
        &bare.join("usr/share/applications/pax-notes.desktop"),
        "[Desktop Entry]\nName=Pax Notes\n",
    )
    .unwrap();
    let mut inputs = BundleInputs::new(
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用".to_string(),
            homepage: None,
        },
        root.join("out"),
    );
    inputs.bare = Some(crate::inputs::BareProduct {
        root: bare.clone(),
        executable: bare.join("usr/bin/pax-notes"),
        host: Host::Linux,
    });
    (root, inputs)
}

#[test]
fn generates_pkg_stage_and_manifest_matching_payload() {
    let (root, inputs) = inputs("gen");
    let artifact = FreeBsdPkg
        .package(&inputs, &spec("freebsd", FORMAT))
        .expect("pkg 暂存树应生成");
    assert!(
        artifact.format == FORMAT || artifact.format == "pkgstage",
        "产物格式：{}",
        artifact.format
    );
    let stage = stage_dir(&inputs);
    assert!(stage.join("usr/local/bin/pax-notes").is_file());
    assert!(stage.join("usr/local/lib/pax-notes/app.js").is_file());
    assert!(
        stage
            .join("usr/local/share/applications/pax-notes.desktop")
            .is_file()
    );

    let manifest = std::fs::read_to_string(manifest_path(&inputs)).unwrap();
    assert!(manifest.contains("name: \"pax-notes\""), "{manifest}");
    assert!(manifest.contains("version: \"1.2.3\""), "{manifest}");
    assert!(manifest.contains("prefix: \"/usr/local\""), "{manifest}");
    assert!(
        manifest.contains("maintainer: \"PolyPack <ports@polypack.com>\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("origin: \"misc/pax-notes\""),
        "{manifest}"
    );

    let structure =
        check_pkg_structure(&stage, &manifest_path(&inputs)).expect("自产暂存树必过结构检查");
    assert_eq!(structure.name, "pax-notes");
    assert_eq!(structure.version, "1.2.3");
    assert_eq!(structure.files.len(), 3);
    assert!(structure.files.contains(&"bin/pax-notes".to_string()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn structure_check_fails_hard_on_manifest_and_payload_drift() {
    let (root, inputs) = inputs("drift");
    FreeBsdPkg
        .package(&inputs, &spec("freebsd", FORMAT))
        .unwrap();
    let stage = stage_dir(&inputs);
    let manifest = manifest_path(&inputs);

    // 可执行缺失 → 硬失败（manifest name 与载荷不一致）。
    std::fs::remove_file(stage.join("usr/local/bin/pax-notes")).unwrap();
    let err = check_pkg_structure(&stage, &manifest).expect_err("缺可执行必须失败");
    assert_eq!(err.code(), crate::error::codes::E_BND_0006);
    assert!(err.to_string().contains("bin/pax-notes"), "{err}");

    // 缺 prefix → 硬失败。
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("prefix:", "# prefix:")).unwrap();
    let err = check_pkg_structure(&stage, &manifest).expect_err("缺 prefix 必须失败");
    assert!(err.to_string().contains("prefix"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn format_mismatch_is_rejected() {
    let (root, inputs) = inputs("format");
    let err = FreeBsdPkg
        .plan(&inputs, &spec("freebsd", "deb"))
        .expect_err("格式不符必须拒绝");
    assert_eq!(err.code(), crate::error::codes::E_BND_0003);
    let _ = std::fs::remove_dir_all(&root);
}
