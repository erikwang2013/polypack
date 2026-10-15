//! Android AAR 组装（T-M2-02）：核心 → 各 ABI 的 JNI `.so` + Kotlin 绑定 → `.aar`。
//!
//! ## 链路
//!
//! ```text
//! cargo build --release --target <abi triple>   # 每个 ABI 一次（linker 指 NDK clang）
//!   → target/<triple>/release/lib<核心>.so
//! kotlinc（可选）→ classes.jar                  # 生成的 Kotlin 绑定（JNA 在 classpath）
//! 组装 → <lib>.aar = AndroidManifest.xml + classes.jar + jni/<abi>/lib<核心>.so
//! ```
//!
//! ## AAR 布局（消费者契约）
//!
//! `jni/<abi>/` 的 ABI 目录名与 `poly-bundle` 的 `NativeLib.abi` 同域
//! （`arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`）；`classes.jar` 是 `dex_inputs` 的输入。
//! Android 打包阶段（T-M2-06/07）按这两个位置取件，不依赖 Gradle 的存在。
//!
//! ## 工具缺失语义（skip-if-missing）
//!
//! - **NDK 缺失** → `poly::E_BRIDGE_0003` + 修复提示（交叉编译无法降级，缺 linker 就是缺 linker）；
//! - **kotlinc 缺失** → AAR 仍组装（`.so` + 清单 + 绑定源码随附），并报告 `classes.jar` 缺位的原因，
//!   绝不伪造一个空的 `classes.jar`。消费者的 dex 缺失会由打包阶段如实暴露（T-M2-06 的 notes）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use poly_cache::Digest;

use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::plan::StageError;

/// 源树摘要排除项（构建产物与依赖目录不参与）。
pub const SOURCE_EXCLUDE: &[&str] = &["target", "node_modules", "dist", ".polypack", ".git"];

/// AAR 内 Kotlin 绑定源码的随附落点（`classes.jar` 缺位时仍有源码可查）。
const SOURCES_DIR: &str = "bridge-sources";
/// AAR 内 ABI 库目录前缀。
const JNI_DIR: &str = "jni";
/// 默认 `minSdkVersion`（与 `poly-bundle` 的 `AndroidInputs::default().min_sdk` 同口径）。
pub const DEFAULT_MIN_SDK: u32 = 24;

/// 一个 Android ABI：产物目录名 ↔ Rust triple ↔ NDK clang 前缀。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abi {
    /// AAR / APK 里的 ABI 目录名。
    pub dir: &'static str,
    /// Rust 目标三元组（rustup target）。
    pub triple: &'static str,
    /// NDK clang 的三元组前缀（注意 `armv7` 在 NDK 侧写作 `armv7a-…-androideabi`）。
    pub clang: &'static str,
}

/// 四个 Android ABI（固定顺序：64 位优先）。
pub const ABIS: [Abi; 4] = [
    Abi {
        dir: "arm64-v8a",
        triple: "aarch64-linux-android",
        clang: "aarch64-linux-android",
    },
    Abi {
        dir: "armeabi-v7a",
        triple: "armv7-linux-androideabi",
        clang: "armv7a-linux-androideabi",
    },
    Abi {
        dir: "x86_64",
        triple: "x86_64-linux-android",
        clang: "x86_64-linux-android",
    },
    Abi {
        dir: "x86",
        triple: "i686-linux-android",
        clang: "i686-linux-android",
    },
];

impl Abi {
    /// 按 ABI 目录名 / triple 解析（`--abi` 参数与测试用）。
    pub fn parse(text: &str) -> Option<Self> {
        ABIS.into_iter()
            .find(|abi| abi.dir == text || abi.triple == text)
    }

    /// 链接器环境变量名：`CARGO_TARGET_<TRIPLE 大写去连字符>_LINKER`。
    fn linker_var(self) -> String {
        format!(
            "CARGO_TARGET_{}_LINKER",
            self.triple.replace('-', "_").to_uppercase()
        )
    }

    /// 交叉编译用的 `cc` / `ar` 环境变量名（`cc` crate 消费，如核心依赖带 C 代码）。
    fn cc_var(self, kind: &str) -> String {
        format!("{kind}_{}", self.triple.replace('-', "_"))
    }
}

/// NDK 的一个 clang 链接器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdkLinker {
    /// NDK 根目录。
    pub ndk_root: PathBuf,
    /// `<ndk>/toolchains/llvm/prebuilt/<宿主标签>/bin`。
    pub bin: PathBuf,
}

