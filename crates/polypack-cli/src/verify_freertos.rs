//! FreeRTOS QEMU 冒烟校验器（T-M6-07）。
//!
//! 板级包结构与内核钉扎始终真跑（与打包器同一实现）；arm-none-eabi-gcc /
//! qemu-system-arm / make 在场且内核可拉取时，`make smoke` 在 mps2-an385 上真跑：
//! 引擎任务调用 `polypack_engine_*` 打印版本，并以 semihosting `SYS_EXIT_EXTENDED`
//! 退出码 0 结束。工具链 / 网络缺席按环境槽 SKIP-with-print（不伪造通过）。

use std::path::PathBuf;
use std::process::Command;

use polypack_bundle::packagers::freertos;
use polypack_core::Project;
use polypack_core::events::EventSink;
use polypack_core::plan::StageError;
use polypack_targets::TargetSpec;

use crate::verify_rtos::{FREERTOS_ROOT, defect, emit_verified, package_dir, tail, warning};

/// `freertos-qemu-smoke`：板级包结构 / 内核钉扎真跑；工具链与网络齐备时 QEMU 真跑。
pub(crate) fn verify_freertos_qemu(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let root = package_dir(project, spec).join(FREERTOS_ROOT);
    if !root.is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验 freertos-qemu-smoke：{} 不存在（先跑 polypack build --target freertos）",
                spec.id,
                root.display()
            ),
        );
        return Ok(false);
    }
    let structure =
        freertos::check_freertos_structure(&root).map_err(|err| defect(spec, err.to_string()))?;
    warning(
        events,
        &format!(
            "目标 {} 的 FreeRTOS 板级包结构真跑：{}，triple={}，内核 {}（sha256 钉扎），\
             {} 个 ABI 符号",
            spec.id,
            structure.board,
            structure.triple,
            structure.kernel_tag,
            structure.symbols.len()
        ),
    );

    let missing: Vec<&str> = ["arm-none-eabi-gcc", "qemu-system-arm", "make"]
        .into_iter()
        .filter(|tool| polypack_bundle::exec::tool_path(tool).is_none())
        .collect();
    let sample = root.join("sample");
    if !missing.is_empty() {
        warning(
            events,
            &format!(
                "目标 {} 的 QEMU 冒烟腿 SKIP-with-print：本机缺 {}（板级结构已真跑）。\
                 复跑：make -C {} fetch && make -C {} smoke",
                spec.id,
                missing.join(" / "),
                sample.display(),
                sample.display()
            ),
        );
        return Ok(true);
    }

    let kernel_dir = std::env::var_os("POLYPACK_FREERTOS_KERNEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| project.root.join(".polypack").join("freertos-kernel"));
    let fetch = Command::new("sh")
        .arg("fetch_kernel.sh")
        .env("KERNEL_DIR", &kernel_dir)
        .env("KERNEL_TAG", &structure.kernel_tag)
        .env("KERNEL_SHA256", &structure.kernel_sha256)
        .current_dir(&sample)
        .output()
        .map_err(|err| {
            defect(
                spec,
                format!("无法执行 {}/fetch_kernel.sh：{err}", sample.display()),
            )
        })?;
    match fetch.status.code() {
        Some(0) => {}
        // 2 = sha256 不匹配：硬失败，绝不换内核继续。
        Some(2) => {
            return Err(defect(
                spec,
                format!(
                    "FreeRTOS 内核 {} sha256 校验失败：{}",
                    structure.kernel_tag,
                    tail(&fetch.stderr)
                ),
            ));
        }
        code => {
            warning(
                events,
                &format!(
                    "目标 {} 的 QEMU 冒烟腿 SKIP-with-print：内核 {} 拉取不可用\
                     （exit={:?}，{}）；预置内核后设 POLYPACK_FREERTOS_KERNEL_DIR 可离线复跑",
                    spec.id,
                    structure.kernel_tag,
                    code,
                    tail(&fetch.stderr)
                ),
            );
            return Ok(true);
        }
    }

    let build_dir = project
        .root
        .join(".polypack")
        .join("verify-freertos")
        .join("build");
    let _ = std::fs::remove_dir_all(&build_dir);
    let output = Command::new("make")
        .arg("smoke")
        .arg(format!("KERNEL_DIR={}", kernel_dir.display()))
        .arg(format!("BUILD_DIR={}", build_dir.display()))
        .current_dir(&sample)
        .output()
        .map_err(|err| {
            defect(
                spec,
                format!("无法执行 make smoke（{}）：{err}", sample.display()),
            )
        })?;
    let mut log = output.stdout.clone();
    log.extend_from_slice(&output.stderr);
    if !output.status.success() {
        return Err(defect(
            spec,
            format!(
                "FreeRTOS QEMU 冒烟失败（{}）：{}",
                output.status,
                tail(&log)
            ),
        ));
    }
    let text = String::from_utf8_lossy(&log);
    let Some(version_line) = text
        .lines()
        .find(|line| line.contains("polypack_engine version="))
        .map(str::trim)
    else {
        return Err(defect(
            spec,
            "QEMU 冒烟输出缺引擎版本行（polypack_engine version=）".to_string(),
        ));
    };
    if !text.contains("SMOKE PASS") {
        return Err(defect(
            spec,
            "QEMU 冒烟未打印 SMOKE PASS（退出码断言未通过）".to_string(),
        ));
    }
    warning(
        events,
        &format!(
            "目标 {} 的 FreeRTOS QEMU 冒烟真跑通过：{} 启动，引擎任务输出 `{version_line}`，\
             semihosting SYS_EXIT_EXTENDED 退出码 0（内核 {} / {}）",
            spec.id, structure.board, structure.kernel_tag, structure.triple
        ),
    );
    emit_verified(events, ids);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::verify_rtos::FREERTOS_ALLOWED_UNRESOLVED;

    #[test]
    fn allowlist_covers_measured_closure() {
        for symbol in [
            "__aeabi_unwind_cpp_pr0",
            "polypack_engine_rt_panic",
            "pvPortMalloc",
            "vPortFree",
        ] {
            assert!(FREERTOS_ALLOWED_UNRESOLVED.contains(&symbol), "{symbol}");
        }
        assert!(
            !FREERTOS_ALLOWED_UNRESOLVED.contains(&"xQueueGenericSend"),
            "内核内部符号不应出现在引擎归档闭包白名单"
        );
    }
}
