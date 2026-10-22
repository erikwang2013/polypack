//! T-M9-53/54：真实 Tauri v2 样例的 MSI 安装 → 启动 → 卸载 → 残留冒烟。
//!
//! 环境门（任一不满足即打印 SKIP，绝不把「没跑」写成 PASS）：
//! - 非 Windows 宿主：`candle` / `light` / `msiexec` / WebView2 均不可用。
//! - 未设置 `POLYPACK_MSI_REAL=1`：普通 `cargo test` 不做机器级安装。
//! - 样例未构建且未设置 `POLYPACK_TAURI_CORE_BIN`。
//! - WiX（`candle` / `light`）或 `msiexec` 不在 PATH。
//!
//! 真实路径（CI `.github/workflows/rust-ci.yml` 的 `windows-msi-real` 作业）：
//! `cargo build --release` 构建 `examples/windows-msi-app`（真实 Tauri v2 应用），
//! 设置 `POLYPACK_TAURI_CORE_BIN` / `POLYPACK_MSI_REAL=1` 后运行本测试。本测试用**仓库内**
//! WiX 打包器（`WixMsi`）产出 MSI，`msiexec /i /qn` 安装到默认
//! `%ProgramFiles%\<产品名>`，启动**已安装副本**的 `--smoke` 自退出路径，
//! `msiexec /x /qn` 卸载，再断言安装目录 / 开始菜单 / HKCU 标记值 / ARP 条目均消失。
//!
//! 失败路径稳定错误码（panic 前缀，便于 CI 判定阶段）：
//! - `POLYPACK_MSI_REAL_0001` 样例 / `POLYPACK_TAURI_CORE_BIN` 输入缺失或无效
//! - `POLYPACK_MSI_REAL_0002` 裸产物布局或 `candle` / `light` 构建失败
//! - `POLYPACK_MSI_REAL_0003` `msiexec /i /qn` 安装失败 / 超时
//! - `POLYPACK_MSI_REAL_0004` 已安装 exe 启动、`--smoke` 自退出或标记校验失败
//! - `POLYPACK_MSI_REAL_0005` `msiexec /x /qn` 卸载失败 / 超时
//! - `POLYPACK_MSI_REAL_0006` 残留检查失败
//!
//! 证据落 `POLYPACK_MSI_REAL_OUT`（CI 指向 `%RUNNER_TEMP%\polypack-msi-real`）：`.wxs`、
//! `.wixobj`、`.msi`、`install.log`、`uninstall.log`、`smoke-marker.txt`、`msi.sri.txt`。

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use polypack_bundle::packagers::windows::WixMsi;
use polypack_bundle::{BundleInputs, Icon, IconKind, Packager, ProductInfo, tauri};
use polypack_targets::{Host, TargetSpec};

const PRODUCT_NAME: &str = "Polypack MSI Sample";
const PRODUCT_VERSION: &str = "0.1.0";
const PRODUCT_IDENTIFIER: &str = "com.polypack.windows-msi-sample";
const BINARY: &str = "polypack-msi-sample";

const E_INPUT: &str = "POLYPACK_MSI_REAL_0001";
const E_BUILD: &str = "POLYPACK_MSI_REAL_0002";
const E_INSTALL: &str = "POLYPACK_MSI_REAL_0003";
const E_LAUNCH: &str = "POLYPACK_MSI_REAL_0004";
const E_UNINSTALL: &str = "POLYPACK_MSI_REAL_0005";
const E_RESIDUE: &str = "POLYPACK_MSI_REAL_0006";

const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(120);
const RESIDUE_TIMEOUT: Duration = Duration::from_secs(30);

/// MSI 是 OLE 复合文档（CFB）。
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

fn fail(code: &str, detail: impl std::fmt::Display) -> ! {
    panic!("{code}: {detail}");
}

/// 仓库根（`crates/polypack-bundle` 的祖父目录）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/polypack-bundle 应有仓库根祖先")
        .to_path_buf()
}

