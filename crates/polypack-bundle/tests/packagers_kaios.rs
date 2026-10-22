//! KaiOS 打包器契约测试：UI dist → `manifest.webapp` + 整树的确定性 zip。
//!
//! 真跑面：打包 → 与 verify 腿**同一份** [`polypack_bundle::check_kaios_structure`] 过检 →
//! 清单字段 / 条目金样 → 二跑字节一致 → notes 如实带设备 / 商店腿 SKIP 句。
//! 另验三个必需失败面：缺 ui_dist / 缺入口 / 结构自检四类坏件。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use polypack_bundle::{BundleInputs, ProductInfo, check_kaios_structure};
use polypack_targets::Registry;

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "polypack-bundle-kaios-{}-{tag}-{seq}",
            std::process::id()
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

fn spec() -> polypack_targets::TargetSpec {
    Registry::builtin()
        .get("kaios")
        .expect("内建 kaios 目标")
        .clone()
}

/// 建一份最小 UI dist（`index.html` + 子目录资产）。
fn ui_dist(temp: &Temp) -> PathBuf {
    let dist = temp.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist/assets");
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><html><head><title>Pax</title></head><body><div id=\"app\"></div></body></html>",
    )
    .expect("写 index.html");
    std::fs::write(dist.join("assets/app.js"), "console.log('pax');").expect("写资产");
    dist
}

#[test]
fn kaios_packager_builds_a_self_checked_deterministic_webapp() {
    let temp = Temp::new("real-run");
    let out = temp.join("out");
    let dist = ui_dist(&temp);
    let spec = spec();
    assert_eq!(spec.package.format, "zip");
    assert_eq!(spec.verify, vec!["kaios-structure-check"]);
    let packager = polypack_bundle::for_spec(&spec).expect("kaios 目标应有内建打包器");
    assert_eq!((packager.id(), packager.format()), ("kaios", "zip"));

    let mut inputs = BundleInputs::new(product(), &out);
    inputs.ui_dist = Some(dist);

    let artifact = packager
        .package(&inputs, &spec)
        .expect("KaiOS 打包应真跑成功");
    assert_eq!(artifact.target, "kaios");
    assert_eq!(artifact.format, "zip");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.zip"));
    assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::File);

    // 结构：与 verify 腿同一实现回读产物，对照金样（条目 / 清单字段）。
    let structure = check_kaios_structure(&artifact.path).expect("打包产物必须过同一份结构检查");
    assert_eq!(
        structure.entries,
        ["assets/app.js", "index.html", "manifest.webapp"].map(String::from),
        "zip 条目须为 dist 整树 + 包根清单（排序）"
    );
    assert_eq!(structure.name, "Pax Notes");
    assert_eq!(structure.version, "1.2.3");
    assert_eq!(structure.launch_path, "/index.html");

    let text = polypack_bundle::zipwrite::entry_text(&artifact.path, "manifest.webapp")
        .expect("读清单")
        .expect("包根必须有 manifest.webapp");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("清单必须是合法 JSON");
    assert_eq!(manifest["name"], "Pax Notes");
    assert_eq!(manifest["version"], "1.2.3");
    assert_eq!(manifest["launch_path"], "/index.html");
    assert_eq!(manifest["developer"]["name"], "PolyPack");

    // 确定性：同输入二跑（暂存目录每次重建）字节一致。
    let first = std::fs::read(&artifact.path).expect("读首跑产物");
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(
        first,
        std::fs::read(&again.path).expect("读二跑产物"),
        "同输入二跑必须字节一致"
    );

    // 设备 / 商店腿如实注记，不假装跑过。
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("SKIP-with-print") && note.contains("KaiStore")),
        "notes 须如实注明设备 / 商店腿 SKIP：{:?}",
        artifact.notes
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("kaios-structure-check")),
        "notes 须点名 verify 腿：{:?}",
        artifact.notes
    );
}

#[test]
fn kaios_packager_requires_ui_dist_and_entry() {
    let temp = Temp::new("missing-input");
    let out = temp.join("out");
    let spec = spec();
    let packager = polypack_bundle::for_spec(&spec).expect("kaios 打包器");

    // 缺 ui_dist → E_BND_0002，不产出半成品包。
    let inputs = BundleInputs::new(product(), &out);
    let err = packager
        .package(&inputs, &spec)
        .expect_err("缺 ui_dist 必须报错");
    assert_eq!(err.code(), polypack_bundle::codes::E_BND_0002);
    assert!(err.to_string().contains("ui_dist"), "{err}");
    assert!(!out.join("pax-notes-1.2.3.zip").exists());

    // dist 缺 index.html → E_BND_0003（launch_path 无处可指）。
    let dist = temp.join("dist-empty");
    std::fs::create_dir_all(&dist).expect("建空 dist");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.ui_dist = Some(dist);
    let err = packager
        .package(&inputs, &spec)
        .expect_err("缺入口必须报错");
    assert_eq!(err.code(), polypack_bundle::codes::E_BND_0003);
    assert!(err.to_string().contains("index.html"), "{err}");
    assert!(!out.join("pax-notes-1.2.3.zip").exists());
}

#[test]
fn kaios_structure_check_reports_every_bad_package() {
    let temp = Temp::new("structure-negatives");
    let index = temp.join("index.html");
    std::fs::write(&index, "<!doctype html><title>x</title>").expect("写入口");
    let check = |name: &str, entries: &[(String, PathBuf)]| {
        let path = temp.join(name);
        polypack_bundle::zipwrite::write_zip(entries, &path).expect("合成包");
        let err = check_kaios_structure(&path).expect_err("坏包必须硬失败");
        assert_eq!(
            err.code(),
            polypack_bundle::codes::E_BND_0006,
            "{name}: {err}"
        );
        err.to_string()
    };

    // 缺清单。
    let err = check(
        "missing-manifest.zip",
        &[("index.html".to_string(), index.clone())],
    );
    assert!(err.contains("manifest.webapp"), "{err}");

    // 清单非法 JSON。
    let bad_json = temp.join("bad-json.webapp");
    std::fs::write(&bad_json, "{not json").expect("写坏 JSON");
    let err = check(
        "bad-json.zip",
        &[
            ("manifest.webapp".to_string(), bad_json),
            ("index.html".to_string(), index.clone()),
        ],
    );
    assert!(err.contains("非法 JSON"), "{err}");

    // 缺必填字段 launch_path。
    let no_launch = temp.join("no-launch.webapp");
    std::fs::write(&no_launch, r#"{"name":"X","version":"1.0.0"}"#).expect("写清单");
    let err = check(
        "no-launch.zip",
        &[
            ("manifest.webapp".to_string(), no_launch),
            ("index.html".to_string(), index.clone()),
        ],
    );
    assert!(err.contains("launch_path"), "{err}");

    // launch_path 指向包内不存在的入口。
    let dangling = temp.join("dangling.webapp");
    std::fs::write(
        &dangling,
        r#"{"name":"X","version":"1.0.0","launch_path":"/missing.html"}"#,
    )
    .expect("写清单");
    let err = check(
        "dangling.zip",
        &[
            ("manifest.webapp".to_string(), dangling),
            ("index.html".to_string(), index),
        ],
    );
    assert!(err.contains("missing.html"), "{err}");
}
