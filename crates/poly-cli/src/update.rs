//! `poly update`：OTA 更新清单（T-M9-11）。
//!
//! `manifest` 产出 Tauri v2 updater 静态 JSON（`latest.json`）：顶层键按字典序、两空格
//! 缩进、末尾换行；`pub_date` 可由 `--pub-date` / `SOURCE_DATE_EPOCH` 固定，同输入二跑
//! 字节一致。清单用 [`GpgSigner`]（[`RepoSigner::detach_sign`]）做 GPG 分离签名，凭据只以
//! [`poly_core::CredentialRef`] 引用出现；`<manifest>.sig` 与保留的 `<version>.json(.sig)`
//! 供回滚。
//!
//! `check` 做结构校验，并可用 [`ArtifactVerifier`] + `--trust-root` 验证清单签名。
//! 注意：Tauri 原生校验的是各平台产物 `.sig`（Tauri 密钥对）；本模块的 GPG 清单签名是
//! 自托管渠道的附加层，两者不可互相替代。真实二进制安装 / 回滚（T-M9-12）是环境门控。

use std::path::PathBuf;

use poly_core::codes;
use poly_core::diagnostics::Diagnostic;
use poly_sign::secret;
use poly_sign::{ArtifactVerifier, GpgSigner, RepoSigner};

mod manifest;
#[cfg(test)]
mod tests;

use manifest::{
    credential_ref, find_artifact, input_error, io_error, percent_encode, pub_date,
    render_manifest, sig_path, sign_diagnostic, validate_channel, validate_manifest,
    validate_platform, validate_version, validated_base_url, write,
};

/// `poly update` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 子命令。
    #[command(subcommand)]
    pub command: Command,
}

/// `poly update` 子命令。
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// 生成并签名 Tauri v2 更新清单（latest.json）。
    Manifest(Box<ManifestArgs>),
    /// 校验更新清单结构，并可选验证 GPG 分离签名。
    Check(CheckArgs),
}

/// `poly update manifest` 的参数。
#[derive(Debug, clap::Args)]
pub struct ManifestArgs {
    /// 更新渠道（stable / beta 等；小写字母、数字、`.`、`_`、`-`）。
    #[arg(long)]
    pub channel: String,
    /// 版本号（SemVer，如 1.2.3）。
    #[arg(long)]
    pub version: String,
    /// 更新说明。
    #[arg(long, default_value = "")]
    pub notes: String,
    /// 产物下载基址（HTTP(S)；生产环境用 HTTPS）。
    #[arg(long)]
    pub base_url: String,
    /// Tauri 平台键的 OS 段（linux / darwin / windows）。
    #[arg(long)]
    pub platform: String,
    /// Tauri 平台键的 ARCH 段（x86_64 / aarch64 / i686 / armv7）。
    #[arg(long)]
    pub arch: String,
    /// 发布产物目录。
    #[arg(long)]
    pub package_dir: PathBuf,
    /// 目录内有多个产物时显式指定文件名。
    #[arg(long)]
    pub artifact: Option<String>,
    /// 产物分离签名（缺省 `<产物>.sig`；内容原样写入清单 signature 字段）。
    #[arg(long)]
    pub artifact_signature: Option<PathBuf>,
    /// 清单输出路径（缺省 `<package-dir>/<channel>/latest.json`）。
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// 清单签名凭据引用（env:VAR / keyring[:条目名] / vault[:条目名]）。
    #[arg(long)]
    pub credential: Option<String>,
    /// 发布时间（RFC 3339）；缺省读 SOURCE_DATE_EPOCH，再缺省当前时间。
    #[arg(long)]
    pub pub_date: Option<String>,
}

/// `poly update check` 的参数。
#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// 更新清单路径（latest.json）。
    #[arg(long)]
    pub manifest: PathBuf,
    /// 清单分离签名；缺省只做结构校验，不验签。
    #[arg(long)]
    pub signature: Option<PathBuf>,
    /// 信任根公钥的凭据引用（验签时必填；载荷 base64(JSON) `{"public_key":...}`）。
    #[arg(long)]
    pub trust_root: Option<String>,
}

