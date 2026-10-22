//! Garmin Connect IQ 打包器（T-M7-09 的 Rust 打包器接线）：`miniapp_host` 下的 `garmin/`
//! （或宿主根自身）须是 Connect IQ 工程（`manifest.xml` + `monkey.jungle`）。
//!
//! `monkeyc` 是**编译器环境槽**（`CIQ_MONKEYC` / `CIQ_HOME/bin/monkeyc` /
//! `CONNECTIQ_HOME/bin/monkeyc` / PATH），开发者密钥取 `CIQ_DEVKEY`（只取路径，不读内容）；
//! 任一缺席即 [`BundleError::ToolMissing`]——SKIP-with-print 并打印手动命令，绝不伪造 `.iq`。
//!
//! 产物结构：`.iq` = ZIP，根含 `manifest.xml` + ≥1 个按设备编译的 `.prg`；结构自检与 verify
//! 腿同一实现（[`check_garmin_iq_structure`]）。确定性归 monkeyc（编译器自带时间戳 / 签名），
//! 本打包器只做结构断言，不重打包、不改写字节。

use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::zipwrite;

/// 目标 id / 产物格式（与 `builtin.rs` 的 `garmin` 声明逐字一致）。
const ID: &str = "garmin";
const FORMAT: &str = "iq";

/// 能力边界文档（Monkey C 子集 / API 白名单 / 无 JS 运行时）。
const CAPABILITY_DOC: &str = "ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md";

/// Connect IQ `.iq` 打包器。
pub struct GarminIq;

impl Packager for GarminIq {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        // 计划面只验格式与工程在场：编译器是环境槽，plan 阶段不得因缺 SDK 失败。
        check_plan(inputs, spec)?;
        Ok(BundlePlan::new())
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        check_plan(inputs, spec)?;
        let project = project_root(inputs)?;
        let out = inputs.out_dir.join(format!(
            "{}-{}.iq",
            inputs.product.kebab_name(),
            inputs.product.version
        ));

        // 工具先于落盘解析：缺席时不留半成品 / 空目录（CLI 据此 SKIP-with-print）。
        let monkeyc = locate_monkeyc().ok_or_else(|| BundleError::ToolMissing {
            packager: ID,
            tool: "monkeyc".to_string(),
            hint: tool_hint(&project, &out, "未找到 Connect IQ 编译器 monkeyc"),
        })?;
        let devkey = locate_devkey(&project, &out)?;

        std::fs::create_dir_all(&inputs.out_dir).map_err(io_err(&inputs.out_dir))?;
        // 旧产物先清掉：monkeyc 失败时不能留下上一次的 .iq 冒充本次产物。
        let _ = std::fs::remove_file(&out);
        let args = vec![
            "-e".to_string(),
            "-w".to_string(),
            "-r".to_string(),
            "-o".to_string(),
            out.to_string_lossy().into_owned(),
            "-y".to_string(),
            devkey.to_string_lossy().into_owned(),
            "-f".to_string(),
            "monkey.jungle".to_string(),
        ];
        // Windows 的 monkeyc 通常是 `.bat` / `.cmd`：CreateProcess 不能直接执行批处理，
        // 需经 `cmd /C`（否则仅 Windows 宿主会报 ENOENT，Linux / macOS 不受影响）。
        let command = if is_batch_file(&monkeyc) {
            let mut wrapped = vec!["/C".to_string(), monkeyc.to_string_lossy().into_owned()];
            wrapped.extend(args);
            Command::new("cmd", wrapped)
        } else {
            Command::new(monkeyc.to_string_lossy().into_owned(), args)
        }
        .in_dir(&project);
        if let Err(err) = exec::run(&command) {
            let _ = std::fs::remove_file(&out);
            return Err(err);
        }
        if !out.is_file() {
            return Err(BundleError::Archive {
                path: out,
                reason: "monkeyc 返回 0 但未生成 .iq".to_string(),
            });
        }

