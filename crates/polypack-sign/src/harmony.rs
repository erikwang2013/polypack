//! HarmonyOS HAP 签名（T-M3-04）：hap-sign-tool 的 `sign-app` / `verify-app` 两段。
//!
//! 两段式（对齐 [`crate::android`]）：[`HarmonySigner::plan`] 解析凭据 + 组装命令，**不执行**；
//! [`HarmonySigner::sign`] 把密钥材料落一次性临时目录（0700 目录 / 0600 文件）→
//! `java -jar hap-sign-tool.jar sign-app` 签名 → `verify-app` 就地校验并留证据。
//! [`verify`] 是独立入口：`polypack verify` 的 `hap-sign-verify` 校验器直接复用（签名单元是 HAP）。
//!
//! 凭据（全部来自 [`SigningMaterial`]，值零入库）：
//! - `keystore` + `store_password` + `key_alias` + `key_password`：应用签名密钥（PKCS12）；
//! - `app_cert`：应用证书链（PEM / 证书链字节）——HAP 的 `sign-app -appCertFile`；
//! - `profile`：已签名 Profile（p7b 字节）——`sign-app -profileFile`。
//!
//! `app_cert` / `profile` 缺失时在此快速失败（E_SIG_0002），绝不空手签名。
//!
//! **脱敏**：与 apksigner 不同，hap-sign-tool 的口令只能进 **argv**（无 `env:` 间接）。
//! 因此命令的一切展示形态（计划 [`Display`] / 错误 / 事件流）统一经 [`display_command`]
//! 脱敏：`-keyPwd` / `-keystorePwd` 的后随实参一律替换为 `****`。
//!
//! shortcut: 口令进 argv 意味着签名瞬间同机进程表（`/proc/<pid>/cmdline`）可见口令——
//! hap-sign-tool 的接口限制，本侧无法消除；已把展示 / 日志 / 事件流全部脱敏，临时目录 0700
//! 且用后即清。若华为后续给出 env 间接入口，改此一处即可。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use polypack_core::{CredentialRef, CredentialSource};

use crate::android::{exe_names, find_in_path, tail};
use crate::error::{SignError, io_err};
use crate::secret::{self, SecretBackend, SigningMaterial};
use crate::temp::{TempDir, write_private};

/// 签名算法（OpenHarmony 生态当前统一 ECDSA P-256）。
const SIGN_ALG: &str = "SHA256withECDSA";
/// 签名模式（本地密钥库；远程签名服务是另一档，超出当前链）。
const MODE: &str = "localSign";
/// 临时签名密钥库文件名（内容 PKCS12，扩展名仅便于人读）。
const KEYSTORE_FILE: &str = "keystore.p12";
/// 临时应用证书链文件名（PEM）。
const APP_CERT_FILE: &str = "app-cert.pem";
/// 临时已签名 Profile 文件名（p7b）。
const PROFILE_FILE: &str = "profile.p7b";
/// 签名的口令参数名（展示脱敏按这两个键的后随实参处理）。
const PASSWORD_FLAGS: [&str; 2] = ["-keyPwd", "-keystorePwd"];

/// hap-sign-tool 缺失时的安装建议。
const INSTALL_HINT: &str = "装 HarmonyOS command-line-tools（SDK 的 default/openharmony/toolchains/lib/\
     含 hap-sign-tool.jar）并设 $HARMONYOS_TOOLS，或设置 POLYPACK_HAP_SIGN_TOOL 指向该 jar";

/// 签名请求。
#[derive(Debug, Clone)]
pub struct HarmonySignRequest {
    /// 未签名 HAP。
    pub input: PathBuf,
    /// 产物路径；缺省为输入同目录的 `<stem>-signed.hap`（[`signed_output`]）。
    pub out: Option<PathBuf>,
    /// 凭据引用（来源决定解析后端）。
    pub credential: CredentialRef,
}

