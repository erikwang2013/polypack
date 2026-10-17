//! `qnx-qemu-smoke` / `zephyr-qemu-smoke` 校验腿（T-M6-09 / T-M6-10）。
//!
//! 两条腿只在「准备并启动 QEMU」上不同；退出码 / 具名步骤 / `SMOKE PASS` 判定与
//! 串口日志归档全部走 [`crate::smoke`]（同一份解析代码，无第二套判定逻辑）。
//!
//! - **QNX**：镜像载荷结构真跑 → 无 SDP / mkifs / QEMU 时 SKIP-with-print；齐备时用
//!   SDP 编译镜像内自检程序（`qcc`）→ `mkifs` 出 `.ifs` → `qemu-system-aarch64` 启动，
//!   startup-script 运行自检后关机。QNX 的 BSP 启动面（startup / 串口驱动）随 SDP
//!   安装，不入仓库。
//! - **Zephyr**：west 工程模板结构真跑 → 无 west / `ZEPHYR_BASE` / SDK 时 SKIP-with-print；
//!   齐备时对两块板跑 `west build -t run`，退出码按板级约定断言：
//!   `qemu_cortex_m3` 走 semihosting（成功 0），`qemu_x86` 走 isa-debug-exit
//!   （成功 `(0 << 1) | 1` = 1，该设备只产奇数）。
//!
//! 所有冒烟日志落在 `<root>/.polypack/logs/smoke/<target>/`（`<case>.log` +
//! `<case>.result.json`），供 CI 每晚归档；失败时诊断消息固定包含
//! `Failed{stage, code, hint}`，`stage` 即具体自检步骤。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_core::Project;
use poly_core::events::EventSink;
use poly_core::plan::StageError;
use poly_targets::TargetSpec;

use crate::smoke::SmokeRun;
use crate::verify_rtos::{defect, emit_verified, package_dir, warning, west_topdir};

/// Zephyr 模板根（与打包器 / `zephyr-west-build` 同一布局）。
const ZEPHYR_ROOT: &str = "west";

/// 两块板的成功退出码（板级约定，见模块文档）。
fn zephyr_expected_exit(board: &str) -> Option<i32> {
    match board {
        "qemu_cortex_m3" => Some(0),
        "qemu_x86" => Some(1),
        _ => None,
    }
}

/// QNX 镜像内自检程序的具名步骤（与 Zephyr `src/main.c` 保持同一契约）。
const SMOKE_STEPS: [&str; 3] = ["version", "validate-target", "cache-key"];

