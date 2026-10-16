//! `poly publish`：商店提审薄层（T-M3-12 起接快应用联盟商店；骨架 = poly-publish 的零分叉驱动）。
//!
//! 语义（自动化边界 R4：止步提审，绝不自动发布；回执永久保留）：
//! - **渠道来源**：`--channel` 或目标声明 `TargetSpec.publish[0]`（快应用 = `quickapp-union-store`）；
//! - **产物定位**：`--package` 显式给；缺省按目标产物落点找
//!   `<packages>/<target>/miniapp/dist/*.rpk`（hap-toolkit 的 `.rpk`；多个按名序取尾并告警，
//!   零个 = `E_PUBLISH_0001`——先 `poly build --target quickapp` 出包）；
//! - **凭据引用模型**（同 T-M2-11 / T-M3-09）：`--credential env:VAR`，变量值 = base64(JSON)
//!   载荷；本层只解析**形态**（字段名），值零落屏、零落盘；
//! - **幂等重跑**：回执（`<root>/.polypack/publish/<channel>/<key>.json`）由 poly-publish 驱动
//!   续走——终态重跑 = no-op 不触网，非终态从断点续走（上传 / 提审不重复）；
//! - **端点显式注入**：快应用联盟商店无公开 CI 端点（2026-10 核：quickapp.cn / 各厂商文档
//!   只有 web 控制台与 IDE 上传，无 CI API 面），`--api-base` 必填、无缺省域名；协议面为
//!   **最小模型，待真账号联调**（R4 记录在案）；
//! - **载具 pre-flight**：代码包经 `Package::load`（poly-publish 公开入口，自动判别目录 /
//!   单文件）读到**驱动同源的摘要**——dry-run 报 `package_carrier` / `package_digest` /
//!   `receipt_key`（回执键 = appid-版本-摘要前 16，幂等重跑的落点锚），rpk 腿与四步驱动
//!   零分叉同口径；
//! - **渠道客户端缺席**（publish-client 的 T-M3-10 尚缺 builtin() 内快应用客户端）→
//!   `E_PUBLISH_0002` 如实点名归属，绝不假装提审过；`--dry-run` 面不受影响，可先行验证
//!   输入 / 凭据 / 定位 / 载具。
//!
//! 接口缺口（T-M3-12 报 'main'；main@cd1fcdb 后状态）：①`Channel::QuickApp` 备位已入主干
//! （`from_id("quickapp-union-store")` 可反解）；②`builtin()` 仍为微信 / 支付宝 / 抖音三平台，
//! 快应用客户端与③端点注入面（`--api-base` → 客户端，现无端点形参）归 publish-client；
//! ④单文件载具 `Package::load_file` 已入主干，本层 pre-flight 即经 `Package::load` 接上。
//! 本层已做到调用面（[`poly_publish::publish`] 四步驱动 + 公开入口），客户端进 `builtin()`
//! 后零改动激活（`find_publisher` 自动返回；端点一行接线点在真腿处标注）。

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::{CredentialRef, codes};
use poly_publish::secret::{CredentialProvider, EnvProvider, resolve_payload};
use poly_publish::{
    Archive, Package, PublishError, PublishRequest, Publisher, Transport, UreqTransport,
};
use poly_targets::{Registry, TargetSpec};

use crate::project;

/// 快应用联盟商店渠道 id（registry `TargetSpec.publish` 名，三处同口径）。
const UNION_STORE_CHANNEL: &str = "quickapp-union-store";

/// 小程序族产物目录名（`<packages>/<target>/miniapp/`，与 poly-bundle / verify 同口径）。
const MINIAPP_DIR: &str = "miniapp";
/// 代码包目录名（`<miniapp>/dist/`，快应用的 `.rpk` 落在这里；同上）。
const DIST_DIR: &str = "dist";