        let structure = check_garmin_iq_structure(&out)?;
        let notes = vec![
            format!(
                "产物：{}（{} 条目；manifest.xml + {} 个按设备编译的 .prg，结构自检通过）",
                out.display(),
                structure.entries.len(),
                structure.prg_entries.len()
            ),
            "实验目标（experimental = true）：Monkey C 子集受限转译，核心不直接运行\
             （runtime = none）；编译器 / 开发者密钥为环境槽（$CIQ_MONKEYC 或 \
             $CIQ_HOME / $CONNECTIQ_HOME，$CIQ_DEVKEY），缺席时 SKIP-with-print 并打印手动命令"
                .to_string(),
            format!(
                "能力边界（Monkey C 子集 / API 白名单 / 无 JS 运行时）见 {CAPABILITY_DOC}；\
                 确定性归 monkeyc（编译器自带时间戳 / 签名），本打包器不改写产物字节"
            ),
        ];
        fsutil::file_artifact(ID, FORMAT, &out, notes)
    }
}

/// 打包计划校验（打包与 plan 共用）：格式 + 工程在场，不要求编译器。
fn check_plan(inputs: &BundleInputs, spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format != FORMAT {
        return Err(BundleError::InvalidInput {
            packager: ID,
            field: "spec.package.format",
            value: spec.package.format.clone(),
            reason: format!("打包器要求格式 `{FORMAT}`"),
        });
    }
    project_root(inputs)?;
    inputs.product.validate(ID)
}

/// Connect IQ 工程根：`<miniapp_host>/garmin` 优先，否则宿主根自身
/// （须含 `manifest.xml` + `monkey.jungle`）。
fn project_root(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    let host = inputs
        .miniapp_host
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: ID,
            field: "miniapp_host",
        })?;
    let nested = host.join("garmin");
    if is_project(&nested) {
        return Ok(nested);
    }
    if is_project(host) {
        return Ok(host.to_path_buf());
    }
    // 工程本身缺席 = 输入缺失（环境槽 SKIP-with-print），`--target all` 不应硬失败。
    Err(BundleError::MissingInput {
        packager: ID,
        field: "garmin.project",
    })
}

fn is_project(dir: &Path) -> bool {
    dir.join("manifest.xml").is_file() && dir.join("monkey.jungle").is_file()
}

/// `monkeyc` 解析：`$CIQ_MONKEYC`（须存在）→ `$CIQ_HOME/bin` / `$CONNECTIQ_HOME/bin` → PATH。
fn locate_monkeyc() -> Option<PathBuf> {
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
    exec::tool_path("monkeyc")
}

/// Windows 批处理（`.bat` / `.cmd`）：CreateProcess 不能直接执行，需经 `cmd /C`。
fn is_batch_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
}

/// 开发者密钥：`$CIQ_DEVKEY` 指向的文件必须存在（密钥只取路径，不读内容）。
fn locate_devkey(project: &Path, out: &Path) -> Result<PathBuf, BundleError> {
    let missing = |detail: String| BundleError::ToolMissing {
        packager: ID,
        tool: "CIQ_DEVKEY".to_string(),
        hint: tool_hint(project, out, &detail),
    };
    let Some(value) = std::env::var_os("CIQ_DEVKEY").filter(|value| !value.is_empty()) else {
        return Err(missing("未设置开发者密钥 $CIQ_DEVKEY".to_string()));
    };
    let path = PathBuf::from(value);
    if !path.is_file() {
        return Err(missing(format!(
            "$CIQ_DEVKEY 指向的开发者密钥不存在：{}",
            path.display()
        )));
    }
    Ok(path)
}

/// SKIP-with-print 提示：环境变量 + 可原样复制的手动编译命令。
fn tool_hint(project: &Path, out: &Path, missing: &str) -> String {
    format!(
        "{missing}。Connect IQ SDK：设 $CIQ_MONKEYC=<monkeyc 路径>，或 $CIQ_HOME / \
         $CONNECTIQ_HOME=<SDK 根（含 bin/monkeyc）>；开发者密钥：设 $CIQ_DEVKEY=<.der 路径>\
         （只取路径，不入库）。手动编译：cd {} && monkeyc -e -w -r -o {} -y \"$CIQ_DEVKEY\" \
         -f monkey.jungle",
        project.display(),
        out.display()
    )
}