/// 签名计划：**不含任何凭据值**（口令只进 argv，展示形态已脱敏）。
#[derive(Debug, Clone)]
pub struct HarmonySignPlan {
    /// `java` 可执行文件（hap-sign-tool 的运行时）。
    pub java: PathBuf,
    /// `hap-sign-tool.jar`。
    pub tool: PathBuf,
    /// 输入 HAP。
    pub input: PathBuf,
    /// 输出 HAP。
    pub output: PathBuf,
    /// 密钥别名（`-keyAlias`）。
    pub key_alias: String,
}

impl std::fmt::Display for HarmonySignPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "hap-sign-tool 签名计划：")?;
        writeln!(f, "  java   {}", self.java.display())?;
        writeln!(f, "  工具   {}", self.tool.display())?;
        writeln!(f, "  输入   {}", self.input.display())?;
        writeln!(f, "  输出   {}", self.output.display())?;
        writeln!(f, "  密钥别名 {}", self.key_alias)?;
        writeln!(f, "  算法   {SIGN_ALG}（{MODE}）")?;
        write!(
            f,
            "  口令   argv 内联（hap-sign-tool 接口限制；展示 / 日志统一脱敏为 ****）"
        )
    }
}

/// 签名产物与校验证据。
#[derive(Debug, Clone)]
pub struct HarmonySignedArtifact {
    /// 产物路径。
    pub path: PathBuf,
    /// `verify-app` 的关键证据行（签名块版本 / Profile 类型 / Digest 校验）。
    pub verify_evidence: String,
    /// 产物字节数。
    pub bytes: u64,
}

/// HarmonyOS 签名器。
pub struct HarmonySigner {
    /// 注入的凭据后端链（多实现可替换，见 [`crate::secret`]）。
    backends: Vec<Box<dyn SecretBackend>>,
}

impl HarmonySigner {
    /// 用给定后端链构造。
    pub fn new(backends: Vec<Box<dyn SecretBackend>>) -> Self {
        Self { backends }
    }

    /// 用 [`secret::default_backends`]（系统密钥库）构造。
    pub fn with_default_backends() -> Self {
        Self::new(secret::default_backends())
    }

    /// 解析凭据 + 组装命令（不执行）；缺失 / 错误引用在此快速失败。
    pub fn plan(&self, request: &HarmonySignRequest) -> Result<HarmonySignPlan, SignError> {
        Ok(self.assemble(request)?.0)
    }

    /// 签名并自动校验；凭据每次调用重新拉取，临时材料用后即清。
    pub fn sign(&self, request: &HarmonySignRequest) -> Result<HarmonySignedArtifact, SignError> {
        let (plan, material) = self.assemble(request)?;
        let temp = TempDir::create("polypack-hap-sign-")?;
        let keystore = temp.path().join(KEYSTORE_FILE);
        let app_cert = temp.path().join(APP_CERT_FILE);
        let profile = temp.path().join(PROFILE_FILE);
        write_private(&keystore, &material.keystore)?;
        // app_cert / profile 由 assemble 保证在场（缺则 E_SIG_0002 已提前失败）。
        write_private(&app_cert, material.app_cert.as_deref().unwrap_or_default())?;
        write_private(&profile, material.profile.as_deref().unwrap_or_default())?;

        let key_pass = material
            .key_password
            .as_deref()
            .unwrap_or(&material.store_password);
        let args: Vec<OsString> = vec![
            "-jar".into(),
            plan.tool.clone().into_os_string(),
            "sign-app".into(),
            "-keyAlias".into(),
            plan.key_alias.clone().into(),
            "-signAlg".into(),
            SIGN_ALG.into(),
            "-mode".into(),
            MODE.into(),
            "-appCertFile".into(),
            app_cert.into_os_string(),
            "-profileFile".into(),
            profile.into_os_string(),
            "-inFile".into(),
            plan.input.clone().into_os_string(),
            "-keystoreFile".into(),
            keystore.clone().into_os_string(),
            "-outFile".into(),
            plan.output.clone().into_os_string(),
            "-keyPwd".into(),
            key_pass.into(),
            "-keystorePwd".into(),
            material.store_password.clone().into(),
        ];
        run(&plan.java, &args)?;

        // 就地校验（签名单元 = HAP）：证据行进产物说明 / 事件流
        let verify = run(&plan.java, &verify_args(&plan.tool, &plan.output, &temp))?;
        let bytes = std::fs::metadata(&plan.output)
            .map_err(io_err(&plan.output))?
            .len();
        Ok(HarmonySignedArtifact {
            path: plan.output,
            verify_evidence: verify_evidence(&String::from_utf8_lossy(&verify.stdout)),
            bytes,
        })
    }