/// `poly publish` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 目标 id（产物定位与渠道声明的来源；缺省 quickapp）。
    #[arg(long, default_value = "quickapp")]
    pub target: String,
    /// 渠道 id（registry `publish` 名；缺省取目标声明的第一个）。
    #[arg(long)]
    pub channel: Option<String>,
    /// 平台应用 id。
    #[arg(long)]
    pub appid: String,
    /// 版本号。
    #[arg(long)]
    pub version: String,
    /// 版本描述（上传 / 提审都会带上）。
    #[arg(long, default_value = "")]
    pub description: String,
    /// 代码包路径（缺省 = 按目标产物落点定位快应用 `.rpk`；目录载体亦可）。
    #[arg(long)]
    pub package: Option<PathBuf>,
    /// 凭据引用（`env:变量名`；变量值 = base64(JSON) 载荷，同 T-M2-11 模型）。
    #[arg(long, value_name = "REF")]
    pub credential: String,
    /// 提审材料 JSON 文件（平台各自解释；快应用联盟为最小模型，待真账号联调）。
    #[arg(long)]
    pub materials: Option<PathBuf>,
    /// 平台 API 端点（快应用联盟商店必填：无公开 CI 端点，显式注入、无缺省域名）。
    #[arg(long, value_name = "URL")]
    pub api_base: Option<String>,
    /// 只解析输入（渠道 / 产物 / 凭据形态 / 端点），不触网、不落回执。
    #[arg(long)]
    pub dry_run: bool,
}

/// 一次提审的解析结果（dry-run 与真腿共用；值零落屏）。
#[derive(Debug)]
struct Prepared {
    /// 渠道 id。
    channel: String,
    /// 代码包路径（文件 = `.rpk`；目录 = 各平台代码包目录）。
    package: PathBuf,
    /// 代码包字节数（文件载体才给）。
    package_bytes: Option<u64>,
    /// 载具形态（`single-file` = `.rpk` 等单文件；`dir` = T-M3-06 产物目录）。
    carrier: &'static str,
    /// 载具摘要（blake3 hex；与四步驱动 `Package::load` 同源同口径）。
    digest: String,
    /// 回执键（appid-版本-摘要前 16；幂等重跑 = 同键续走 / 终态 no-op）。
    receipt_key: String,
    /// 凭据引用（引用模型；值经后端链现取）。
    reference: CredentialRef,
    /// 命中的凭据后端 id（`env` 等）。
    provider: &'static str,
    /// 凭据载荷字段名（有序；**只报形态，不报值**）。
    payload_fields: Vec<String>,
    /// 显式注入的端点（快应用联盟必填）。
    api_base: Option<String>,
    /// 提审材料（文件解析后）。
    materials: Option<serde_json::Value>,
}

/// 执行 `poly publish`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let registry = Registry::builtin();
    let spec = registry.get(&args.target).cloned().ok_or_else(|| {
        Diagnostic::error(
            codes::E_TGT_0001,
            format!("目标 {} 不在内建注册表", args.target),
        )
        .with_hint("用 poly doctor 核对目标矩阵")
    })?;
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(EnvProvider)];
    let prepared = prepare(args, &project, &spec, &providers)?;

    if args.dry_run {
        project::report(
            "publish",
            json,
            serde_json::json!({
                "event": "publish-dry-run",
                "channel": prepared.channel,
                "target": spec.id,
                "appid": args.appid,
                "version": args.version,
                "package": prepared.package.display().to_string(),
                "package_bytes": prepared.package_bytes,
                "package_carrier": prepared.carrier,
                "package_digest": prepared.digest,
                "receipt_key": prepared.receipt_key,
                "credential_reference": prepared.reference.name,
                "credential_provider": prepared.provider,
                "credential_fields": prepared.payload_fields,
                "api_base": prepared.api_base,
                "materials": prepared.materials.is_some(),
                "note": "dry-run：只解析输入，未触网、未落回执（R4：自动化止步提审）",
            }),
        );
        return Ok(0);
    }

    // 真腿：渠道客户端（builtin() 尚无快应用客户端 → E_PUBLISH_0002 缺口诊断）+ 四步驱动。
    let transport = UreqTransport::new().map_err(publish_error)?;
    let publisher = find_publisher(&prepared.channel, Arc::new(transport))?;
    // 端点注入面（T-M3-12 缺口②，归 publish-client）：现 `builtin(transport)` 无端点形参；
    // `prepared.api_base`（快应用联盟必填）已解析校验，客户端接口落位后在此一行接线——
    // 本层不预改 poly-publish 核心。四步驱动的载具入口 `Package::load` 已自动判别单文件
    // （pre-flight 摘要 / 回执键与其同源），rpk leg 至此除客户端外全通。
    let archive = Archive::new(&project.root);
    let mut request = PublishRequest::new(
        args.appid.clone(),
        args.version.clone(),
        args.description.clone(),
        prepared.package.clone(),
        prepared.reference.clone(),
    );
    if let Some(materials) = prepared.materials.clone() {
        request = request.with_materials(materials);
    }
    let outcome = poly_publish::publish(publisher.as_ref(), &request, &providers, &archive)
        .map_err(publish_error)?;
    let receipt = &outcome.receipt;
    project::report(
        "publish",
        json,
        serde_json::json!({
            "event": "publish",
            "channel": receipt.channel,
            "appid": receipt.appid,
            "version": receipt.version,
            "digest": receipt.digest,
            "state": receipt.state.to_string(),
            "runs": receipt.runs,
            "resumed_from": match outcome.resumed_from {
                Some(state) => state.to_string(),
                None => "新执行".to_string(),
            },
            "receipt": outcome.path.display().to_string(),
            "note": "R4：自动化止步提审——审核结果以平台为准，重跑同键命令只续轮询 / no-op",
        }),
    );
    Ok(0)
}

