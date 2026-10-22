//! Snap 打包器（T-M5-03）。
//!
//! 产物 = `<out_dir>/<name>_<version>_<arch>.snap`（snapcraft）。暂存目录带
//! `snapcraft.yaml`（confinement / 接口声明）与 `files/` 载荷 + 店端 AppStream 元数据；
//! snapcraft 缺席时以 `snap-stage` 目录如实降级（结构自检照跑），不伪造 squashfs。
//!
//! 接口声明唯一来源：[`SNAP_PLUGS`]——snapcraft.yaml 的 `plugs` 与 `TargetSpec.notes`
//! 都从这里派生，结构检查拒绝多一条 / 少一条。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use super::flatpak::{check_payload_metainfo, collect_files, count_icons};
use super::{
    SNAP, SNAP_TARGETS, check_format, install_appstream, path_arg, portable_payload,
    source_date_epoch, stage_dir, with_epoch,
};
use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// Snap 打包器（snapcraft；工具缺失时产出已自检的暂存目录）。
pub struct Snap;

/// 最小接口声明（snapcraft.yaml 的 `plugs` 与 TargetSpec notes 的唯一来源）。
///
/// GNOME 扩展补齐 WebKitGTK 运行时；这些 plug 负责桌面入口与显示。
pub const SNAP_PLUGS: &[&str] = &["desktop", "wayland", "x11", "opengl"];

impl Packager for Snap {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn format(&self) -> &'static str {
        SNAP
    }

    fn serves(&self, target_id: &str) -> bool {
        SNAP_TARGETS.contains(&target_id)
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, SNAP)?;
        inputs.require_bare(SNAP)?;
        let stage = stage_dir(inputs, SNAP);
        let yaml = stage.join("snapcraft.yaml");
        let mut plan = BundlePlan::new().with_script(yaml.clone(), snapcraft_yaml(inputs)?);
        if exec::tool_present("snapcraft") {
            plan = plan.with_command(with_epoch(
                Command::new(
                    "snapcraft",
                    [
                        "pack".to_string(),
                        path_arg(&stage),
                        "--output".to_string(),
                        path_arg(&inputs.out_dir),
                    ],
                )
                // 容器 / CI 无 LXD 时的标准构建面（不拉虚拟机）。
                .with_env("SNAPCRAFT_BUILD_ENVIRONMENT", "host"),
                source_date_epoch(),
            ));
        } else {
            plan = plan.skipping(
                "snapcraft",
                "安装 snapcraft（snap install snapcraft --classic）后重试",
            );
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(SNAP)?;
        let stage = stage_dir(inputs, SNAP);
        let _ = std::fs::remove_dir_all(&stage);
        let payload = stage.join("files");
        for dir in ["bin", "lib", "share"] {
            std::fs::create_dir_all(payload.join(dir)).map_err(io_err(&payload.join(dir)))?;
        }
        portable_payload(bare, &payload)?;
        let metainfo = install_appstream(&payload.join("share"), &inputs.product)?;
        // 店端要求 snapcraft.yaml 同级带 AppStream（文件名随 snap 名）。
        let appstream = stage.join(format!("{}.metainfo.xml", snap_name(&inputs.product)?));
        fsutil::copy_file(&metainfo, &appstream, false)?;
        fsutil::write_scripts(&plan)?;
        super::normalize_modes(&payload)?;
        super::apply_epoch(&payload)?;

        if !plan.skipped.is_empty() {
            let notes = plan
                .skipped
                .iter()
                .map(|s| format!("未打包：{}（{}）", s.tool, s.reason))
                .collect();
            return fsutil::dir_artifact(&spec.id, "snap-stage", &stage, notes);
        }
        exec::run_all(&plan.commands)?;
        let out = snap_path(inputs)?;
        let structure = check_snap_structure(&stage)?;
        fsutil::file_artifact(
            &spec.id,
            SNAP,
            &out,
            vec![
                format!(
                    "结构检查：name={}，base={}，confinement={}，plugs={:?}",
                    structure.name, structure.base, structure.confinement, structure.plugs
                ),
                "安装冒烟：snap install --dangerous <snap>（接口声明见 plugs）".to_string(),
            ],
        )
    }
}

/// Snap 产物路径（`<name>_<version>_<arch>.snap`，测试按同一函数推导）。
pub(crate) fn snap_path(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    Ok(inputs.out_dir.join(format!(
        "{}_{}_{}.snap",
        snap_name(&inputs.product)?,
        inputs.product.version,
        super::deb_arch()
    )))
}

