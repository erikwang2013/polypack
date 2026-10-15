//! Kotlin 编译器定位与调用（T-M2-02 的 `classes.jar` 一步）。
//!
//! AAR 里的 Kotlin 绑定要先编成 JVM class 再打成 `classes.jar`。定位顺序：
//!
//! 1. `$POLYPACK_KOTLINC` —— 显式指定 `kotlinc` 可执行文件；
//! 2. `PATH` 上的 `kotlinc`（Windows 认 `.bat` / `.cmd`）；
//! 3. 工具缓存里的官方发行包：`~/.cache/polypack/tools/kotlin/kotlinc/bin/kotlinc[.bat]`
//!    （与 bundletool 的 `~/.cache/polypack/tools/` 约定同源）。
//!
//! 三处都没有 → 返回 `None`：调用方按 skip-if-missing 组装不含 `classes.jar` 的 AAR 并如实告警，
//! **绝不伪造一个空的 `classes.jar`**（那会让产物在设备上以难以定位的方式失败）。
//!
//! 绑定的 Kotlin 源码 `import com.sun.jna.*`，故编译 classpath 必须含 JNA：取
//! `$POLYPACK_JNA_JAR`，或工具缓存里的 `jna-*.jar`。JNA 缺失时本步骤同样按缺失处理。
//!
//! 为什么用整包发行版而不是 `kotlin-compiler-embeddable-*.jar`：embeddable 只保证「被 Gradle
//! 之类已经备齐依赖的宿主」使用，直接 `java -cp` 起会一路缺 kotlin-stdlib → kotlin-reflect →
//! kotlinx-coroutines……；发行包自带 `lib/` 全套，一条命令就够（栈里不留依赖追踪）。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::diagnostics::{Diagnostic, codes};
use crate::plan::StageError;

/// Kotlin 编译器的定位结果：可执行文件 + 编译期需要的 JNA。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kotlinc {
    /// `kotlinc` 可执行文件（`.bat` / `.cmd` 亦在此）。
    pub bin: PathBuf,
    /// JNA jar（绑定源码 `import com.sun.jna.*` 的编译期依赖）。
    pub jna: PathBuf,
}

impl Kotlinc {
    /// 定位编译器与 JNA；缺任一即 `None`。
    pub fn discover() -> Option<Self> {
        Some(Self {
            bin: explicit_kotlinc()
                .or_else(kotlinc_on_path)
                .or_else(cached_kotlinc)?,
            jna: find_jna()?,
        })
    }

    /// 编译 `sources`（`.kt` 文件清单）为 jar，落点 `out_jar`。
    ///
    /// `-classpath` 只补 JNA：stdlib / reflect 由发行包自带（见模块文档里为什么不用 embeddable）。
    pub fn compile(&self, sources: &[PathBuf], out_jar: &Path) -> Result<(), StageError> {
        if let Some(parent) = out_jar.parent() {
            std::fs::create_dir_all(parent).map_err(|err| io_error(parent, err))?;
        }
        let mut command = Command::new(&self.bin);
        // -nowarn：绑定代码的未用变量等告警不是使用者的负担；错误仍然会让退出码非零
        command
            .arg("-classpath")
            .arg(&self.jna)
            .arg("-nowarn")
            .arg("-d")
            .arg(out_jar);
        for source in sources {
            command.arg(source);
        }
        let output = command.output().map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("无法启动 Kotlin 编译器（{}）: {err}", self.bin.display()),
            )
            .with_hint("确认 $POLYPACK_KOTLINC / PATH 上的 kotlinc 可用（或工具缓存里的发行包）")
        })?;
        if !output.status.success() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!(
                    "Kotlin 绑定编译失败: {}",
                    stderr_tail(&output.stdout, &output.stderr)
                ),
            )
            .with_hint(
                "绑定源码需要 JNA 在 classpath（$POLYPACK_JNA_JAR 或工具缓存 jna-*.jar）；\
                 缺 JNA 时去掉 --kotlin 即可只组装 .so 形态的 AAR",
            )
            .into());
        }
        Ok(())
    }
}

/// `$POLYPACK_KOTLINC` 显式指定。
fn explicit_kotlinc() -> Option<PathBuf> {
    std::env::var_os("POLYPACK_KOTLINC")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

/// 工具缓存根：`~/.cache/polypack/tools/kotlin`（与 bundletool 的约定同源）。
fn tool_cache() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(
        home.join(".cache")
            .join("polypack")
            .join("tools")
            .join("kotlin"),
    )
}

/// `PATH` 上的 `kotlinc`（Windows 认 `.bat` / `.cmd`）。
fn kotlinc_on_path() -> Option<PathBuf> {
    shell_tool(&["kotlinc", "kotlinc.bat", "kotlinc.cmd"])
}

/// 工具缓存里解开的发行包：`<缓存>/kotlinc/bin/kotlinc[.bat]`。
fn cached_kotlinc() -> Option<PathBuf> {
    let bin = tool_cache()?.join("kotlinc").join("bin");
    ["kotlinc", "kotlinc.bat", "kotlinc.cmd"]
        .into_iter()
        .map(|name| bin.join(name))
        .find(|path| path.is_file())
}

/// 找 JNA jar：`$POLYPACK_JNA_JAR` 优先，其次工具缓存里的 `jna-*.jar`。
fn find_jna() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("POLYPACK_JNA_JAR").map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    let cache = tool_cache()?;
    let mut matches: Vec<PathBuf> = std::fs::read_dir(&cache)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("jna-"))
        })
        .collect();
    matches.sort();
    matches.pop()
}

/// `PATH` 上按候选名找可执行文件。
fn shell_tool(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let full = dir.join(name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// 报错文本（kotlinc 的进度与报错混在 stdout，取前几行非空行）。
fn stderr_tail(stdout: &[u8], stderr: &[u8]) -> String {
    let mut lines: Vec<String> = String::from_utf8_lossy(stdout)
        .lines()
        .chain(String::from_utf8_lossy(stderr).lines())
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    lines.truncate(4);
    lines.join(" | ")
}

/// IO 失败 → `poly::E_BRIDGE_0003`。
fn io_error(path: &Path, err: std::io::Error) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!("Kotlin 编译输出失败（{}）: {err}", path.display()),
    )
    .with_hint("确认输出目录可写")
    .into()
}
