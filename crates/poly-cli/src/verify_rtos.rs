//! RTOS 校验器（T-M6-05 VxWorks / T-M6-06 Zephyr / T-M6-07 FreeRTOS）。
//!
//! 校验腿共用打包器的同一实现，避免「打包器自检绿、verify 另写一套」的漂移：
//! - `c-abi-symbols`：头文件声明 vs 归档定义（`poly_engine_*`）、未解析符号闭包白名单、
//!   no_std 断言、§5.3 绝对路径扫描。`nm` 缺席时告警跳过（不假装比过）。
//! - `vxworks-ld-check`：kit 结构断言始终真跑；`wr-cc` 在场时用厂商链接器跑
//!   `check.sh`（无未解析符号即链接成功），缺席按环境槽 SKIP-with-print。
//! - `zephyr-west-build`：west 模板结构 / 版本化 CONFIG 集始终真跑；west + Zephyr SDK
//!   在场时对 qemu_x86 与 qemu_cortex_m3 真跑 `west build`，并检查链接映射里引擎模块
//!   与 no_std，缺席按环境槽 SKIP-with-print。
//! - `freertos-qemu-smoke`：见 [`crate::verify_freertos`]（板级包结构真跑 + QEMU 冒烟）。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::packagers::embedded as abi;
use poly_bundle::packagers::{vxworks, zephyr};
use poly_core::diagnostics::Diagnostic;
use poly_core::events::{BuildEvent, EventSink};
use poly_core::plan::StageError;
use poly_core::{Project, codes};
use poly_targets::TargetSpec;

use crate::project;

const VXWORKS_KIT: &str = "vxworks";
const VXWORKS_LIB: &str = "vxworks/lib/libpolypack_engine.a";
const VXWORKS_HEADER: &str = "vxworks/include/polypack_engine.h";
const ZEPHYR_ROOT: &str = "west";
const ZEPHYR_HEADER: &str = "west/modules/poly_engine/include/polypack_engine.h";
const ZEPHYR_LIBS: [&str; 2] = [
    "west/modules/poly_engine/lib/x86/libpolypack_engine.a",
    "west/modules/poly_engine/lib/arm/libpolypack_engine.a",
];
pub(crate) const FREERTOS_ROOT: &str = "board";
const FREERTOS_HEADER: &str = "board/include/polypack_engine.h";
const FREERTOS_LIB: &str = "board/lib/arm/libpolypack_engine.a";

/// VxWorks 运行时（libc / 编译器运行时）允许的未解析符号；闭包里出现别的即硬失败。
const VXWORKS_ALLOWED_UNRESOLVED: &[&str] = &[
    "bcmp", "free", "memalign", "memcmp", "memcpy", "memmove", "memset", "strlen",
];
/// Zephyr 运行时（内核堆 / 控制台 / libc / ARM unwind）允许的未解析符号。
const ZEPHYR_ALLOWED_UNRESOLVED: &[&str] = &[
    "__aeabi_unwind_cpp_pr0",
    "bcmp",
    "k_aligned_alloc",
    "k_free",
    "memcmp",
    "memcpy",
    "memmove",
    "memset",
    "printk",
    "strlen",
];
/// FreeRTOS 运行时（heap_4 / 板级 panic 钩子 / ARM unwind）允许的未解析符号。
pub(crate) const FREERTOS_ALLOWED_UNRESOLVED: &[&str] = &[
    "__aeabi_unwind_cpp_pr0",
    "poly_engine_rt_panic",
    "pvPortMalloc",
    "vPortFree",
];

pub(crate) fn package_dir(project: &Project, spec: &TargetSpec) -> PathBuf {
    project::app_packages_root(project).join(&spec.id)
}

pub(crate) fn defect(spec: &TargetSpec, message: String) -> StageError {
    Diagnostic::error(codes::E_VERIFY_0003, format!("目标 {}：{message}", spec.id)).into()
}

pub(crate) fn warning(events: &mut dyn EventSink, message: &str) {
    events.emit(&BuildEvent::Warning {
        stage: "verify".to_string(),
        message: message.to_string(),
    });
}

