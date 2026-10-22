//! `polypack preview`：设备预览池协议（T-M9-20）。
//!
//! 本模块只定义 **DevicePool / DeviceLease** 类型与确定性计划生成，不做真实设备执行。
//! `polypack preview --dry-run` 是纯函数路径：不读凭据、不起子进程、不建 socket，
//! 只把目标/设备列表展开成带租约、超时与清理语义的 JSON 计划。
//! 真实安装 / 启动 / 截图（T-M9-21）是外部环境工作，见 `docs/DEVICE-PREVIEW.md`。
//!
//! 词汇与农场协议 v1 对齐：计划声明 `farm_protocol = "farm/v1"` 与
//! `job_kind = "preview"`，租约错误复用 `polypack::E_FARM_*` 稳定码。

use polypack_core::{Diagnostic, codes};
use serde::{Deserialize, Serialize};

/// 预览协议版本（计划 JSON 的 `protocol` 字段）。
pub const PROTOCOL: &str = "preview/v1";

/// 兼容的农场协议版本（设备池是农场协议 v1 的扩展）。
pub const FARM_PROTOCOL: &str = polypack_core::farm::PROTOCOL;

/// 任务类型：农场 v1 的 job 帧若扩展 `preview` 任务，使用该标识。
pub const JOB_KIND: &str = "preview";

/// 设备 provider。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    /// Firebase Test Lab（Google Cloud）。
    FirebaseTestLab,
    /// BrowserStack App Automate。
    BrowserstackAppAutomate,
    /// AWS Device Farm。
    AwsDeviceFarm,
    /// 自建设备池（adb / STF / Appium）。
    SelfHosted,
}

impl Provider {
    /// 全部 provider（错误提示与文档用）。
    pub const ALL: [Provider; 4] = [
        Provider::FirebaseTestLab,
        Provider::BrowserstackAppAutomate,
        Provider::AwsDeviceFarm,
        Provider::SelfHosted,
    ];

    /// 解析 provider 名（接受短别名；输出始终是规范 kebab-case 名）。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "firebase-test-lab" | "firebase" => Some(Self::FirebaseTestLab),
            "browserstack-app-automate" | "browserstack" => Some(Self::BrowserstackAppAutomate),
            "aws-device-farm" | "aws" => Some(Self::AwsDeviceFarm),
            "self-hosted" | "self" => Some(Self::SelfHosted),
            _ => None,
        }
    }

    /// 规范名称（序列化值）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirebaseTestLab => "firebase-test-lab",
            Self::BrowserstackAppAutomate => "browserstack-app-automate",
            Self::AwsDeviceFarm => "aws-device-farm",
            Self::SelfHosted => "self-hosted",
        }
    }
}

/// 一台可租用设备：`<provider>/<device-id>/<os>[/<target>]`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSpec {
    /// 设备来源。
    pub provider: Provider,
    /// provider 内的设备标识（型号、序列号或 `host:port`）。
    pub device_id: String,
    /// 设备操作系统（如 `android` / `ios` / `tvos`）。
    pub os: String,
    /// PolyPack 目标 id；缺省等于 `os`。
    pub target: String,
}

impl DeviceSpec {
    /// 解析 CLI 设备表达式；target 缺省等于 os。
    pub fn parse(raw: &str) -> Result<Self, Diagnostic> {
        let parts: Vec<&str> = raw.split('/').collect();
        let (provider, device_id, os, target) = match parts.as_slice() {
            [provider, device_id, os] => (*provider, *device_id, *os, *os),
            [provider, device_id, os, target] => (*provider, *device_id, *os, *target),
            _ => {
                return Err(invalid(format!(
                    "设备表达式非法：{raw}（需要 <provider>/<device-id>/<os>[/<target>]）"
                )));
            }
        };
        let provider = Provider::parse(provider).ok_or_else(|| {
            let valid = Provider::ALL
                .iter()
                .map(|provider| provider.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            invalid(format!("未知设备 provider：{provider}（可用：{valid}）"))
        })?;
        validate_component("device-id", device_id)?;
        validate_component("os", os)?;
        validate_component("target", target)?;
        Ok(Self {
            provider,
            device_id: device_id.to_string(),
            os: os.to_string(),
            target: target.to_string(),
        })
    }
}

/// 租约到期的清理动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupAction {
    /// 正常释放租约。
    Release,
    /// 释放租约，并保留证据目录。
    ReleaseAndArchive,
    /// 超时强制释放（不等待 provider 确认）。
    ForceRelease,
}

/// 租约清理语义（随每份租约下发）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupPolicy {
    /// 成功路径的清理动作。
    pub on_success: CleanupAction,
    /// 失败路径的清理动作。
    pub on_failure: CleanupAction,
    /// 超时路径的清理动作。
    pub on_timeout: CleanupAction,
    /// 释放前是否卸载夹具应用。
    pub uninstall: bool,
    /// 证据目录（相对工程根；截图 / logcat / 设备信息）。
    pub evidence_dir: String,
}