/// 解析输入（渠道 / 代码包 / 凭据形态 / 端点 / 材料）；dry-run 与真腿共用同一份口径。
fn prepare(
    args: &Args,
    project: &poly_core::Project,
    spec: &TargetSpec,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<Prepared, Diagnostic> {
    // 渠道：显式优先，否则取目标声明的第一个（快应用 = quickapp-union-store）。
    let channel = args
        .channel
        .clone()
        .or_else(|| spec.publish.first().cloned())
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("目标 {} 未声明发布渠道（TargetSpec.publish 为空）", spec.id),
            )
            .with_hint("用 --channel 显式指定，或核对目标注册表的 publish 声明")
        })?;

    // 端点：快应用联盟商店无公开 CI 端点 → 必填、无缺省域名（不许猜一个域名去撞）。
    if channel == UNION_STORE_CHANNEL && args.api_base.is_none() {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("渠道 {channel} 未注入 API 端点（--api-base 必填）"),
        )
        .with_hint(
            "快应用联盟商店无公开 CI 端点（web 控制台 / IDE 上传），端点必须显式注入；\
             协议面为最小模型，待真账号联调（R4 记录在案）",
        ));
    }

    // 代码包：显式路径原样透传；缺省定位目标产物落点里的 `.rpk`。
    let (package, package_bytes) = match &args.package {
        Some(path) => (path.clone(), file_bytes(path)?),
        None => locate_rpk(project, spec)?,
    };

    // 载具 pre-flight（rpk 腿接线）：经 poly-publish 公开入口 `Package::load` 自动判别载具，
    // 拿到与四步驱动**同源**的摘要，据此算回执键——dry-run 即完整预演幂等重跑的落点锚。
    let carrier = Package::load(&package).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("代码包 {} 不可用作载具：{err}", package.display()),
        )
        .with_hint(
            "载具 = 单文件（快应用 .rpk）或 T-M3-06 产物目录（<…>/miniapp/dist，根须有 app.json）；\
             先 poly build --target quickapp 重新出包",
        )
    })?;
    let digest = carrier.digest().to_string();
    let carrier_kind = if carrier.single_file().is_some() {
        "single-file"
    } else {
        "dir"
    };
    let receipt_key = poly_publish::receipt_key(&args.appid, &args.version, &digest);

    // 凭据引用（引用模型）：`env:VAR` → CredentialRef + EnvProvider 现取；只留形态不留值。
    let (source, alias) = crate::retry::parse_credential(&args.credential)?;
    let reference = CredentialRef {
        name: alias.unwrap_or_else(|| format!("{channel}.{}", args.appid)),
        source,
    };
    let (payload, provider) = resolve_payload(&reference, providers).map_err(publish_error)?;
    let payload_fields = payload_fields(&payload)?;

    // 提审材料（平台各自解释；快应用联盟为最小模型）。
    let materials = match &args.materials {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|err| {
                Diagnostic::error(
                    codes::E_PUBLISH_0001,
                    format!("提审材料 {} 不可读：{err}", path.display()),
                )
            })?;
            Some(serde_json::from_str(&text).map_err(|err| {
                Diagnostic::error(
                    codes::E_PUBLISH_0001,
                    format!("提审材料 {} 不是合法 JSON：{err}", path.display()),
                )
            })?)
        }
        None => None,
    };

    Ok(Prepared {
        channel,
        package,
        package_bytes,
        carrier: carrier_kind,
        digest,
        receipt_key,
        reference,
        provider,
        payload_fields,
        api_base: args.api_base.clone(),
        materials,
    })
}

