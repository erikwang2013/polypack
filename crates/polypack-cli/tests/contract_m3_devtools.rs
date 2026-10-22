//! T-M3-11 验收：小程序三端开发者工具 CLI 校验腿（wechat / alipay / douyin）。
//!
//! 与 `contract_m3_miniapp.rs`（T-M3-06：Taro 打包 + 代码包结构断言 + 金样比对）配套：
//! 本文件补 ROADMAP §3.3 verify 列声明的开发者工具 CLI 校验（`wechat-devtools-cli` /
//! `alipay-devtools-cli` / `douyin-devtools-cli`，注册表声明在 `crates/polypack-targets/src/builtin.rs`）。
//! 快应用的 `hap-toolkit-lint` 不在此列（归 T-M3-12）。
//!
//! ## 模式开关（同 M1 / M2 / M3 其余契约腿）
//!
//! - **mock（默认，每提交）**：①CLI 描述符表 ↔ 注册表声明一致（三端 verify id 逐一对应）；
//!   ②调用面组装（程序 / 参数 / 环境，纯函数组装，任意宿主可跑）；③**登录态注入与脱敏**——
//!   合成登录态经 `CredentialRef` 注入 → 0600 临时文件 → 只以**路径**进子进程环境，值不进
//!   `line()` / 任何文本；④脱敏检查器非空转证明（含秘密的文本必被检出）。
//! - **real（`POLYPACK_CONTRACT_REAL=1`，每夜）**：Taro 工具链缺席 → NOTE 返回；三端 CLI 全缺席
//!   → 逐端 SKIP-with-print 注记后返回（不白跑一次真编译）；否则 `polypack init` → 真编译
//!   （`polypack build --target <t>`）→ 逐端：CLI 在场且登录态已注入 → 预览形态真调起（有界等待）
//!   + 退出码与输出脱敏断言；在场但未注入 → NOTE（预览需登录，注入凭据引用后生效）。
//!
//! ## 凭据引用（T-M2-11 模型）
//!
//! `CredentialRef { name: "<target>-devtools-login", source: Env { variable: "POLYPACK_<TARGET>_DEVTOOLS_LOGIN" } }`
//! → 值只存在于内存与 0600 临时文件（用后即弃）；交付给 CLI 的只有**路径**（env
//! `POLYPACK_DEVTOOLS_LOGIN_FILE`；支付宝另按官方变量名 `ALIPAY_IDENTITY_KEY_PATH` 同值交付）。
//! 官方 CLI 对登录态的消费方式各异（微信开发者工具需交互登录、支付宝 minidev 认身份密钥、
//! 抖音 tt-ide-cli 认免密 Token）——本腿钉死「注入与脱敏」两端；CLI 侧消费在有开发者工具的
//! 宿主上实证（本机与公开 CI 无 devtools，按纪律 SKIP-with-print）。

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use polypack_core::{CredentialRef, CredentialSource};
use polypack_targets::Registry;

/// 三端目标（T-M3-11 范围）。
const TARGETS: [&str; 3] = ["wechat", "alipay", "douyin"];

/// 预览形态的有界等待：交互 / GUI 型 CLI 不得把 nightly 挂死。
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(180);

/// 子进程环境里交付登录态**路径**的变量名（值永不进参数 / 日志）。
const LOGIN_FILE_VAR: &str = "POLYPACK_DEVTOOLS_LOGIN_FILE";

// ---------------------------------------------------------------- 模式开关

/// 真实模式开关：`POLYPACK_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    ProcessCommand::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

// ---------------------------------------------------------------- CLI 描述符表

