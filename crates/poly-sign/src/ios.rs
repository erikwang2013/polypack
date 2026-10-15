//! iOS 签名与公证链（T-M2-10）：codesign → ditto → notarytool → stapler → verify。
//!
//! 链路（`plan_*` 纯组装，执行函数逐步调用；宿主门控先于一切）：
//!
//! ```text
//! codesign --force --timestamp --options runtime --sign <identity> <可签产物.app>
//! ditto -c -k --keepParent <app> <out>/notarize/<名>.zip     # notarytool 只收 zip / pkg / dmg
//! xcrun notarytool submit <zip> --keychain-profile <档案> --wait
//! xcrun stapler staple <app> · xcrun stapler validate <app>
//! codesign --verify --deep --strict --verbose=2 <app> · spctl --assess --type execute -vv <app>
//! ```
//!
//! **凭据零明文**（T-M2-11 模型）：本模块只持有**引用与变量名**——口令经
//! `--password @env:VAR` 交给 notarytool 自行读取（值不进进程参数、不进日志、不进收据）；
//! 钥匙串材料经 `notarytool store-credentials` 的档案名引用；非 Env 来源（Vault）用于
//! notarytool 认证时响亮拒绝（`poly::E_SIG_0002`），绝不把材料悄悄内联进命令行。
//!
//! **收据留档**（提审级缓存槽位）：[`receipt_path`] 预留
//! `<out>/receipts/<产品名>-<版本>-notarization.json`，[`notarize`] 把 notarytool 的 JSON
//! 回执（submission id / status / 日志地址）原样落盘，提审与审计可回查。
//!
//! **宿主门控**：codesign / notarytool / stapler / spctl 仅随 macOS 提供——非 macOS 宿主
//! 执行函数在动手前返回 [`SignError::ToolMissing`]（`poly::E_SIG_0001`）；`plan_*` 为纯组装，
//! 任意宿主可跑（mock 契约测试的基础），真机链路由 T-M2-14 nightly（macos-14）覆盖。
//!
//! shortcut: 链作用于可签产物本体（`.app`）；它与 T-M2-08 `exportArchive` 的先后次序
//! （先签后导 / 导出后签）由 poly-cli 集成时定，接入方按产物形态组合 `plan_*`。

use std::path::{Path, PathBuf};

use poly_core::{CredentialRef, CredentialSource};

use crate::error::{SignError, io_err};

/// 一条签名链命令（本地极小类型：本 crate 依赖白名单只允许 poly-core，不引 poly-bundle 的 Command）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignCommand {
    /// 程序名。
    pub program: String,
    /// 参数（逐个传参，绝不拼接 shell 串）。
    pub args: Vec<String>,
}

impl SignCommand {
    /// 组装一条命令。
    pub fn new(program: &str, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            program: program.to_string(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// 供人阅读 / 复跑的命令行（含空格的参数加引号，同 `poly-bundle::Command::line` 形态）。
    pub fn line(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            if arg.contains(' ') {
                out.push('"');
                out.push_str(arg);
                out.push('"');
            } else {
                out.push_str(arg);
            }
        }
        out
    }
}

/// 一步链路（或一次校验）的命令集。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignPlan {
    /// 按序执行的命令。
    pub commands: Vec<SignCommand>,
}

impl SignPlan {
    /// 空计划。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条命令。
    #[must_use]
    pub fn with_command(mut self, command: SignCommand) -> Self {
        self.commands.push(command);
        self
    }

    /// 全部命令行（断言 / 日志）。
    pub fn lines(&self) -> Vec<String> {
        self.commands.iter().map(SignCommand::line).collect()
    }
}

/// 签名档案的约定引用名（= `poly-targets` 内建 ios spec 的 `signing` 字段）。
pub const REF_SIGNING_PROFILE: &str = "apple-codesign";

/// 公证认证形态（notarytool `--keychain-profile` / `--apple-id @env:` 两选一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotaryAuth {
    /// 钥匙串档案（`notarytool store-credentials` 建立；材料留在钥匙串）。
    KeychainProfile(String),
    /// Apple ID + App 专用密码（**变量名**，值由 notarytool 以 `@env:VAR` 自取）。
    Env {
        /// Apple ID 的环境变量名。
        apple_id: String,
        /// App 专用密码的环境变量名。
        password: String,
    },
}

/// iOS 签名链凭据（只持引用与变量名：值不驻留、不入命令、不入收据）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IosCredentials {
    /// `codesign -s` 签名标识（如 `Apple Distribution: Pax (TEAM123)`；非机密）。
    pub identity: Option<String>,
    /// 团队 ID（Apple Developer 门户可见；非机密）。
    pub team_id: Option<String>,
    /// notarytool 认证（钥匙串档案或 env 变量名）。
    pub notary: Option<NotaryAuth>,
    /// 描述文件引用（提审级产物；本链只登记来源，不读取材料）。
    pub provisioning_ref: Option<CredentialRef>,
}