/// 缺省定位：`<packages>/<target>/miniapp/dist/*.rpk`（hap-toolkit 产物；名序取尾，多个告警）。
fn locate_rpk(
    project: &poly_core::Project,
    spec: &TargetSpec,
) -> Result<(PathBuf, Option<u64>), Diagnostic> {
    let dist = project::packages_root(&project.root)
        .join(&spec.id)
        .join(MINIAPP_DIR)
        .join(DIST_DIR);
    let mut rpks: Vec<PathBuf> = std::fs::read_dir(&dist)
        .map_err(|err| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("目标 {} 无产物目录 {}（{err}）", spec.id, dist.display()),
            )
            .with_hint(format!(
                "先跑 poly build --target {} 出包（hap-toolkit 在场时直出 .rpk），或用 --package 指定",
                spec.id
            ))
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rpk"))
        .collect();
    rpks.sort();
    let Some(package) = rpks.pop() else {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("{} 下没有 .rpk（目标 {}）", dist.display(), spec.id),
        )
        .with_hint(
            "hap-toolkit 不在场时打包腿如实跳过（无 .rpk）；装齐工具链重跑 poly build，\
             或用 --package 显式指定代码包",
        ));
    };
    if !rpks.is_empty() {
        // 口径同 verify 的快应用腿：名序取尾，多份如实告警（不静默挑一件）
        eprintln!(
            "publish 告警：{} 下有 {} 个 .rpk，按名序取尾 {}（其余：{}）",
            dist.display(),
            rpks.len() + 1,
            package.display(),
            rpks.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }
    let bytes = file_bytes(&package)?;
    Ok((package, bytes))
}

/// 文件载体给字节数；目录载体给 `None`（代码包形态由 poly-publish 的 `Package` 判别）。
fn file_bytes(path: &std::path::Path) -> Result<Option<u64>, Diagnostic> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
        Ok(_) => Ok(None),
        Err(err) => Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("代码包 {} 不可读：{err}", path.display()),
        )),
    }
}

/// 凭据载荷形态：base64(JSON) → 顶层字段名（有序）。值零落屏——只回字段名。
fn payload_fields(payload: &[u8]) -> Result<Vec<String>, Diagnostic> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|err| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("凭据载荷不是合法 base64({err})"),
            )
            .with_hint("env 后端的变量值须为 base64(JSON) 载荷（同 T-M2-11 模型）")
        })?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("凭据载荷不是合法 JSON({err})"),
        )
        .with_hint("env 后端的变量值须为 base64(JSON) 载荷（同 T-M2-11 模型）")
    })?;
    let Some(object) = value.as_object() else {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            "凭据载荷 JSON 顶层应为对象（字段名可选报，值从不落屏）",
        ));
    };
    let mut fields: Vec<String> = object.keys().cloned().collect();
    fields.sort();
    Ok(fields)
}