/// 执行 `poly update`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match &args.command {
        Command::Manifest(args) => manifest(args, json),
        Command::Check(args) => check(args, json),
    }
}

/// 生成并签名清单：先完成全部校验与签名，再落盘，失败不留半成品。
fn manifest(args: &ManifestArgs, json: bool) -> Result<i32, Diagnostic> {
    validate_channel(&args.channel)?;
    validate_version(&args.version)?;
    validate_platform(&args.platform, &args.arch)?;
    let base_url = validated_base_url(&args.base_url)?;
    let pub_date = pub_date(args.pub_date.as_deref())?;
    let artifact = find_artifact(&args.package_dir, args.artifact.as_deref())?;
    let artifact_sig = args
        .artifact_signature
        .clone()
        .unwrap_or_else(|| sig_path(&artifact));
    let signature =
        std::fs::read(&artifact_sig).map_err(|err| io_error("读取产物签名", &artifact_sig, err))?;
    if signature.iter().all(u8::is_ascii_whitespace) {
        return Err(Diagnostic::error(
            codes::E_UPDATE_0004,
            format!("产物签名为空：{}", artifact_sig.display()),
        )
        .with_hint("Tauri 更新清单的 signature 字段要求 `.sig` 文件内容，不能是路径或 URL"));
    }
    let file_name = artifact
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| input_error("产物文件名不是合法 UTF-8"))?;
    let platform_key = format!("{}-{}", args.platform, args.arch);
    let url = format!("{base_url}/{}", percent_encode(file_name));
    let text = render_manifest(
        &args.version,
        &args.notes,
        &pub_date,
        &platform_key,
        &signature,
        &url,
    );

    let reference = credential_ref(args.credential.as_deref(), "--credential")?;
    let signer = GpgSigner::with_default_backends(&reference).map_err(sign_diagnostic)?;
    let manifest_sig = signer
        .detach_sign(text.as_bytes())
        .map_err(sign_diagnostic)?;

    let output = args
        .output
        .clone()
        .unwrap_or_else(|| args.package_dir.join(&args.channel).join("latest.json"));
    let versioned = output.with_file_name(format!("{}.json", args.version));
    write(&output, text.as_bytes())?;
    write(&sig_path(&output), &manifest_sig)?;
    // 保留版本副本：回滚时把它复制回 latest.json 即恢复旧清单（签名随字节复用）。
    write(&versioned, text.as_bytes())?;
    write(&sig_path(&versioned), &manifest_sig)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "update_manifest",
                "channel": args.channel,
                "version": args.version,
                "platform": platform_key,
                "manifest": output,
                "signature": sig_path(&output),
                "versioned": versioned,
            })
        );
    } else {
        println!(
            "更新清单：{}（渠道 {}，{}）",
            output.display(),
            args.channel,
            platform_key
        );
        println!("清单签名：{}", sig_path(&output).display());
        println!("版本副本：{}（回滚用）", versioned.display());
    }
    Ok(0)
}

/// 结构校验 + 可选验签。
fn check(args: &CheckArgs, json: bool) -> Result<i32, Diagnostic> {
    let bytes =
        std::fs::read(&args.manifest).map_err(|err| io_error("读取清单", &args.manifest, err))?;
    let version = validate_manifest(&bytes)?;
    let verified = match &args.signature {
        None => false,
        Some(path) => {
            let signature =
                std::fs::read(path).map_err(|err| io_error("读取清单签名", path, err))?;
            let reference = credential_ref(args.trust_root.as_deref(), "--trust-root")?;
            let verifier =
                ArtifactVerifier::from_reference(&reference, &secret::default_backends())
                    .map_err(sign_diagnostic)?;
            verifier
                .verify_detached(&bytes, &signature)
                .map_err(sign_diagnostic)?;
            true
        }
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "update_check",
                "manifest": args.manifest,
                "version": version,
                "signature_verified": verified,
            })
        );
    } else if verified {
        println!(
            "清单有效：{}（版本 {version}，GPG 签名已验证）",
            args.manifest.display()
        );
    } else {
        println!(
            "清单有效：{}（版本 {version}；未提供 --signature，签名未验证）",
            args.manifest.display()
        );
    }
    Ok(0)
}