    /// plan 与 sign 的共同前段：工具 → 输入 → 凭据（含 app_cert / profile）→ 计划。
    fn assemble(
        &self,
        request: &HarmonySignRequest,
    ) -> Result<(HarmonySignPlan, SigningMaterial), SignError> {
        let (java, tool) = tool_paths()?;
        let metadata = std::fs::metadata(&request.input).map_err(io_err(&request.input))?;
        if !metadata.is_file() {
            return Err(SignError::Io {
                path: request.input.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "输入不是普通文件（期望未签名 HAP）",
                ),
            });
        }
        let material = secret::resolve(&request.credential, &self.backends)?;
        // HAP 签名必需证书链与 Profile：缺一即拒（空手签名出的是坏产物，不是「少签一步」）
        let backend = backend_id(&request.credential.source);
        for (field, present) in [
            ("app_cert", material.app_cert.is_some()),
            ("profile", material.profile.is_some()),
        ] {
            if !present {
                return Err(SignError::CredentialInvalid {
                    name: request.credential.name.clone(),
                    backend,
                    reason: format!(
                        "载荷缺 {field} 字段（HarmonyOS 签名需应用证书链与已签名 Profile 的 base64）"
                    ),
                });
            }
        }
        let key_alias = material
            .key_alias
            .clone()
            .ok_or_else(|| SignError::CredentialInvalid {
                name: request.credential.name.clone(),
                backend,
                reason: "载荷缺 key_alias 字段（hap-sign-tool 的 -keyAlias 必填）".to_string(),
            })?;
        let output = request
            .out
            .clone()
            .unwrap_or_else(|| signed_output(&request.input));
        let plan = HarmonySignPlan {
            java,
            tool,
            input: request.input.clone(),
            output,
            key_alias,
        };
        Ok((plan, material))
    }
}

/// 缺省产物：输入同目录的 `<stem>-signed.hap`（对 Android 的 `-signed` 约定同款）。
pub fn signed_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());
    input.with_file_name(format!("{stem}-signed.hap"))
}

/// 独立校验入口：`hap-sign-tool verify-app`（`polypack verify` 的 `hap-sign-verify` 校验器）。
///
/// 返回关键证据行；签名非法（命令非零退出）→ [`SignError::ToolFailed`]（stderr 尾部直出）。
pub fn verify(hap: &Path) -> Result<String, SignError> {
    let (java, tool) = tool_paths()?;
    let temp = TempDir::create("polypack-hap-sign-")?;
    let output = run(&java, &verify_args(&tool, hap, &temp))?;
    Ok(verify_evidence(&String::from_utf8_lossy(&output.stdout)))
}

/// `verify-app` 的参数（证书链 / Profile 落在一次性临时目录，用后即清）。
fn verify_args(tool: &Path, hap: &Path, temp: &TempDir) -> Vec<OsString> {
    vec![
        "-jar".into(),
        tool.to_path_buf().into_os_string(),
        "verify-app".into(),
        "-inFile".into(),
        hap.to_path_buf().into_os_string(),
        "-outCertChain".into(),
        temp.path().join("out-cert.cer").into_os_string(),
        "-outProfile".into(),
        temp.path().join("out-profile.p7b").into_os_string(),
    ]
}