/// 口令 / 描述文件的约定引用名（`<档案>.password` / `<档案>.provisioning`）。
fn companion_ref_name(suffix: &str) -> String {
    format!("{REF_SIGNING_PROFILE}{suffix}")
}

/// 缺配置项（非机密字段：签名标识 / 团队 ID / 认证形态）。
fn missing(name: &str, reason: &str) -> SignError {
    SignError::CredentialUnavailable {
        name: name.to_string(),
        backend: "config",
        reason: reason.to_string(),
    }
}

/// 凭据引用形态非法（值只放引用名 / 变量名，绝不放材料）。
fn invalid(name: String, backend: &'static str, reason: String) -> SignError {
    SignError::CredentialInvalid {
        name,
        backend,
        reason,
    }
}

impl IosCredentials {
    /// 由凭据引用装配（未登记的引用忽略——同一列表通常含其他目标的引用）：
    ///
    /// - `apple-codesign`（= `TargetSpec.signing`）：材料**来源**决定 notarytool 认证形态——
    ///   钥匙串 → `--keychain-profile <引用名>`（档案名 = 引用名，由 `notarytool
    ///   store-credentials` 建立）；env → `--apple-id @env:VAR`；Vault → 拒绝（先注入钥匙串）。
    /// - `apple-codesign.password`：App 专用密码（仅接受 env 引用）。
    /// - `apple-codesign.provisioning`：描述文件引用。
    ///
    /// `identity` / `team_id` 为非机密配置项，由调用方从配置直接填（不经引用）。
    pub fn from_credential_ref(refs: &[CredentialRef]) -> Result<Self, SignError> {
        let mut creds = Self::default();
        for reference in refs {
            if reference.name == REF_SIGNING_PROFILE {
                match &reference.source {
                    CredentialSource::Keyring => {
                        creds.notary = Some(NotaryAuth::KeychainProfile(reference.name.clone()));
                    }
                    CredentialSource::Env { variable } => {
                        // 口令档案随后并入（`<档案>.password`）；此处先记 Apple ID。
                        creds.notary = Some(NotaryAuth::Env {
                            apple_id: variable.clone(),
                            password: String::new(),
                        });
                    }
                    CredentialSource::Vault => {
                        return Err(invalid(
                            reference.name.clone(),
                            "vault",
                            "Vault 来源的 Apple 签名材料须先注入钥匙串（notarytool \
                             store-credentials）再以 Keyring 引用"
                                .to_string(),
                        ));
                    }
                }
            } else if reference.name == companion_ref_name(".password") {
                let CredentialSource::Env { variable } = &reference.source else {
                    return Err(invalid(
                        reference.name.clone(),
                        "keyring",
                        "口令只接受 env 引用（CI 注入）；钥匙串材料走 --keychain-profile"
                            .to_string(),
                    ));
                };
                match &mut creds.notary {
                    Some(NotaryAuth::Env { password, .. }) => password.clone_from(variable),
                    _ => {
                        return Err(invalid(
                            reference.name.clone(),
                            "env",
                            format!(
                                "口令引用需与 env 来源的 {REF_SIGNING_PROFILE} 档案配套（当前档案非 env 来源）"
                            ),
                        ));
                    }
                }
            } else if reference.name == companion_ref_name(".provisioning") {
                creds.provisioning_ref = Some(reference.clone());
            }
        }
        Ok(creds)
    }
}

// ---------------------------------------------------------------- 计划（纯组装）

/// 签名计划：codesign 硬化运行时 + 安全时间戳（公证前置）。
pub fn plan_sign(target: &Path, creds: &IosCredentials) -> Result<SignPlan, SignError> {
    let identity = creds
        .identity
        .as_deref()
        .filter(|identity| !identity.trim().is_empty())
        .ok_or_else(|| missing("ios.identity", "签名标识未配置（`Apple Distribution: …`）"))?;
    Ok(SignPlan::new().with_command(SignCommand::new(
        "codesign",
        [
            "--force".into(),
            "--timestamp".into(),
            "--options".into(),
            "runtime".into(),
            "--sign".into(),
            identity.into(),
            path_arg(target),
        ],
    )))
}

/// 公证打包计划：`ditto` 保符号链接 / 扩展属性（notarytool 只收 zip / pkg / dmg）。
pub fn plan_zip(app: &Path, zip: &Path) -> SignPlan {
    SignPlan::new().with_command(SignCommand::new(
        "ditto",
        [
            "-c".into(),
            "-k".into(),
            "--keepParent".into(),
            path_arg(app),
            path_arg(zip),
        ],
    ))
}