/// 一份设备租约：租约 id、设备、时间窗与清理语义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceLease {
    /// 租约标识（计划内稳定且唯一）。
    pub lease_id: String,
    /// 设备来源。
    pub provider: Provider,
    /// provider 内的设备标识。
    pub device_id: String,
    /// 设备操作系统。
    pub os: String,
    /// PolyPack 目标 id。
    pub target: String,
    /// 租约开始时间（Unix 秒）。
    pub acquired_at: u64,
    /// 租约到期时间（Unix 秒 = acquired_at + timeout_secs）。
    pub expires_at: u64,
    /// 租约时长（秒）。
    pub timeout_secs: u64,
    /// 清理语义。
    pub cleanup: CleanupPolicy,
}

/// 设备池：一组待租用设备（dry-run 不探测在线状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePool {
    /// 池内设备（按声明序）。
    pub devices: Vec<DeviceSpec>,
}

impl DevicePool {
    /// 构造并校验设备池：非空、字段合法、同一 provider 内设备不重复。
    pub fn new(devices: Vec<DeviceSpec>) -> Result<Self, Diagnostic> {
        if devices.is_empty() {
            return Err(invalid(
                "设备列表为空：至少给一个 --device <provider>/<device-id>/<os>[/<target>]",
            ));
        }
        for (index, device) in devices.iter().enumerate() {
            validate_component("device-id", &device.device_id)?;
            validate_component("os", &device.os)?;
            validate_component("target", &device.target)?;
            if devices[..index]
                .iter()
                .any(|prev| prev.provider == device.provider && prev.device_id == device.device_id)
            {
                return Err(invalid(format!(
                    "设备重复租用：{}/{}（同池内每台设备只能出现一次）",
                    device.provider.as_str(),
                    device.device_id
                )));
            }
        }
        Ok(Self { devices })
    }

    /// 生成确定性计划：租约按声明序编号，同一输入与起点返回逐字节相同的 JSON。
    pub fn plan(&self, options: &PlanOptions) -> Result<PreviewPlan, Diagnostic> {
        if options.timeout_secs == 0 {
            return Err(
                Diagnostic::error(codes::E_PLAN_0001, "租约时长必须大于 0 秒")
                    .with_hint("用 --timeout <秒> 指定租约窗口（例如 900）"),
            );
        }
        let expires_at = options
            .acquired_at
            .checked_add(options.timeout_secs)
            .ok_or_else(|| {
                Diagnostic::error(codes::E_PLAN_0001, "租约到期时间溢出 u64")
                    .with_hint("减小 --at / --timeout")
            })?;
        let leases = self
            .devices
            .iter()
            .enumerate()
            .map(|(index, device)| DeviceLease {
                lease_id: format!("lease-{:04}", index + 1),
                provider: device.provider,
                device_id: device.device_id.clone(),
                os: device.os.clone(),
                target: device.target.clone(),
                acquired_at: options.acquired_at,
                expires_at,
                timeout_secs: options.timeout_secs,
                cleanup: CleanupPolicy {
                    on_success: CleanupAction::Release,
                    on_failure: CleanupAction::ReleaseAndArchive,
                    on_timeout: CleanupAction::ForceRelease,
                    uninstall: true,
                    evidence_dir: format!(
                        ".polypack/preview/{}/{}/",
                        device.target, device.device_id
                    ),
                },
            })
            .collect();
        Ok(PreviewPlan {
            protocol: PROTOCOL.to_string(),
            farm_protocol: FARM_PROTOCOL.to_string(),
            job_kind: JOB_KIND.to_string(),
            dry_run: true,
            leases,
        })
    }
}

/// 计划生成参数（显式时间让输出可复现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanOptions {
    /// 计划起点（Unix 秒）。
    pub acquired_at: u64,
    /// 每份租约的时长（秒）。
    pub timeout_secs: u64,
}

/// 预览计划（`polypack preview --dry-run` 的 JSON 载荷）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewPlan {
    /// 预览协议版本。
    pub protocol: String,
    /// 兼容的农场协议版本。
    pub farm_protocol: String,
    /// 任务类型（`preview`）。
    pub job_kind: String,
    /// 始终为 true：计划不接触设备或网络。
    pub dry_run: bool,
    /// 设备租约（按设备声明序）。
    pub leases: Vec<DeviceLease>,
}

/// `polypack preview` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 只生成计划，不接触设备 / 网络（当前唯一支持的运行方式）。
    #[arg(long)]
    pub dry_run: bool,
    /// 设备：`<provider>/<device-id>/<os>[/<target>]`（可重复；target 缺省等于 os）。
    #[arg(long = "device", value_name = "PROVIDER/DEVICE/OS[/TARGET]")]
    pub devices: Vec<String>,
    /// 每份租约的时长（秒）。
    #[arg(long, default_value_t = 900)]
    pub timeout: u64,
    /// 计划起点（Unix 秒；缺省 0 使 dry-run 输出可复现）。
    #[arg(long, default_value_t = 0)]
    pub at: u64,
}

