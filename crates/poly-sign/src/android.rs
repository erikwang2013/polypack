//! Android 签名（T-M2-09）：`CredentialRef` → keystore 解析 → apksigner 签名 + 校验。
//!
//! 两段式（对齐 poly-bundle 的 plan / 执行习惯）：
//! - [`AndroidSigner::plan`]：解析凭据 + 组装命令，**不执行**；缺失 / 错误引用在此快速失败；
//! - [`AndroidSigner::sign`]：keystore 落一次性临时目录（0600）→ apksigner 签名 → verify。
//!
//! 脱敏与残留在计划里：口令只以 `env:POLYPACK_KS_PASS` / `env:POLYPACK_KEY_PASS` 引用，
//! 执行期经环境变量注入，绝不进命令行 / 计划 / 错误 / `Debug`；临时目录带 `Drop` 兜底，
//! 成功与失败路径都不留密钥材料。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use poly_core::CredentialRef;

use crate::error::{SignError, io_err};
use crate::secret::{self, SecretBackend, SigningMaterial};

/// 密钥库口令的注入变量名（`--ks-pass env:…`）。
pub const STORE_PASS_VAR: &str = "POLYPACK_KS_PASS";
/// 密钥口令的注入变量名（`--key-pass env:…`）。
pub const KEY_PASS_VAR: &str = "POLYPACK_KEY_PASS";

/// 临时 keystore 文件名（apksigner 按内容判别 PKCS12 / JKS，扩展名无关紧要）。
const KEYSTORE_FILE: &str = "keystore.ks";

/// apksigner 缺失时的安装建议。
const INSTALL_HINT: &str = "安装 Android SDK build-tools（sdkmanager \"build-tools;36.0.0\"）并\
     把 apksigner 放入 PATH，或设置 POLYPACK_APKSIGNER / ANDROID_HOME";

/// 临时目录序号（同一进程内多次签名不重名）。
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 签名请求。
#[derive(Debug, Clone)]
pub struct SignRequest {
    /// 未签名 APK。
    pub input: PathBuf,
    /// 产物路径；缺省为输入同目录的 `<stem>-signed.apk`。
    pub out: Option<PathBuf>,
    /// 凭据引用（来源决定解析后端）。
    pub credential: CredentialRef,
}

/// 签名计划：**不含任何凭据值**，口令只以环境变量名出现。
#[derive(Debug, Clone)]
pub struct SignPlan {
    /// apksigner 可执行文件。
    pub tool: PathBuf,
    /// 输入 APK。
    pub input: PathBuf,
    /// 输出 APK。
    pub output: PathBuf,
    /// 密钥别名；`None` 时 apksigner 用密钥库内唯一别名。
    pub key_alias: Option<String>,
}

impl std::fmt::Display for SignPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "apksigner 签名计划：")?;
        writeln!(f, "  工具     {}", self.tool.display())?;
        writeln!(f, "  输入     {}", self.input.display())?;
        writeln!(f, "  输出     {}", self.output.display())?;
        writeln!(
            f,
            "  密钥别名 {}",
            self.key_alias.as_deref().unwrap_or("<密钥库内唯一别名>")
        )?;
        writeln!(
            f,
            "  库口令   env:{STORE_PASS_VAR}（执行期注入，不进命令行）"
        )?;
        write!(f, "  密钥口令 env:{KEY_PASS_VAR}（省略时回退库口令）")
    }
}

/// 签名产物与校验证据。
#[derive(Debug, Clone)]
pub struct SignedArtifact {
    /// 产物路径。
    pub path: PathBuf,
    /// `apksigner verify --print-certs` 的关键证据行（`Verifies` / `Signer #1 …`）。
    pub verify_evidence: String,
    /// 产物字节数。
    pub bytes: u64,
}

/// Android 签名器。
pub struct AndroidSigner {
    /// 注入的凭据后端链（多实现可替换，见 [`crate::secret`]）。
    backends: Vec<Box<dyn SecretBackend>>,
}

impl AndroidSigner {
    /// 用给定后端链构造。
    pub fn new(backends: Vec<Box<dyn SecretBackend>>) -> Self {
        Self { backends }
    }

    /// 用 [`secret::default_backends`]（系统密钥库）构造。
    pub fn with_default_backends() -> Self {
        Self::new(secret::default_backends())
    }

    /// 解析凭据 + 组装命令（不执行）；缺失 / 错误引用在此快速失败。
    pub fn plan(&self, request: &SignRequest) -> Result<SignPlan, SignError> {
        Ok(self.assemble(request)?.0)
    }