/// 冒烟超时（秒）：含 west 首次构建；`POLYPACK_SMOKE_TIMEOUT_SECS` 可覆盖。
fn smoke_timeout(default_secs: u64) -> std::time::Duration {
    let secs = std::env::var("POLYPACK_SMOKE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default_secs);
    std::time::Duration::from_secs(secs)
}

fn steps() -> Vec<String> {
    SMOKE_STEPS.iter().map(|step| (*step).to_string()).collect()
}

fn log_root(project: &Project) -> PathBuf {
    project.root.join(".polypack").join("logs").join("smoke")
}

// ---------------------------------------------------------------- Zephyr

/// `zephyr-qemu-smoke`：`west build -t run` + 退出码 / 具名步骤断言。
pub(crate) fn verify_zephyr_qemu(
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
                "目标 {} 未校验 zephyr-qemu-smoke：{} 不存在（先跑 poly build --target zephyr）",
                spec.id,
                root.display()
            ),
        );
        return Ok(false);
    }

    let west = std::env::var_os("POLYPACK_WEST")
        .map(PathBuf::from)
        .or_else(|| poly_bundle::exec::tool_path("west"));
    let zephyr_base = std::env::var_os("ZEPHYR_BASE").map(PathBuf::from);
    let sdk = std::env::var_os("ZEPHYR_SDK_INSTALL_DIR").map(PathBuf::from);
    let (Some(west), Some(zephyr_base), Some(sdk)) = (west, zephyr_base, sdk) else {
        warning(
            events,
            &format!(
                "目标 {} 的 zephyr-qemu-smoke SKIP-with-print：需 west + ZEPHYR_BASE + \
                 ZEPHYR_SDK_INSTALL_DIR（工程模板结构校验已由 zephyr-west-build 腿真跑）。\
                 复跑：ZEPHYR_BASE=<zephyr> ZEPHYR_SDK_INSTALL_DIR=<sdk> west build -b \
                 qemu_cortex_m3 -s {} -t run",
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
            "目标 {} 的 zephyr-qemu-smoke 真跑：west={} ZEPHYR_BASE={} SDK={}",
            spec.id,
            west.display(),
            zephyr_base.display(),
            sdk.display()
        ),
    );

    for board in poly_bundle::packagers::zephyr::BOARDS {
        let Some(expected_exit) = zephyr_expected_exit(board) else {
            return Err(defect(
                spec,
                format!("板型 {board} 没有登记 QEMU 退出码约定（zephyr-qemu-smoke）"),
            ));
        };
        let build_dir = project
            .root
            .join(".polypack")
            .join("verify-zephyr")
            .join(board);
        // zephyr-west-build 腿已按同一模板构建过时直接 `-t run`（增量）；构建目录缺失或
        // 模板版本变化（如新增板级 semihosting 覆盖）时必须重建，否则旧 .config 会沿用。
        let template = poly_bundle::packagers::zephyr::TEMPLATE_VERSION;
        let stamp = build_dir.join("polypack-template");
        let stale = std::fs::read_to_string(&stamp)
            .map(|value| value.trim() != template)
            .unwrap_or(true);
        let need_build = !build_dir.join("build.ninja").is_file() || stale;
        let mut args = vec![
            "build".to_string(),
            "-d".to_string(),
            build_dir.display().to_string(),
        ];
        if need_build {
            args.extend([
                "-b".to_string(),
                board.to_string(),
                "-s".to_string(),
                root.display().to_string(),
                "--pristine=always".to_string(),
            ]);
        }
        args.extend(["-t".to_string(), "run".to_string()]);
        let run = SmokeRun {
            target: spec.id.clone(),
            case: board.to_string(),
            program: west.clone(),
            args,
            cwd: Some(cwd.clone()),
            env: vec![
                ("ZEPHYR_BASE".to_string(), zephyr_base.display().to_string()),
                (
                    "ZEPHYR_SDK_INSTALL_DIR".to_string(),
                    sdk.display().to_string(),
                ),
            ],
            expected_exit,
            steps: steps(),
            log_root: log_root(project),
            timeout: smoke_timeout(1200),
        };
        match run.execute() {
            Ok(report) => {
                let _ = std::fs::write(&stamp, template);
                warning(
                    events,
                    &format!(
                        "目标 {} 的 {board} QEMU 冒烟真跑通过：{}，退出码 {}（板级约定 {}）；\
                         日志 {}，结果 {}",
                        spec.id,
                        report.version_line,
                        report.exit_code,
                        expected_exit,
                        report.log.display(),
                        report.result.display()
                    ),
                );
            }
            Err(failure) => return Err(failure.diagnostic(&spec.id).into()),
        }
    }
    emit_verified(events, ids);
    Ok(true)
}

// ---------------------------------------------------------------- QNX

