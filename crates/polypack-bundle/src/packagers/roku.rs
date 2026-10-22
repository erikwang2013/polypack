//! Roku 频道包打包器（T-M4-15）：SceneGraph 子集渲染器产物 → 频道 zip + 侧载结构自检。
//!
//! 链路（与 spike `ui/adapters/roku/scripts/build-and-check.mjs` 同口径）：
//! - 频道骨架（`<adapter>/channel/`：`source/main.brs` + `components/PolypackScene.brs`）入暂存；
//! - `manifest` 按产品元数据生成（title / major_version / minor_version / build_version +
//!   `ui_resolutions=fhd`）；
//! - 复用 spike 发射器（`node <adapter>/scripts/emit.mjs <staging>` → `components/PolypackScene.xml`
//!   + `source/focusgraph.brs`；焦点表与 adapter-tv 共享内核同源，T-M4-11 口径）；
//! - 确定性 zip（[`zipwrite`]：条目排序 + 固定时间戳，二跑字节一致）；
//! - [`check_roku_structure`] 结构自检——与 verify 腿 `sideload-structure-check` **同一实现**。
//!
//! 边界（如实注记）：真机侧载腿本机不可得（无 Roku SDK / 设备 / 模拟器）——结构自检是本机
//! 真跑面，设备实证是 T-M4-12 决策写明的后续前置；侧载 zip 不签名（官方 makefile 的
//! png 免压 / 其余 `-9` 两段压缩策略按 T-M4-11 取证；本工具用确定性 Stored 写法），发布
//! `.pkg` 需 dev key + `plugin_package`，归发布侧。

use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo, version_parts};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::webapp;
use crate::zipwrite;

/// 目标 id 与产物格式（与 `TargetSpec.package.{format,builder}` 一致）。
pub const ID: &str = "roku";
const FORMAT: &str = "zip";

/// 打包暂存目录（`<out_dir>/roku/`：频道树暂存根，产物是它旁边的单文件）。
const WORK_DIR: &str = "roku";
/// 适配器包内频道骨架目录名。
const CHANNEL_DIR: &str = "channel";
/// 适配器包内发射器入口（T-M4-15 新增：复用 `src/emit.js` 的最小 CLI）。
const EMITTER: &str = "scripts/emit.mjs";
/// 适配器包内受限能力说明书（T-M4-11 交付，随产物发布）。
const CAPABILITY_DOC: &str = "CAPABILITY-BOUNDARY-DRAFT.md";
/// 工程根 → 适配器根（本仓布局；显式 `RokuInputs.adapter` 优先）。
///
/// 发射器经相对路径复用同级 `tv/` 与 `native/` 适配器的共享焦点内核——适配器根必须与
/// 这两个同级目录同布局；散装仅拷 `roku/` 会在 node 侧报模块缺失（E_BND_0004 原文直出）。
const ADAPTER_REL: &str = "ui/adapters/roku";

/// 频道清单文件名（暂存根 + zip 根）。
const MANIFEST: &str = "manifest";

mod check;

pub use check::{RokuStructure, check_roku_structure};

/// Roku 频道包打包器（`zip`）。
pub struct Roku;

impl Packager for Roku {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, FORMAT)?;
        inputs.product.validate(ID)?;
        let layout = layout(inputs)?;
        // 发射器是 JS（复用 spike，不重写）：node 缺席按环境性缺失跳过，绝不产出缺生成件的包。
        let node = exec::tool_path("node").ok_or_else(|| BundleError::ToolMissing {
            packager: ID,
            tool: "node".to_string(),
            hint: "安装 Node.js ≥ 20.11 并把 node 放进 PATH（SceneGraph 发射器复用 \
                   @polypack/adapter-roku 的 JS 实现，打包器经 node 调用）"
                .to_string(),
        })?;
        let stage = stage(inputs);
        Ok(BundlePlan::new()
            .with_script(stage.manifest.clone(), manifest_text(&inputs.product))
            .with_command(Command::new(
                node.display().to_string(),
                [
                    layout.emitter.display().to_string(),
                    stage.root.display().to_string(),
                ],
            )))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let layout = layout(inputs)?;
        let stage = stage(inputs);

        // 骨架入暂存 → 产品 manifest 覆盖骨架自带的 spike manifest → 发射器生成两件。
        let _ = std::fs::remove_dir_all(&stage.root);
        fsutil::copy_tree(&layout.channel, &stage.root)?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        let capability_source = layout.adapter.join(CAPABILITY_DOC);
        let capability_shipped =
            write_capability(spec, &inputs.product, &capability_source, &stage.capability)?;

        zipwrite::write_zip(&webapp::collect_files(&stage.root)?, &stage.zip)?;
        // 结构自检：产物必过自己的检查器（与 verify 腿同一实现，「结构真跑」的本地证据面）
        let structure = check_roku_structure(&stage.zip)?;

        let mut notes = vec![
            format!(
                "产物：{}（Roku 频道 zip，{} 条目；manifest + source/（main.brs + focusgraph.brs）\
                 + components/（PolypackScene.xml + .brs））",
                stage.zip.display(),
                structure.entries.len()
            ),
            format!(
                "manifest：title={}，版本 {}.{}.{}（生成自 polypack.toml；ui_resolutions=fhd）",
                structure.title, structure.major, structure.minor, structure.build
            ),
            format!(
                "焦点表：{} 节点，initial={}（与 adapter-tv 共享焦点内核同源，T-M4-11 口径）",
                structure.focus_nodes, structure.initial_focus
            ),
            format!(
                "受限能力说明书：{}（随产物发布{}；与 TargetSpec.experimental / notes 一致）",
                stage.capability.display(),
                if capability_shipped {
                    ""
                } else {
                    "——适配器内源文档缺席，按目标声明生成"
                }
            ),
            "侧载：真机侧载腿 SKIP-with-print——本机无 Roku SDK / 设备 / 模拟器（T-M4-12 \
             决策写明真机侧载为后续前置）；结构自检是本机真跑面"
                .to_string(),
            "签名：侧载 zip 不签名（官方 makefile 的 png 免压 / 其余 -9 两段压缩策略按 \
             T-M4-11 取证；本工具用确定性 Stored 写法）；发布 `.pkg` 需 dev key + \
             plugin_package，归发布侧"
                .to_string(),
        ];
        if let Some(boundary) = &spec.notes {
            notes.push(format!("目标声明边界：{boundary}"));
        }
        fsutil::file_artifact(ID, FORMAT, &stage.zip, notes)
    }
}

