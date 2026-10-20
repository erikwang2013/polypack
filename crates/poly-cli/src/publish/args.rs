//! `poly publish` 的命令行参数。

use std::path::PathBuf;

use poly_publish::metadata::DEFAULT_LOCALE;

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
    /// 平台应用 id（`--check-metadata` 时不需要）。
    #[arg(long, required_unless_present = "check_metadata")]
    pub appid: Option<String>,
    /// 版本号（`--check-metadata` 时不需要）。
    #[arg(long, required_unless_present = "check_metadata")]
    pub version: Option<String>,
    /// 版本描述（上传 / 提审都会带上）。
    #[arg(long, default_value = "")]
    pub description: String,
    /// 代码包路径（缺省 = 按目标产物落点定位快应用 `.rpk`；目录载体亦可）。
    #[arg(long)]
    pub package: Option<PathBuf>,
    /// 凭据引用（`env:变量名`；变量值 = base64(JSON) 载荷，同 T-M2-11 模型；
    /// `--check-metadata` 时不需要）。
    #[arg(long, value_name = "REF", required_unless_present = "check_metadata")]
    pub credential: Option<String>,
    /// 提审材料 JSON 文件（平台各自解释；与 `--metadata-dir` 互斥）。
    #[arg(long, conflicts_with = "metadata_dir")]
    pub materials: Option<PathBuf>,
    /// 商店 metadata 根目录（内含 `<platform>/<locale>.toml`；与 `--materials` 互斥）。
    #[arg(long, value_name = "DIR", conflicts_with = "materials")]
    pub metadata_dir: Option<PathBuf>,
    /// metadata locale（缺省 `default`；请求缺失时回退 default 并显式告警）。
    #[arg(long, default_value = DEFAULT_LOCALE)]
    pub metadata_locale: String,
    /// 只校验 metadata（不解析产物 / 凭据，不触网、不落回执）；违规 → 非零退出。
    #[arg(long)]
    pub check_metadata: bool,
    /// 平台 API 端点（快应用联盟商店必填：无公开 CI 端点，显式注入、无缺省域名）。
    #[arg(long, value_name = "URL")]
    pub api_base: Option<String>,
    /// 只解析输入（渠道 / 产物 / 凭据形态 / 端点），不触网、不落回执。
    #[arg(long)]
    pub dry_run: bool,
}
