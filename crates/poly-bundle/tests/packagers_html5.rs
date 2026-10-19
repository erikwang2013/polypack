//! HTML5 L2 平台打包器契约测试：TikTok Minis / Facebook Instant Games。
//!
//! 真跑面：UI dist → 包根平台配置（+ TikTok 资源清单）→ 确定性 zip →
//! 与 verify 腿**同一份**结构检查过检 → 二跑字节一致 → notes 如实带平台侧 SKIP 句。
//! 另验缺输入 / 缺入口与结构检查的坏件面。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::{
    BundleInputs, ProductInfo, check_facebook_instant_games_structure, check_tiktok_minis_structure,
};
use poly_targets::{Registry, TargetSpec};

const TIKTOK: &str = "tiktok-minis";
const FACEBOOK: &str = "facebook-instant-games";

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "poly-bundle-html5-{}-{tag}-{seq}",
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

fn spec(id: &str) -> TargetSpec {
    Registry::builtin()
        .get(id)
        .unwrap_or_else(|| panic!("内建 {id} 目标"))
        .clone()
}

/// 平台要求的入口 / SDK 引用 + 子目录资产（含应被资源清单排除的 `.map` 与无扩展名文件）。
fn ui_dist(temp: &Temp, id: &str) -> PathBuf {
    let dist = temp.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist/assets");
    let sdk = if id == TIKTOK {
        "<script>TTMinis.init({clientKey:\"fixture\"});</script>"
    } else {
        "<script src=\"https://connect.facebook.net/en_US/fbinstant.latest.js\"></script>"
    };
    std::fs::write(
        dist.join("index.html"),
        format!(
            "<!doctype html><html><head><title>Pax</title>{sdk}</head><body><div id=\"app\"></div></body></html>"
        ),
    )
    .expect("写 index.html");
    std::fs::write(dist.join("assets/app.js"), "console.log('pax');").expect("写资产");
    std::fs::write(dist.join("assets/app.js.map"), "{}").expect("写 source map");
    std::fs::write(dist.join("assets/NOTICE"), "license").expect("写无扩展名文件");
    dist
}

#[test]
fn tiktok_minis_builds_a_self_checked_deterministic_zip() {
    let temp = Temp::new("tiktok-real-run");
    let out = temp.join("out");
    let dist = ui_dist(&temp, TIKTOK);
    let spec = spec(TIKTOK);
    assert_eq!(spec.package.format, "zip");
    assert_eq!(spec.verify, vec!["tiktok-minis-structure-check"]);
    let packager = poly_bundle::for_spec(&spec).expect("tiktok-minis 应有内建打包器");
    assert_eq!((packager.id(), packager.format()), (TIKTOK, "zip"));

    let mut inputs = BundleInputs::new(product(), &out);
    inputs.ui_dist = Some(dist);
    let artifact = packager
        .package(&inputs, &spec)
        .expect("TikTok Minis 打包应真跑成功");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.zip"));
    assert_eq!(artifact.kind, poly_bundle::ArtifactKind::File);

    let structure = check_tiktok_minis_structure(&artifact.path).expect("产物必须过同一份结构检查");
    assert_eq!(
        structure.entries,
        [
            "assets/NOTICE",
            "assets/app.js",
            "assets/app.js.map",
            "index.html",
            "minis.config.json",
            "minis.manifest.json",
        ]
        .map(String::from),
        "zip 条目 = dist 整树 + 包根配置 / 清单（排序）"
    );
    assert_eq!(structure.html_entry, "index.html");
    assert_eq!(structure.output_dir, ".");
    assert_eq!(
        structure.resources, 2,
        "index.html + assets/app.js（.map / 无扩展名不计）"
    );

    let config = read_json(&artifact.path, "minis.config.json");
    assert!(config.get("dev").is_none(), "build 输出必须删 dev 块");
    assert_eq!(config["build"]["htmlEntry"], "index.html");
    assert_eq!(config["build"]["outputDir"], ".");
    let manifest = read_json(&artifact.path, "minis.manifest.json");
    assert_eq!(
        manifest["resource_list"],
        serde_json::json!([
            {"type": "file", "name": "index.html"},
            {"type": "folder", "name": "assets", "children": [
                {"type": "file", "name": "app.js"},
            ]},
        ])
    );

    let first = std::fs::read(&artifact.path).expect("读首跑产物");
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(first, std::fs::read(&again.path).expect("读二跑产物"));
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("SKIP-with-print") && note.contains("TikTok")),
        "notes 须如实注明平台侧 SKIP：{:?}",
        artifact.notes
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("tiktok-minis-structure-check")),
        "notes 须点名 verify 腿：{:?}",
        artifact.notes
    );
}