/// 公证提交计划（`--wait` 阻塞至出结果；notarytool 非 `Accepted` 时以非零退出，执行侧如实报错）。
pub fn plan_notarize(zip: &Path, creds: &IosCredentials) -> Result<SignPlan, SignError> {
    let mut args = vec![
        "notarytool".to_string(),
        "submit".to_string(),
        path_arg(zip),
    ];
    args.extend(notary_auth_args(creds)?);
    args.push("--wait".to_string());
    Ok(SignPlan::new().with_command(SignCommand::new("xcrun", args)))
}

/// 装订计划：`stapler staple` 把公证票订进产物，`validate` 就地复核。
pub fn plan_staple(target: &Path) -> SignPlan {
    SignPlan::new()
        .with_command(SignCommand::new(
            "xcrun",
            ["stapler".into(), "staple".into(), path_arg(target)],
        ))
        .with_command(SignCommand::new(
            "xcrun",
            ["stapler".into(), "validate".into(), path_arg(target)],
        ))
}

/// 校验计划（分发前自检，对应 `TargetSpec.verify` 的 `codesign-verify` + `spctl`）。
pub fn plan_verify(target: &Path) -> SignPlan {
    SignPlan::new()
        .with_command(SignCommand::new(
            "codesign",
            [
                "--verify".into(),
                "--deep".into(),
                "--strict".into(),
                "--verbose=2".into(),
                path_arg(target),
            ],
        ))
        .with_command(SignCommand::new(
            "spctl",
            [
                "--assess".into(),
                "--type".into(),
                assess_type(target).into(),
                "--verbose=4".into(),
                path_arg(target),
            ],
        ))
}

/// notarytool 认证参数：钥匙串档案优先（Apple 推荐；材料留在钥匙串）。
///
/// env 形态把**变量名**以 `@env:VAR` 交给 notarytool 自行取值——明文永不进命令行 / 日志。
fn notary_auth_args(creds: &IosCredentials) -> Result<Vec<String>, SignError> {
    match &creds.notary {
        Some(NotaryAuth::KeychainProfile(profile)) => {
            Ok(vec!["--keychain-profile".to_string(), profile.clone()])
        }
        Some(NotaryAuth::Env { apple_id, password }) => {
            let team = creds
                .team_id
                .as_deref()
                .filter(|team| !team.trim().is_empty())
                .ok_or_else(|| missing("ios.team_id", "env 认证形态需要团队 ID（`--team-id`）"))?;
            Ok(vec![
                "--apple-id".to_string(),
                format!("@env:{}", env_var_name(apple_id, "ios.apple_id_env")?),
                "--password".to_string(),
                format!("@env:{}", env_var_name(password, "ios.password_env")?),
                "--team-id".to_string(),
                team.to_string(),
            ])
        }
        None => Err(missing(
            "ios.notary",
            "未配置 notarytool 认证（钥匙串档案引用或 env 变量名）",
        )),
    }
}

/// `spctl --type`：`.pkg` / `.dmg` 为 install，其余（`.app`）为 execute。
fn assess_type(target: &Path) -> &'static str {
    match target.extension().and_then(|ext| ext.to_str()) {
        Some("pkg") | Some("dmg") => "install",
        _ => "execute",
    }
}

/// 环境变量名形态校验（`[A-Za-z_][A-Za-z0-9_]*`；配置到命令的信任边界）。
fn env_var_name<'a>(name: &'a str, field: &str) -> Result<&'a str, SignError> {
    let valid = !name.is_empty()
        && name.chars().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphabetic() || ch == '_'
            } else {
                ch.is_ascii_alphanumeric() || ch == '_'
            }
        });
    if valid {
        return Ok(name);
    }
    Err(invalid(
        name.to_string(),
        "env",
        format!("{field} 须为 [A-Za-z_][A-Za-z0-9_]* 形态的环境变量名"),
    ))
}

// ---------------------------------------------------------------- 执行

/// 执行签名（宿主门控最前置：非 macOS 宿主无从谈起）；返回被签产物路径。
pub fn sign(target: &Path, creds: &IosCredentials) -> Result<PathBuf, SignError> {
    require_macos("codesign")?;
    let plan = plan_sign(target, creds)?;
    run_all(&plan.commands)?;
    Ok(target.to_path_buf())
}

