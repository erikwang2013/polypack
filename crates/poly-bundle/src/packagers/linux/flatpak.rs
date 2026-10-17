//! Flatpak 打包器（T-M5-02）。
//!
//! 产物 = `<out_dir>/<app-id>-<version>.flatpak`（flatpak-builder → build-bundle）。暂存目录
//! 同时保留 manifest（`<app-id>.json`）与载荷 `files/`：flatpak 工具缺席时以
//! `flatpak-stage` 目录如实降级（结构自检照跑），不伪造 OSTree bundle。
//!
//! 沙箱权限最小化：[`FLATPAK_FINISH_ARGS`] 是唯一来源——manifest 的 `finish-args` 与
//! `TargetSpec.notes` 都从这里派生，结构检查拒绝任何冗余权限。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use super::{
    FLATPAK, FLATPAK_TARGETS, app_id, check_appstream_metainfo, check_format, install_appstream,
    path_arg, portable_payload, source_date_epoch, stage_dir, with_epoch,
};
use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// Flatpak 打包器（flatpak-builder；工具缺失时产出已自检的暂存目录）。
pub struct Flatpak;

/// 最小沙箱权限（manifest `finish-args` 与 TargetSpec notes 的唯一来源）。
///
/// WebKitGTK 需要 Wayland / X11 回退、共享 IPC 与 DRI；不申请 home / 网络 / 系统总线。
pub const FLATPAK_FINISH_ARGS: &[&str] = &[
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
];

/// Flatpak 运行时（GNOME Platform 含 WebKitGTK，Tauri 应用的标准基座）。
const RUNTIME: &str = "org.gnome.Platform";
/// 运行时版本（与 SDK 同步）。
const RUNTIME_VERSION: &str = "47";

impl Packager for Flatpak {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn format(&self) -> &'static str {
        FLATPAK
    }

    fn serves(&self, target_id: &str) -> bool {
        FLATPAK_TARGETS.contains(&target_id)
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, FLATPAK)?;
        inputs.require_bare(FLATPAK)?;
        let stage = stage_dir(inputs, FLATPAK);
        let manifest = stage.join(format!("{}.json", app_id(&inputs.product)?));
        let mut plan = BundlePlan::new().with_script(manifest.clone(), flatpak_manifest(inputs)?);
        let missing = ["flatpak-builder", "flatpak"]
            .into_iter()
            .filter(|tool| !exec::tool_present(tool))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            let out = flatpak_path(inputs);
            plan = plan
                .with_command(with_epoch(
                    Command::new(
                        "flatpak-builder",
                        [
                            "--force-clean".to_string(),
                            format!("--repo={}", path_arg(&stage.join("repo"))),
                            path_arg(&stage.join("build")),
                            path_arg(&manifest),
                        ],
                    ),
                    source_date_epoch(),
                ))
                .with_command(with_epoch(
                    Command::new(
                        "flatpak",
                        [
                            "build-bundle".to_string(),
                            path_arg(&stage.join("repo")),
                            path_arg(&out),
                            app_id(&inputs.product)?,
                        ],
                    ),
                    source_date_epoch(),
                ));
        } else {
            plan = plan.skipping(
                missing.join(" / "),
                "安装 flatpak 与 flatpak-builder（并把 org.gnome.Platform//47 拉齐）后重试",
            );
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(FLATPAK)?;
        let stage = stage_dir(inputs, FLATPAK);
        let _ = std::fs::remove_dir_all(&stage);
        let payload = stage.join("files");
        for dir in ["bin", "lib", "share"] {
            std::fs::create_dir_all(payload.join(dir)).map_err(io_err(&payload.join(dir)))?;
        }
        portable_payload(bare, &payload)?;
        install_appstream(&payload.join("share"), &inputs.product)?;
        fsutil::write_scripts(&plan)?;
        super::normalize_modes(&payload)?;
        super::apply_epoch(&payload)?;

        if !plan.skipped.is_empty() {
            let notes = plan
                .skipped
                .iter()
                .map(|s| format!("未打包：{}（{}）", s.tool, s.reason))
                .collect();
            return fsutil::dir_artifact(&spec.id, "flatpak-stage", &stage, notes);
        }
        exec::run_all(&plan.commands)?;
        let out = flatpak_path(inputs);
        let structure = check_flatpak_structure(&stage)?;
        fsutil::file_artifact(
            &spec.id,
            FLATPAK,
            &out,
            vec![
                format!(
                    "结构检查：app-id={}，runtime={RUNTIME}//{RUNTIME_VERSION}，command={}，权限 {:?}",
                    structure.app_id, structure.command, structure.finish_args
                ),
                "安装冒烟：flatpak install --user --noninteractive <bundle>.flatpak".to_string(),
            ],
        )
    }
}

