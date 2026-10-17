//! pkgsrc 打包器单测（T-M5-08）：Makefile / PLIST / DESCR 同源生成与结构检查硬失败。

use super::*;
use poly_targets::{Bridge, Host, PackageSpec, Runtime};

fn spec(id: &str, format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(id, 5, &[Host::Linux]);
    spec.runtime = Runtime::Native;
    spec.bridge = Bridge::TauriIpc;
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "pkgsrc".to_string(),
    };
    spec
}

fn inputs(tag: &str) -> (PathBuf, BundleInputs) {
    let root =
        std::env::temp_dir().join(format!("poly-bundle-pkgsrc-{tag}-{}", std::process::id()));
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
fn generates_pkgsrc_skeleton_matching_payload() {
    let (root, inputs) = inputs("gen");
    let artifact = Pkgsrc
        .package(&inputs, &spec("netbsd", FORMAT))
        .expect("pkgsrc 骨架应生成");
    assert_eq!(artifact.format, FORMAT);
    let port = port_dir(&inputs);
    assert!(port.join("Makefile").is_file());
    assert!(port.join("PLIST").is_file());
    assert!(port.join("DESCR").is_file());
    assert!(port.join("files/payload/bin/pax-notes").is_file());

    let makefile = std::fs::read_to_string(port.join("Makefile")).unwrap();
    assert!(
        makefile.contains("DISTNAME=\tpax-notes-1.2.3"),
        "{makefile}"
    );
    assert!(makefile.contains("LICENSE=\tapache-2.0"), "{makefile}");
    assert!(makefile.contains("NO_BUILD=\tyes"), "{makefile}");
    assert!(makefile.contains("do-extract:"), "{makefile}");
    assert!(makefile.contains("do-install:"), "{makefile}");
    assert!(
        makefile.contains(
            "${INSTALL_PROGRAM} \"${WRKSRC}/bin/pax-notes\" \"${DESTDIR}${PREFIX}/bin/pax-notes\""
        ),
        "{makefile}"
    );
    assert!(
        makefile.contains("INSTALLATION_DIRS+=\tshare/applications"),
        "{makefile}"
    );
    assert!(
        makefile.contains(".include \"../../mk/bsd.pkg.mk\""),
        "{makefile}"
    );

    let plist = std::fs::read_to_string(port.join("PLIST")).unwrap();
    assert_eq!(plist.lines().count(), 3, "{plist}");
    assert!(plist.contains("bin/pax-notes\n"), "{plist}");
    assert!(plist.contains("lib/pax-notes/app.js\n"), "{plist}");

    let structure = check_pkgsrc_structure(&port).expect("自产骨架必过结构检查");
    assert_eq!(structure.distname, "pax-notes-1.2.3");
    assert_eq!(structure.name, "pax-notes");
    assert_eq!(structure.version, "1.2.3");
    assert_eq!(structure.entries.len(), 3);
    let _ = std::fs::remove_dir_all(&root);
}

/// 真实 Tauri 裸产物资源目录用产品显示名（可含空格）；安装命令行必须加引号，
/// 目录不进无引号的 INSTALLATION_DIRS，PLIST 保留原路径（pkg_install 整行读入）。
#[test]
fn payload_paths_with_spaces_are_quoted_and_installed() {
    let (root, inputs) = inputs("spaces");
    let bare = inputs.bare.as_ref().expect("裸产物").root.clone();
    fsutil::write_text(&bare.join("usr/lib/Pax Notes/icons/icon.svg"), "<svg/>").unwrap();
    Pkgsrc
        .package(&inputs, &spec("netbsd", FORMAT))
        .expect("含空格的载荷应可生成");
    let port = port_dir(&inputs);
    let makefile = std::fs::read_to_string(port.join("Makefile")).unwrap();
    assert!(
        makefile.contains("${MKDIR} \"${DESTDIR}${PREFIX}/lib/Pax Notes/icons\""),
        "{makefile}"
    );
    assert!(
        makefile.contains(
            "${INSTALL_DATA} \"${WRKSRC}/lib/Pax Notes/icons/icon.svg\" \
             \"${DESTDIR}${PREFIX}/lib/Pax Notes/icons/icon.svg\""
        ),
        "{makefile}"
    );
    assert!(
        !makefile.contains("INSTALLATION_DIRS+=\tlib/Pax Notes"),
        "含空白目录不得进无引号的 INSTALLATION_DIRS：{makefile}"
    );
    let plist = std::fs::read_to_string(port.join("PLIST")).unwrap();
    assert!(plist.contains("lib/Pax Notes/icons/icon.svg\n"), "{plist}");
    let structure = check_pkgsrc_structure(&port).expect("含空格路径骨架必过结构检查");
    assert_eq!(structure.entries.len(), 4);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn structure_check_fails_hard_on_plist_drift_and_missing_fields() {
    let (root, inputs) = inputs("drift");
    Pkgsrc.package(&inputs, &spec("netbsd", FORMAT)).unwrap();
    let port = port_dir(&inputs);
    // 载荷多一个文件但 PLIST 没跟上 → 硬失败（安装树与 packing list 不一致）。
    fsutil::write_text(&port.join("files/payload/lib/pax-notes/extra.bin"), "x").unwrap();
    let err = check_pkgsrc_structure(&port).expect_err("PLIST 漏项必须失败");
    assert_eq!(err.code(), crate::error::codes::E_BND_0006);
    assert!(err.to_string().contains("extra.bin"), "{err}");

    // Makefile 缺 LICENSE → 硬失败。
    std::fs::remove_file(port.join("files/payload/lib/pax-notes/extra.bin")).unwrap();
    let makefile = std::fs::read_to_string(port.join("Makefile")).unwrap();
    std::fs::write(
        port.join("Makefile"),
        makefile.replace("LICENSE=", "# LICENSE="),
    )
    .unwrap();
    let err = check_pkgsrc_structure(&port).expect_err("缺 LICENSE 必须失败");
    assert!(err.to_string().contains("LICENSE"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn format_mismatch_is_rejected() {
    let (root, inputs) = inputs("format");
    let err = Pkgsrc
        .plan(&inputs, &spec("netbsd", "pkg"))
        .expect_err("格式不符必须拒绝");
    assert_eq!(err.code(), crate::error::codes::E_BND_0003);
    let _ = std::fs::remove_dir_all(&root);
}
