//! VxWorks 库 / 镜像打包器（T-M6-05）。
//!
//! 产物是 `wr-cc` 可直接消费的部署 kit：
//! - `lib/libpolypack_engine.a`：应用侧 wrapper staticlib（no_std 引擎 + VxWorks
//!   `memalign` / `free` 分配器 + panic 自旋 + 六个 `poly_engine_*` 保留锚点）；
//! - `include/polypack_engine.h`：T-M6-02 生成器产出的唯一头文件；
//! - `src/engine_selfcheck.c` + `check.sh`：VxWorks 工具链在场时真跑链接校验
//!   （`wr-cc` 驱动宿主 `ld`，未解析符号即链接失败）；缺席时 verify 腿按
//!   SKIP-with-print 处理，nm 符号闭包 / 结构断言仍真跑；
//! - `manifest.json`：triple / ABI / 符号 / 模板版本（可复现记录）。
//!
//! `format = "library"` 主产物 = `.a`；`format = "image"` 主产物 = 确定性部署 zip
//! （lib + 头文件 + 自检源 + check.sh + manifest）。两 format 都构建同一 kit，差异只在
//! 主产物选择——不做第二套引擎构建。交叉 triple 默认 `x86_64-wrs-vxworks`，可用
//! `POLYPACK_VXWORKS_TRIPLE` 覆盖；Rust tier-3 目标没有预置 std，wrapper 走
//! nightly `-Z build-std=core,alloc`（不需要 VxWorks SDK 即可产出 .a）。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::embedded::EmbeddedInputs;
use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, ArtifactKind, BundlePlan, Packager};
use crate::packagers::embedded::{self, EngineBuild, RETENTION_ANCHORS, TEMPLATE_VERSION};

/// 目标 id。
pub const ID: &str = "vxworks";
/// 静态库格式。
pub const FORMAT_LIBRARY: &str = "library";
/// 镜像 kit 格式。
pub const FORMAT_IMAGE: &str = "image";

/// 默认交叉 triple（`POLYPACK_VXWORKS_TRIPLE` 可覆盖）。
pub const DEFAULT_TRIPLE: &str = "x86_64-wrs-vxworks";
/// 预置库架构键。
const ARCH: &str = "vxworks";

const WORK_DIR: &str = "vxworks";
const LIB_FILE: &str = "lib/libpolypack_engine.a";
const HEADER_FILE: &str = "include/polypack_engine.h";
const SELFCHECK_FILE: &str = "src/engine_selfcheck.c";
const CHECK_FILE: &str = "check.sh";
const MANIFEST_FILE: &str = "manifest.json";

/// VxWorks 静态库打包器。
pub struct VxWorksLibrary;

/// VxWorks 镜像（部署 kit zip）打包器。
pub struct VxWorksImage;

impl Packager for VxWorksLibrary {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT_LIBRARY
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_for(inputs, spec, FORMAT_LIBRARY)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_for(inputs, spec, FORMAT_LIBRARY)
    }
}

impl Packager for VxWorksImage {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT_IMAGE
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_for(inputs, spec, FORMAT_IMAGE)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_for(inputs, spec, FORMAT_IMAGE)
    }
}

/// kit 结构检查结果（打包器自检与 `poly verify` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxWorksStructure {
    /// 交叉 triple。
    pub triple: String,
    /// 模板版本。
    pub template_version: String,
    /// 头文件声明的 `poly_engine_*` 符号。
    pub symbols: Vec<String>,
}