pub(crate) fn emit_verified(events: &mut dyn EventSink, ids: &[String]) {
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
}

/// `c-abi-symbols`：ABI 符号一致性 + 未解析闭包白名单 + no_std + §5.3 路径审计。
pub(crate) fn verify_c_abi_symbols(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let package = package_dir(project, spec);
    let (header, libs, allowed): (PathBuf, Vec<PathBuf>, &[&str]) = match spec.id.as_str() {
        "vxworks" => (
            package.join(VXWORKS_HEADER),
            vec![package.join(VXWORKS_LIB)],
            VXWORKS_ALLOWED_UNRESOLVED,
        ),
        "zephyr" => (
            package.join(ZEPHYR_HEADER),
            ZEPHYR_LIBS.iter().map(|rel| package.join(rel)).collect(),
            ZEPHYR_ALLOWED_UNRESOLVED,
        ),
        "freertos" => (
            package.join(FREERTOS_HEADER),
            vec![package.join(FREERTOS_LIB)],
            FREERTOS_ALLOWED_UNRESOLVED,
        ),
        other => {
            warning(
                events,
                &format!("目标 {other} 没有 C ABI 静态库可查（c-abi-symbols 仅用于 RTOS 目标）"),
            );
            return Ok(false);
        }
    };
    if !header.is_file() {
        warning(
            events,
            &format!(
                "目标 {} 未校验 c-abi-symbols：缺 {}（先跑 poly build --target {}）",
                spec.id,
                header.display(),
                spec.id
            ),
        );
        return Ok(false);
    }
    if let Some(missing) = libs.iter().find(|lib| !lib.is_file()) {
        warning(
            events,
            &format!(
                "目标 {} 未校验 c-abi-symbols：缺引擎静态库 {}（先跑 poly build --target {}）",
                spec.id,
                missing.display(),
                spec.id
            ),
        );
        return Ok(false);
    }
    let header_text = abi::read_header(&header).map_err(|err| defect(spec, err.to_string()))?;
    let symbols = abi::parse_c_abi(&header_text);
    for lib in &libs {
        abi::check_c_abi(lib, &header_text).map_err(|err| defect(spec, err.to_string()))?;
        let unexpected = abi::unexpected_unresolved(lib, allowed)
            .map_err(|err| defect(spec, err.to_string()))?;
        if !unexpected.is_empty() {
            return Err(defect(
                spec,
                format!(
                    "{} 的未解析符号不在白名单：{}（核对运行时库/分配器接入）",
                    lib.display(),
                    unexpected.join(", ")
                ),
            ));
        }
        abi::check_no_std(lib).map_err(|err| defect(spec, err.to_string()))?;
    }
    // §5.3 真跑：整个暂存目标目录（头文件 + 库 + 模板/kit）不得含绝对路径。
    abi::check_no_absolute_paths("poly verify", &package)
        .map_err(|err| defect(spec, err.to_string()))?;
    warning(
        events,
        &format!(
            "目标 {} 的 C ABI 真跑：头文件声明 {} 个 poly_engine_* 符号，{} 个归档符号一致；\
             未解析闭包仅运行时白名单；无 std 符号；暂存目录无绝对路径",
            spec.id,
            symbols.len(),
            libs.len()
        ),
    );
    emit_verified(events, ids);
    Ok(true)
}