/// 解析 hap-sign-tool 的工具对：`(java, jar)`。
///
/// jar 顺序：`$POLYPACK_HAP_SIGN_TOOL`（显式路径，权威：指向不存在即报错）→
/// `$HARMONYOS_TOOLS/sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar` →
/// `/home/component/command-line-tools/...`（本机固定位置，仅作本地便利，同 apksigner 的兜底）。
/// java 取 PATH（Windows 上是 `java.exe` / `.bat`）。
pub fn tool_paths() -> Result<(PathBuf, PathBuf), SignError> {
    let names = exe_names("java");
    let java = std::env::var_os("PATH")
        .and_then(|var| find_in_path(&var, &names))
        .ok_or_else(|| SignError::ToolMissing {
            tool: "java".to_string(),
            hint: "java 不在 PATH：装 JRE / JDK（hap-sign-tool.jar 的运行时）后重试".to_string(),
        })?;
    Ok((java, jar_path()?))
}

/// jar 解析（见 [`tool_paths`] 的顺序）。
fn jar_path() -> Result<PathBuf, SignError> {
    jar_path_from(
        std::env::var_os("POLYPACK_HAP_SIGN_TOOL").map(PathBuf::from),
        std::env::var_os("HARMONYOS_TOOLS").filter(|value| !value.is_empty()),
    )
}

/// [`jar_path`] 的纯函数核（显式路径 → 各根，注入输入便于单测）。
fn jar_path_from(
    explicit: Option<PathBuf>,
    harmony_root: Option<OsString>,
) -> Result<PathBuf, SignError> {
    if let Some(explicit) = explicit {
        if explicit.is_file() {
            return Ok(explicit);
        }
        return Err(SignError::ToolMissing {
            tool: explicit.display().to_string(),
            hint: "POLYPACK_HAP_SIGN_TOOL 指向的文件不存在；修正该变量，或删除它改用 \
                   $HARMONYOS_TOOLS / 固定位置解析"
                .to_string(),
        });
    }
    let relative = Path::new("sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar");
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(root) = harmony_root {
        roots.push(PathBuf::from(root));
    }
    roots.push(PathBuf::from("/home/component/command-line-tools"));
    for root in roots {
        let candidate = root.join(relative);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(SignError::ToolMissing {
        tool: "hap-sign-tool.jar".to_string(),
        hint: INSTALL_HINT.to_string(),
    })
}

/// 凭据来源 → 后端 id（错误归因用；`resolve` 已保证该后端在场）。
fn backend_id(source: &CredentialSource) -> &'static str {
    match source {
        CredentialSource::Keyring => secret::KEYRING_ID,
        CredentialSource::Vault => secret::VAULT_ID,
        CredentialSource::Env { .. } => secret::ENV_ID,
    }
}

/// 执行外部命令：缺席工具 → [`SignError::ToolMissing`]；非零退出 → [`SignError::ToolFailed`]。
///
/// 错误里的命令展示串经 [`display_command`] 脱敏（口令参数的实参一律 `****`）。
fn run(java: &Path, args: &[OsString]) -> Result<Output, SignError> {
    let mut command = Command::new(java);
    command.args(args);
    let output = command.output().map_err(|source| SignError::ToolMissing {
        tool: java.display().to_string(),
        hint: format!("{INSTALL_HINT}（底层错误：{source}）"),
    })?;
    if !output.status.success() {
        // hap-sign-tool 的诊断走 stdout（INFO/WARN 日志）与 stderr 两侧，取非空侧
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let diag: &str = if stderr.trim().is_empty() {
            stdout.as_ref()
        } else {
            stderr.as_ref()
        };
        return Err(SignError::ToolFailed {
            command: display_command(java, args),
            status: output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stderr: tail(diag, 800),
        });
    }
    Ok(output)
}

/// 命令展示串（脱敏）：`java <args…>`，口令参数的后随实参替换为 `****`。
fn display_command(java: &Path, args: &[OsString]) -> String {
    let mut out = java.display().to_string();
    let mut redact_next = false;
    for arg in args {
        out.push(' ');
        if redact_next {
            out.push_str("****");
            redact_next = false;
        } else {
            let text = arg.to_string_lossy();
            out.push_str(&text);
            redact_next = PASSWORD_FLAGS.contains(&text.as_ref());
        }
    }
    out
}