/// 校验 VxWorks kit：文件齐全 / manifest 字段 / 头文件符号 / check.sh 链接腿。
pub fn check_vxworks_structure(kit: &Path) -> Result<VxWorksStructure, BundleError> {
    for (label, relative) in [
        ("引擎静态库", LIB_FILE),
        ("C 头文件", HEADER_FILE),
        ("自检源", SELFCHECK_FILE),
        ("链接检查脚本", CHECK_FILE),
        ("manifest", MANIFEST_FILE),
    ] {
        if !kit.join(relative).is_file() {
            return Err(bad(kit, format!("缺{label}（{relative}）")));
        }
    }
    let header = embedded::read_header(&kit.join(HEADER_FILE))?;
    let symbols = embedded::parse_c_abi(&header);
    if symbols.is_empty() {
        return Err(bad(kit, "头文件没有 poly_engine_* 声明".to_string()));
    }
    let check =
        std::fs::read_to_string(kit.join(CHECK_FILE)).map_err(io_err(&kit.join(CHECK_FILE)))?;
    for needle in ["wr-cc", "libpolypack_engine.a", "engine_selfcheck.c"] {
        if !check.contains(needle) {
            return Err(bad(kit, format!("check.sh 缺 {needle}（VxWorks 链接腿）")));
        }
    }
    let manifest_text = std::fs::read_to_string(kit.join(MANIFEST_FILE))
        .map_err(io_err(&kit.join(MANIFEST_FILE)))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| bad(kit, format!("manifest.json 非法：{err}")))?;
    let triple = manifest["triple"]
        .as_str()
        .ok_or_else(|| bad(kit, "manifest.json 缺 triple".to_string()))?;
    let version = manifest["template_version"]
        .as_str()
        .ok_or_else(|| bad(kit, "manifest.json 缺 template_version".to_string()))?;
    if version != TEMPLATE_VERSION {
        return Err(bad(
            kit,
            format!("manifest 模板版本 {version} ≠ {TEMPLATE_VERSION}"),
        ));
    }
    let recorded: Vec<String> = manifest["symbols"]
        .as_array()
        .ok_or_else(|| bad(kit, "manifest.json 缺 symbols".to_string()))?
        .iter()
        .filter_map(|symbol| symbol.as_str().map(str::to_string))
        .collect();
    if recorded != symbols {
        return Err(bad(
            kit,
            format!("manifest symbols = {recorded:?} ≠ 头文件 {symbols:?}"),
        ));
    }
    Ok(VxWorksStructure {
        triple: triple.to_string(),
        template_version: version.to_string(),
        symbols,
    })
}

fn plan_for(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    format: &'static str,
) -> Result<BundlePlan, BundleError> {
    check_format(spec, format)?;
    inputs.product.validate(ID)?;
    let embedded = embedded_inputs(inputs)?;
    let header = embedded::read_header(&embedded.header)?;
    Ok(BundlePlan::new()
        .with_script(HEADER_FILE, header)
        .with_script(SELFCHECK_FILE, selfcheck_c())
        .with_script(CHECK_FILE, check_sh())
        .with_script(MANIFEST_FILE, manifest(inputs, spec)))
}