/// `vxworks-ld-check`：结构断言真跑；`wr-cc` 在场时用厂商链接器真链一遍。
pub(crate) fn verify_vxworks_ld(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let kit = package_dir(project, spec).join(VXWORKS_KIT);
    if !kit.is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验 vxworks-ld-check：{} 不存在（先跑 poly build --target vxworks）",
                spec.id,
                kit.display()
            ),
        );
        return Ok(false);
    }
    let structure =
        vxworks::check_vxworks_structure(&kit).map_err(|err| defect(spec, err.to_string()))?;
    let wr_cc = std::env::var_os("WR_CC")
        .map(PathBuf::from)
        .or_else(|| poly_bundle::exec::tool_path("wr-cc"));
    let Some(wr_cc) = wr_cc else {
        warning(
            events,
            &format!(
                "目标 {} 的 VxWorks kit 结构真跑（triple={}，模板 v{}，{} 个 ABI 符号）；\
                 链接腿 SKIP-with-print：本机无 wr-cc（source 厂商 SDK 环境后设 WR_CC 或入 \
                 PATH 可复跑 `sh {}/check.sh`）",
                spec.id,
                structure.triple,
                structure.template_version,
                structure.symbols.len(),
                kit.display()
            ),
        );
        return Ok(true);
    };
    let output = Command::new("sh")
        .arg("check.sh")
        .env("WR_CC", &wr_cc)
        .current_dir(&kit)
        .output()
        .map_err(|err| {
            defect(
                spec,
                format!(
                    "无法执行 {}/check.sh（wr-cc={}）：{err}",
                    kit.display(),
                    wr_cc.display()
                ),
            )
        })?;
    // 校验副产物不留在可发布 kit 里。
    let _ = std::fs::remove_file(kit.join("selfcheck.o"));
    let _ = std::fs::remove_file(kit.join("poly_engine_selfcheck.vxe"));
    if !output.status.success() {
        return Err(defect(
            spec,
            format!(
                "wr-cc 链接失败（{}）：{}",
                output.status,
                tail(&output.stderr)
            ),
        ));
    }
    warning(
        events,
        &format!(
            "目标 {} 的 VxWorks 链接腿真跑：wr-cc 编译自检源并链接静态库成功（无未解析符号）",
            spec.id
        ),
    );
    emit_verified(events, ids);
    Ok(true)
}

/// `zephyr-west-build`：模板结构 / CONFIG 集真跑；环境齐备时两块板真构建。
pub(crate) fn verify_zephyr_west(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let root = package_dir(project, spec).join(ZEPHYR_ROOT);
    if !root.is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验 zephyr-west-build：{} 不存在（先跑 poly build --target zephyr）",
                spec.id,
                root.display()
            ),
        );
        return Ok(false);
    }
    let structure =
        zephyr::check_zephyr_structure(&root).map_err(|err| defect(spec, err.to_string()))?;
    warning(
        events,
        &format!(
            "目标 {} 的 west 模板结构真跑：模板 v{}，boards = {}，最小 CONFIG 集 = {}",
            spec.id,
            structure.template_version,
            structure.boards.join(" / "),
            structure.config_keys.join(", ")
        ),
    );

    let west = std::env::var_os("POLYPACK_WEST")
        .map(PathBuf::from)
        .or_else(|| poly_bundle::exec::tool_path("west"));
    let zephyr_base = std::env::var_os("ZEPHYR_BASE").map(PathBuf::from);
    let sdk = std::env::var_os("ZEPHYR_SDK_INSTALL_DIR").map(PathBuf::from);
    let (Some(west), Some(zephyr_base), Some(sdk)) = (west, zephyr_base, sdk) else {
        warning(
            events,
            &format!(
                "目标 {} 的 west build 腿 SKIP-with-print：需 west + ZEPHYR_BASE + \
                 ZEPHYR_SDK_INSTALL_DIR（模板结构 / 版本化 CONFIG 集已真跑）。\
                 复跑：ZEPHYR_BASE=<zephyr> ZEPHYR_SDK_INSTALL_DIR=<sdk> west build \
                 -b qemu_x86|qemu_cortex_m3 -s {}",
                spec.id,
                root.display()
            ),
        );
        return Ok(true);
    };
    let cwd = west_topdir(&zephyr_base).unwrap_or_else(|| project.root.clone());
    warning(
        events,
        &format!(
            "目标 {} 的 west build 真跑：west={} ZEPHYR_BASE={} SDK={}，工作区 {}",
            spec.id,
            west.display(),
            zephyr_base.display(),
            sdk.display(),
            cwd.display()
        ),
    );
    for board in zephyr::BOARDS {
        let build_dir = project
            .root
            .join(".polypack")
            .join("verify-zephyr")
            .join(board);
        let _ = std::fs::remove_dir_all(&build_dir);
        let output = Command::new(&west)
            .args(["build", "-b", board, "-d"])
            .arg(&build_dir)
            .args(["-s"])
            .arg(&root)
            .arg("--pristine=auto")
            .current_dir(&cwd)
            .output()
            .map_err(|err| {
                defect(
                    spec,
                    format!("无法执行 west build（{}）：{err}", west.display()),
                )
            })?;
        if !output.status.success() {
            return Err(defect(
                spec,
                format!(
                    "{board} west build 失败（{}）：{}",
                    output.status,
                    tail(&output.stderr)
                ),
            ));
        }
        check_zephyr_build(spec, board, &build_dir)?;
        // 供 zephyr-qemu-smoke 判断构建目录是否与当前模板同版（板级覆盖变化需重建）。
        let _ = std::fs::write(
            build_dir.join("polypack-template"),
            zephyr::TEMPLATE_VERSION,
        );
        warning(
            events,
            &format!(
                "目标 {} 的 {board} west build 真跑通过：链接映射含 libpolypack_engine.a，无 libstd-*",
                spec.id
            ),
        );
    }
    emit_verified(events, ids);
    Ok(true)
}

