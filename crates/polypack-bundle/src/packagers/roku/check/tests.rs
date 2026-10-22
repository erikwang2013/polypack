//! [`check_roku_structure`] 的单元面：合成频道五件，不经 node。

use std::path::{Path, PathBuf};

use super::*;
use crate::zipwrite;

/// 合成频道五件。
fn good_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    std::fs::create_dir_all(dir).unwrap();
    let write = |name: &str, body: &str| {
        let path = dir.join(name.replace('/', "_"));
        std::fs::write(&path, body).unwrap();
        (name.to_string(), path)
    };
    vec![
        write(
            "manifest",
            "title=Pax Notes\nmajor_version=1\nminor_version=2\nbuild_version=3\n\
             ui_resolutions=fhd\n",
        ),
        write(
            "source/main.brs",
            "sub Main()\n    screen = CreateObject(\"roSGScreen\")\n    \
             scene = screen.CreateScene(\"PolypackScene\")\nend sub\n",
        ),
        write(
            "source/focusgraph.brs",
            "function ppFocusGraph() as Object\n    return \
             {\"order\":[\"counter\",\"reset\"],\"edges\":{\"counter\":{\"up\":\"counter\"},\
             \"reset\":{\"down\":\"reset\"}},\"initial\":\"counter\"}\nend function\n",
        ),
        write(
            "components/PolypackScene.xml",
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\n\
             <component name=\"PolypackScene\" extends=\"Scene\" initialFocus=\"node_counter\">\n\
             \x20 <children>\n    <Group id=\"node_counter\" translation=\"[0,0]\" \
             focusable=\"true\">\n      <Rectangle id=\"ring_counter\" translation=\"[-4,-4]\" \
             width=\"10\" height=\"10\" color=\"0xEA580CFF\" visible=\"false\" />\n    \
             </Group>\n  </children>\n</component>\n",
        ),
        write(
            "components/PolypackScene.brs",
            "sub init()\nend sub\nfunction onKeyEvent(key as String, press as Boolean) as \
             Boolean\n    m.top.findNode(\"node_counter\").setFocus(true)\n    return true\n\
             end function\n",
        ),
    ]
}

fn synthetic(dir: &Path, tag: &str, entries: Vec<(String, PathBuf)>) -> PathBuf {
    let out = dir.join(format!("{tag}.zip"));
    zipwrite::write_zip(&entries, &out).unwrap();
    out
}

/// 合规件：结构检查回读 manifest / 焦点表 / 条目面。
#[test]
fn structure_check_reads_a_good_channel_back() {
    let dir = std::env::temp_dir().join(format!("polypack-roku-good-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let package = synthetic(&dir, "good", good_entries(&dir));
    let structure = check_roku_structure(&package).expect("合规频道包应过检");
    assert_eq!(structure.title, "Pax Notes");
    assert_eq!(
        (structure.major, structure.minor, structure.build),
        (1, 2, 3)
    );
    assert_eq!(structure.initial_focus, "counter");
    assert_eq!(structure.focus_nodes, 2);
    assert_eq!(structure.entries.len(), REQUIRED_ENTRIES.len());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 缺陷必报错（Archive / E_BND_0006）：缺条目 / 版本非数字 / XML 不良构 / 根外条目。
#[test]
fn structure_check_fails_hard_on_defects() {
    let dir = std::env::temp_dir().join(format!("polypack-roku-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let mut entries = good_entries(&dir.join("missing"));
    entries.retain(|(name, _)| name != FOCUS_BRS);
    let package = synthetic(&dir, "missing", entries);
    let err = check_roku_structure(&package).expect_err("缺焦点表必须硬失败");
    assert_eq!(err.code(), crate::error::codes::E_BND_0006);
    assert!(err.to_string().contains(FOCUS_BRS), "{err}");

    let mut entries = good_entries(&dir.join("version"));
    let manifest = entries
        .iter_mut()
        .find(|(name, _)| name == MANIFEST)
        .expect("manifest 在场");
    std::fs::write(
        &manifest.1,
        "title=X\nmajor_version=1\nminor_version=x\nbuild_version=3\n",
    )
    .unwrap();
    let package = synthetic(&dir, "version", entries);
    let err = check_roku_structure(&package).expect_err("版本非数字必须硬失败");
    assert!(err.to_string().contains("minor_version"), "{err}");

    let mut entries = good_entries(&dir.join("xml"));
    let xml = entries
        .iter_mut()
        .find(|(name, _)| name == SCENE_XML)
        .expect("XML 在场");
    std::fs::write(
        &xml.1,
        "<component name=\"PolypackScene\" extends=\"Scene\"><A></B></component>",
    )
    .unwrap();
    let package = synthetic(&dir, "xml", entries);
    let err = check_roku_structure(&package).expect_err("不良构 XML 必须硬失败");
    assert!(err.to_string().contains("不良构"), "{err}");

    let mut entries = good_entries(&dir.join("root"));
    entries.push(("docs/readme.md".to_string(), entries[0].1.clone()));
    let package = synthetic(&dir, "root", entries);
    let err = check_roku_structure(&package).expect_err("根外条目必须硬失败");
    assert!(err.to_string().contains("docs"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// XML 扫描器：良构 / 自闭合 / 属性引号 / 文本转义（对齐 spike 的 `xmlWellFormed` 反例）。
#[test]
fn xml_scanner_matches_the_spike_verdicts() {
    assert!(xml_well_formed("<a><b x=\"1\"/></a>").is_ok());
    assert!(xml_well_formed("<?xml version=\"1.0\"?>\n<!-- c -->\n<a>5 &amp; 3</a>").is_ok());
    assert!(xml_well_formed("<a><b></a>").is_err());
    assert!(xml_well_formed("<a>5 & 3</a>").is_err());
    assert!(xml_well_formed("<a x=1></a>").is_err());
    assert!(xml_well_formed("<a></a>tail").is_err());
}