/// 证据目录：CI 用 `POLYPACK_MSI_REAL_OUT`，本机默认每进程独立临时目录。
fn out_dir() -> PathBuf {
    std::env::var_os("POLYPACK_MSI_REAL_OUT").map_or_else(
        || std::env::temp_dir().join(format!("polypack-msi-real-{}", std::process::id())),
        PathBuf::from,
    )
}

/// 64 位 Program Files（测试进程为 x64；`ProgramW6432` 明确指向 64 位视图）。
fn program_files() -> PathBuf {
    std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from)
}

fn program_data() -> PathBuf {
    std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from)
}

fn product() -> ProductInfo {
    ProductInfo {
        name: PRODUCT_NAME.to_string(),
        version: PRODUCT_VERSION.to_string(),
        identifier: PRODUCT_IDENTIFIER.to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Minimal Tauri v2 MSI sample".to_string(),
        homepage: None,
    }
}

fn spec() -> TargetSpec {
    let mut spec = TargetSpec::new_stub("windows", 1, &[Host::Win]);
    spec.package.format = "msi".to_string();
    spec.package.builder = "wix".to_string();
    spec
}

/// 真实样例 exe → Tauri 裸产物布局 → 仓库内 WiX 打包器出 MSI。
fn build_msi(sample: &Path, core: &Path, out: &Path) -> polypack_bundle::Artifact {
    let mut inputs = BundleInputs::new(product(), out);
    inputs.core_binary = Some(core.to_path_buf());
    inputs.ui_dist = Some(sample.join("web"));
    let icon = sample.join("src-tauri/icons/icon.ico");
    if icon.is_file() {
        inputs.icons.push(Icon {
            path: icon,
            kind: IconKind::Ico,
            size: None,
        });
    }
    let bare = tauri::bare_product(&inputs, Host::Win)
        .unwrap_or_else(|err| fail(E_BUILD, format!("裸产物布局失败：{err}")));
    inputs.bare = Some(bare);
    WixMsi
        .package(&inputs, &spec())
        .unwrap_or_else(|err| fail(E_BUILD, format!("candle/light 构建 MSI 失败：{err}")))
}