/// Snap 名（kebab；Snap Store 的硬约束：小写字母数字与连字符、≤40 字符）。
pub fn snap_name(product: &crate::inputs::ProductInfo) -> Result<String, BundleError> {
    let name = product.kebab_name();
    let valid = !name.is_empty()
        && name.len() <= 40
        && name.starts_with(|ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if !valid {
        return Err(BundleError::InvalidInput {
            packager: SNAP,
            field: "product.name",
            value: product.name.clone(),
            reason: "snap 名须为小写字母数字/-、字母或数字开头、≤40 字符".to_string(),
        });
    }
    Ok(name)
}

/// `snapcraft.yaml`：strict confinement + 预布置载荷（`plugin: dump`）。
pub fn snapcraft_yaml(inputs: &BundleInputs) -> Result<String, BundleError> {
    let product = &inputs.product;
    let name = snap_name(product)?;
    let summary = snap_summary(product);
    let mut out = String::new();
    out.push_str(&format!("name: {name}\n"));
    out.push_str("base: core24\n");
    out.push_str(&format!("version: '{}'\n", product.version));
    out.push_str(&format!("summary: {}\n", yaml_quote(&summary)));
    out.push_str(&format!(
        "description: |\n{}\n",
        product
            .description
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    ));
    out.push_str("confinement: strict\n");
    out.push_str("grade: stable\n");
    out.push_str("\napps:\n");
    out.push_str(&format!("  {name}:\n"));
    out.push_str(&format!("    command: bin/{}\n", product.binary_name()));
    out.push_str("    extensions:\n      - gnome\n");
    out.push_str("    plugs:\n");
    for plug in SNAP_PLUGS {
        out.push_str(&format!("      - {plug}\n"));
    }
    out.push_str("\nparts:\n");
    out.push_str(&format!("  {name}:\n"));
    out.push_str("    plugin: dump\n");
    out.push_str("    source: files\n");
    Ok(out)
}

/// summary：描述首行，snapcraft 上限 79 字符（超出按字符截断，不留半个字）。
fn snap_summary(product: &crate::inputs::ProductInfo) -> String {
    let line = product.description.lines().next().unwrap_or("").trim();
    let line = line.trim_end_matches(['.', '。']).trim();
    line.chars().take(79).collect()
}

/// YAML 单引号标量（单引号以 `''` 转义）。
fn yaml_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Snap 暂存目录的结构检查结果（打包器自检与 verify 腿 `snap-lint` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapStructure {
    /// `name`。
    pub name: String,
    /// `base`（如 `core24`）。
    pub base: String,
    /// `confinement`。
    pub confinement: String,
    /// `apps.<name>.plugs`（排序）。
    pub plugs: Vec<String>,
    /// 载荷文件相对路径（`files/` 前缀，排序）。
    pub files: Vec<String>,
}

/// 校验 `snapcraft.yaml` 的 confinement / 接口声明与 `files/` 载荷。
///
/// 接口集合必须恰为 [`SNAP_PLUGS`]（与 TargetSpec notes 同源）；confinement 必须 strict。
pub fn check_snap_structure(stage: &Path) -> Result<SnapStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: stage.to_path_buf(),
        reason,
    };
    let yaml_path = stage.join("snapcraft.yaml");
    let text = std::fs::read_to_string(&yaml_path).map_err(io_err(&yaml_path))?;
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{key}:")))
            .map(|value| value.trim().trim_matches('\'').to_string())
            .filter(|value| !value.is_empty())
    };
    let name = value("name").ok_or_else(|| bad("snapcraft.yaml 缺 name".to_string()))?;
    let base = value("base").ok_or_else(|| bad("snapcraft.yaml 缺 base".to_string()))?;
    if !base.starts_with("core") {
        return Err(bad(format!("base 须为 coreNN（实际 {base}）")));
    }
    if value("confinement").as_deref() != Some("strict") {
        return Err(bad("confinement 须为 strict".to_string()));
    }
    if value("grade").as_deref() != Some("stable") {
        return Err(bad("grade 须为 stable".to_string()));
    }
    let command = value("command").ok_or_else(|| bad("缺 apps.<name>.command".to_string()))?;
    if !command.starts_with("bin/") {
        return Err(bad(format!("command 须落 bin/（实际 {command}）")));
    }
    let source = value("source").ok_or_else(|| bad("parts 缺 source".to_string()))?;
    if source != "files" {
        return Err(bad(format!("source 须为 files（实际 {source}）")));
    }
    if value("plugin").as_deref() != Some("dump") {
        return Err(bad("parts 插件须为 dump（预布置载荷）".to_string()));
    }
    let actual: BTreeSet<String> = yaml_block_list(&text, "plugs").into_iter().collect();
    let expected: BTreeSet<String> = SNAP_PLUGS.iter().map(|plug| plug.to_string()).collect();
    if actual != expected {
        return Err(bad(format!(
            "plugs 必须恰为声明集（多：{:?}；缺：{:?}）",
            actual.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&actual).collect::<Vec<_>>()
        )));
    }

    let payload = stage.join("files");
    let binary = command.trim_start_matches("bin/");
    if !payload.join("bin").join(binary).is_file() {
        return Err(bad(format!("载荷缺 bin/{binary}")));
    }
    if !std::fs::read_dir(payload.join("share/applications"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().ends_with(".desktop"))
    {
        return Err(bad("载荷缺 share/applications/*.desktop".to_string()));
    }
    check_payload_metainfo(&payload)?;
    if count_icons(&payload.join("share/icons/hicolor")) == 0 {
        return Err(bad("载荷缺 share/icons/hicolor/*/apps/*.png".to_string()));
    }
    let appstream = stage.join(format!("{name}.metainfo.xml"));
    if !appstream.is_file() {
        return Err(bad(format!("缺店端元数据 {}.metainfo.xml", name)));
    }
    Ok(SnapStructure {
        name,
        base,
        confinement: "strict".to_string(),
        plugs: actual.into_iter().collect(),
        files: collect_files(&payload)?
            .into_iter()
            .map(|path| format!("files/{path}"))
            .collect(),
    })
}

