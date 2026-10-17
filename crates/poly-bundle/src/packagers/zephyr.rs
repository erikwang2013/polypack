//! Zephyr west 工程模板打包器（T-M6-06）。
//!
//! 产物是一棵可直接 `west build` 的工程树：
//! - 应用工程（`CMakeLists.txt` / `src/main.c` / `prj.conf`）；
//! - zephyr module（`modules/poly_engine/`：`module.yml` + `zephyr_library_import` 的
//!   no_std 引擎静态库 + T-M6-02 生成的唯一头文件）；
//! - `poly-template.json`：模板版本、板型、最小 `CONFIG_*` 集（随模板版本化，CI 可断言）。
//!
//! 引擎静态库由应用侧 wrapper crate 产出（poly-core 只出 rlib，见 T-M6-02 契约）。
//!
//! 两个架构都走 nightly `-Z build-std=core,alloc`：`qemu_cortex_m3` 用
//! `thumbv7m-none-eabi`；`qemu_x86` 用 `i686-unknown-linux-gnu` 并加
//! `relocation-model=static`（预置 i686 core 为 PIC，Zephyr 的 x86 链接器脚本会丢弃
//! `.got.plt`）。预编译 alloc 里烙着 `/rustc/<hash>` 绝对路径，build-std + sysroot
//! remap 是 §5.3 的必要条件。真跑 `west build` 由 verify 腿 `zephyr-west-build`
//! 在 west + Zephyr SDK 在场时执行，缺席按环境槽 SKIP-with-print。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::{BundleInputs, EmbeddedInputs};
use crate::packager::{Artifact, ArtifactKind, BundlePlan, Packager};
use crate::packagers::embedded::{
    self, EngineBuild, RETENTION_ANCHORS, TEMPLATE_VERSION,
};

/// 目标 id 与产物格式（与 `TargetSpec.package.{format,builder}` 一致）。
pub const ID: &str = "zephyr";
const FORMAT: &str = "west";

/// 模板覆盖的两块 QEMU 板（验收面：两块均 `west build` 成功）。
pub const BOARDS: [&str; 2] = ["qemu_x86", "qemu_cortex_m3"];

/// 最小 `CONFIG_*` 集（顺序即 prj.conf 落盘顺序；随 [`TEMPLATE_VERSION`] 版本化）。
///
/// - PRINTK / CONSOLE / SERIAL：自检输出走串口；
/// - HEAP_MEM_POOL_SIZE：引擎层 global_allocator 走 `k_aligned_alloc`，堆池不得为零；
/// - MAIN_STACK_SIZE：serde_json 解析 / 格式化在栈上留余量（默认 1024 不够）。
pub const CONFIGS: [(&str, &str); 5] = [
    ("CONFIG_PRINTK", "y"),
    ("CONFIG_CONSOLE", "y"),
    ("CONFIG_SERIAL", "y"),
    ("CONFIG_HEAP_MEM_POOL_SIZE", "8192"),
    ("CONFIG_MAIN_STACK_SIZE", "4096"),
];

/// 模板树内的固定相对路径。
const APP_CMAKE: &str = "CMakeLists.txt";
const APP_MAIN: &str = "src/main.c";
const APP_CONF: &str = "prj.conf";
const MODULE_YML: &str = "modules/poly_engine/zephyr/module.yml";
const MODULE_CMAKE: &str = "modules/poly_engine/CMakeLists.txt";
const MODULE_HEADER: &str = "modules/poly_engine/include/polypack_engine.h";
const TEMPLATE_MANIFEST: &str = "poly-template.json";

/// 架构键 → Rust triple（`x86` / `arm` 也是模块 CMake 的分支键）。
const ARCHES: [(&str, &str, bool, bool); 2] = [
    // arch, triple, nightly, static-reloc。
    // ARM 也走 build-std：预编译 alloc 里烙着 `/rustc/<hash>/library/...` 绝对路径，
    // 只有从 rust-src 重编 + remap sysroot 才能满足 §5.3；x86 另需 static reloc。
    ("x86", "i686-unknown-linux-gnu", true, true),
    ("arm", "thumbv7m-none-eabi", true, false),
];

/// Zephyr west 模板打包器。
pub struct ZephyrWest;