    /// 签名并自动校验；凭据每次调用重新拉取，临时材料用后即清。
    pub fn sign(&self, request: &SignRequest) -> Result<SignedArtifact, SignError> {
        let (plan, material) = self.assemble(request)?;
        let temp = TempDir::create()?;
        let keystore = temp.path().join(KEYSTORE_FILE);
        write_private(&keystore, &material.keystore)?;

        let mut args: Vec<OsString> = vec![
            "sign".into(),
            "--ks".into(),
            keystore.clone().into(),
            "--ks-pass".into(),
            format!("env:{STORE_PASS_VAR}").into(),
        ];
        if let Some(alias) = &plan.key_alias {
            args.push("--ks-key-alias".into());
            args.push(alias.into());
        }
        args.extend([
            "--key-pass".into(),
            format!("env:{KEY_PASS_VAR}").into(),
            "--out".into(),
            plan.output.clone().into(),
            plan.input.clone().into(),
        ]);
        // 口令值只经 env 注入；错误里的命令展示串由程序 + 参数拼出，两者都不会带出口令。
        let key_pass = material
            .key_password
            .as_deref()
            .unwrap_or(&material.store_password);
        run(
            &plan.tool,
            &args,
            &[
                (STORE_PASS_VAR, material.store_password.as_str()),
                (KEY_PASS_VAR, key_pass),
            ],
        )?;

        let verify = run(
            &plan.tool,
            &[
                "verify".into(),
                "--print-certs".into(),
                "--verbose".into(),
                plan.output.clone().into(),
            ],
            &[],
        )?;
        let bytes = std::fs::metadata(&plan.output)
            .map_err(io_err(&plan.output))?
            .len();
        Ok(SignedArtifact {
            path: plan.output,
            verify_evidence: verify_evidence(&String::from_utf8_lossy(&verify.stdout)),
            bytes,
        })
    }

    /// plan 与 sign 的共同前段：工具 → 输入 → 凭据 → 计划。
    fn assemble(&self, request: &SignRequest) -> Result<(SignPlan, SigningMaterial), SignError> {
        let tool = apksigner_path()?;
        let metadata = std::fs::metadata(&request.input).map_err(io_err(&request.input))?;
        if !metadata.is_file() {
            return Err(SignError::Io {
                path: request.input.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "输入不是普通文件（期望未签名 APK）",
                ),
            });
        }
        let material = secret::resolve(&request.credential, &self.backends)?;
        let output = request
            .out
            .clone()
            .unwrap_or_else(|| default_output(&request.input));
        let plan = SignPlan {
            tool,
            input: request.input.clone(),
            output,
            key_alias: material.key_alias.clone(),
        };
        Ok((plan, material))
    }
}

/// 缺省产物：输入同目录的 `<stem>-signed.apk`。
fn default_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());
    input.with_file_name(format!("{stem}-signed.apk"))
}

/// 解析 apksigner 可执行文件。
///
/// 顺序：`$POLYPACK_APKSIGNER` → PATH → `$ANDROID_HOME` / `$ANDROID_SDK_ROOT` /
/// `~/Android/Sdk` 的 `build-tools/<版本>`（版本从大到小，目录里缺 apksigner 就继续降级；
/// 最后追加 `/home/component/Android/sdk`，那是本机 SDK 固定位置，仅作本地便利）。
pub fn apksigner_path() -> Result<PathBuf, SignError> {
    let names = exe_names("apksigner");
    let mut roots: Vec<PathBuf> = Vec::new();
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        roots.push(PathBuf::from(home).join("Android").join("Sdk"));
    }
    roots.push(PathBuf::from("/home/component/Android/sdk"));
    resolve_apksigner_from(
        std::env::var_os("POLYPACK_APKSIGNER").map(PathBuf::from),
        std::env::var_os("PATH"),
        &roots,
        &names,
    )
}

/// [`apksigner_path`] 的纯函数内核（环境值由调用方给出，便于单测）。
fn resolve_apksigner_from(
    explicit: Option<PathBuf>,
    path_var: Option<OsString>,
    roots: &[PathBuf],
    names: &[String],
) -> Result<PathBuf, SignError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path);
        }
        return Err(SignError::ToolMissing {
            tool: path.display().to_string(),
            hint: "POLYPACK_APKSIGNER 指向的文件不存在；修正该变量，或删除它以改用 PATH / Android SDK 解析"
                .to_string(),
        });
    }
    if let Some(found) = path_var
        .as_deref()
        .and_then(|var| find_in_path(var, names))
        .or_else(|| find_in_sdk_build_tools(roots, names))
    {
        return Ok(found);
    }
    Err(SignError::ToolMissing {
        tool: "apksigner".to_string(),
        hint: INSTALL_HINT.to_string(),
    })
}