/// 从 `ZEPHYR_BASE` 向上找 `.west` 工作区；找不到返回 `None`（调用方退回项目根）。
pub(crate) fn west_topdir(zephyr_base: &Path) -> Option<PathBuf> {
    let mut current = Some(zephyr_base);
    for _ in 0..5 {
        let dir = current?;
        if dir.join(".west").is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// 构建产物断言：链接映射里有引擎模块、没有 std；`.config` 与版本化 CONFIG 集一致。
fn check_zephyr_build(spec: &TargetSpec, board: &str, build_dir: &Path) -> Result<(), StageError> {
    let map = build_dir.join("zephyr/zephyr.map");
    let text = std::fs::read_to_string(&map).map_err(|err| {
        defect(
            spec,
            format!("{board} 构建缺链接映射 {}：{err}", map.display()),
        )
    })?;
    if !text.contains("libpolypack_engine.a") {
        return Err(defect(
            spec,
            format!("{board} 链接映射未包含 libpolypack_engine.a（引擎层未以模块链入）"),
        ));
    }
    if text.contains("libstd-") {
        return Err(defect(
            spec,
            format!("{board} 链接映射出现 libstd-*（no_std 接入被破坏）"),
        ));
    }
    let config_path = build_dir.join("zephyr/.config");
    let config = std::fs::read_to_string(&config_path).map_err(|err| {
        defect(
            spec,
            format!("{board} 构建缺 .config {}：{err}", config_path.display()),
        )
    })?;
    for (key, value) in zephyr::CONFIGS {
        let expected = format!("{key}={value}");
        if !config.lines().any(|line| line.trim() == expected) {
            return Err(defect(
                spec,
                format!("{board} 的 .config 未生效版本化 CONFIG：{expected}"),
            ));
        }
    }
    Ok(())
}

/// 命令 stderr 的末段（诊断里只留可读的失败现场）。
pub(crate) fn tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(12);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlists_cover_measured_closures() {
        for symbol in [
            "bcmp", "free", "memalign", "memcmp", "memcpy", "memmove", "memset", "strlen",
        ] {
            assert!(VXWORKS_ALLOWED_UNRESOLVED.contains(&symbol), "{symbol}");
        }
        for symbol in [
            "__aeabi_unwind_cpp_pr0",
            "k_aligned_alloc",
            "k_free",
            "printk",
            "memcmp",
            "libc_memcpy",
        ] {
            if symbol == "libc_memcpy" {
                assert!(!ZEPHYR_ALLOWED_UNRESOLVED.contains(&symbol));
            } else {
                assert!(ZEPHYR_ALLOWED_UNRESOLVED.contains(&symbol), "{symbol}");
            }
        }
    }

    #[test]
    fn west_topdir_walks_up_to_workspace() {
        let base = std::env::temp_dir().join("polypack-west-topdir-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("workspace/.west")).expect("mkdir");
        std::fs::create_dir_all(base.join("workspace/zephyr")).expect("mkdir");
        assert_eq!(
            west_topdir(&base.join("workspace/zephyr")),
            Some(base.join("workspace"))
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