impl Packager for ZephyrWest {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec)?;
        inputs.product.validate(ID)?;
        let embedded = embedded_inputs(inputs)?;
        let header = embedded::read_header(&embedded.header)?;
        Ok(BundlePlan::new()
            .with_script(APP_CMAKE, include_str!("zephyr_template/CMakeLists.txt"))
            .with_script(APP_MAIN, include_str!("zephyr_template/main.c"))
            .with_script(APP_CONF, prj_conf())
            .with_script(MODULE_YML, include_str!("zephyr_template/module.yml"))
            .with_script(
                MODULE_CMAKE,
                include_str!("zephyr_template/module_CMakeLists.txt"),
            )
            .with_script(MODULE_HEADER, header)
            .with_script(TEMPLATE_MANIFEST, manifest()))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let embedded = embedded_inputs(inputs)?;
        let root = inputs.out_dir.join("west");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).map_err(io_err(&root))?;

        for script in &plan.scripts {
            fsutil::write_text(&root.join(&script.path), &script.contents)?;
        }
        for (arch, triple, nightly, static_reloc) in ARCHES {
            let lib = engine_lib(inputs, embedded, arch, triple, nightly, static_reloc)?;
            fsutil::copy_file(
                &lib,
                &root
                    .join("modules/poly_engine/lib")
                    .join(arch)
                    .join("libpolypack_engine.a"),
                false,
            )?;
            // wrapper 构建中间物不属于 west 模板；留着会污染 §5.3 扫描。
            let _ = std::fs::remove_dir_all(inputs.out_dir.join("engine").join(arch));
        }
        // 产物自检（与 verify 腿同一实现）+ §5.3 路径审计都必须真跑
        check_zephyr_structure(&root)?;
        embedded::check_no_absolute_paths(ID, &root)?;
        Ok(Artifact {
            target: spec.id.clone(),
            format: FORMAT.to_string(),
            path: root.clone(),
            bytes: fsutil::tree_bytes(&root)?,
            kind: ArtifactKind::Directory,
            notes: vec![
                format!(
                    "west 工程模板 v{TEMPLATE_VERSION}：boards = {}；CONFIG 集随模板版本化",
                    BOARDS.join(" / ")
                ),
                "引擎层经 zephyr_library_import 以 module 链入（--whole-archive，no_std）"
                    .to_string(),
            ],
        })
    }
}

/// 结构检查结果（打包器自检与 `poly verify` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZephyrStructure {
    /// 模板版本。
    pub template_version: String,
    /// 板型清单。
    pub boards: Vec<String>,
    /// 最小 CONFIG 键。
    pub config_keys: Vec<String>,
}

/// 校验 west 工程树：模板文件 / 引擎库 / 头文件 / 版本化 CONFIG 集 / manifest。
pub fn check_zephyr_structure(root: &Path) -> Result<ZephyrStructure, BundleError> {
    for (label, relative) in [
        ("CMakeLists.txt", APP_CMAKE),
        ("应用入口", APP_MAIN),
        ("prj.conf", APP_CONF),
        ("module.yml", MODULE_YML),
        ("module CMakeLists", MODULE_CMAKE),
        ("C 头文件", MODULE_HEADER),
        ("x86 引擎库", "modules/poly_engine/lib/x86/libpolypack_engine.a"),
        ("arm 引擎库", "modules/poly_engine/lib/arm/libpolypack_engine.a"),
        ("模板 manifest", TEMPLATE_MANIFEST),
    ] {
        let path = root.join(relative);
        if !path.is_file() {
            return Err(bad(root, format!("缺{label}（{relative}）")));
        }
    }

    let conf = std::fs::read_to_string(root.join(APP_CONF)).map_err(io_err(&root.join(APP_CONF)))?;
    if !conf.contains(&format!("template v{TEMPLATE_VERSION}")) {
        return Err(bad(
            root,
            format!("prj.conf 未标注模板版本 v{TEMPLATE_VERSION}"),
        ));
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in conf.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| bad(root, format!("prj.conf 行不是 key=value：{line}")))?;
        pairs.push((key.trim().to_string(), value.trim().to_string()));
    }
    let expected: Vec<(String, String)> = CONFIGS
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    if pairs != expected {
        return Err(bad(
            root,
            format!("prj.conf CONFIG 集与模板 v{TEMPLATE_VERSION} 不一致：{pairs:?}"),
        ));
    }

    let manifest_text =
        std::fs::read_to_string(root.join(TEMPLATE_MANIFEST)).map_err(io_err(&root.join(TEMPLATE_MANIFEST)))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| bad(root, format!("poly-template.json 非法：{err}")))?;
    let version = manifest["template_version"]
        .as_str()
        .ok_or_else(|| bad(root, "poly-template.json 缺 template_version".to_string()))?;
    if version != TEMPLATE_VERSION {
        return Err(bad(
            root,
            format!("模板 manifest 版本 {version} ≠ {TEMPLATE_VERSION}"),
        ));
    }
    let boards: Vec<String> = manifest["boards"]
        .as_array()
        .ok_or_else(|| bad(root, "poly-template.json 缺 boards".to_string()))?
        .iter()
        .filter_map(|board| board.as_str().map(str::to_string))
        .collect();
    if boards != BOARDS {
        return Err(bad(root, format!("boards = {boards:?}，期望 {BOARDS:?}")));
    }
    let mut config_keys: Vec<String> = manifest["config"]
        .as_object()
        .ok_or_else(|| bad(root, "poly-template.json 缺 config".to_string()))?
        .keys()
        .cloned()
        .collect();
    config_keys.sort();
    let mut expected_keys: Vec<String> = CONFIGS.iter().map(|(key, _)| (*key).to_string()).collect();
    expected_keys.sort();
    if config_keys != expected_keys {
        return Err(bad(
            root,
            format!("manifest CONFIG 键 {config_keys:?} ≠ {expected_keys:?}"),
        ));
    }
    Ok(ZephyrStructure {
        template_version: version.to_string(),
        boards,
        config_keys,
    })
}