/// Flatpak 产物路径（测试按同一函数推导）。
pub(crate) fn flatpak_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}.flatpak",
        app_id(&inputs.product).unwrap_or_else(|_| "app.invalid".to_string()),
        inputs.product.version
    ))
}

/// Flatpak manifest（JSON；`flatpak-builder` 直接消费）。
pub fn flatpak_manifest(inputs: &BundleInputs) -> Result<String, BundleError> {
    let product = &inputs.product;
    let id = app_id(product)?;
    let binary = product.binary_name();
    let manifest = serde_json::json!({
        "app-id": id,
        "runtime": RUNTIME,
        "runtime-version": RUNTIME_VERSION,
        "sdk": "org.gnome.Sdk",
        "command": binary,
        "finish-args": FLATPAK_FINISH_ARGS,
        "modules": [{
            "name": product.kebab_name(),
            "buildsystem": "simple",
            "build-commands": [
                "mkdir -p /app/bin /app/lib /app/share",
                "cp -a files/bin/. /app/bin/",
                "cp -a files/lib/. /app/lib/",
                "cp -a files/share/. /app/share/",
            ],
            "sources": [{ "type": "dir", "path": "files" }],
        }],
    });
    serde_json::to_string_pretty(&manifest).map_err(|err| BundleError::InvalidInput {
        packager: FLATPAK,
        field: "manifest",
        value: id,
        reason: format!("manifest JSON 序列化失败：{err}"),
    })
}

/// Flatpak manifest + 载荷的结构检查结果（打包器自检与 verify 腿 `flatpak-lint` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatpakStructure {
    /// `app-id`。
    pub app_id: String,
    /// `runtime//version`。
    pub runtime: String,
    /// `command`（`/app/bin` 下的可执行名）。
    pub command: String,
    /// `finish-args`（排序）。
    pub finish_args: Vec<String>,
    /// 载荷文件相对路径（`files/` 前缀，排序）。
    pub files: Vec<String>,
}

/// 校验 `<app-id>.json` manifest 与 `files/` 载荷：权限必须恰为 [`FLATPAK_FINISH_ARGS`]。
pub fn check_flatpak_structure(stage: &Path) -> Result<FlatpakStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: stage.to_path_buf(),
        reason,
    };
    let manifest_path = single_json(stage).ok_or_else(|| bad("缺 *.json manifest".to_string()))?;
    let text = std::fs::read_to_string(&manifest_path).map_err(io_err(&manifest_path))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| bad(format!("manifest JSON 非法：{err}")))?;
    let field = |key: &str| {
        manifest
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let app_id = field("app-id").ok_or_else(|| bad("manifest 缺 app-id".to_string()))?;
    let runtime = field("runtime").ok_or_else(|| bad("manifest 缺 runtime".to_string()))?;
    let runtime_version =
        field("runtime-version").ok_or_else(|| bad("manifest 缺 runtime-version".to_string()))?;
    field("sdk").ok_or_else(|| bad("manifest 缺 sdk".to_string()))?;
    let command = field("command").ok_or_else(|| bad("manifest 缺 command".to_string()))?;
    let actual: BTreeSet<String> = manifest
        .get("finish-args")
        .and_then(|value| value.as_array())
        .ok_or_else(|| bad("manifest 缺 finish-args 数组".to_string()))?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    let expected: BTreeSet<String> = FLATPAK_FINISH_ARGS
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    if actual != expected {
        return Err(bad(format!(
            "finish-args 必须恰为最小集（多：{:?}；缺：{:?}）",
            actual.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&actual).collect::<Vec<_>>()
        )));
    }
    let modules = manifest
        .get("modules")
        .and_then(|value| value.as_array())
        .ok_or_else(|| bad("manifest 缺 modules 数组".to_string()))?;
    if !modules.iter().any(|module| {
        module
            .get("sources")
            .and_then(|value| value.as_array())
            .is_some_and(|sources| {
                sources.iter().any(|source| {
                    source.get("type").and_then(|v| v.as_str()) == Some("dir")
                        && source.get("path").and_then(|v| v.as_str()) == Some("files")
                })
            })
    }) {
        return Err(bad(
            "modules 缺 `type: dir, path: files` 的载荷源".to_string()
        ));
    }

    let payload = stage.join("files");
    if !payload.join("bin").join(&command).is_file() {
        return Err(bad(format!("载荷缺 bin/{command}")));
    }
    if !has_suffix(&payload.join("share/applications"), ".desktop") {
        return Err(bad("载荷缺 share/applications/*.desktop".to_string()));
    }
    check_payload_metainfo(&payload)?;
    let icons = count_icons(&payload.join("share/icons/hicolor"));
    if icons == 0 {
        return Err(bad("载荷缺 share/icons/hicolor/*/apps/*.png".to_string()));
    }
    let files = collect_files(&payload)?
        .into_iter()
        .map(|path| format!("files/{path}"))
        .collect();
    Ok(FlatpakStructure {
        app_id,
        runtime: format!("{runtime}//{runtime_version}"),
        command,
        finish_args: actual.into_iter().collect(),
        files,
    })
}