/// 定位 NDK 根：`$POLYPACK_NDK_HOME` → `$ANDROID_NDK_HOME` → `<sdk>/ndk/<版本最大>`。
///
/// `<sdk>` 取 `$POLYPACK_ANDROID_SDK` → `$ANDROID_HOME` → `$ANDROID_SDK_ROOT` →
/// `~/Android/Sdk`（Linux）→ `~/Library/Android/sdk`（macOS）。与 `poly-toolchain` 的
/// Android SDK/NDK 探测器同口径，另加 `POLYPACK_*` 显式覆盖（CI / 农场用）。
pub fn ndk_root() -> Option<PathBuf> {
    for key in ["POLYPACK_NDK_HOME", "ANDROID_NDK_HOME"] {
        if let Some(dir) = std::env::var_os(key).map(PathBuf::from)
            && dir.is_dir()
        {
            return Some(dir);
        }
    }
    let sdk = sdk_root()?;
    latest_ndk(&sdk)
}

/// SDK 根：`POLYPACK_ANDROID_SDK` → `ANDROID_HOME` → `ANDROID_SDK_ROOT` → 常见安装根。
pub fn sdk_root() -> Option<PathBuf> {
    for key in ["POLYPACK_ANDROID_SDK", "ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(dir) = std::env::var_os(key).map(PathBuf::from)
            && dir.is_dir()
        {
            return Some(dir);
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    [home.join("Android/Sdk"), home.join("Library/Android/sdk")]
        .into_iter()
        .find(|dir| dir.is_dir())
}

/// `<sdk>/ndk/` 下版本号最大的目录（纯数字版本名参与比较，其余跳过）。
fn latest_ndk(sdk: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(sdk.join("ndk")).ok()?.flatten() {
        let path = entry.path();
        let key: Vec<u32> = entry
            .file_name()
            .to_string_lossy()
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if path.is_dir() && best.as_ref().is_none_or(|(top, _)| key > *top) {
            best = Some((key, path));
        }
    }
    best.map(|(_, path)| path)
}

/// NDK 的 clang 链接器目录：`<ndk>/toolchains/llvm/prebuilt/<宿主标签>/bin`。
///
/// 宿主标签按编译期平台推导（`linux-x86_64` / `darwin-x86_64` / `darwin-arm64` /
/// `windows-x86_64`）；目录不存在时再扫描 `prebuilt/` 下的唯一子目录兜底。
pub fn ndk_bin(ndk_root: &Path) -> Option<PathBuf> {
    let prebuilt = ndk_root.join("toolchains").join("llvm").join("prebuilt");
    let tag = host_tag();
    let candidate = prebuilt.join(&tag).join("bin");
    if candidate.is_dir() {
        return Some(candidate);
    }
    std::fs::read_dir(&prebuilt)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("bin"))
        .find(|dir| dir.is_dir())
        .or(Some(candidate))
}

/// NDK 的宿主标签。
fn host_tag() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{}", std::env::consts::OS, arch)
}

/// 该 ABI 的 clang 链接器路径：`bin/<clang 前缀><API>-clang`（Windows 认 `.cmd`）。
pub fn linker_for(ndk_root: &Path, abi: Abi, api: u32) -> Option<PathBuf> {
    let bin = ndk_bin(ndk_root)?;
    let base = format!("{}{}-clang", abi.clang, api);
    let candidates: [String; 2] = [base.clone(), format!("{base}.cmd")];
    candidates
        .into_iter()
        .map(|name| bin.join(name))
        .find(|path| path.is_file())
}

/// 交叉编译一个 ABI 的 `.so`（`cargo build --release --target <triple> --lib`）。
///
/// 失败 → `poly::E_BRIDGE_0003` + 修复提示（含 stderr 尾部）。工具链缺失（NDK 无该 ABI 的
/// clang、rustup 未装 triple）都在这里一次性报清楚。
pub fn build_lib(
    core_dir: &Path,
    abi: Abi,
    ndk_root: &Path,
    api: u32,
    events: &mut dyn EventSink,
) -> Result<PathBuf, StageError> {
    let linker =
        linker_for(ndk_root, abi, api).ok_or_else(|| missing_linker(abi, ndk_root, api))?;
    let bin = linker.parent().unwrap_or(ndk_root);
    let output = Command::new("cargo")
        .args(["build", "--release", "--lib", "--target", abi.triple])
        .current_dir(core_dir)
        .env("CARGO_TARGET_DIR", core_dir.join("target"))
        .env(abi.linker_var(), &linker)
        .env(abi.cc_var("CC"), &linker)
        .env(abi.cc_var("AR"), bin.join("llvm-ar"))
        .env(abi.cc_var("RANLIB"), bin.join("llvm-ranlib"))
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("无法执行 cargo 交叉编译（{}）: {err}", abi.triple),
            )
            .with_hint("确认 cargo 在 PATH 中，且核心 crate 目录存在")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "核心 crate 交叉编译失败（{}）: {}",
                abi.triple,
                stderr_tail(&output.stderr)
            ),
        )
        .with_hint(format!(
            "①`rustup target add {}`；②确认 NDK 的 clang 可用（{}）",
            abi.triple,
            linker.display()
        ))
        .into());
    }
    let so = core_dir
        .join("target")
        .join(abi.triple)
        .join("release")
        .join(format!("lib{}.so", lib_name(core_dir)?));
    if !so.is_file() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("交叉编译成功但未找到 {}", so.display()),
        )
        .with_hint("核心 crate 的 [lib] crate-type 需含 \"cdylib\"（uniffi 桥的载体）")
        .into());
    }
    events.emit(&BuildEvent::Compiled {
        crate_name: so
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        triple: abi.triple.to_string(),
    });
    Ok(so)
}