/// `.iq` 结构自检（打包器自检与 verify 腿同一实现）：
/// ZIP 根含 `manifest.xml`（非空、`version="3"`、有 `<iq:application>`）+ ≥1 个 `.prg`。
pub fn check_garmin_iq_structure(package: &Path) -> Result<GarminIqStructure, BundleError> {
    let bad = |reason: String| BundleError::InvalidInput {
        packager: ID,
        field: "iq(结构自检)",
        value: package.display().to_string(),
        reason,
    };
    let bytes = std::fs::metadata(package).map_err(io_err(package))?.len();
    if bytes == 0 {
        return Err(bad("包体积为 0".to_string()));
    }
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    if !entries.iter().any(|entry| entry == "manifest.xml") {
        return Err(bad(
            ".iq 根缺少 manifest.xml（Connect IQ 清单必须落在包根）".to_string(),
        ));
    }
    let manifest = zipwrite::entry_text(package, "manifest.xml")?
        .ok_or_else(|| bad("manifest.xml 读不出内容".to_string()))?;
    if manifest.trim().is_empty() {
        return Err(bad("manifest.xml 为空".to_string()));
    }
    if !manifest.contains("version=\"3\"") {
        return Err(bad(
            "manifest.xml 缺 version=\"3\"（Connect IQ manifest 版本）".to_string(),
        ));
    }
    if !manifest.contains("<iq:application") {
        return Err(bad(
            "manifest.xml 缺 <iq:application> 元素（应用声明缺失）".to_string()
        ));
    }
    let prg_entries: Vec<String> = entries
        .iter()
        .filter(|entry| entry.ends_with(".prg"))
        .cloned()
        .collect();
    if prg_entries.is_empty() {
        return Err(bad(
            ".iq 缺按设备编译的 .prg（检查 manifest 的 iq:products 与 monkeyc 输出）".to_string(),
        ));
    }
    Ok(GarminIqStructure {
        entries,
        prg_entries,
        bytes,
    })
}

/// 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GarminIqStructure {
    /// `.iq` 条目名（排序）。
    pub entries: Vec<String>,
    /// 按设备编译的 `.prg` 条目名（排序同 `entries`）。
    pub prg_entries: Vec<String>,
    /// 包体积（字节）。
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_batch_files_are_detected() {
        assert!(is_batch_file(Path::new("C:/ConnectIQ/bin/monkeyc.bat")));
        assert!(is_batch_file(Path::new("monkeyc.CMD")));
        assert!(!is_batch_file(Path::new("/opt/connectiq/bin/monkeyc")));
        assert!(!is_batch_file(Path::new("monkeyc.exe")));
    }

    #[test]
    fn project_root_prefers_nested_garmin_dir() {
        let base = std::env::temp_dir().join(format!("polypack-garmin-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let host = base.join("host");
        let nested = host.join("garmin");
        std::fs::create_dir_all(&nested).expect("建工程目录");
        for dir in [&nested, &host] {
            std::fs::write(dir.join("manifest.xml"), "<iq:manifest version=\"3\"/>")
                .expect("写清单");
            std::fs::write(
                dir.join("monkey.jungle"),
                "project.manifest = manifest.xml\n",
            )
            .expect("写 jungle");
        }
        let host_inputs = BundleInputs::new(
            crate::inputs::ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "Pax".to_string(),
                homepage: None,
            },
            base.join("out"),
        );
        let mut inputs = host_inputs;
        inputs.miniapp_host = Some(host.clone());
        assert_eq!(project_root(&inputs).expect("工程根"), nested);
        std::fs::remove_dir_all(&nested).expect("删嵌套工程");
        assert_eq!(project_root(&inputs).expect("宿主根自身"), host);
        std::fs::remove_dir_all(&host).expect("删宿主工程");
        assert!(matches!(
            project_root(&inputs),
            Err(BundleError::MissingInput {
                field: "garmin.project",
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(&base);
    }
}