/// 解析后的输入面（适配器根 / 频道骨架 / 发射器入口）。
struct Layout {
    adapter: PathBuf,
    channel: PathBuf,
    emitter: PathBuf,
}

/// 暂存落点（`plan()` 与 `package()` 共用的纯路径推导）。
struct Stage {
    root: PathBuf,
    manifest: PathBuf,
    zip: PathBuf,
    capability: PathBuf,
}

fn stage(inputs: &BundleInputs) -> Stage {
    let root = inputs.out_dir.join(WORK_DIR);
    let stem = format!("{}-{}", inputs.product.kebab_name(), inputs.product.version);
    Stage {
        manifest: root.join(MANIFEST),
        root,
        zip: inputs.out_dir.join(format!("{stem}.zip")),
        capability: inputs.out_dir.join(format!("{stem}-capability.md")),
    }
}

/// 适配器根 → 频道骨架 / 发射器入口（缺任何一件都是缺输入，不产半成品包）。
fn layout(inputs: &BundleInputs) -> Result<Layout, BundleError> {
    let Some(roku) = inputs.roku.as_ref() else {
        return Err(missing("roku.adapter"));
    };
    let adapter = match &roku.adapter {
        Some(adapter) if adapter.is_dir() => adapter.clone(),
        Some(adapter) => {
            return Err(BundleError::InvalidInput {
                packager: ID,
                field: "roku.adapter",
                value: adapter.display().to_string(),
                reason: "显式适配器根不是目录".to_string(),
            });
        }
        None => {
            let path = roku.project_root.join(ADAPTER_REL);
            if !path.is_dir() {
                return Err(missing("roku.adapter"));
            }
            path
        }
    };
    let channel = roku
        .channel
        .clone()
        .unwrap_or_else(|| adapter.join(CHANNEL_DIR));
    if !channel.is_dir() {
        return Err(missing("roku.channel"));
    }
    // node 不认 Windows `\\?\` verbatim 前缀（canonicalize 常产出），参数侧先剥掉。
    let emitter = fsutil::strip_verbatim_prefix(adapter.join(EMITTER));
    if !emitter.is_file() {
        return Err(missing("roku.emitter"));
    }
    Ok(Layout {
        adapter,
        channel,
        emitter,
    })
}

/// 缺输入（CLI 按环境性缺失跳过目标）。
fn missing(field: &'static str) -> BundleError {
    BundleError::MissingInput {
        packager: ID,
        field,
    }
}

/// `manifest` 文本：产品元数据 + Roku fhd 画布（版本拆成三个数字段）。
fn manifest_text(product: &ProductInfo) -> String {
    let (major, minor, build) = version_parts(&product.version);
    format!(
        "title={}\nmajor_version={major}\nminor_version={minor}\nbuild_version={build}\n\
         ui_resolutions=fhd\n",
        product.name
    )
}

/// 受限能力说明书（纯文本 markdown，落 `<out_dir>/<stem>-capability.md`）：
/// 目标声明头 + 适配器包内 T-M4-11 文档正文。`Ok(false)` = 源文档缺席（已按声明生成）。
fn write_capability(
    spec: &TargetSpec,
    product: &ProductInfo,
    source: &Path,
    dest: &Path,
) -> Result<bool, BundleError> {
    let mut text = format!(
        "# Roku 频道受限能力说明书（随产物发布）\n\n\
         目标：`{}`（tier {}，experimental = {}）\n\n",
        spec.id, spec.tier, spec.experimental
    );
    if let Some(boundary) = &spec.notes {
        text.push_str(&format!("声明边界：{boundary}\n\n"));
    }
    text.push_str(&format!(
        "产物：`{}-{}.zip`（SceneGraph 子集受限渲染，BrightScript 原生逻辑；核心不参与）\n\n---\n\n",
        product.kebab_name(),
        product.version
    ));
    let shipped = source.is_file();
    if shipped {
        let body = std::fs::read_to_string(source).map_err(|err| BundleError::Io {
            path: source.to_path_buf(),
            source: err,
        })?;
        text.push_str(&body);
    } else {
        text.push_str(
            "（适配器包内未找到 CAPABILITY-BOUNDARY-DRAFT.md——边界以上方目标声明为准）\n",
        );
    }
    fsutil::write_text(dest, &text)?;
    Ok(shipped)
}

/// 打包格式与打包器声明必须一致（防「按 zip 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}