/// 渠道客户端查找（真腿）：注册表 `publish` 名 → poly-publish `Channel` 备位 → `builtin()` 内客户端。
///
/// 任一层缺席 → `E_PUBLISH_0002` 缺口诊断（点名渠道与归属），绝不假装提审过。
/// 自动激活：备位已入主干（main@cd1fcdb 起 `from_id` 可反解 QuickApp）——客户端进
/// `builtin()` 后本函数原样返回，测试 Ok 臂同日转绿。
fn find_publisher(
    channel: &str,
    transport: Arc<dyn Transport>,
) -> Result<Box<dyn Publisher>, Diagnostic> {
    let gap = |reason: String| {
        Diagnostic::error(
            codes::E_PUBLISH_0002,
            format!("渠道 {channel} 的提审客户端未落位：{reason}"),
        )
        .with_hint(
            "快应用联盟商店：Channel 备位已入主干（main@cd1fcdb）；尚缺 builtin() 内客户端 \
             与端点注入面，归 publish-client（T-M3-10 后续）——本层已做到调用面，\
             落位后零改动激活（--dry-run 可先行验证输入 / 载具）",
        )
    };
    let Some(wanted) = poly_publish::Channel::from_id(channel) else {
        return Err(gap(
            "poly-publish 无该渠道备位（Channel::from_id 未识别）".to_string()
        ));
    };
    poly_publish::builtin(transport)
        .into_iter()
        .find(|publisher| publisher.channel() == wanted)
        .ok_or_else(|| gap("builtin() 里没有该渠道的客户端".to_string()))
}