/// 执行公证并把 JSON 回执留档到 `receipt`；返回收据路径。
///
/// notarytool 的 `--wait` 在状态非 `Accepted` 时以非零退出 → 如实报错（`E_SIG_0003`），
/// 收据只在成功时落盘（失败无「提审级缓存」可言）。
pub fn notarize(zip: &Path, creds: &IosCredentials, receipt: &Path) -> Result<PathBuf, SignError> {
    require_macos("xcrun")?;
    let plan = plan_notarize(zip, creds)?;
    let output = run(&plan.commands[0])?;
    if let Some(parent) = receipt.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    std::fs::write(receipt, output).map_err(io_err(receipt))?;
    Ok(receipt.to_path_buf())
}

/// 执行装订与复核（`stapler staple` + `stapler validate`）。
pub fn staple(target: &Path) -> Result<PathBuf, SignError> {
    require_macos("xcrun")?;
    let plan = plan_staple(target);
    run_all(&plan.commands)?;
    Ok(target.to_path_buf())
}

/// 执行分发前校验（`codesign --verify` + `spctl`）。
pub fn verify(target: &Path) -> Result<PathBuf, SignError> {
    require_macos("codesign")?;
    let plan = plan_verify(target);
    run_all(&plan.commands)?;
    Ok(target.to_path_buf())
}

/// 收据路径（提审级缓存槽位）：`<out>/receipts/<产品名>-<版本>-notarization.json`。
pub fn receipt_path(out_dir: &Path, product: &str, version: &str) -> PathBuf {
    out_dir
        .join("receipts")
        .join(format!("{product}-{version}-notarization.json"))
}

// ---------------------------------------------------------------- 执行辅助

/// 宿主门控：Apple 签名 / 公证工具仅随 macOS 提供。
fn require_macos(tool: &str) -> Result<(), SignError> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    Err(SignError::ToolMissing {
        tool: tool.to_string(),
        hint: "Apple 签名 / 公证工具仅随 macOS 提供（codesign / notarytool / stapler / spctl）：\
               签名链需 macOS 宿主（Linux 发起走远程农场 macOS worker，T-M2-13）"
            .to_string(),
    })
}

/// 执行一条命令，捕获 stdout（notarytool 的 JSON 回执由此而来）。
fn run(command: &SignCommand) -> Result<String, SignError> {
    let output = std::process::Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                SignError::ToolMissing {
                    tool: command.program.clone(),
                    hint: install_hint(&command.program),
                }
            } else {
                SignError::ToolFailed {
                    command: command.line(),
                    status: "spawn-failed".to_string(),
                    stderr: source.to_string(),
                }
            }
        })?;
    if !output.status.success() {
        return Err(SignError::ToolFailed {
            command: command.line(),
            status: output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stderr: tail(&String::from_utf8_lossy(&output.stderr), 800),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 依次执行。
fn run_all(commands: &[SignCommand]) -> Result<(), SignError> {
    for command in commands {
        run(command)?;
    }
    Ok(())
}

/// 工具缺失时的安装建议。
fn install_hint(tool: &str) -> String {
    match tool {
        "codesign" | "spctl" => {
            "该工具随 macOS 命令行工具提供（xcode-select --install）：签名链路需 macOS 宿主"
                .to_string()
        }
        "xcrun" | "ditto" => {
            "该工具随 Xcode / macOS 提供（xcode-select --install）：公证链需 macOS 宿主".to_string()
        }
        other => format!("安装 {other} 后重试"),
    }
}

/// 取字符串尾部（诊断信息限长）。
fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let skip = trimmed.chars().count() - limit;
    trimmed.chars().skip(skip).collect()
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    /// 命令行引号规则与 poly-bundle::Command::line 同形（含空格即加引号）。
    #[test]
    fn command_line_quotes_spaced_args() {
        let command = SignCommand::new("codesign", ["--sign", "Apple Distribution: Pax"]);
        assert_eq!(
            command.line(),
            "codesign --sign \"Apple Distribution: Pax\""
        );
    }

    /// spctl 类型派生：pkg / dmg 为 install，其余为 execute。
    #[test]
    fn assess_type_follows_artifact_form() {
        assert_eq!(assess_type(Path::new("/x/App.app")), "execute");
        assert_eq!(assess_type(Path::new("/x/App.pkg")), "install");
        assert_eq!(assess_type(Path::new("/x/App.dmg")), "install");
    }

    /// 环境变量名形态校验（空 / 含非法字符即拒，错误码归凭据域）。
    #[test]
    fn env_var_name_is_validated() {
        assert_eq!(env_var_name("APPLE_ID", "f").unwrap(), "APPLE_ID");
        assert_eq!(env_var_name("_x1", "f").unwrap(), "_x1");
        for bad in ["", "1X", "A B", "A-B", "值"] {
            let err = env_var_name(bad, "ios.password_env").expect_err("应拒绝");
            assert_eq!(err.code(), codes::E_SIG_0002, "{bad:?}");
        }
    }
}