/// `qnx-qemu-smoke`：镜像载荷结构真跑；SDP / mkifs / QEMU 齐备时编译自检程序、出 `.ifs`
/// 并在 QEMU 启动，按共享解析器判定。
pub(crate) fn verify_qnx_qemu(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let package = package_dir(project, spec);
    let structure = match poly_bundle::check_qnx_structure(&package) {
        Ok(structure) => structure,
        Err(err) => {
            warning(
                events,
                &format!(
                    "目标 {} 未校验 qnx-qemu-smoke：{}（先跑 poly build --target qnx，\
                     或核对镜像载荷结构；有 SDP / QEMU 时该腿会真编译自检程序并启动 QEMU）",
                    spec.id, err
                ),
            );
            return Ok(false);
        }
    };
    warning(
        events,
        &format!(
            "目标 {} 的 QNX 镜像载荷结构真跑：库 {}，{} 个 ABI 符号，buildfile {}",
            spec.id,
            structure.library,
            structure.symbols.len(),
            structure.buildfile.display()
        ),
    );

    let Some(sdp) = poly_toolchain::sdp_env() else {
        warning(
            events,
            &format!(
                "目标 {} 的 qnx-qemu-smoke SKIP-with-print：未找到 QNX SDP（见 `poly doctor` \
                 的 qnx-sdp 行）；镜像载荷结构已真跑，不伪造启动结果",
                spec.id
            ),
        );
        return Ok(true);
    };
    let qcc =
        tool_in(&sdp.host.join("usr/bin"), "qcc").or_else(|| poly_bundle::exec::tool_path("qcc"));
    let mkifs = tool_in(&sdp.host.join("usr/bin"), "mkifs")
        .or_else(|| poly_bundle::exec::tool_path("mkifs"));
    let qemu = std::env::var_os("POLYPACK_QNX_QEMU")
        .map(PathBuf::from)
        .or_else(|| poly_bundle::exec::tool_path("qemu-system-aarch64"));
    let (Some(qcc), Some(mkifs), Some(qemu)) = (qcc, mkifs, qemu) else {
        warning(
            events,
            &format!(
                "目标 {} 的 qnx-qemu-smoke SKIP-with-print：本机缺 qcc / mkifs / \
                 qemu-system-aarch64（SDP={}）；镜像载荷结构已真跑",
                spec.id,
                sdp.root.display()
            ),
        );
        return Ok(true);
    };

    // 编译镜像内自检程序：独立 scratch，不污染可发布载荷（避免绝对路径进入暂存目录）。
    let scratch = project.root.join(".polypack").join("verify-qnx-smoke");
    let _ = std::fs::remove_dir_all(&scratch);
    if let Err(err) = std::fs::create_dir_all(scratch.join("lib"))
        .and_then(|_| std::fs::create_dir_all(scratch.join("include")))
    {
        return Err(defect(
            spec,
            format!("无法创建 QNX 冒烟 scratch {}：{err}", scratch.display()),
        ));
    }
    let library = package.join(&structure.library);
    let header = package.join("include/polypack_engine.h");
    let source = scratch.join("selfcheck.c");
    std::fs::write(&source, qnx_selfcheck_source())
        .map_err(|err| defect(spec, format!("写 QNX 自检源失败：{err}")))?;
    std::fs::copy(
        &library,
        scratch.join("lib").join("libpolypack_engine_qnx.a"),
    )
    .map_err(|err| {
        defect(
            spec,
            format!("复制 QNX 引擎库 {} 失败：{err}", library.display()),
        )
    })?;
    std::fs::copy(&header, scratch.join("include/polypack_engine.h")).map_err(|err| {
        defect(
            spec,
            format!("复制 QNX 头文件 {} 失败：{err}", header.display()),
        )
    })?;

    let compile = Command::new(&qcc)
        .args(["-V", "gcc_ntoaarch64le", "-o"])
        .arg(scratch.join("polypack_engine_selfcheck"))
        .arg(&source)
        .arg(format!("-I{}", scratch.join("include").display()))
        .arg(scratch.join("lib").join("libpolypack_engine_qnx.a"))
        .env("QNX_HOST", sdp.host.display().to_string())
        .env("QNX_TARGET", sdp.target.display().to_string())
        .env("PATH", qnx_path(&sdp.host))
        .current_dir(&scratch)
        .output()
        .map_err(|err| defect(spec, format!("无法执行 qcc（{}）：{err}", qcc.display())))?;
    if !compile.status.success() {
        return Err(defect(
            spec,
            format!(
                "QNX 自检程序编译失败（{}）：{}",
                compile.status,
                tail(&compile.stderr)
            ),
        ));
    }

    // mkifs 输入：自检二进制 + 引擎库 + startup-script（自检后关机，QEMU 才会退出）。
    let buildfile = scratch.join("image.build");
    std::fs::write(&buildfile, qnx_buildfile("libpolypack_engine_qnx.a"))
        .map_err(|err| defect(spec, format!("写 QNX 冒烟 buildfile 失败：{err}")))?;
    let image = scratch.join("image.ifs");
    let mk = Command::new(&mkifs)
        .arg("-o")
        .arg(&image)
        .arg(&buildfile)
        .env("QNX_HOST", sdp.host.display().to_string())
        .env("QNX_TARGET", sdp.target.display().to_string())
        .env("PATH", qnx_path(&sdp.host))
        .current_dir(&scratch)
        .output()
        .map_err(|err| {
            defect(
                spec,
                format!("无法执行 mkifs（{}）：{err}", mkifs.display()),
            )
        })?;
    if !mk.status.success() {
        return Err(defect(
            spec,
            format!(
                "mkifs 出镜像失败（{}）：{}（BSP 启动 / 串口驱动缺失时需装对应 QNX BSP）",
                mk.status,
                tail(&mk.stderr)
            ),
        ));
    }

    let mut args = std::env::var("POLYPACK_QNX_QEMU_ARGS")
        .ok()
        .map(|value| value.split_whitespace().map(str::to_string).collect())
        .unwrap_or_else(|| {
            vec![
                "-M".to_string(),
                "virt".to_string(),
                "-cpu".to_string(),
                "cortex-a57".to_string(),
                "-m".to_string(),
                "512M".to_string(),
                "-nographic".to_string(),
            ]
        });
    args.extend(["-kernel".to_string(), image.display().to_string()]);
    let run = SmokeRun {
        target: spec.id.clone(),
        case: "image".to_string(),
        program: qemu.clone(),
        args,
        cwd: Some(scratch.clone()),
        env: Vec::new(),
        expected_exit: 0,
        steps: steps(),
        log_root: log_root(project),
        timeout: smoke_timeout(180),
    };
    let report = run
        .execute()
        .map_err(|failure| -> StageError { failure.diagnostic(&spec.id).into() })?;
    warning(
        events,
        &format!(
            "目标 {} 的 QNX QEMU 冒烟真跑通过：{}，退出码 0；日志 {}，结果 {}",
            spec.id,
            report.version_line,
            report.log.display(),
            report.result.display()
        ),
    );
    emit_verified(events, ids);
    Ok(true)
}