/// 执行 `polypack preview`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    if !args.dry_run {
        return Err(Diagnostic::error(
            codes::E_FARM_0005,
            "真实设备预览尚未接线（T-M9-21 为外部环境工作，不伪造执行）",
        )
        .with_hint(
            "当前只支持 `polypack preview --dry-run`；真实安装 / 启动 / 截图步骤与证据要求见 \
             docs/DEVICE-PREVIEW.md §8",
        ));
    }
    let devices = args
        .devices
        .iter()
        .map(|raw| DeviceSpec::parse(raw))
        .collect::<Result<Vec<_>, _>>()?;
    let pool = DevicePool::new(devices)?;
    let plan = pool.plan(&PlanOptions {
        acquired_at: args.at,
        timeout_secs: args.timeout,
    })?;
    let rendered = if json {
        serde_json::to_string(&plan).expect("预览计划可序列化")
    } else {
        serde_json::to_string_pretty(&plan).expect("预览计划可序列化")
    };
    println!("{rendered}");
    Ok(0)
}

/// 输入选择错误统一复用农场「能力不匹配」码（设备池是农场 v1 扩展）。
fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_FARM_0002, message).with_hint(
        "设备写作 <provider>/<device-id>/<os>[/<target>]；provider 可用 \
         firebase-test-lab、browserstack-app-automate、aws-device-farm、self-hosted",
    )
}

/// 设备 / 操作系统 / 目标名进入证据路径前拒绝空值与路径分隔符。
fn validate_component(kind: &str, value: &str) -> Result<(), Diagnostic> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(invalid(format!("{kind} 非法：{value:?}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(provider: &str, device: &str, os: &str, target: Option<&str>) -> DeviceSpec {
        let raw = match target {
            Some(target) => format!("{provider}/{device}/{os}/{target}"),
            None => format!("{provider}/{device}/{os}"),
        };
        DeviceSpec::parse(&raw).expect("设备表达式合法")
    }

    #[test]
    fn plan_is_deterministic() {
        let pool = DevicePool::new(vec![
            spec("firebase-test-lab", "Pixel 7", "android", None),
            spec("self-hosted", "tv-lab-01", "android", Some("android-tv")),
        ])
        .expect("设备池合法");
        let options = PlanOptions {
            acquired_at: 1_700_000_000,
            timeout_secs: 900,
        };
        let first = pool.plan(&options).expect("计划可生成");
        let second = pool.plan(&options).expect("计划可生成");
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_string(&first).expect("可序列化"),
            serde_json::to_string(&second).expect("可序列化")
        );
        assert_eq!(first.leases[0].lease_id, "lease-0001");
        assert_eq!(first.leases[1].lease_id, "lease-0002");
        assert_eq!(first.leases[0].expires_at, 1_700_000_900);
        assert_eq!(first.leases[1].target, "android-tv");
        assert_eq!(
            first.leases[0].cleanup.evidence_dir,
            ".polypack/preview/android/Pixel 7/"
        );
    }

    #[test]
    fn invalid_provider_is_rejected() {
        let err = DeviceSpec::parse("azure/pixel/android").expect_err("未知 provider");
        assert_eq!(err.code, codes::E_FARM_0002);
        assert!(err.message.contains("azure"));
        assert!(err.hint.as_deref().unwrap_or("").contains("self-hosted"));
    }

    #[test]
    fn invalid_device_expression_is_rejected() {
        for raw in [
            "firebase-test-lab/pixel",
            "firebase-test-lab//android",
            "/pixel/android",
        ] {
            let err = DeviceSpec::parse(raw).expect_err(raw);
            assert_eq!(err.code, codes::E_FARM_0002);
        }
        let err = DeviceSpec::parse("self-hosted/../android").expect_err("路径穿越");
        assert_eq!(err.code, codes::E_FARM_0002);
    }

    #[test]
    fn empty_and_duplicate_pools_are_rejected() {
        let err = DevicePool::new(Vec::new()).expect_err("空池");
        assert_eq!(err.code, codes::E_FARM_0002);

        let device = spec("self-hosted", "tv-lab-01", "android", Some("android-tv"));
        let err = DevicePool::new(vec![device.clone(), device]).expect_err("重复设备");
        assert_eq!(err.code, codes::E_FARM_0002);
        assert!(err.message.contains("重复"));
    }

    #[test]
    fn invalid_timeout_is_rejected() {
        let pool = DevicePool::new(vec![spec("self-hosted", "tv-lab-01", "android", None)])
            .expect("设备池合法");
        let err = pool
            .plan(&PlanOptions {
                acquired_at: 0,
                timeout_secs: 0,
            })
            .expect_err("零时长");
        assert_eq!(err.code, codes::E_PLAN_0001);
        let err = pool
            .plan(&PlanOptions {
                acquired_at: u64::MAX,
                timeout_secs: 1,
            })
            .expect_err("溢出");
        assert_eq!(err.code, codes::E_PLAN_0001);
    }
}