/// 开发者工具 CLI 描述符（每目标一行；预览形态与安装落点均为官方文档口径）。
struct DevtoolsCli {
    /// 目标 id（`TargetSpec.id`）。
    target: &'static str,
    /// 注册表声明的校验器 id（`TargetSpec.verify` 的唯一项）。
    verifier: &'static str,
    /// 人读名（SKIP / NOTE 指引）。
    name: &'static str,
    /// 环境变量覆盖（完整可执行路径；自装宿主 / CI 用）。
    path_var: &'static str,
    /// 常见安装落点（绝对值；按序探测）。
    candidates: &'static [&'static str],
    /// PATH 上的命令名（npm 装的三方 CLI；无则 `None`）。
    command: Option<&'static str>,
    /// 预览形态参数（`<dir>` = 代码包目录占位；appid 等平台参数由代码包侧描述符承担）。
    preview_args: &'static [&'static str],
    /// 登录态环境变量（内容；缺省 = 未注入）。
    login_var: &'static str,
    /// 登录态路径的官方交付变量名（有则同值交付；见模块头）。
    login_env_name: Option<&'static str>,
}

/// 三端 CLI 表（`crates/polypack-targets/src/builtin.rs` 的 verify 列逐一对应）。
const CLIS: [DevtoolsCli; 3] = [
    DevtoolsCli {
        target: "wechat",
        verifier: "wechat-devtools-cli",
        name: "微信开发者工具 CLI",
        path_var: "POLYPACK_WECHAT_DEVTOOLS_CLI",
        // 官方默认安装路径（macOS / Windows）；Linux 无官方发行
        candidates: &[
            "/Applications/wechatwebdevtools.app/Contents/MacOS/cli",
            "C:/Program Files (x86)/Tencent/微信web开发者工具/cli.bat",
        ],
        command: None,
        // `cli -p <project_root>`：预览（需登录；二维码缺省打到命令行）
        preview_args: &["-p", "<dir>"],
        login_var: "POLYPACK_WECHAT_DEVTOOLS_LOGIN",
        login_env_name: None,
    },
    DevtoolsCli {
        target: "alipay",
        verifier: "alipay-devtools-cli",
        name: "支付宝小程序开发者工具 CLI（minidev）",
        path_var: "POLYPACK_ALIPAY_DEVTOOLS_CLI",
        candidates: &[],
        command: Some("minidev"),
        // `minidev preview -p <project>`：真机预览（需授权；appid 取描述符 / ALIPAY_APP_ID）
        preview_args: &["preview", "-p", "<dir>"],
        login_var: "POLYPACK_ALIPAY_DEVTOOLS_LOGIN",
        login_env_name: Some("ALIPAY_IDENTITY_KEY_PATH"),
    },
    DevtoolsCli {
        target: "douyin",
        verifier: "douyin-devtools-cli",
        name: "抖音开发者工具 CLI（tt-ide-cli / tma）",
        path_var: "POLYPACK_DOUYIN_DEVTOOLS_CLI",
        candidates: &[],
        command: Some("tma"),
        // `tma preview <entry>`：远程预览（项目根须含 project.config.json；免密 Token 走 CI）
        preview_args: &["preview", "<dir>"],
        login_var: "POLYPACK_DOUYIN_DEVTOOLS_LOGIN",
        login_env_name: None,
    },
];

/// 按目标取描述符。
fn cli_for(target: &str) -> &'static DevtoolsCli {
    CLIS.iter()
        .find(|cli| cli.target == target)
        .unwrap_or_else(|| panic!("CLI 表缺目标 {target}"))
}

/// 内建注册表中的目标（断言为 L2）。
fn registry_spec(id: &str) -> polypack_targets::TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 L2 目标 {id}"));
    assert_eq!(spec.tier, 2, "{id} 应为 L2（tier 2）");
    spec
}

// ---------------------------------------------------------------- 探测与组装

/// 探测 CLI：环境变量覆盖 → 候选落点 → PATH 命令名（解析链同 polypack-bundle 的 bundletool 纪律）。
fn probe(cli: &DevtoolsCli) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(cli.path_var).map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    for candidate in cli.candidates {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    cli.command.and_then(on_path)
}