fn package_for(
    inputs: &BundleInputs,
    spec: &TargetSpec,
    format: &'static str,
) -> Result<Artifact, BundleError> {
    let plan = plan_for(inputs, spec, format)?;
    let embedded = embedded_inputs(inputs)?;
    let kit = inputs.out_dir.join(WORK_DIR);
    let _ = std::fs::remove_dir_all(&kit);
    std::fs::create_dir_all(&kit).map_err(io_err(&kit))?;
    for script in &plan.scripts {
        if script.path == Path::new(HEADER_FILE) {
            fsutil::write_text(&kit.join(HEADER_FILE), &script.contents)?;
        } else if script.path == Path::new(MANIFEST_FILE) {
            // manifest 在库构建后按实际 triple 重写（plan 期只给预览）
            continue;
        } else {
            fsutil::write_text(&kit.join(&script.path), &script.contents)?;
        }
    }
    let triple = vxworks_triple();
    let lib = engine_lib(inputs, embedded, &triple)?;
    fsutil::copy_file(&lib, &kit.join(LIB_FILE), false)?;
    // wrapper 构建中间物（Cargo.toml / target）不属于发布 kit；留着会污染 §5.3 扫描。
    let _ = std::fs::remove_dir_all(inputs.out_dir.join("engine").join(ARCH));
    fsutil::write_text(
        &kit.join(MANIFEST_FILE),
        &manifest_with_triple(inputs, spec, &triple),
    )?;
    fsutil::write_text(&kit.join(CHECK_FILE), check_sh())?;
    make_executable(&kit.join(CHECK_FILE))?;

    check_vxworks_structure(&kit)?;
    embedded::check_no_absolute_paths(ID, &kit)?;

    let notes = vec![
        format!(
            "VxWorks kit v{TEMPLATE_VERSION}：triple = {triple}；wr-cc 在场时 `sh {CHECK_FILE}` \
             链接校验（无未解析符号），缺席按 SKIP-with-print；nm 符号闭包 / 结构断言真跑"
        ),
        format!("头文件：{HEADER_FILE}；静态库：{LIB_FILE}"),
    ];
    let (path, kind) = match format {
        FORMAT_IMAGE => {
            let zip = inputs
                .out_dir
                .join(format!("{}-vxworks-image.zip", inputs.product.kebab_name()));
            let entries = kit_entries(&kit)?;
            crate::zipwrite::write_zip(&entries, &zip)?;
            embedded::check_no_absolute_paths(ID, &zip)?;
            (zip, ArtifactKind::File)
        }
        _ => (kit.join(LIB_FILE), ArtifactKind::File),
    };
    let bytes = if kind == ArtifactKind::File {
        std::fs::metadata(&path).map_err(io_err(&path))?.len()
    } else {
        fsutil::tree_bytes(&path)?
    };
    Ok(Artifact {
        target: spec.id.clone(),
        format: format.to_string(),
        path,
        bytes,
        kind,
        notes,
    })
}

fn engine_lib(
    inputs: &BundleInputs,
    embedded: &EmbeddedInputs,
    triple: &str,
) -> Result<PathBuf, BundleError> {
    if let Some(lib) = embedded::prebuilt_lib(embedded, ARCH) {
        return Ok(lib.to_path_buf());
    }
    let core_crate = embedded.engine_crate.as_path();
    let staging = inputs.out_dir.join("engine").join("vxworks");
    embedded::build_staticlib(&EngineBuild {
        core_crate,
        staging: &staging,
        target: triple,
        wrapper_rs: &wrapper_rs(),
        nightly: true,
        static_reloc: true,
    })
}

fn kit_entries(kit: &Path) -> Result<Vec<(String, PathBuf)>, BundleError> {
    let mut entries = Vec::new();
    for (name, relative) in [
        ("lib/libpolypack_engine.a", LIB_FILE),
        ("include/polypack_engine.h", HEADER_FILE),
        ("src/engine_selfcheck.c", SELFCHECK_FILE),
        ("check.sh", CHECK_FILE),
        ("manifest.json", MANIFEST_FILE),
    ] {
        entries.push((name.to_string(), kit.join(relative)));
    }
    Ok(entries)
}

fn manifest(inputs: &BundleInputs, spec: &TargetSpec) -> String {
    manifest_with_triple(inputs, spec, &vxworks_triple())
}

fn manifest_with_triple(inputs: &BundleInputs, spec: &TargetSpec, triple: &str) -> String {
    let embedded = inputs.embedded.as_ref();
    let symbols = embedded
        .and_then(|embedded| std::fs::read_to_string(&embedded.header).ok())
        .map(|header| embedded::parse_c_abi(&header))
        .unwrap_or_default();
    let manifest = serde_json::json!({
        "template": "polypack-vxworks-kit",
        "template_version": TEMPLATE_VERSION,
        "target": spec.id,
        "triple": triple,
        "abi": 1,
        "symbols": symbols,
        "link_check": CHECK_FILE,
        "formats": [FORMAT_LIBRARY, FORMAT_IMAGE],
        "no_ui": true,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).expect("VxWorks manifest 序列化")
    )
}