#[test]
fn facebook_instant_games_builds_a_self_checked_deterministic_zip() {
    let temp = Temp::new("facebook-real-run");
    let out = temp.join("out");
    let dist = ui_dist(&temp, FACEBOOK);
    let spec = spec(FACEBOOK);
    assert_eq!(spec.package.format, "zip");
    assert_eq!(spec.verify, vec!["facebook-instant-games-structure-check"]);
    let packager = poly_bundle::for_spec(&spec).expect("facebook-instant-games 应有内建打包器");
    assert_eq!((packager.id(), packager.format()), (FACEBOOK, "zip"));

    let mut inputs = BundleInputs::new(product(), &out);
    inputs.ui_dist = Some(dist);
    let artifact = packager
        .package(&inputs, &spec)
        .expect("Facebook Instant Games 打包应真跑成功");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.zip"));

    let structure =
        check_facebook_instant_games_structure(&artifact.path).expect("产物必须过同一份结构检查");
    assert_eq!(
        structure.entries,
        [
            "assets/NOTICE",
            "assets/app.js",
            "assets/app.js.map",
            "fbapp-config.json",
            "index.html",
        ]
        .map(String::from)
    );
    assert_eq!(structure.platform_version, "RICH_GAMEPLAY");
    assert_eq!(structure.orientation, "LANDSCAPE");
    assert_eq!(structure.navigation_menu_version, "NAV_FLOATING");

    let config = read_json(&artifact.path, "fbapp-config.json");
    assert_eq!(config["instant_games"]["platform_version"], "RICH_GAMEPLAY");
    assert_eq!(config["instant_games"]["orientation"], "LANDSCAPE");
    assert_eq!(
        config["instant_games"]["navigation_menu_version"],
        "NAV_FLOATING"
    );

    let first = std::fs::read(&artifact.path).expect("读首跑产物");
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(first, std::fs::read(&again.path).expect("读二跑产物"));
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("SKIP-with-print") && note.contains("Meta")),
        "notes 须如实注明平台侧 SKIP：{:?}",
        artifact.notes
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("facebook-instant-games-structure-check")),
        "notes 须点名 verify 腿：{:?}",
        artifact.notes
    );
}

#[test]
fn html5_packagers_require_ui_dist_and_entry() {
    for id in [TIKTOK, FACEBOOK] {
        let temp = Temp::new(&format!("{id}-missing-input"));
        let out = temp.join("out");
        let spec = spec(id);
        let packager = poly_bundle::for_spec(&spec).unwrap_or_else(|| panic!("{id} 打包器"));

        let inputs = BundleInputs::new(product(), &out);
        let err = packager
            .package(&inputs, &spec)
            .expect_err("缺 ui_dist 必须报错");
        assert_eq!(err.code(), poly_bundle::codes::E_BND_0002, "{id}: {err}");
        assert!(err.to_string().contains("ui_dist"), "{id}: {err}");

        let dist = temp.join("dist-empty");
        std::fs::create_dir_all(&dist).expect("建空 dist");
        let mut inputs = BundleInputs::new(product(), &out);
        inputs.ui_dist = Some(dist);
        let err = packager
            .package(&inputs, &spec)
            .expect_err("缺入口必须报错");
        assert_eq!(err.code(), poly_bundle::codes::E_BND_0003, "{id}: {err}");
        assert!(err.to_string().contains("index.html"), "{id}: {err}");
    }
}