/// 候选可执行文件名（Windows 上 Android SDK / PATH 提供的是 `apksigner.bat` 等）。
fn exe_names(stem: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{stem}.bat"),
            format!("{stem}.exe"),
            format!("{stem}.cmd"),
            stem.to_string(),
        ]
    } else {
        vec![stem.to_string()]
    }
}

/// 在 PATH 各目录里找第一个存在的候选文件。
fn find_in_path(path_var: &std::ffi::OsStr, names: &[String]) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 在各 SDK 根的 `build-tools/<版本>` 里找 apksigner（版本降序，逐目录尝试）。
fn find_in_sdk_build_tools(roots: &[PathBuf], names: &[String]) -> Option<PathBuf> {
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root.join("build-tools")) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        versions.sort_by_key(|path| std::cmp::Reverse(version_key(path)));
        for dir in versions {
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// build-tools 目录名的数值版本键（`36.1.0` → `[36, 1, 0]`；非数字段记 0 排最后）。
fn version_key(dir: &Path) -> Vec<u64> {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    name.split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// 执行外部命令：缺席工具 → [`SignError::ToolMissing`]；非零退出 → [`SignError::ToolFailed`]。
fn run(tool: &Path, args: &[OsString], envs: &[(&str, &str)]) -> Result<Output, SignError> {
    let mut command = Command::new(tool);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().map_err(|source| SignError::ToolMissing {
        tool: tool.display().to_string(),
        hint: format!("{INSTALL_HINT}（底层错误：{source}）"),
    })?;
    if !output.status.success() {
        let shown = args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        return Err(SignError::ToolFailed {
            command: format!("{} {shown}", tool.display()),
            status: output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stderr: tail(&String::from_utf8_lossy(&output.stderr), 800),
        });
    }
    Ok(output)
}

/// 从 `apksigner verify --print-certs --verbose` 输出提取关键证据行。
///
/// 标签按**子串**匹配：apksigner 各版本对签名者行的措辞不同（36.x 为
/// `Signer #1 certificate DN: …`，37.x 起为 `V3.0 Signer: certificate DN: …`），
/// 只锚定稳定的关键词，避免绑死某一版本。
fn verify_evidence(stdout: &str) -> String {
    const KEYS: [&str; 4] = [
        "Verifies",
        "Verified using",
        "Number of signers",
        "certificate DN",
    ];
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| KEYS.iter().any(|key| line.contains(key)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 取字符串尾部（诊断信息限长，避免报告被截断日志淹没）。
fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let skip = trimmed.chars().count() - limit;
    trimmed.chars().skip(skip).collect()
}

/// 写 0600 私有文件（Windows 无等价 API，忽略权限：临时目录整体随 [`TempDir`] 删除）。
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SignError> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_err(path))?;
    file.write_all(bytes).map_err(io_err(path))
}

/// 一次性临时目录：`Drop` 兜底删除，签名成功 / 失败都不留密钥材料。
struct TempDir(PathBuf);

impl TempDir {
    /// 在系统临时目录下建 `polypack-sign-<pid>-<序号>`（0700）。
    fn create() -> Result<Self, SignError> {
        let base = std::env::temp_dir();
        for _ in 0..16 {
            let candidate = base.join(format!(
                "polypack-sign-{}-{}",
                std::process::id(),
                TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&candidate) {
                Ok(()) => return Ok(Self(candidate)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(io_err(&candidate)(err)),
            }
        }
        Err(SignError::Io {
            path: base,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "临时目录名连续 16 次冲突（polypack-sign-<pid>-<序号>）",
            ),
        })
    }

    /// 临时目录路径。
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 建 0700 目录（Windows 无等价 API，忽略权限）。
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    /// 建一个一次性目录（测试用，返回路径；调用方自行清理）。
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("polypack-sign-unit-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("测试目录可创建");
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("有父目录")).expect("父目录可创建");
        std::fs::write(path, b"#!/bin/sh\n").expect("占位可写");
    }

    #[test]
    fn explicit_path_wins_over_path_lookup() {
        let dir = scratch("explicit");
        let explicit = dir.join("my-apksigner");
        touch(&explicit);
        let found = resolve_apksigner_from(
            Some(explicit.clone()),
            Some(OsString::from("/nonexistent")),
            &[],
            &exe_names("apksigner"),
        )
        .expect("显式路径优先");
        assert_eq!(found, explicit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_path_missing_reports_tool_missing_with_fix() {
        let err = resolve_apksigner_from(
            Some(PathBuf::from("/nonexistent/apksigner")),
            None,
            &[],
            &exe_names("apksigner"),
        )
        .expect_err("显式路径不存在必须报错，不得静默回退");
        assert_eq!(err.code(), codes::E_SIG_0001);
        assert!(
            err.to_string().contains("POLYPACK_APKSIGNER"),
            "提示须点名该环境变量：{err}"
        );
    }

    #[test]
    fn sdk_roots_pick_max_version_and_degrade_when_incomplete() {
        let dir = scratch("sdk");
        let build_tools = dir.join("sdk").join("build-tools");
        for version in ["34.0.0", "36.0.0", "36.1.0"] {
            touch(&build_tools.join(version).join("apksigner"));
        }
        // 最大版本目录存在但缺 apksigner（半装状态）→ 降级到下一个版本。
        std::fs::create_dir_all(build_tools.join("37.0.0")).expect("37.0.0 目录");
        let roots = vec![dir.join("sdk")];
        let found = resolve_apksigner_from(None, None, &roots, &exe_names("apksigner"))
            .expect("SDK 目录可解析");
        assert_eq!(
            found,
            build_tools.join("36.1.0").join("apksigner"),
            "取最大版本，缺文件则降级（实测 37.0.0 半装 → 36.1.0）"
        );
        let var = std::ffi::OsString::from(dir.join("empty-path"));
        assert!(
            resolve_apksigner_from(None, Some(var), &roots, &exe_names("apksigner")).is_ok(),
            "PATH 为空串目录时仍应从 SDK 解析"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_lookup_and_missing_tool() {
        let dir = scratch("path");
        let bin = dir.join("bin");
        touch(&bin.join("apksigner"));
        let found = resolve_apksigner_from(
            None,
            Some(bin.clone().into_os_string()),
            &[],
            &exe_names("apksigner"),
        )
        .expect("PATH 命中");
        assert_eq!(found, bin.join("apksigner"));

        let err = resolve_apksigner_from(None, None, &[], &exe_names("apksigner"))
            .expect_err("哪里都没有 → ToolMissing");
        assert_eq!(err.code(), codes::E_SIG_0001);
        assert!(err.hint().is_some(), "缺工具须有修复提示");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keystore_file_is_private_and_temp_dir_is_removed() {
        let temp = TempDir::create().expect("临时目录可建");
        let path = temp.path().join(KEYSTORE_FILE);
        write_private(&path, b"not-a-real-keystore").expect("0600 写入");
        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "keystore 必须是 0600");
            let dir_mode = std::fs::metadata(temp.path())
                .expect("stat")
                .permissions()
                .mode();
            assert_eq!(dir_mode & 0o777, 0o700, "临时目录必须是 0700");
        }
        let dir = temp.path().to_path_buf();
        drop(temp);
        assert!(!dir.exists(), "Drop 后临时目录必须消失");
    }

    #[test]
    fn default_output_appends_signed_suffix() {
        assert_eq!(
            default_output(Path::new("/tmp/demo/app.apk")),
            PathBuf::from("/tmp/demo/app-signed.apk")
        );
        assert_eq!(
            default_output(Path::new("/tmp/demo/app")),
            PathBuf::from("/tmp/demo/app-signed.apk")
        );
    }

    #[test]
    fn verify_evidence_keeps_only_key_lines() {
        // 两种签名者标签措辞都要认：36.x `Signer #1 …` / 37.x `V3.0 Signer: …`。
        let noisy = "Verifies\nDEBUG junk\nVerified using v2 scheme (APK Signature Scheme v2): true\n\
                     Signer #1 certificate DN: CN=Pax Notes, O=PolyPack, C=CN\n\
                     Signer #1 certificate SHA-256 digest: deadbeef\nNumber of signers: 1\n";
        let evidence = verify_evidence(noisy);
        assert!(evidence.contains("Verifies"));
        assert!(evidence.contains("Signer #1 certificate DN: CN=Pax Notes"));
        assert!(evidence.contains("Number of signers: 1"));
        assert!(!evidence.contains("junk"), "无关行不进证据：{evidence}");

        let newer = "Verifies\nV3.0 Signer: certificate DN: CN=Pax Notes, O=PolyPack, C=CN\n\
                     V3.0 Signer: certificate SHA-256 digest: deadbeef\n";
        assert!(
            verify_evidence(newer).contains("certificate DN: CN=Pax Notes"),
            "apksigner 37.x 的 `V3.0 Signer:` 标签同样要提取"
        );
    }

    #[test]
    fn tail_keeps_end() {
        let long = "x".repeat(2000) + "END";
        assert!(tail(&long, 10).ends_with("END"));
        assert_eq!(tail(&long, 10).chars().count(), 10);
    }
}