/// SDP host bin 或 PATH 中的工具。
fn tool_in(host_bin: &Path, name: &str) -> Option<PathBuf> {
    let candidate = host_bin.join(name);
    candidate.is_file().then_some(candidate)
}

/// QNX 工具链 bin 前置到 PATH（与 qnx 打包器同口径）。
fn qnx_path(host: &Path) -> String {
    let mut paths = vec![host.join("usr/bin")];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(paths).map_or_else(
        |_| host.join("usr/bin").display().to_string(),
        |joined| joined.to_string_lossy().into_owned(),
    )
}

/// 冒烟 buildfile：自检二进制放镜像根，startup-script 运行后关机。
fn qnx_buildfile(library: &str) -> String {
    format!(
        "[image=0x10000000]\n[virtual=aarch64le,elf]\n\
         # PolyPack QNX QEMU smoke (T-M6-09)：启动 → 引擎自检 → 关机（QEMU 退出 0）\n\
         [+script] startup-script = {{\n\
         \x20   /polypack_engine_selfcheck\n\
         \x20   shutdown -f -S\n\
         }}\n\
         /polypack_engine_selfcheck\n\
         lib/{library}\ninclude/polypack_engine.h\n"
    )
}

/// 镜像内自检程序：只调用 T-M6-02 的 C ABI；输出契约与 Zephyr `main.c` 相同。
fn qnx_selfcheck_source() -> &'static str {
    r#"/* PolyPack QNX engine self-check (T-M6-09). */
#include <stdio.h>
#include <string.h>
#include "polypack_engine.h"

static const char TARGET_JSON[] =
    "{\"id\":\"qnx\",\"tier\":6,\"hosts\":[\"linux\"],"
    "\"runtime\":\"staticlib\",\"bridge\":\"cffi\",\"ui\":[],"
    "\"package\":{\"format\":\"image\",\"builder\":\"qnx-sdp\"},"
    "\"verify\":[],\"publish\":[\"self-hosted\"],\"experimental\":false}";

int main(void)
{
    int ok = 1;
    int32_t rc;
    char *key;
    static const char sources_hex[] =
        "0000000000000000000000000000000000000000000000000000000000000000";
    static const char lockfile_hex[] =
        "1111111111111111111111111111111111111111111111111111111111111111";

    printf("poly_engine version=%s abi=%u\n",
           poly_engine_version(), (unsigned)poly_engine_abi_version());
    printf("[POLYPACK-STEP version] %s\n",
           poly_engine_version() != NULL ? "PASS" : "FAIL");
    ok &= poly_engine_version() != NULL;

    rc = poly_engine_validate_target_json(
        (const uint8_t *)TARGET_JSON, strlen(TARGET_JSON));
    printf("[POLYPACK-STEP validate-target] %s\n", rc == 0 ? "PASS" : "FAIL");
    ok &= rc == 0;

    key = poly_engine_cache_key(sources_hex, lockfile_hex, "qnx",
                                "aarch64-unknown-nto-qnx710", "release");
    printf("[POLYPACK-STEP cache-key] %s\n", key != NULL ? "PASS" : "FAIL");
    ok &= key != NULL;
    poly_engine_string_free(key);

    printf(ok ? "SMOKE PASS\n" : "SMOKE FAIL\n");
    return ok ? 0 : 1;
}
"#
}

/// 命令 stderr 的末段（诊断里只留可读的失败现场）。
fn tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(12);
    lines[start..].join("\n")
}

#[cfg(test)]
#[path = "verify_smoke_tests.rs"]
mod tests;