#[test]
fn tiktok_structure_check_reports_every_bad_package() {
    let temp = Temp::new("tiktok-negatives");
    let index = temp.join("index.html");
    std::fs::write(
        &index,
        "<!doctype html><html><body><script>TTMinis.init({});</script></body></html>",
    )
    .expect("写入口");
    let config = temp.join("minis.config.json");
    std::fs::write(
        &config,
        r#"{"build":{"htmlEntry":"index.html","outputDir":"."}}"#,
    )
    .expect("写配置");
    let manifest = temp.join("minis.manifest.json");
    std::fs::write(
        &manifest,
        r#"{"name":"minis.manifest.json","resource_list":[{"type":"file","name":"index.html"}]}"#,
    )
    .expect("写清单");
    let check = |name: &str, entries: &[(String, PathBuf)]| {
        let path = temp.join(name);
        poly_bundle::zipwrite::write_zip(entries, &path).expect("合成包");
        let err = check_tiktok_minis_structure(&path).expect_err("坏包必须硬失败");
        assert_eq!(err.code(), poly_bundle::codes::E_BND_0006, "{name}: {err}");
        err.to_string()
    };

    // 缺 index.html。
    let err = check(
        "no-index.zip",
        &[
            ("minis.config.json".to_string(), config.clone()),
            ("minis.manifest.json".to_string(), manifest.clone()),
        ],
    );
    assert!(err.contains("index.html"), "{err}");

    // config 带 dev 块（源配置形态，不是 CLI build 输出）。
    let with_dev = temp.join("with-dev.json");
    std::fs::write(
        &with_dev,
        r#"{"dev":{"clientKey":"x"},"build":{"htmlEntry":"src/index.html","outputDir":"dist"}}"#,
    )
    .expect("写源配置");
    let err = check(
        "with-dev.zip",
        &[
            ("index.html".to_string(), index.clone()),
            ("minis.config.json".to_string(), with_dev),
            ("minis.manifest.json".to_string(), manifest.clone()),
        ],
    );
    assert!(err.contains("dev"), "{err}");

    // 资源清单漏列包内文件。
    let missing = temp.join("missing.js");
    std::fs::write(&missing, "console.log(1);").expect("写资产");
    let err = check(
        "manifest-mismatch.zip",
        &[
            ("assets/app.js".to_string(), missing),
            ("index.html".to_string(), index.clone()),
            ("minis.config.json".to_string(), config),
            ("minis.manifest.json".to_string(), manifest),
        ],
    );
    assert!(err.contains("资源树"), "{err}");
}

#[test]
fn facebook_structure_check_reports_every_bad_package() {
    let temp = Temp::new("facebook-negatives");
    let index = temp.join("index.html");
    std::fs::write(
        &index,
        "<!doctype html><html><body><script src=\"https://connect.facebook.net/en_US/fbinstant.latest.js\"></script></body></html>",
    )
    .expect("写入口");
    let config = temp.join("fbapp-config.json");
    std::fs::write(
        &config,
        r#"{"instant_games":{"platform_version":"RICH_GAMEPLAY","orientation":"LANDSCAPE","navigation_menu_version":"NAV_FLOATING"}}"#,
    )
    .expect("写配置");
    let check = |name: &str, entries: &[(String, PathBuf)]| {
        let path = temp.join(name);
        poly_bundle::zipwrite::write_zip(entries, &path).expect("合成包");
        let err = check_facebook_instant_games_structure(&path).expect_err("坏包必须硬失败");
        assert_eq!(err.code(), poly_bundle::codes::E_BND_0006, "{name}: {err}");
        err.to_string()
    };

    // 缺包根 index.html。
    let err = check(
        "no-index.zip",
        &[("fbapp-config.json".to_string(), config.clone())],
    );
    assert!(err.contains("index.html"), "{err}");

    // orientation 非法。
    let bad_orientation = temp.join("bad-orientation.json");
    std::fs::write(
        &bad_orientation,
        r#"{"instant_games":{"platform_version":"RICH_GAMEPLAY","orientation":"SIDEWAYS","navigation_menu_version":"NAV_FLOATING"}}"#,
    )
    .expect("写坏配置");
    let err = check(
        "bad-orientation.zip",
        &[
            ("index.html".to_string(), index.clone()),
            ("fbapp-config.json".to_string(), bad_orientation),
        ],
    );
    assert!(err.contains("orientation"), "{err}");

    // 缺 navigation_menu_version。
    let missing_field = temp.join("missing-field.json");
    std::fs::write(
        &missing_field,
        r#"{"instant_games":{"platform_version":"RICH_GAMEPLAY","orientation":"LANDSCAPE"}}"#,
    )
    .expect("写坏配置");
    let err = check(
        "missing-field.zip",
        &[
            ("index.html".to_string(), index),
            ("fbapp-config.json".to_string(), missing_field),
        ],
    );
    assert!(err.contains("navigation_menu_version"), "{err}");
}

fn read_json(package: &std::path::Path, name: &str) -> serde_json::Value {
    let text = poly_bundle::zipwrite::entry_text(package, name)
        .expect("读条目")
        .unwrap_or_else(|| panic!("包根必须有 {name}"));
    serde_json::from_str(&text).expect("条目必须是合法 JSON")
}