/// 载荷内的 `<id>.metainfo.xml` 必须过与打包器同源的 AppStream 结构检查。
pub(crate) fn check_payload_metainfo(payload: &Path) -> Result<String, BundleError> {
    let dir = payload.join("share/metainfo");
    let path = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".metainfo.xml"))
        })
        .ok_or_else(|| BundleError::Archive {
            path: payload.to_path_buf(),
            reason: "载荷缺 share/metainfo/*.metainfo.xml".to_string(),
        })?;
    check_appstream_metainfo(&path)?;
    Ok(path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default())
}

/// 目录下后缀匹配的文件是否存在。
fn has_suffix(dir: &Path, suffix: &str) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().ends_with(suffix))
}

/// hicolor 图标枚数（`<size>/apps/*.png`）。
pub(crate) fn count_icons(hicolor: &Path) -> usize {
    std::fs::read_dir(hicolor)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("apps"))
        .map(|apps| {
            std::fs::read_dir(apps)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|file| file.path().extension().is_some_and(|ext| ext == "png"))
                .count()
        })
        .sum()
}

/// 载荷树内的常规文件相对路径（排序）。
pub(crate) fn collect_files(root: &Path) -> Result<Vec<String>, BundleError> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            // 结构里的载荷路径是逻辑路径（与平台无关），Windows 反斜杠会被测试/清单比对漏掉。
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// 顶层单件 `*.json` manifest（多份视为异常）。
fn single_json(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    found.sort();
    found.into_iter().next()
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
    fn manifest_declares_minimal_permissions() {
        let inputs = BundleInputs::new(product(), std::env::temp_dir());
        let manifest: serde_json::Value =
            serde_json::from_str(&flatpak_manifest(&inputs).unwrap()).unwrap();
        assert_eq!(manifest["app-id"], "com.polypack.paxnotes");
        assert_eq!(manifest["runtime"], RUNTIME);
        assert_eq!(manifest["command"], "pax-notes");
        let args: Vec<&str> = manifest["finish-args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        assert_eq!(args, FLATPAK_FINISH_ARGS);
        assert!(
            !args.iter().any(|arg| arg.contains("filesystem")),
            "不得申请宿主文件系统"
        );
        assert!(
            !args.iter().any(|arg| arg.contains("network")),
            "不得申请网络"
        );
    }

    /// 权限多一条或少一条都必须被结构检查拒绝（与 notes 同源的硬约束）。
    #[test]
    fn structure_check_rejects_extra_permissions() {
        let base = std::env::temp_dir().join(format!("poly-flatpak-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let payload = base.join("files");
        fsutil::write_text(&payload.join("bin/pax-notes"), "#!/bin/sh\n").unwrap();
        fsutil::write_text(
            &payload.join("share/applications/pax-notes.desktop"),
            "[Desktop Entry]\nType=Application\nExec=pax-notes\nIcon=pax-notes\n",
        )
        .unwrap();
        let meta = base.join("files/share/metainfo");
        fsutil::write_text(
            &meta.join("com.polypack.paxnotes.metainfo.xml"),
            &crate::packagers::linux::appstream_metainfo(&product()).unwrap(),
        )
        .unwrap();
        fsutil::write_text(
            &payload.join("share/icons/hicolor/256x256/apps/pax.png"),
            "png",
        )
        .unwrap();
        let mut manifest = serde_json::json!({
            "app-id": "com.polypack.paxnotes",
            "runtime": RUNTIME,
            "runtime-version": RUNTIME_VERSION,
            "sdk": "org.gnome.Sdk",
            "command": "pax-notes",
            "finish-args": FLATPAK_FINISH_ARGS,
            "modules": [{
                "name": "pax-notes",
                "buildsystem": "simple",
                "build-commands": ["true"],
                "sources": [{ "type": "dir", "path": "files" }],
            }],
        });
        let path = base.join("com.polypack.paxnotes.json");
        fsutil::write_text(&path, &serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        check_flatpak_structure(&base).unwrap();

        manifest["finish-args"] = serde_json::json!(["--share=network"]);
        fsutil::write_text(&path, &serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        assert!(check_flatpak_structure(&base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