/// 读取 `key:` 下的 YAML 块列表（缩进更深的 `- item` 行，遇同级/更浅即止）。
fn yaml_block_list(text: &str, key: &str) -> Vec<String> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() != format!("{key}:") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let mut items = Vec::new();
        for next in lines.by_ref() {
            let trimmed = next.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            if next.len() - trimmed.len() <= indent {
                break;
            }
            if let Some(item) = trimmed.strip_prefix("- ") {
                items.push(item.trim().to_string());
            }
        }
        return items;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::ProductInfo;

    fn product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用。".to_string(),
            homepage: Some("https://example.com".to_string()),
        }
    }

    #[test]
    fn yaml_declares_strict_confinement_and_plugs() {
        let inputs = BundleInputs::new(product(), std::env::temp_dir());
        let yaml = snapcraft_yaml(&inputs).unwrap();
        assert!(yaml.contains("name: pax-notes\n"));
        assert!(yaml.contains("confinement: strict\n"));
        assert!(yaml.contains("command: bin/pax-notes\n"));
        assert!(yaml.contains("plugin: dump\n"));
        assert!(yaml.contains("source: files\n"));
        let plugs = yaml_block_list(&yaml, "plugs");
        assert_eq!(plugs, SNAP_PLUGS);
        assert!(!yaml.contains("confinement: devmode"));
    }

    /// 结构检查拒绝多余接口（与 TargetSpec notes 同源的最小集）。
    #[test]
    fn structure_check_rejects_extra_plug() {
        let base =
            std::env::temp_dir().join(format!("polypack-snap-struct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let payload = base.join("files");
        fsutil::write_text(&payload.join("bin/pax-notes"), "#!/bin/sh\n").unwrap();
        fsutil::write_text(
            &payload.join("share/applications/pax-notes.desktop"),
            "[Desktop Entry]\nType=Application\nExec=pax-notes\nIcon=pax-notes\n",
        )
        .unwrap();
        fsutil::write_text(
            &payload.join("share/metainfo/com.polypack.paxnotes.metainfo.xml"),
            &crate::packagers::linux::appstream_metainfo(&product()).unwrap(),
        )
        .unwrap();
        fsutil::write_text(
            &payload.join("share/icons/hicolor/256x256/apps/pax.png"),
            "png",
        )
        .unwrap();
        fsutil::write_text(
            &base.join("pax-notes.metainfo.xml"),
            &crate::packagers::linux::appstream_metainfo(&product()).unwrap(),
        )
        .unwrap();
        let inputs = BundleInputs::new(product(), std::env::temp_dir());
        fsutil::write_text(
            &base.join("snapcraft.yaml"),
            &snapcraft_yaml(&inputs).unwrap(),
        )
        .unwrap();
        check_snap_structure(&base).unwrap();

        let yaml = snapcraft_yaml(&inputs)
            .unwrap()
            .replace("      - opengl\n", "      - opengl\n      - network\n");
        fsutil::write_text(&base.join("snapcraft.yaml"), &yaml).unwrap();
        assert!(check_snap_structure(&base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn snap_name_enforces_store_rules() {
        let mut product = product();
        assert_eq!(snap_name(&product).unwrap(), "pax-notes");
        product.name = "x".repeat(41);
        assert!(snap_name(&product).is_err());
    }
}