/// 从 `verify-app` 输出提取关键证据行（签名块版本 / Profile 类型 / Digest 校验 / 证书主体）。
fn verify_evidence(stdout: &str) -> String {
    const KEYS: [&str; 6] = [
        "Signing Block",
        "profile type",
        "Digest verify result",
        "verify codesign success",
        "Verify success",
        "Subject:",
    ];
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| KEYS.iter().any(|key| line.contains(key)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    #[test]
    fn display_command_redacts_password_values() {
        let args: Vec<OsString> = vec![
            "-jar".into(),
            "/sdk/hap-sign-tool.jar".into(),
            "sign-app".into(),
            "-keyAlias".into(),
            "release".into(),
            "-keyPwd".into(),
            "s3cret-key".into(),
            "-keystorePwd".into(),
            "s3cret-store".into(),
        ];
        let shown = display_command(Path::new("/usr/bin/java"), &args);
        assert!(!shown.contains("s3cret-key"), "密钥口令不得进展示：{shown}");
        assert!(!shown.contains("s3cret-store"), "库口令不得进展示：{shown}");
        assert_eq!(shown.matches("****").count(), 2, "{shown}");
        assert!(shown.contains("release"), "非口令参数原样保留：{shown}");
    }

    /// 建一个一次性夹具文件（内容无关，只求存在）。
    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(dir).expect("夹具目录可建");
        std::fs::write(&path, b"fixture").expect("夹具可写");
        path
    }

    #[test]
    fn jar_resolution_prefers_explicit_then_roots() {
        let dir = std::env::temp_dir().join(format!("polypack-hap-jar-{}", std::process::id()));
        let explicit = touch(&dir, "explicit.jar");
        // 显式路径在场：直接胜出，不看根目录。
        assert_eq!(
            jar_path_from(Some(explicit.clone()), Some(OsString::from("/nonexistent")))
                .expect("显式路径优先"),
            explicit
        );
        // 显式路径缺席：报错且点名变量，绝不静默回退。
        let err =
            jar_path_from(Some(dir.join("missing.jar")), None).expect_err("显式路径不存在必须报错");
        assert_eq!(err.code(), codes::E_SIG_0001);
        assert!(err.to_string().contains("POLYPACK_HAP_SIGN_TOOL"), "{err}");
        // 根目录注入：命中 SDK 布局 `<root>/sdk/default/openharmony/toolchains/lib/`。
        let root = dir.join("tools");
        let jar = touch(
            &root.join("sdk/default/openharmony/toolchains/lib"),
            "hap-sign-tool.jar",
        );
        assert_eq!(
            jar_path_from(None, Some(root.into_os_string())).expect("根目录命中"),
            jar
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_evidence_keeps_only_key_lines() {
        let noisy = "10-16 19:06:38.880  INFO - Start verify-app\n\
                     10-16 19:06:39.357  INFO - Find Hap Signing Block success, version: 3, block count: 3\n\
                     10-16 19:06:40.085  INFO - profile type is: debug\n\
                     10-16 19:06:40.255  INFO - Subject: CN=OpenHarmony Application Release\n\
                     10-16 19:06:40.273  INFO - Digest verify result: true, DigestAlgorithm: SHA-256\n\
                     10-16 19:06:40.354  INFO - verify-app success\n";
        let evidence = verify_evidence(noisy);
        assert!(
            evidence.contains("Signing Block success, version: 3"),
            "{evidence}"
        );
        assert!(evidence.contains("profile type is: debug"), "{evidence}");
        assert!(
            evidence.contains("Digest verify result: true"),
            "{evidence}"
        );
        assert!(
            evidence.contains("Subject: CN=OpenHarmony Application Release"),
            "{evidence}"
        );
        assert!(
            !evidence.contains("Start verify-app"),
            "噪声行不进证据：{evidence}"
        );
    }

    #[test]
    fn signed_output_appends_signed_suffix() {
        assert_eq!(
            signed_output(Path::new("/tmp/demo/app-unsigned.hap")),
            PathBuf::from("/tmp/demo/app-unsigned-signed.hap")
        );
    }
}
