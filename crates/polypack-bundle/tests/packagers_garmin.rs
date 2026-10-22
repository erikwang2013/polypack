//! Garmin Connect IQ `.iq` 打包器契约测试（T-M7-09 的 Rust 打包器接线）。
//!
//! 真跑面：合成 `.iq`（`manifest.xml` + dummy `.prg`）→ 结构检查通过；缺 manifest /
//! 缺 `.prg` / 空 manifest → 可定位错误。编译器环境槽缺席（常态）→ `ToolMissing` +
//! 含 `monkeyc` / `CIQ_DEVKEY` 的手动命令提示，且不落 `.iq`。
//!
//! 编译器腿只在 `monkeyc` 与 `$CIQ_DEVKEY` 同时在场时真跑（最小工程 → 真产物 → 结构检查）；
//! 无 SDK 的机器打印 SKIP。确定性归 monkeyc（编译器自带时间戳 / 签名），本打包器不重打包
//! 字节，故测试锁结构检查而非二跑字节。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use polypack_bundle::packagers::garmin::GarminIq;
use polypack_bundle::{BundleError, BundleInputs, Packager, ProductInfo, check_garmin_iq_structure};
use polypack_targets::{Registry, TargetSpec};

/// 结构检查要求的最小清单面（`version="3"` + `<iq:application`）。
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<iq:manifest xmlns:iq="http://www.garmin.com/xml/connectiq" version="3">
  <iq:application id="0123456789abcdef0123456789abcdef" type="watch-app" entry="PolypackApp" name="@Strings.AppName" version="1.2.3" minSdkVersion="3.1.0">
    <iq:products>
      <iq:product id="venu2"/>
    </iq:products>
  </iq:application>
</iq:manifest>
"#;

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "polypack-bundle-garmin-{}-{tag}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        Self(dir)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "契约测试合成输入".to_string(),
        homepage: None,
    }
}

fn spec() -> TargetSpec {
    Registry::builtin()
        .get("garmin")
        .expect("内建 garmin 目标")
        .clone()
}

fn inputs(host: &Path, out: &Path) -> BundleInputs {
    let mut inputs = BundleInputs::new(product(), out);
    inputs.miniapp_host = Some(host.to_path_buf());
    inputs
}