fn vxworks_triple() -> String {
    std::env::var("POLYPACK_VXWORKS_TRIPLE").unwrap_or_else(|_| DEFAULT_TRIPLE.to_string())
}

/// 应用侧 wrapper：no_std 导入 + 六个 C ABI 保留锚点 + VxWorks libc allocator / panic 自旋。
fn wrapper_rs() -> String {
    format!(
        r#"{RETENTION_ANCHORS}
// VxWorks libc：memalign / free；panic 路径不依赖日志 API，自旋停机。
unsafe extern "C" {{
    fn memalign(align: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}}

struct VxWorksAlloc;

unsafe impl core::alloc::GlobalAlloc for VxWorksAlloc {{
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {{
        let align = layout.align().max(core::mem::size_of::<usize>());
        // SAFETY: 参数来自 GlobalAlloc 契约；失败返回 NULL 由调用方处理。
        unsafe {{ memalign(align, layout.size().max(1)) }}.cast()
    }}

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: core::alloc::Layout) {{
        // SAFETY: ptr 由 alloc 返回且只释放一次。
        unsafe {{ free(ptr.cast()) }};
    }}
}}

#[global_allocator]
static ALLOC: VxWorksAlloc = VxWorksAlloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {{
    loop {{
        core::hint::spin_loop();
    }}
}}
"#
    )
}

fn selfcheck_c() -> &'static str {
    r#"/* PolyPack VxWorks 引擎自检（T-M6-05）：只调用 T-M6-02 生成的 C ABI。 */
#include <stdint.h>

#include "polypack_engine.h"

int poly_engine_selfcheck(void)
{
	return poly_engine_abi_version() == 1 ? 0 : 1;
}
"#
}

fn check_sh() -> &'static str {
    r#"#!/bin/sh
# VxWorks 链接腿（T-M6-05）：wr-cc 在场时真跑；未解析符号 = 链接失败。
# 前置：source 厂商 SDK 的环境脚本，使 wr-cc / wr-ld 进入 PATH。
set -eu
CC="${WR_CC:-wr-cc}"
"$CC" -c src/engine_selfcheck.c -Iinclude -o selfcheck.o
"$CC" selfcheck.o lib/libpolypack_engine.a -o poly_engine_selfcheck.vxe
"#
}

fn embedded_inputs(inputs: &BundleInputs) -> Result<&EmbeddedInputs, BundleError> {
    inputs.embedded.as_ref().ok_or(BundleError::MissingInput {
        packager: ID,
        field: "embedded",
    })
}

fn check_format(spec: &TargetSpec, format: &str) -> Result<(), BundleError> {
    if spec.package.format == format {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "package.format",
        value: spec.package.format.clone(),
        reason: format!(
            "VxWorks 打包器 format 须为 `{FORMAT_LIBRARY}` 或 `{FORMAT_IMAGE}`（配置可覆盖）"
        ),
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), BundleError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(io_err(path))?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(io_err(path))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), BundleError> {
    Ok(())
}

fn bad(kit: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field: "vxworks.kit",
        value: kit.display().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_sh_uses_vendor_linker() {
        let script = check_sh();
        assert!(script.contains("wr-cc"));
        assert!(script.contains("libpolypack_engine.a"));
    }

    #[test]
    fn manifest_records_triple_and_symbols() {
        let inputs = BundleInputs::new(
            crate::inputs::ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.0.0".to_string(),
                identifier: "com.polypack.pax".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "notes".to_string(),
                homepage: None,
            },
            std::env::temp_dir(),
        );
        let spec = TargetSpec::new_stub(ID, 6, &[poly_targets::Host::Linux]);
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest(&inputs, &spec)).expect("json");
        assert_eq!(manifest["triple"], DEFAULT_TRIPLE);
        assert_eq!(manifest["abi"], 1);
    }
}
