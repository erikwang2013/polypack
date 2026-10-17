//! QNX 打包器（T-M6-03）：静态库 + 部署镜像，复用 poly-core 的 `poly_engine_*` C ABI。
//!
//! - **库**：生成应用侧 `staticlib` 包装 crate（path 依赖 poly-core；不改 poly-core 的
//!   crate-type），用 QNX SDP 工具链 `cargo build --target <qnx-triple>`，产物
//!   `libpolypack_engine_qnx.a`。四端以 rlib 链入应用、应用提供 global_allocator /
//!   panic_handler（见 `poly-core/src/engine/ffi.rs` 的契约）。
//! - **镜像**：确定性部署镜像 tar.gz（库 + 生成的头文件 + mkifs buildfile + manifest）；
//!   可引导 `.ifs` 与 QEMU 冒烟归 T-M6-09（本任务只交付镜像载荷与 buildfile）。
//! - **SKIP 口径**：无 QNX SDP → `ToolMissing` → CLI 环境槽 SKIP-with-print；绝不伪造
//!   `.a` / 镜像。QNX Rust target 未安装时由 cargo 报错（不静默降级）。
//!
//! 结构检查 [`check_qnx_structure`] 与 verify 腿 `qnx-image-structure` 同一实现。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::Command;
use crate::embedded::{self, EmbeddedInputs};
use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Packager};

/// 目标 id。
const ID: &str = "qnx";

/// 镜像主产物格式。
const FORMAT_IMAGE: &str = "image";

/// 库主产物格式（`[targets.qnx].format = "library"` 覆盖）。
const FORMAT_LIBRARY: &str = "library";

/// 静态库文件名（包装 crate `polypack-engine-qnx`）。
const LIBRARY: &str = "libpolypack_engine_qnx.a";

/// 包装 crate 的包名（产物名派生自它）。
const WRAPPER_NAME: &str = "polypack-engine-qnx";

/// QNX 打包器（镜像为主产物）。
pub struct QnxImage;

/// QNX 打包器（库为主产物）。
pub struct QnxLibrary;

impl Packager for QnxImage {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT_IMAGE
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_qnx(inputs, spec, FORMAT_IMAGE)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_qnx(inputs, spec, FORMAT_IMAGE)
    }
}

impl Packager for QnxLibrary {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT_LIBRARY
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_qnx(inputs, spec, FORMAT_LIBRARY)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_qnx(inputs, spec, FORMAT_LIBRARY)
    }
}

/// QNX 产物结构检查结果（打包器自检与 verify 腿共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QnxStructure {
    /// 交叉 triple。
    pub triple: String,
    /// 静态库相对路径（镜像内）。
    pub library: String,
    /// C ABI 符号清单。
    pub symbols: Vec<String>,
    /// buildfile 路径。
    pub buildfile: PathBuf,
}

fn plan_qnx(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    primary: &str,
) -> Result<BundlePlan, BundleError> {
    check_format(spec, primary)?;
    inputs.product.validate(ID)?;
    let embedded = embedded::require(inputs, ID)?;
    let triple = embedded.qnx_triple().to_string();
    let mut plan = BundlePlan::new()
        .with_script(
            "engine-static/Cargo.toml".to_string(),
            embedded::wrapper_manifest_text(WRAPPER_NAME, &embedded.engine_crate, true),
        )
        .with_script(
            "engine-static/src/lib.rs".to_string(),
            embedded::wrapper_source_text(true),
        )
        .with_script(buildfile_path(inputs), buildfile_text(inputs))
        .with_script(manifest_path(), manifest_text(inputs, embedded, &triple)?);
    match &embedded.qnx {
        Some(qnx) => {
            let mut command = Command::new(
                "cargo",
                [
                    "build".to_string(),
                    "--release".to_string(),
                    "--manifest-path".to_string(),
                    engine_manifest(inputs).display().to_string(),
                    "--target".to_string(),
                    triple.clone(),
                ],
            )
            .with_env("QNX_HOST", qnx.host.display().to_string())
            .with_env("QNX_TARGET", qnx.target.display().to_string())
            .with_env("PATH", qnx_path(&qnx.host))
            .with_env(
                "CARGO_TARGET_DIR",
                engine_target_dir(inputs).display().to_string(),
            );
            if std::env::var_os("POLYPACK_QNX_BUILD_STD").is_some() {
                // tier-3 目标通常没有预编译 core/alloc；显式开 build-std 时按 nightly 口径。
                for arg in ["-Z", "build-std=core,alloc"] {
                    command.args.push(arg.to_string());
                }
            }
            plan = plan.with_command(command);
        }
        None => {
            plan = plan.skipping(
                "cargo build（QNX 交叉）",
                "未找到 QNX SDP（见 `poly doctor` 的 qnx-sdp 行）：交叉编译按环境槽 \
                 SKIP-with-print，不伪造静态库 / 镜像",
            );
        }
    }
    Ok(plan)
}

