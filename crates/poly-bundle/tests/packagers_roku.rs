//! T-M4-15 契约测试：Roku 频道打包器真跑——复用仓库内适配器的 spike 发射器。
//!
//! 真跑面：`node <adapter>/scripts/emit.mjs` 发射生成件 → 频道 zip → 与 verify 腿**同一份**
//! [`poly_bundle::check_roku_structure`] 过检 → 金样（条目 / manifest / 焦点表）→ 二跑字节
//! 一致 → 受限能力说明书随产物。node 缺席按仓库惯例 SKIP-with-print（见 `common`），不伪造。
//! 真机侧载腿不可得（无 Roku SDK / 设备 / 模拟器）：notes 必须如实带 SKIP 句，测试不假装跑过。

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::{BundleInputs, ProductInfo, RokuInputs, check_roku_structure};
use poly_targets::Registry;

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "poly-bundle-roku-{}-{tag}-{seq}",
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

/// 仓库根（本 crate 在 `<root>/crates/poly-bundle`）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("仓库根可解析")
}

/// 真跑：复用仓库内适配器出包，结构 / 金样 / 确定性 / 能力说明书四类断言。
#[test]
fn roku_packager_builds_the_spike_channel_and_self_checks_it() {
    if common::skip_without("node") {
        return;
    }
    let root = repo_root();
    let adapter = root.join("ui/adapters/roku");
    if !adapter.join("scripts/emit.mjs").is_file() {
        eprintln!("SKIP：仓库内缺 ui/adapters/roku/scripts/emit.mjs（适配器不随 crate 发布）");
        return;
    }

    let temp = Temp::new("real-run");
    let out = temp.join("out");
    let spec = Registry::builtin()
        .get("roku")
        .expect("内建 roku 目标")
        .clone();
    assert_eq!(spec.package.format, "zip");
    assert_eq!(spec.verify, vec!["sideload-structure-check"]);
    let packager = poly_bundle::for_spec(&spec).expect("roku 目标应有内建打包器");
    assert_eq!((packager.id(), packager.format()), ("roku", "zip"));

    let mut inputs = BundleInputs::new(product(), &out);
    inputs.roku = Some(RokuInputs {
        project_root: root,
        ..Default::default()
    });

    let artifact = packager
        .package(&inputs, &spec)
        .expect("Roku 频道打包应真跑成功");
    assert_eq!(artifact.target, "roku");
    assert_eq!(artifact.format, "zip");
    assert_eq!(artifact.path, out.join("pax-notes-1.2.3.zip"));

    // 结构：与 verify 腿同一实现回读产物，对照金样（条目 / manifest / 焦点表）。
    let structure = check_roku_structure(&artifact.path).expect("打包产物必须过同一份结构检查");
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/roku.channel.json")).expect("金样 JSON 合法");
    let entries: Vec<String> = golden["entries"]
        .as_array()
        .expect("金样 entries")
        .iter()
        .map(|value| value.as_str().expect("条目名").to_string())
        .collect();
    assert_eq!(structure.entries, entries, "zip 条目须与金样一致");
    assert_eq!(
        structure.title,
        golden["manifest"]["title"].as_str().unwrap()
    );
    assert_eq!(
        (structure.major, structure.minor, structure.build),
        (1, 2, 3),
        "manifest 版本取自 polypack.toml"
    );
    assert_eq!(
        structure.focus_nodes,
        golden["focus"]["nodes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        structure.initial_focus,
        golden["focus"]["initial"].as_str().unwrap()
    );

    // 确定性：同输入二跑（暂存目录每次重建）字节一致。
    let first = std::fs::read(&artifact.path).expect("读首跑产物");
    let again = packager.package(&inputs, &spec).expect("二跑应同样成功");
    assert_eq!(
        first,
        std::fs::read(&again.path).expect("读二跑产物"),
        "同输入二跑必须字节一致"
    );

    // 受限能力说明书随产物（与 TargetSpec.experimental / notes 一致）。
    let capability = out.join("pax-notes-1.2.3-capability.md");
    let text = std::fs::read_to_string(&capability).expect("能力说明书应随产物落盘");
    assert!(text.contains("目标：`roku`"), "{text}");
    assert!(text.contains("experimental = true"), "{text}");
    assert!(text.contains("SceneGraph 子集受限渲染"), "{text}");

    // 设备腿如实注记：notes 里既有 SKIP 句也有 T-M4-12 决策引用。
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("SKIP-with-print") && note.contains("T-M4-12")),
        "notes 须如实注明真机侧载腿 SKIP：{:?}",
        artifact.notes
    );
    assert!(
        artifact
            .notes
            .iter()
            .any(|note| note.contains("capability.md")),
        "notes 须点名能力说明书：{:?}",
        artifact.notes
    );
}

/// 缺输入（`BundleInputs.roku = None`）→ 按环境性缺失报 E_BND_0002，不产出半成品包。
#[test]
fn roku_packager_reports_missing_adapter_input() {
    let temp = Temp::new("missing-input");
    let spec = Registry::builtin()
        .get("roku")
        .expect("内建 roku 目标")
        .clone();
    let packager = poly_bundle::for_spec(&spec).expect("roku 打包器");
    let inputs = BundleInputs::new(product(), temp.join("out"));
    let err = packager
        .package(&inputs, &spec)
        .expect_err("缺适配器输入必须报错");
    assert_eq!(err.code(), poly_bundle::codes::E_BND_0002);
    assert!(err.to_string().contains("roku.adapter"), "{err}");
    assert!(!temp.join("out").join("pax-notes-1.2.3.zip").exists());
}