/// 读核心 crate 名（`-` → `_`：Rust 库文件名用下划线）。
fn lib_name(core_dir: &Path) -> Result<String, StageError> {
    Ok(crate::compile::read_crate_name(core_dir)?.replace('-', "_"))
}

/// 组装 AAR：`AndroidManifest.xml` + `classes.jar`（有则写入）+ `jni/<abi>/*.so`
/// + 随附绑定源码（`bridge-sources/`，`classes.jar` 缺位时的可查依据）。
///
/// 条目按名称排序、时间戳固定（`zip` crate 的默认时间），保证同输入二跑字节一致。
pub fn assemble(
    out: &Path,
    manifest: &str,
    libs: &[(Abi, PathBuf)],
    classes_jar: Option<&Path>,
    sources: &Path,
) -> Result<(), StageError> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_error(parent, err))?;
    }
    let file = std::fs::File::create(out).map_err(|err| io_error(out, err))?;
    let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = zip::write::SimpleFileOptions::default();

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let manifest_path = out.with_extension("manifest.tmp");
    std::fs::write(&manifest_path, manifest).map_err(|err| io_error(&manifest_path, err))?;
    entries.push(("AndroidManifest.xml".to_string(), manifest_path.clone()));
    for (abi, so) in libs {
        let name = so
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "libcore.so".to_string());
        entries.push((format!("{JNI_DIR}/{}/{name}", abi.dir), so.clone()));
    }
    for file in collect_files(sources)? {
        // zip 条目名恒用 `/` 分隔：Windows 上 `Path::display()` 会给 `\`，而 gradle/Java
        // 只认 `/`（CI windows 腿实测条目名成了 `bridge-sources/uniffi\pax\pax.kt`）
        let relative = file
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        entries.push((format!("{SOURCES_DIR}/{relative}"), sources.join(&file)));
    }
    if let Some(jar) = classes_jar {
        entries.push(("classes.jar".to_string(), jar.to_path_buf()));
    }
    entries.sort();

    for (name, path) in &entries {
        writer
            .start_file(name.clone(), options)
            .map_err(|err| zip_error(out, err))?;
        let bytes = std::fs::read(path).map_err(|err| io_error(path, err))?;
        writer.write_all(&bytes).map_err(|err| io_error(out, err))?;
    }
    writer.finish().map_err(|err| zip_error(out, err))?;
    let _ = std::fs::remove_file(&manifest_path);
    Ok(())
}

/// AAR 解包结果（Android 打包阶段的输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackedAar {
    /// 逐 ABI 的原生库（解包后的落地路径，按 [`ABIS`] 表序）。
    pub libs: Vec<(Abi, PathBuf)>,
    /// `classes.jar`（AAR 里没有就是没有——不伪造）。
    pub classes_jar: Option<PathBuf>,
}