fn package_qnx(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    primary: &str,
) -> Result<Artifact, BundleError> {
    let plan = plan_qnx(inputs, spec, primary)?;
    let embedded = embedded::require(inputs, ID)?;
    if embedded.qnx.is_none() {
        return Err(BundleError::ToolMissing {
            packager: ID,
            tool: "qnx-sdp".to_string(),
            hint: "安装 QNX SDP 7.1 后 source qnxsdp-env.sh（$QNX_HOST / $QNX_TARGET），\
                   或设 $POLYPACK_QNX_SDP 指向 SDP 根；然后重试"
                .to_string(),
        });
    }
    fsutil::write_scripts(&plan)?;
    let include = inputs.out_dir.join("include");
    fsutil::copy_file(&embedded.header, &include.join("polypack_engine.h"), false)?;

    exec::run_all(&plan.commands)?;

    let built = engine_target_dir(inputs)
        .join(embedded.qnx_triple())
        .join("release")
        .join(LIBRARY);
    if !built.is_file() {
        return Err(BundleError::MissingInput {
            packager: ID,
            field: "qnx staticlib（QNX Rust target 未安装时先 rustup target add）",
        });
    }
    fsutil::copy_file(&built, &inputs.out_dir.join("lib").join(LIBRARY), false)?;

    let structure = check_qnx_structure(&inputs.out_dir)?;
    let image = image_tar(inputs, &inputs.product.kebab_name())?;
    let mut notes = vec![format!(
        "QNX 静态库：lib/{LIBRARY}（{} 个 C ABI 符号）；镜像：{}",
        structure.symbols.len(),
        image.file_name().unwrap_or_default().to_string_lossy()
    )];
    notes.push(
        "镜像为确定性部署 tar.gz（库 + 头文件 + mkifs buildfile + manifest）；可引导 .ifs / \
         QEMU 冒烟归 T-M6-09"
            .to_string(),
    );
    if primary == FORMAT_LIBRARY {
        return fsutil::file_artifact(
            ID,
            FORMAT_LIBRARY,
            &inputs.out_dir.join("lib").join(LIBRARY),
            notes,
        );
    }
    fsutil::file_artifact(ID, FORMAT_IMAGE, &image, notes)
}

/// 结构检查：buildfile 标记 + manifest 三键 + 库引用一致。
pub fn check_qnx_structure(dir: &Path) -> Result<QnxStructure, BundleError> {
    let buildfile = find_buildfile(dir)?;
    let text = std::fs::read_to_string(&buildfile).map_err(crate::error::io_err(&buildfile))?;
    for marker in [
        "[image=",
        "[virtual=",
        "lib/",
        ".a",
        "include/polypack_engine.h",
    ] {
        if !text.contains(marker) {
            return Err(bad(&buildfile, format!("buildfile 缺标记 {marker:?}")));
        }
    }
    let manifest = dir.join("qnx/manifest.json");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).map_err(crate::error::io_err(&manifest))?)
            .map_err(|err| bad(&manifest, format!("manifest 不是合法 JSON：{err}")))?;
    if value["target"] != "qnx" {
        return Err(bad(&manifest, "manifest target 必须是 qnx".to_string()));
    }
    let triple = value["triple"]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| bad(&manifest, "manifest 缺 triple".to_string()))?
        .to_string();
    let library = value["library"]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| bad(&manifest, "manifest 缺 library".to_string()))?
        .to_string();
    let symbols: Vec<String> = value["symbols"]
        .as_array()
        .ok_or_else(|| bad(&manifest, "manifest 缺 symbols".to_string()))?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    if symbols.is_empty() {
        return Err(bad(&manifest, "manifest symbols 为空".to_string()));
    }
    if !dir.join(&library).is_file() {
        return Err(bad(
            &manifest,
            format!("manifest 指向的库不存在：{library}"),
        ));
    }
    Ok(QnxStructure {
        triple,
        library,
        symbols,
        buildfile,
    })
}