/// PATH 上的可执行名（Windows 顺带试常见后缀）。
fn on_path(name: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        [".exe", ".cmd", ".bat", ""]
            .iter()
            .map(|extension| format!("{name}{extension}"))
            .collect()
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// 组装 devtools 调用（纯函数）：程序 = 探测所得路径，参数 = 预览形态 + 代码包目录，
/// 登录态只以**路径**进子进程环境。
fn assemble(
    cli: &DevtoolsCli,
    program: &Path,
    pack_dir: &Path,
    login: Option<&LoginState>,
) -> polypack_bundle::Command {
    let args = cli.preview_args.iter().map(|arg| {
        if *arg == "<dir>" {
            pack_dir.display().to_string()
        } else {
            (*arg).to_string()
        }
    });
    let mut command =
        polypack_bundle::Command::new(program.display().to_string(), args).in_dir(pack_dir);
    if let Some(login) = login {
        command = command.with_env(LOGIN_FILE_VAR, login.path.display().to_string());
        if let Some(name) = cli.login_env_name {
            command = command.with_env(name, login.path.display().to_string());
        }
    }
    command
}

/// 有界执行：超时即杀，返回 `None`（GUI / 交互型 CLI 不得挂死调用方）。
fn run_bounded(command: &polypack_bundle::Command, timeout: Duration) -> Option<Output> {
    let mut process = ProcessCommand::new(&command.program);
    process.args(&command.args);
    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }
    for (key, value) in &command.env {
        process.env(key, value);
    }
    let mut child = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

// ---------------------------------------------------------------- 凭据注入（T-M2-11 模型）

/// 登录态句柄：值（内存）+ 0600 临时文件（用后即弃）。
///
/// 不派生 `Debug`：任何 `{:?}` / 序列化都不得带出值；对外只暴露文件**路径**。
struct LoginState {
    path: PathBuf,
    value: String,
}

impl Drop for LoginState {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 目标的凭据引用（只以引用出现，值经环境注入）。
fn login_reference(cli: &DevtoolsCli) -> CredentialRef {
    CredentialRef {
        name: format!("{}-login", cli.target),
        source: CredentialSource::Env {
            variable: cli.login_var.to_string(),
        },
    }
}

/// 生产取用：进程环境（CI 由 workflow 把 secret 映进同名变量；未配置 = 空串）。
fn login_from_process_env(variable: &str) -> Option<String> {
    std::env::var(variable).ok()
}

/// 凭据引用 → 登录态：值经 `from_env` 取出（测试注入代替 `set_var`——edition 2024 下
/// `set_var` 是 unsafe，本仓 `unsafe_code = "forbid"`），落 0600 临时文件，用后即弃。
///
/// 只接 `Env` 注入（CI 模型）；keyring / Vault 走 polypack-sign 后端链（T-M2-11 面），不在此重复。
fn resolve_login(
    reference: &CredentialRef,
    from_env: &dyn Fn(&str) -> Option<String>,
) -> Option<LoginState> {
    let variable = match &reference.source {
        CredentialSource::Env { variable } => variable,
        CredentialSource::Keyring | CredentialSource::Vault => return None,
    };
    // 空值 = 未注入（CI 里未配置的 secret 就是空串），只在此处判一次
    let value = from_env(variable).filter(|value| !value.is_empty())?;
    // 文件名不含引用名的任意字符：临时落点只由本进程控制
    let name: String = reference
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = std::env::temp_dir().join(format!(
        "polypack-devtools-login-{name}-{}",
        std::process::id()
    ));
    std::fs::write(&path, &value).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok()?;
    }
    Some(LoginState { path, value })
}

// ---------------------------------------------------------------- 脱敏断言

/// 泄露判定（纯函数；非空转证明见 `redaction_checker_detects_leaks`）。
fn contains_secret(text: &str, secret: &str) -> bool {
    !secret.is_empty() && text.contains(secret)
}

/// 脱敏断言：`text` 的任何位置都不得出现秘密（消息自身不带秘密）。
fn assert_no_secret(what: &str, text: &str, secret: &str) {
    assert!(
        !contains_secret(text, secret),
        "{what} 泄露了登录态（脱敏断言）"
    );
}

// ---------------------------------------------------------------- mock 契约

/// 描述符表 ↔ 注册表声明一致：三端 verify id 即本表的校验器 id（少一端 / 多一端即漂移）。
#[test]
fn devtools_table_covers_declared_verifiers() {
    for target in TARGETS {
        let spec = registry_spec(target);
        let cli = cli_for(target);
        assert_eq!(
            spec.verify,
            vec![cli.verifier.to_string()],
            "{target} 注册表 verify 声明与 CLI 表不一致"
        );
    }
    // 表内目标不多不少（quickapp 的 hap-toolkit-lint 归 T-M3-12）
    let mut targets: Vec<&str> = CLIS.iter().map(|cli| cli.target).collect();
    targets.sort_unstable();
    let mut declared = TARGETS.to_vec();
    declared.sort_unstable();
    assert_eq!(targets, declared, "CLI 表目标集合漂移");
}

/// 调用面组装（mock，任意宿主）：程序 = 探测所得路径，参数含代码包目录且占位符已替换，
/// 未注入登录态时环境为空、`line()` 无秘密。
#[test]
fn invocation_is_assembled_from_the_table() {
    let pack =
        std::env::temp_dir().join(format!("polypack-devtools-assemble-{}", std::process::id()));
    std::fs::create_dir_all(&pack).expect("代码包目录可建");
    let program = Path::new("/opt/polypack-test/devtools-cli");
    for target in TARGETS {
        let cli = cli_for(target);
        let command = assemble(cli, program, &pack, None);
        assert_eq!(
            command.program,
            program.display().to_string(),
            "{target} 程序"
        );
        assert!(
            command.args.contains(&pack.display().to_string()),
            "{target} 参数须含代码包目录：{:?}",
            command.args
        );
        assert!(
            !command.args.iter().any(|arg| arg.contains("<dir>")),
            "{target} 占位符未替换：{:?}",
            command.args
        );
        assert!(command.env.is_empty(), "{target} 未注入登录态时环境应为空");
        // `line()` 把参数串起来（占位符 / 目录替换的事实来源即组装函数本身）
        assert!(
            command.line().contains(&pack.display().to_string()),
            "{target} line() 缺代码包目录"
        );
    }
    let _ = std::fs::remove_dir_all(&pack);
}

/// 登录态注入与脱敏（核心）：引用 → 值（合成注入）→ 0600 临时文件 → 只以路径进环境；
/// 值不进 `line()` / `Debug`；用后即弃（临时文件零残留）。
#[test]
fn login_state_injection_materializes_and_never_leaks() {
    const SECRET_VAR: &str = "POLYPACK_TEST_DEVTOOLS_LOGIN";
    let secret = "synthetic-login-state-9f2c7d";
    let reference = CredentialRef {
        name: "wechat-devtools-login".to_string(),
        source: CredentialSource::Env {
            variable: SECRET_VAR.to_string(),
        },
    };
    let lookup = |variable: &str| (variable == SECRET_VAR).then(|| secret.to_string());

    let login = resolve_login(&reference, &lookup).expect("注入的登录态必须解析成功");
    // 正控：值真落盘（注入不是空转）
    assert_eq!(
        std::fs::read_to_string(&login.path).expect("临时文件可读"),
        secret
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&login.path)
            .expect("临时文件可查")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "登录态临时文件须 0600");
    }

    let pack =
        std::env::temp_dir().join(format!("polypack-devtools-inject-{}", std::process::id()));
    std::fs::create_dir_all(&pack).expect("代码包目录可建");
    let cli = cli_for("wechat");
    let command = assemble(
        cli,
        Path::new("/opt/polypack-test/devtools-cli"),
        &pack,
        Some(&login),
    );
    let shown = format!("{}\n{command:?}", command.line());
    assert_no_secret("调用面文本", &shown, secret);
    // Windows 的 Debug 会把路径反斜杠转义（`\` → `\\`）：原始或转义形态任一命中即算「以路径出现」
    //（CI windows-2022 实证，2026-10-16）。
    let raw = login.path.display().to_string();
    let escaped = format!("{raw:?}").trim_matches('"').to_string();
    assert!(
        shown.contains(&raw) || shown.contains(&escaped),
        "登录态须以路径出现在调用面：{shown}"
    );
    assert!(
        command
            .env
            .iter()
            .any(|(key, value)| key == LOGIN_FILE_VAR && value == &login.path.display().to_string()),
        "登录态路径须经 {LOGIN_FILE_VAR} 交付：{:?}",
        command.env
    );
    // 支付宝另按官方变量名同值交付（变量名来自本表，非散落断言）
    let alipay = cli_for("alipay");
    let official = alipay.login_env_name.expect("支付宝须有官方变量名");
    let alipay_command = assemble(
        alipay,
        Path::new("/opt/polypack-test/minidev"),
        &pack,
        Some(&login),
    );
    assert!(
        alipay_command
            .env
            .iter()
            .any(|(key, value)| key == official && value == &login.path.display().to_string()),
        "支付宝须按 {official} 交付路径：{:?}",
        alipay_command.env
    );

    // 用后即弃：句柄落地 → 临时文件清除
    let path = login.path.clone();
    drop(login);
    assert!(!path.exists(), "登录态临时文件须零残留");

    // 缺席 / 空值 = 未注入（NOTE 路径），不是错误
    assert!(resolve_login(&reference, &|_| None).is_none());
    assert!(resolve_login(&reference, &|_| Some(String::new())).is_none());
    // 非 Env 来源本腿不接（keyring / Vault 归 polypack-sign 后端链）
    assert!(
        resolve_login(
            &CredentialRef {
                name: "x".to_string(),
                source: CredentialSource::Keyring,
            },
            &lookup,
        )
        .is_none()
    );
    let _ = std::fs::remove_dir_all(&pack);
}