/// `PublishError` → 诊断：错误码原样透传（`poly::E_PUB_*`，绝不重新编号），提示取 `hint()`。
fn publish_error(err: PublishError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(Code::new(err.code()), err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::CredentialSource;
    use poly_publish::MemoryProvider;

    /// 最小工程骨架：`polypack.toml` + 空的产物落点。
    fn fixture(tag: &str) -> (PathBuf, poly_core::Project) {
        let root =
            std::env::temp_dir().join(format!("polypack-publish-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("工程目录可建");
        std::fs::write(
            root.join("polypack.toml"),
            "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
        )
        .expect("写配置");
        let project = crate::project::load(&root.join("polypack.toml")).expect("配置可加载");
        (root, project)
    }

    /// 种一个 `.rpk` 到目标产物落点（内容随意——本层只看定位与字节数）。
    fn seed_rpk(root: &std::path::Path, target: &str, name: &str) -> PathBuf {
        let dist = root
            .join(".polypack/packages")
            .join(target)
            .join("miniapp/dist");
        std::fs::create_dir_all(&dist).expect("dist 可建");
        let path = dist.join(name);
        std::fs::write(&path, b"PK\x03\x04stub-rpk").expect("写 .rpk");
        path
    }

    fn spec(target: &str) -> TargetSpec {
        Registry::builtin()
            .get(target)
            .cloned()
            .unwrap_or_else(|| panic!("注册表缺少 {target}"))
    }

    fn args(target: &str) -> Args {
        Args {
            config: PathBuf::from("polypack.toml"),
            target: target.to_string(),
            channel: None,
            appid: "com.polypack.fixture".to_string(),
            version: "0.1.0".to_string(),
            description: "契约测试".to_string(),
            package: None,
            // 条目名经 `keyring:条目名` 覆盖档案名（env 的变量名不覆盖）；后端用 memory 注入值
            credential: "keyring:union".to_string(),
            materials: None,
            api_base: None,
            dry_run: true,
        }
    }

    /// 内存后端一条载荷（值 = base64({"token":"s3cret-value"})；测试断言它绝不落屏）。
    fn memory() -> Vec<Box<dyn CredentialProvider>> {
        vec![Box::new(MemoryProvider::with_entry(
            "union",
            base64::engine::general_purpose::STANDARD.encode(r#"{"token":"s3cret-value"}"#),
        ))]
    }

    #[test]
    fn rpk_locator_picks_by_name_order_and_reports_bytes() {
        let (root, project) = fixture("locate");
        seed_rpk(&root, "quickapp", "a.debug.0.1.0.rpk");
        let last = seed_rpk(&root, "quickapp", "com.polypack.fixture.debug.0.1.0.rpk");
        let (path, bytes) = locate_rpk(&project, &spec("quickapp")).expect("定位须成功");
        assert_eq!(path, last, "名序取尾（口径同 verify 的快应用腿）");
        assert_eq!(
            bytes,
            Some(12),
            "文件载体给字节数（PK\\x03\\x04 + stub-rpk）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rpk_locator_without_artifact_is_input_diagnostic() {
        let (root, project) = fixture("locate-empty");
        std::fs::create_dir_all(root.join(".polypack/packages/quickapp/miniapp/dist"))
            .expect("空 dist");
        let err = locate_rpk(&project, &spec("quickapp")).expect_err("无 .rpk 必须报输入未就绪");
        assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
        assert!(err.message.contains("没有 .rpk"), "{}", err.message);
        assert!(
            err.hint.unwrap_or_default().contains("poly build"),
            "提示须给动作"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn union_store_requires_explicit_endpoint() {
        let (root, project) = fixture("endpoint");
        seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
        let err = prepare(&args("quickapp"), &project, &spec("quickapp"), &memory())
            .expect_err("联盟商店缺 --api-base 必须报错");
        assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
        assert!(err.message.contains("--api-base"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_reports_credential_shape_never_values() {
        let (root, project) = fixture("prepare");
        seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
        let mut a = args("quickapp");
        a.api_base = Some("https://example.invalid/union".to_string());
        let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
        assert_eq!(prepared.channel, "quickapp-union-store", "渠道来自目标声明");
        assert_eq!(prepared.provider, "memory");
        assert_eq!(prepared.payload_fields, vec!["token".to_string()]);
        assert!(
            !format!("{prepared:?}").contains("s3cret"),
            "凭据值绝不进 Debug"
        );
        assert_eq!(
            prepared.api_base.as_deref(),
            Some("https://example.invalid/union")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 自动激活接缝：渠道备位 / 客户端缺席 → 缺口诊断；落位后同一断言走下一阶段（返回客户端）。
    #[test]
    fn union_store_gap_is_named_until_the_client_lands() {
        struct PanicTransport;
        impl Transport for PanicTransport {
            fn send(
                &self,
                _request: &poly_publish::HttpRequest,
            ) -> Result<poly_publish::HttpResponse, PublishError> {
                panic!("查找客户端不得触网");
            }
        }
        match find_publisher(UNION_STORE_CHANNEL, Arc::new(PanicTransport)) {
            Err(diagnostic) => {
                // 现态（main@cd1fcdb 后）：备位已反解，缺口在 builtin() 内客户端——
                // 如实点名归属与阶段，绝不假绿
                assert_eq!(diagnostic.code.to_string(), "poly::E_PUBLISH_0002");
                assert!(
                    diagnostic.message.contains("quickapp-union-store"),
                    "{}",
                    diagnostic.message
                );
                assert!(
                    diagnostic.message.contains("builtin()"),
                    "备位已入主干（from_id 已反解），缺口应精确到客户端：{}",
                    diagnostic.message
                );
                let hint = diagnostic.hint.unwrap_or_default();
                assert!(hint.contains("publish-client"), "缺口须点名归属：{hint}");
                assert!(hint.contains("T-M3-10"), "归属到任务：{hint}");
            }
            Ok(publisher) => {
                // 客户端进 builtin() 当日（自动激活）：下一阶段是端点注入面接线
                assert_eq!(publisher.channel().id(), UNION_STORE_CHANNEL);
            }
        }
    }

    /// rpk 腿接线（main@cd1fcdb 单文件载具）：pre-flight 摘要 / 回执键与四步驱动同源。
    ///
    /// pre-flight 走 `Package::load`（驱动同一入口）——断言逐字符相等 = 驱动日后重算的
    /// 回执键必与本层预演的相同（幂等重跑锚点），零分叉。
    #[test]
    fn rpk_leg_preflight_digest_and_receipt_key_match_the_driver_entry() {
        let (root, project) = fixture("rpk-leg");
        let seeded = seed_rpk(&root, "quickapp", "com.polypack.fixture.debug.0.1.0.rpk");
        let mut a = args("quickapp");
        a.api_base = Some("https://example.invalid/union".to_string());
        let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
        assert_eq!(prepared.carrier, "single-file", "rpk 判为单文件载具");
        let driven = Package::load(&seeded).expect("驱动入口可读同一 rpk");
        assert_eq!(
            prepared.digest,
            driven.digest(),
            "pre-flight 摘要 = 驱动同源摘要（Package::load 同入口同口径）"
        );
        assert_eq!(
            prepared.receipt_key,
            poly_publish::receipt_key(&a.appid, &a.version, driven.digest()),
            "回执键 = appid-版本-摘要前 16（驱动落回执同键）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 载具 pre-flight 的拒绝面：空 .rpk（构建未完成）与目录载具一样要如实判别。
    #[test]
    fn carrier_preflight_rejects_empty_rpk_and_accepts_dir() {
        let (root, project) = fixture("carrier");
        let empty = seed_rpk(&root, "quickapp", "empty.debug.0.1.0.rpk");
        std::fs::write(&empty, b"").expect("空文件");
        let mut a = args("quickapp");
        a.api_base = Some("https://example.invalid/union".to_string());
        let err = prepare(&a, &project, &spec("quickapp"), &memory())
            .expect_err("空 .rpk 必须报输入未就绪");
        assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
        assert!(
            err.hint.unwrap_or_default().contains("poly build"),
            "提示须给重建动作"
        );

        // 目录载具（T-M3-06 产物形态）同样过 pre-flight：显式 --package 指向产物目录
        let dist = root.join("dist");
        std::fs::create_dir_all(&dist).expect("建目录");
        std::fs::write(dist.join("app.json"), r#"{"pages":["pages/index/index"]}"#).expect("写");
        std::fs::write(dist.join("app.js"), "App({})").expect("写");
        let mut a = args("quickapp");
        a.api_base = Some("https://example.invalid/union".to_string());
        a.package = Some(dist);
        let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("目录载具须过");
        assert_eq!(prepared.carrier, "dir");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_end_to_end_stays_offline_and_writes_no_receipt() {
        let (root, project) = fixture("dry-run");
        seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
        let mut a = args("quickapp");
        a.api_base = Some("https://example.invalid/union".to_string());
        let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
        assert!(prepared.package.ends_with("x.debug.0.1.0.rpk"));
        assert!(
            !root.join(".polypack/publish").exists(),
            "dry-run 不得落回执目录"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn credential_payload_must_be_base64_json_object() {
        let fields = payload_fields(b"eyJhIjoxfQ==").expect("合法载荷"); // {"a":1}
        assert_eq!(fields, vec!["a".to_string()]);
        let err = payload_fields(b"not-base64!").expect_err("非 base64 必须报错");
        assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
        let err = payload_fields(b"bm90LWpzb24=").expect_err("非 JSON 必须报错"); // not-json
        assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
        let err = payload_fields(b"IjQyIg==").expect_err("非对象必须报错"); // "42"
        assert!(err.message.contains("顶层应为对象"), "{}", err.message);
    }

    #[test]
    fn foreign_error_codes_pass_through_unrenumbered() {
        let err = PublishError::CredentialUnavailable {
            name: "n".to_string(),
            providers: "env".to_string(),
            reason: "unset".to_string(),
        };
        let diagnostic = publish_error(err);
        assert_eq!(
            diagnostic.code.to_string(),
            "poly::E_PUB_0001",
            "码原样透传"
        );
        assert!(diagnostic.hint.is_some(), "提示取 hint()");
    }

    #[test]
    fn credential_source_is_the_referenced_env_variable() {
        // 引用翻译：env:VAR 的变量名进 CredentialSource（值仍由后端链现取）
        let (source, alias) = crate::retry::parse_credential("env:POLY_ANY").expect("引用合法");
        assert!(matches!(source, CredentialSource::Env { .. }));
        assert!(alias.is_none());
    }
}