/// 命令结果（带超时标记；超时后子进程已被 kill）。
struct Run {
    status: ExitStatus,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Run {
    fn text(&self) -> String {
        format!(
            "stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

/// 有界等待：轮询子进程，超时则 kill 并返回 `timed_out = true`。
fn run_timed(cmd: &mut Command, timeout: Duration, capture: bool) -> std::io::Result<Run> {
    cmd.stdin(Stdio::null());
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            return Ok(Run {
                status: output.status,
                timed_out: false,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            return Ok(Run {
                status: output.status,
                timed_out: true,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// MSI 成功码：0 = 成功；3010 = 成功但需重启；1641 = 已启动重启。
fn msi_succeeded(status: ExitStatus) -> bool {
    matches!(status.code(), Some(0 | 3010 | 1641))
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `msiexec <verb> <msi> /qn /norestart /l*v <log>`（日志落证据目录）。
fn msiexec(verb: &str, msi: &Path, log: &Path, timeout: Duration, code: &str) -> Run {
    let mut cmd = Command::new("msiexec");
    cmd.arg(verb)
        .arg(msi)
        .args(["/qn", "/norestart"])
        .arg("/l*v")
        .arg(log);
    run_timed(&mut cmd, timeout, false)
        .unwrap_or_else(|err| fail(code, format!("无法执行 msiexec {verb}：{err}")))
}

#[test]
fn real_tauri_msi_install_launch_uninstall() {
    if !cfg!(windows) {
        eprintln!(
            "SKIP：真实 Tauri MSI 安装/启动/卸载仅 Windows 宿主可执行（candle/light/msiexec/WebView2）"
        );
        return;
    }
    if !matches!(std::env::var("POLYPACK_MSI_REAL").as_deref(), Ok("1")) {
        eprintln!(
            "SKIP：未设置 POLYPACK_MSI_REAL=1（机器级安装测试需显式开启；CI 的 windows-msi-real 作业会设置）"
        );
        return;
    }

    let sample = repo_root().join("examples/windows-msi-app");
    if !sample.join("web/index.html").is_file() {
        eprintln!("SKIP：真实 Tauri 样例缺失：{}", sample.display());
        return;
    }
    let default_exe = sample.join("src-tauri/target/release/polypack-msi-sample.exe");
    let core = match std::env::var_os("POLYPACK_TAURI_CORE_BIN") {
        Some(raw) => {
            let path = PathBuf::from(raw);
            if !path.is_file() {
                fail(
                    E_INPUT,
                    format!("POLYPACK_TAURI_CORE_BIN 指向的文件不存在：{}", path.display()),
                );
            }
            path
        }
        None if default_exe.is_file() => default_exe,
        None => {
            eprintln!(
                "SKIP：样例未构建（先运行 examples/windows-msi-app/build.ps1）且未设置 POLYPACK_TAURI_CORE_BIN；期望 {}",
                default_exe.display()
            );
            return;
        }
    };
    if common::skip_without_any(&["candle", "light", "msiexec"]) {
        return;
    }

    let out = out_dir();
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap_or_else(|err| {
        fail(
            E_INPUT,
            format!("无法创建证据目录 {}：{err}", out.display()),
        )
    });

    // 1. 真实 Tauri exe → 裸产物 → WiX MSI。
    let artifact = build_msi(&sample, &core, &out);
    let bytes = std::fs::read(&artifact.path)
        .unwrap_or_else(|err| fail(E_BUILD, format!("读取 MSI 失败：{err}")));
    if bytes.len() < CFB_MAGIC.len() || bytes[..CFB_MAGIC.len()] != CFB_MAGIC {
        fail(
            E_BUILD,
            format!("产物不是 CFB 复合文档（MSI）：{}", artifact.path.display()),
        );
    }
    let digest = polypack_bundle::sri::integrity_of(&bytes);
    println!(
        "MSI: {} ({} bytes)",
        artifact.path.display(),
        artifact.bytes
    );
    println!("MSI digest: {digest}");
    std::fs::write(out.join("msi.sri.txt"), format!("{digest}\n"))
        .unwrap_or_else(|err| fail(E_BUILD, format!("写 MSI 摘要失败：{err}")));

    // 2. 静默安装（perMachine → 默认 Program Files；CI runner 已提权）。
    let install_log = out.join("install.log");
    let install = msiexec(
        "/i",
        &artifact.path,
        &install_log,
        INSTALL_TIMEOUT,
        E_INSTALL,
    );
    if install.timed_out {
        fail(
            E_INSTALL,
            format!(
                "安装超时（{}s），日志：{}",
                INSTALL_TIMEOUT.as_secs(),
                install_log.display()
            ),
        );
    }
    if !msi_succeeded(install.status) {
        fail(
            E_INSTALL,
            format!(
                "msiexec /i 退出码 {:?}，日志：{}",
                install.status.code(),
                install_log.display()
            ),
        );
    }

    let install_dir = program_files().join(PRODUCT_NAME);
    let installed_exe = install_dir.join(format!("{BINARY}.exe"));
    if !wait_until(Duration::from_secs(15), || installed_exe.is_file()) {
        fail(
            E_INSTALL,
            format!("安装后缺少已安装可执行：{}", installed_exe.display()),
        );
    }
    println!("installed: {}", installed_exe.display());

    // 3. 启动已安装副本；--smoke 在 setup 钩子后自退出（钩子可达即 WebView2 已初始化）。
    let marker = out.join("smoke-marker.txt");
    let _ = std::fs::remove_file(&marker);
    let mut launch_cmd = Command::new(&installed_exe);
    launch_cmd
        .arg("--smoke")
        .env("POLYPACK_MSI_SMOKE_MARKER", &marker);
    let launch = run_timed(&mut launch_cmd, LAUNCH_TIMEOUT, true)
        .unwrap_or_else(|err| fail(E_LAUNCH, format!("无法启动已安装 exe：{err}")));
    if launch.timed_out {
        fail(
            E_LAUNCH,
            format!(
                "已安装 exe 未在 {}s 内退出（--smoke 自退出失败）；{}",
                LAUNCH_TIMEOUT.as_secs(),
                launch.text()
            ),
        );
    }
    if !launch.status.success() {
        fail(
            E_LAUNCH,
            format!(
                "已安装 exe 退出码 {:?}；{}",
                launch.status.code(),
                launch.text()
            ),
        );
    }
    let marker_text = std::fs::read_to_string(&marker).unwrap_or_else(|err| {
        fail(
            E_LAUNCH,
            format!(
                "--smoke 未写标记 {}：{err}；{}",
                marker.display(),
                launch.text()
            ),
        )
    });
    if marker_text.trim() != "ok" {
        fail(E_LAUNCH, format!("--smoke 标记内容异常：{marker_text:?}"));
    }
    println!(
        "launch: OK (installed exe exited 0, marker={})",
        marker.display()
    );

    // 4. 静默卸载。
    let uninstall_log = out.join("uninstall.log");
    let uninstall = msiexec(
        "/x",
        &artifact.path,
        &uninstall_log,
        UNINSTALL_TIMEOUT,
        E_UNINSTALL,
    );
    if uninstall.timed_out {
        fail(
            E_UNINSTALL,
            format!(
                "卸载超时（{}s），日志：{}",
                UNINSTALL_TIMEOUT.as_secs(),
                uninstall_log.display()
            ),
        );
    }
    if !msi_succeeded(uninstall.status) {
        fail(
            E_UNINSTALL,
            format!(
                "msiexec /x 退出码 {:?}，日志：{}",
                uninstall.status.code(),
                uninstall_log.display()
            ),
        );
    }

    // 5. 残留检查：安装目录 / 开始菜单 / HKCU 标记值 / ARP 条目。
    if !wait_until(RESIDUE_TIMEOUT, || !install_dir.exists()) {
        fail(
            E_RESIDUE,
            format!("卸载后安装目录仍在：{}", install_dir.display()),
        );
    }
    let start_menu = program_data()
        .join("Microsoft/Windows/Start Menu/Programs")
        .join(PRODUCT_NAME);
    if start_menu.exists() {
        fail(
            E_RESIDUE,
            format!("卸载后开始菜单目录仍在：{}", start_menu.display()),
        );
    }

    let key = format!(r"HKCU\Software\{PRODUCT_NAME}");
    let value = Command::new("reg")
        .args(["query", &key, "/v", "installed"])
        .output()
        .unwrap_or_else(|err| fail(E_RESIDUE, format!("无法查询 HKCU 注册表：{err}")));
    let value_text = String::from_utf8_lossy(&value.stdout);
    if value.status.success() && value_text.contains("installed") {
        fail(
            E_RESIDUE,
            format!("卸载后 HKCU\\Software\\{PRODUCT_NAME} 的 installed 值仍在：{value_text}"),
        );
    }

    let arp = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "/s",
            "/f",
            PRODUCT_NAME,
            "/d",
        ])
        .output()
        .unwrap_or_else(|err| fail(E_RESIDUE, format!("无法查询 ARP 条目：{err}")));
    let arp_text = String::from_utf8_lossy(&arp.stdout);
    if arp_text.contains(PRODUCT_NAME) {
        fail(
            E_RESIDUE,
            format!("卸载后 ARP 仍列出 {PRODUCT_NAME}：{arp_text}"),
        );
    }
    println!(
        "residue: CLEAR (install dir / Start Menu / HKCU value / ARP all absent; per-user WebView2 caches are out of installer-residue scope)"
    );
}