/// 脱敏检查器非空转证明：含秘密的文本必被检出，干净文本不误报。
#[test]
fn redaction_checker_detects_leaks() {
    let secret = "synthetic-login-state-9f2c7d";
    assert!(
        contains_secret(&format!("token={secret}"), secret),
        "必被检出"
    );
    assert!(
        contains_secret(&format!("前缀 {secret} 后缀"), secret),
        "夹在文本中间同样检出"
    );
    assert!(
        !contains_secret("clean output, no credential", secret),
        "不得误报"
    );
    assert!(!contains_secret("anything", ""), "空秘密不构成泄露");
}

// ---------------------------------------------------------------- real 模式

/// 仓库根里的 Taro 宿主工程（代码包来源；工具链缺 node_modules 时走 NOTE）。
fn repo_taro_adapter() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/adapters/taro")
        .canonicalize()
        .expect("仓库 Taro 适配器可解析")
}

/// 最新一份 `miniapp` 产物目录（与 `contract_m3_miniapp.rs` 同约定）。
fn find_miniapp(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut dirs: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == "miniapp") {
                dirs.push(path.clone());
            }
            stack.push(path);
        }
    }
    dirs.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    dirs.pop()
}

/// real：三端开发者工具 CLI 校验——探测 → （在场且登录态已注入）预览形态真调起 + 脱敏断言。
#[test]
fn contract_devtools_real_preview() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLYPACK_CONTRACT_REAL=1 运行");
        return;
    }

    // 三端 CLI 全缺席：逐端 SKIP-with-print 注记后返回（不为一次真编译白跑）
    let probed: Vec<(&str, PathBuf)> = TARGETS
        .iter()
        .filter_map(|target| probe(cli_for(target)).map(|path| (*target, path)))
        .collect();
    if probed.is_empty() {
        for target in TARGETS {
            let cli = cli_for(target);
            eprintln!(
                "SKIP {target}：未发现 {}（探测链：{} → 常见落点 → PATH；nightly 挂上开发者工具后走真预览）",
                cli.name, cli.path_var
            );
        }
        return;
    }

    // 代码包来源：Taro 工具链缺席 → NOTE 返回（同 contract_m3_miniapp.rs 纪律）
    let adapter = repo_taro_adapter();
    if !adapter.join("host/node_modules/.bin/taro").is_file() {
        eprintln!(
            "NOTE：Taro 工具链未装（{} 缺 node_modules）——真预览腿需先 npm install",
            adapter.join("host").display()
        );
        return;
    }

    let workdir = std::env::temp_dir().join(format!(
        "polypack-contract-m3-devtools-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("临时工程目录可建");
    let init = polypack(&["init"], &workdir);
    assert!(
        init.status.success(),
        "polypack init 失败（{}）：{}",
        init.status,
        String::from_utf8_lossy(&init.stderr)
    );
    // 工程 ui/ 挂本仓 Taro 宿主（候选链 `ui` → 根 → `ui/adapters/taro/host` 第三跳）
    let link = workdir.join("ui/adapters/taro");
    std::fs::create_dir_all(link.parent().expect("ui/adapters")).expect("ui/adapters 可建");
    #[cfg(unix)]
    if std::os::unix::fs::symlink(&adapter, &link).is_err() {
        eprintln!("NOTE：无法挂载 Taro 宿主工程（symlink 失败）");
        return;
    }

    let present: Vec<&str> = probed.iter().map(|(target, _)| *target).collect();
    let mut args = vec!["build"];
    for target in &present {
        args.push("--target");
        args.push(target);
    }
    args.push("--json");
    let build = polypack(&args, &workdir);
    assert!(
        build.status.success(),
        "polypack build（{present:?}）失败（{}）：{}",
        build.status,
        String::from_utf8_lossy(&build.stderr)
    );

    for (target, program) in &probed {
        let cli = cli_for(target);
        let packages = workdir.join(".polypack/packages").join(target);
        let Some(pack) = find_miniapp(&packages) else {
            eprintln!(
                "NOTE {target}：未发现 miniapp 产物（{}）——打包腿未就绪时如实跳过",
                packages.display()
            );
            continue;
        };
        let Some(login) = resolve_login(&login_reference(cli), &login_from_process_env) else {
            eprintln!(
                "NOTE {target}：{} 在场但登录态未注入（{}）——预览需登录，注入凭据引用后走真预览",
                cli.name, cli.login_var
            );
            continue;
        };
        let command = assemble(cli, program, &pack, Some(&login));
        assert_no_secret("调用面文本", &command.line(), &login.value);
        let Some(output) = run_bounded(&command, PREVIEW_TIMEOUT) else {
            eprintln!(
                "NOTE {target}：{} 预览形态 {}s 未返回（交互 / GUI 未就绪），如实跳过",
                cli.name,
                PREVIEW_TIMEOUT.as_secs()
            );
            continue;
        };
        let captured = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // 脱敏断言先行：即便预览失败，输出也不得带出登录态
        assert_no_secret("devtools CLI 输出", &captured, &login.value);
        assert!(
            output.status.success(),
            "{target} 预览失败（{}）：{captured}",
            output.status
        );
        eprintln!(
            "OK {target}：{} 预览形态退出 0（输出 {} 字节，脱敏断言通过；代码包 {}）",
            cli.name,
            captured.len(),
            pack.display()
        );
    }
    // 真跑产物留档：不删临时工程（CI 由「留产物」步上传；本地留一份供人工核验）
    eprintln!("OK：devtools 真预览腿完成，工程 {}", workdir.display());
}