/// 最小 Connect IQ 工程（manifest + jungle + strings + 入口 / 视图源），供 plan / 编译器腿。
fn write_project(project: &Path) {
    std::fs::create_dir_all(project.join("resources/strings")).expect("建资源目录");
    std::fs::create_dir_all(project.join("source")).expect("建源码目录");
    std::fs::write(project.join("manifest.xml"), MANIFEST).expect("写 manifest");
    std::fs::write(
        project.join("monkey.jungle"),
        "project.manifest = manifest.xml\nbase.sourcePath = source\nbase.resourcePath = resources\n",
    )
    .expect("写 jungle");
    std::fs::write(
        project.join("resources/strings/strings.xml"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<strings>\n  <string id=\"AppName\">Pax Notes</string>\n</strings>\n",
    )
    .expect("写 strings");
    std::fs::write(
        project.join("source/PolypackApp.mc"),
        "using Toybox.Application as App;\n\nclass PolypackApp extends App.AppBase {\n    function getInitialView() {\n        return [new PolypackView()];\n    }\n}\n",
    )
    .expect("写入口类");
    std::fs::write(
        project.join("source/PolypackView.mc"),
        "using Toybox.WatchUi as Ui;\nusing Toybox.Graphics as Gfx;\n\nclass PolypackView extends Ui.View {\n    function onUpdate(dc) {\n        dc.setColor(Gfx.COLOR_BLACK, Gfx.COLOR_BLACK);\n        dc.clear();\n        dc.drawText(dc.getWidth() / 2, dc.getHeight() / 2, Gfx.FONT_SMALL, \"Pax Notes\", Gfx.TEXT_JUSTIFY_CENTER | Gfx.TEXT_JUSTIFY_VCENTER);\n    }\n}\n",
    )
    .expect("写视图类");
}

/// 合成 `.iq`（zip）：按开关放入 `manifest.xml` 与一个 dummy `.prg`。
fn write_iq(dir: &Path, manifest: Option<&str>, with_prg: bool) -> PathBuf {
    let src = dir.join("iq-src");
    std::fs::create_dir_all(&src).expect("建合成输入目录");
    let mut entries = Vec::new();
    if let Some(text) = manifest {
        let path = src.join("manifest.xml");
        std::fs::write(&path, text).expect("写合成 manifest");
        entries.push(("manifest.xml".to_string(), path));
    }
    if with_prg {
        let path = src.join("venu2.prg");
        std::fs::write(&path, b"PRG\0").expect("写合成 .prg");
        entries.push(("venu2.prg".to_string(), path));
    }
    let out = dir.join("app.iq");
    polypack_bundle::zipwrite::write_zip(&entries, &out).expect("写合成 .iq");
    out
}

/// `ToolMissing` 的可定位原因。
fn assert_reason(err: &BundleError, needle: &str) {
    match err {
        BundleError::InvalidInput { field, reason, .. } => {
            assert_eq!(*field, "iq(结构自检)");
            assert!(
                reason.contains(needle),
                "结构检查原因须含 {needle:?}（实得 {reason:?}）"
            );
        }
        other => panic!("期望 InvalidInput（结构自检），实得 {other:?}"),
    }
}

#[test]
fn garmin_structure_check_accepts_manifest_and_prg() {
    let temp = Temp::new("structure-ok");
    let iq = write_iq(&temp.0, Some(MANIFEST), true);

    let structure = check_garmin_iq_structure(&iq).expect("manifest.xml + .prg 应过检");
    assert_eq!(structure.entries, ["manifest.xml", "venu2.prg"]);
    assert_eq!(structure.prg_entries, ["venu2.prg"]);
    assert!(structure.bytes > 0, "包体积须非零");
}

#[test]
fn garmin_structure_check_rejects_missing_manifest() {
    let temp = Temp::new("structure-no-manifest");
    let iq = write_iq(&temp.0, None, true);

    let err = check_garmin_iq_structure(&iq).expect_err("缺 manifest.xml 必须失败");
    assert_reason(&err, "manifest.xml");
}

#[test]
fn garmin_structure_check_rejects_missing_prg() {
    let temp = Temp::new("structure-no-prg");
    let iq = write_iq(&temp.0, Some(MANIFEST), false);

    let err = check_garmin_iq_structure(&iq).expect_err("缺 .prg 必须失败");
    assert_reason(&err, ".prg");
}

#[test]
fn garmin_structure_check_rejects_empty_manifest() {
    let temp = Temp::new("structure-empty");
    let iq = write_iq(&temp.0, Some(" \n"), true);

    let err = check_garmin_iq_structure(&iq).expect_err("空 manifest.xml 必须失败");
    assert_reason(&err, "为空");
}

/// plan 只要求格式与工程在场：编译器是环境槽，不得在 plan 阶段因缺 SDK 失败。
#[test]
fn garmin_plan_requires_project_but_not_compiler() {
    let temp = Temp::new("plan");
    let host = temp.join("host");
    let out = temp.join("out");
    let inputs = inputs(&host, &out);
    let target = spec();

    match GarminIq.plan(&inputs, &target) {
        Err(BundleError::MissingInput {
            field: "garmin.project",
            ..
        }) => {}
        other => panic!("无工程应报 MissingInput(garmin.project)，实得 {other:?}"),
    }

    write_project(&host.join("garmin"));
    GarminIq
        .plan(&inputs, &target)
        .expect("工程在场时 plan 不要求 monkeyc / devkey");

    let mut wrong = target.clone();
    wrong.package.format = "apk".to_string();
    assert!(
        matches!(
            GarminIq.plan(&inputs, &wrong),
            Err(BundleError::InvalidInput {
                field: "spec.package.format",
                ..
            })
        ),
        "格式不一致必须硬失败"
    );
}

/// 环境槽缺席（常态）：`ToolMissing` + 手动命令提示，且绝不写出 `.iq`。
#[test]
fn garmin_missing_toolchain_reports_skip_hint_and_writes_no_iq() {
    if toolchain().is_some() {
        eprintln!("SKIP：本机 monkeyc + $CIQ_DEVKEY 在场，缺席分支由编译器腿测试覆盖");
        return;
    }
    let temp = Temp::new("toolchain");
    let host = temp.join("host");
    write_project(&host.join("garmin"));
    let out = temp.join("out");
    let inputs = inputs(&host, &out);

    let err = GarminIq
        .package(&inputs, &spec())
        .expect_err("缺 monkeyc / CIQ_DEVKEY 必须 ToolMissing，不得伪造 .iq");
    match &err {
        BundleError::ToolMissing { hint, .. } => {
            for needle in ["monkeyc", "CIQ_DEVKEY", "-e -w -r -o", "monkey.jungle"] {
                assert!(hint.contains(needle), "提示须含 {needle:?}：{hint}");
            }
        }
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
    assert!(!has_iq(&out), "SKIP 时不得写出 .iq：{out:?}");
}

/// 编译器真跑（仅在 SDK + devkey 就绪时）：最小工程 → monkeyc → `.iq` → 结构检查。
#[test]
fn garmin_compiler_runs_when_monkeyc_and_devkey_are_present() {
    if toolchain().is_none() {
        eprintln!(
            "SKIP：monkeyc / $CIQ_DEVKEY 缺席（环境槽常态）——安装 Connect IQ SDK 后本测试真跑编译"
        );
        return;
    }
    let temp = Temp::new("compiler");
    let host = temp.join("host");
    write_project(&host.join("garmin"));
    let out = temp.join("out");
    let inputs = inputs(&host, &out);

    let artifact = GarminIq
        .package(&inputs, &spec())
        .unwrap_or_else(|err| panic!("monkeyc + $CIQ_DEVKEY 在场但编译失败：{err}"));
    assert_eq!(artifact.format, "iq");
    assert!(artifact.path.is_file(), "产物应落盘");
    let structure = check_garmin_iq_structure(&artifact.path).expect("真产物须过结构检查");
    assert!(
        !structure.prg_entries.is_empty(),
        "monkeyc 产物须含按设备编译的 .prg"
    );
    assert!(structure.bytes > 0);
}

/// 产物目录内是否出现 `.iq`（ToolMissing 断言用）。
fn has_iq(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "iq"))
    })
}

/// 环境槽探测（与打包器同一解析顺序；测试只读环境，不改写）。
fn toolchain() -> Option<(PathBuf, PathBuf)> {
    Some((monkeyc_path()?, devkey_path()?))
}

fn monkeyc_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CIQ_MONKEYC")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    for key in ["CIQ_HOME", "CONNECTIQ_HOME"] {
        let Some(home) = std::env::var_os(key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let bin = PathBuf::from(home).join("bin");
        for name in ["monkeyc", "monkeyc.bat", "monkeyc.exe", "monkeyc.cmd"] {
            let path = bin.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["monkeyc", "monkeyc.bat", "monkeyc.exe", "monkeyc.cmd"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn devkey_path() -> Option<PathBuf> {
    std::env::var_os("CIQ_DEVKEY")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}