fn check_format(spec: &TargetSpec, expected: &str) -> Result<(), BundleError> {
    if spec.package.format == expected {
        Ok(())
    } else {
        Err(BundleError::InvalidInput {
            packager: ID,
            field: "package.format",
            value: spec.package.format.clone(),
            reason: format!("QNX 打包器只服务 format = {expected}"),
        })
    }
}

fn buildfile_text(inputs: &BundleInputs) -> String {
    format!(
        "[image=0x10000000]\n[virtual=aarch64le,elf]\n\
         # PolyPack engine deployment image (T-M6-03)\n\
         # 可引导 .ifs 与 QEMU 冒烟归 T-M6-09；本文件是镜像载荷的 mkifs 输入。\n\
         [+script] startup-script = {{ polypack_engine_selfcheck }}\n\
         lib/{LIBRARY}\ninclude/polypack_engine.h\n\
         # {} v{}\n",
        inputs.product.name, inputs.product.version
    )
}

fn manifest_text(
    inputs: &BundleInputs,
    embedded: &EmbeddedInputs,
    triple: &str,
) -> Result<String, BundleError> {
    let value = serde_json::json!({
        "target": "qnx",
        "builder": "qnx-sdp",
        "triple": triple,
        "library": format!("lib/{LIBRARY}"),
        "header": "include/polypack_engine.h",
        "image": format!("{}-image.tar.gz", inputs.product.kebab_name()),
        "symbols": embedded.symbols,
    });
    serde_json::to_string_pretty(&value)
        .map_err(|err| bad(&inputs.out_dir, format!("manifest 序列化失败：{err}")))
}

fn image_tar(inputs: &BundleInputs, kebab: &str) -> Result<PathBuf, BundleError> {
    let dir = &inputs.out_dir;
    let members = [
        "include/polypack_engine.h".to_string(),
        format!("lib/{LIBRARY}"),
        format!("qnx/{kebab}.build"),
        "qnx/manifest.json".to_string(),
    ];
    let mut entries = Vec::with_capacity(members.len());
    for rel in members {
        let path = dir.join(&rel);
        let bytes = std::fs::read(&path).map_err(crate::error::io_err(&path))?;
        entries.push((rel, bytes));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let tar = crate::arwrite::tar_bytes(&entries)?;
    let path = dir.join(format!("{kebab}-image.tar.gz"));
    let gz = crate::arwrite::gzip_bytes(&tar, &path)?;
    std::fs::write(&path, gz).map_err(crate::error::io_err(&path))?;
    Ok(path)
}

fn find_buildfile(dir: &Path) -> Result<PathBuf, BundleError> {
    let qnx = dir.join("qnx");
    let mut builds: Vec<PathBuf> = std::fs::read_dir(&qnx)
        .map_err(crate::error::io_err(&qnx))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "build"))
        .collect();
    builds.sort();
    builds
        .into_iter()
        .next()
        .ok_or_else(|| bad(&qnx, "缺少 mkifs buildfile".to_string()))
}

fn buildfile_path(inputs: &BundleInputs) -> String {
    format!("qnx/{}.build", inputs.product.kebab_name())
}

fn manifest_path() -> String {
    "qnx/manifest.json".to_string()
}

fn engine_manifest(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("engine-static/Cargo.toml")
}

fn engine_target_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("engine-target")
}

/// QNX 工具链 bin 前置到 PATH（rustc 的 QNX linker 驱动按名解析）。
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

fn bad(path: &Path, reason: String) -> BundleError {
    BundleError::Archive {
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(test)]
#[path = "qnx_tests.rs"]
mod tests;