/// `prj.conf` 内容（唯一来源：[`CONFIGS`]）。
fn prj_conf() -> String {
    let mut out = format!("# PolyPack Zephyr template v{TEMPLATE_VERSION} -- minimal CONFIG_* set (versioned with template).\n");
    for (key, value) in CONFIGS {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

/// `poly-template.json` 内容（模板版本 / 板型 / CONFIG 集的机器可读记录）。
fn manifest() -> String {
    let config: serde_json::Map<String, serde_json::Value> = CONFIGS
        .iter()
        .map(|(key, value)| ((*key).to_string(), serde_json::json!(value)))
        .collect();
    let manifest = serde_json::json!({
        "template": "polypack-zephyr-west",
        "template_version": TEMPLATE_VERSION,
        "boards": BOARDS,
        "config": config,
        "engine": {
            "abi": 1,
            "integration": "zephyr_library_import",
            "no_std": true,
        },
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).expect("模板 manifest 序列化")
    )
}

fn engine_lib(
    inputs: &BundleInputs,
    embedded: &EmbeddedInputs,
    arch: &str,
    triple: &str,
    nightly: bool,
    static_reloc: bool,
) -> Result<PathBuf, BundleError> {
    if let Some(lib) = embedded::prebuilt_lib(embedded, arch) {
        return Ok(lib.to_path_buf());
    }
    let core_crate = embedded
        .core_crate
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: ID,
            field: "embedded.core_crate",
        })?;
    let staging = inputs.out_dir.join("engine").join(arch);
    embedded::build_staticlib(&EngineBuild {
        core_crate,
        staging: &staging,
        target: triple,
        wrapper_rs: &wrapper_rs(),
        nightly,
        static_reloc,
    })
}

/// 应用侧 wrapper：no_std 导入 + 六个 C ABI 保留锚点 + Zephyr allocator / panic_handler。
fn wrapper_rs() -> String {
    format!(
        r#"{RETENTION_ANCHORS}
// Zephyr 运行时：堆池分配器（CONFIG_HEAP_MEM_POOL_SIZE > 0）与 panic 自旋。
unsafe extern "C" {{
    fn k_aligned_alloc(align: usize, size: usize) -> *mut c_void;
    fn k_free(ptr: *mut c_void);
    fn printk(fmt: *const u8, ...);
}}

struct ZephyrAlloc;

unsafe impl core::alloc::GlobalAlloc for ZephyrAlloc {{
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {{
        let align = layout.align().max(core::mem::size_of::<usize>());
        // SAFETY: 参数来自 GlobalAlloc 契约；失败返回 NULL 由调用方处理。
        unsafe {{ k_aligned_alloc(align, layout.size().max(1)) }}.cast()
    }}

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: core::alloc::Layout) {{
        // SAFETY: ptr 由 alloc 返回且只释放一次。
        unsafe {{ k_free(ptr.cast()) }};
    }}
}}

#[global_allocator]
static ALLOC: ZephyrAlloc = ZephyrAlloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {{
    // SAFETY: 静态 NUL 结尾格式串；panic 路径只打印固定文本。
    unsafe {{
        printk(b"poly_engine panic\n\0".as_ptr());
    }}
    loop {{
        core::hint::spin_loop();
    }}
}}
"#
    )
}

fn embedded_inputs(inputs: &BundleInputs) -> Result<&EmbeddedInputs, BundleError> {
    inputs.embedded.as_ref().ok_or(BundleError::MissingInput {
        packager: ID,
        field: "embedded",
    })
}

fn check_format(spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format == FORMAT {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "package.format",
        value: spec.package.format.clone(),
        reason: format!("Zephyr 打包器只服务 `{FORMAT}`（west 工程模板）"),
    })
}

fn bad(root: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field: "west",
        value: root.display().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_set_is_versioned() {
        assert_eq!(TEMPLATE_VERSION, "1");
        assert_eq!(CONFIGS.len(), 5);
        let conf = prj_conf();
        assert!(conf.contains("template v1"));
        for (key, value) in CONFIGS {
            assert!(conf.contains(&format!("{key}={value}")));
        }
        assert_eq!(conf.lines().filter(|line| !line.starts_with('#')).count(), 5);
    }

    #[test]
    fn template_manifest_records_config() {
        let manifest: serde_json::Value = serde_json::from_str(&manifest()).expect("json");
        assert_eq!(manifest["template_version"], "1");
        assert_eq!(manifest["boards"], serde_json::json!(BOARDS));
        assert_eq!(manifest["config"].as_object().map(|map| map.len()), Some(5));
    }
}