/// 解包 AAR 供 Android 打包器消费：`jni/<abi>/<名>.so` → `dest/jniLibs/<abi>/<名>.so`，
/// `classes.jar` → `dest/<aar 名>.classes.jar`。清单与随附源码不落地（打包阶段用不上）。
///
/// 不认识的 ABI 目录名（未来新增 ABI）跳过该条，不让整个 AAB 因一条库失败。
pub fn unpack(aar: &Path, dest: &Path) -> Result<UnpackedAar, StageError> {
    let file = std::fs::File::open(aar).map_err(|err| io_error(aar, err))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| zip_error(aar, err))?;
    let stem = aar
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "core".to_string());
    let mut unpacked = UnpackedAar {
        libs: Vec::new(),
        classes_jar: None,
    };
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| zip_error(aar, err))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|err| zip_error(aar, err))?
            .into_owned();
        let target = if name == "classes.jar" {
            let path = dest.join(format!("{stem}.classes.jar"));
            unpacked.classes_jar = Some(path.clone());
            Some(path)
        } else {
            name.strip_prefix(&format!("{JNI_DIR}/"))
                .and_then(|rest| rest.split_once('/'))
                .and_then(|(abi_dir, file_name)| {
                    (!file_name.is_empty() && !file_name.contains('/'))
                        .then(|| Abi::parse(abi_dir).map(|abi| (abi, file_name.to_string())))
                        .flatten()
                })
                .map(|(abi, file_name)| {
                    let path = dest.join("jniLibs").join(abi.dir).join(file_name);
                    unpacked.libs.push((abi, path.clone()));
                    path
                })
        };
        let Some(target) = target else { continue };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| io_error(parent, err))?;
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|err| io_error(aar, err))?;
        std::fs::write(&target, bytes).map_err(|err| io_error(&target, err))?;
    }
    unpacked.libs.sort_by_key(|(abi, _)| {
        ABIS.iter()
            .position(|candidate| candidate == abi)
            .unwrap_or(usize::MAX)
    });
    Ok(unpacked)
}

/// 生成最小 `AndroidManifest.xml`（AAR 形态：`<manifest package=…>`，无 activity）。
///
/// 最小化依据：AAR 是库，不声明组件；`minSdkVersion` 让消费方合并时拿到正确下限。
pub fn manifest_xml(package: &str, min_sdk: u32, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <manifest xmlns:android=\"http://schemas.android.com/apk/res/android\"\n\
         \x20   package=\"{package}\"\n\
         \x20   android:versionCode=\"1\"\n\
         \x20   android:versionName=\"{version}\">\n\
         \x20 <uses-sdk android:minSdkVersion=\"{min_sdk}\" />\n\
         </manifest>\n"
    )
}

/// AAR 的库级缓存键：核心 crate 源树 + 锁文件 + ABI 集合 + **接口摘要** + 引擎 + profile。
pub fn cache_key(
    project: &crate::plan::Project,
    abis: &[Abi],
    interface: &Digest,
    engine: &str,
    profile: &str,
) -> Result<Digest, StageError> {
    let core_dir = project.root.join(&project.config.core.crate_path);
    let sources = Digest::of_tree(&core_dir, SOURCE_EXCLUDE).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("核心 crate 源树摘要失败: {err}"),
        )
        .with_hint("确认 [core].crate 指向的目录存在且可读")
    })?;
    let lockfile = match std::fs::read(core_dir.join("Cargo.lock")) {
        Ok(bytes) => Digest::of_bytes(&bytes),
        Err(_) => Digest::of_bytes(b""),
    };
    let abi_ids: Vec<&str> = abis.iter().map(|abi| abi.dir).collect();
    Ok(poly_cache::cache_key(&poly_cache::CacheInputs {
        sources,
        lockfile,
        target_spec: format!("android-aar:{}", abi_ids.join(",")),
        toolchain: format!("{}+{engine}+{interface}", super::BRIDGE_SCHEMA),
        profile: profile.to_string(),
    }))
}

/// 缺 linker 的诊断（NDK 或该 API 等级的 clang 不在场）。
fn missing_linker(abi: Abi, ndk_root: &Path, api: u32) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!(
            "NDK 缺 {} 的 clang（API {api}）：{}",
            abi.triple,
            ndk_root.display()
        ),
    )
    .with_hint(
        "设 $ANDROID_NDK_HOME / $POLYPACK_NDK_HOME 指向 NDK 根（含 \
         toolchains/llvm/prebuilt/<宿主>/bin/<abi><API>-clang），或用 sdkmanager 装 NDK",
    )
    .into()
}

/// 递归收集目录下普通文件的相对路径（升序）。
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, StageError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(|err| io_error(&current, err))? {
            let entry = entry.map_err(|err| io_error(&current, err))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|err| io_error(&path, err))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                out.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// stderr 尾部关键行（≤3 行）。
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    lines.join(" | ")
}

/// IO 失败 → `poly::E_BRIDGE_0003`。
fn io_error(path: &Path, err: std::io::Error) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!("AAR 组装 IO 失败（{}）: {err}", path.display()),
    )
    .with_hint("确认输出目录可写、磁盘空间充足")
    .into()
}

/// zip 写入失败 → `poly::E_BRIDGE_0003`。
fn zip_error(path: &Path, err: zip::result::ZipError) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!("AAR 归档失败（{}）: {err}", path.display()),
    )
    .with_hint("确认目标文件未被其它进程占用")
    .into()
}
