//! T-M8-07 验收（CLI 侧，真跑）：真 worker 子进程 + 真任务，磁盘 / 日志零明文凭据扫描、
//! 一次性工作目录随任务销毁、`--credential` 只收变量名（误放明文也不回显）。
//!
//! 隔离层级：进程边界（worker 子进程）为本机可验证形态；容器边界未接入，按验收口径
//! SKIP-with-print（见 `container_boundary_is_skip_with_print_on_this_host`）。
//! 凭据哨兵：worker 从环境变量读入内存注入（`--credential tizen-author=env:<变量>`），
//! 用例断言哨兵不出现在农场目录 / 工作目录 / 工程目录 / 两侧日志的任何字节里。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// 凭据哨兵（测试专用，非真实凭据；值只经环境变量下发）。
const CREDENTIAL_VAR: &str = "POLYPACK_TEST_FARM_CREDENTIAL";

/// 夹具配置：tizen-tv 静态出包（毫秒级），并声明签名档案 `tizen-author`——
/// 任务帧因此携带该 `CredentialRef`，覆盖「档案名 → 环境变量 → 内存注入」通道。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-isolation-cli-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-farm-isolation-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm isolation fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    // 静态 UI 打包要求 PWA 图标输入（与 farm_pool.rs 夹具同口径）
    std::fs::write(
        root.join("ui").join("public").join("icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    )
    .expect("写 icon.svg");
}

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 把输出按 JSON Lines 解析（空行忽略）。
fn json_lines(text: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("非 JSON Lines：{line}（{e}）"))
        })
        .collect()
}

/// worker 子进程护栏：即使用例失败也收尸。
struct Worker {
    child: Option<Child>,
}

impl Worker {
    fn spawn(args: &[&str], cwd: &Path, env: (&str, &str)) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_poly"))
            .args(args)
            .current_dir(cwd)
            .env(env.0, env.1)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("poly farm worker 可启动");
        Self { child: Some(child) }
    }

    fn finish(mut self) -> Output {
        self.child
            .take()
            .expect("worker 未被回收")
            .wait_with_output()
            .expect("worker 可回收")
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn wait_for_hello(farm: &Path, worker_id: &str) {
    let inbox = farm.join("v1").join(worker_id).join("to-master");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let ready = std::fs::read_dir(&inbox)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| !entry.file_name().to_string_lossy().starts_with('.'));
        if ready {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("worker {worker_id} 未在 30s 内注册");
}

/// 递归收集普通文件（不跟随符号链接：`file_type` 来自目录项，链接不计入）。
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            files_under(&path, out);
        } else if file_type.is_file() {
            out.push(path);
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// 断言目录下任何文件、以及两侧日志，都不含明文凭据哨兵。
fn assert_no_plaintext(path: &Path, sentinel: &str, label: &str) {
    if !path.exists() {
        return;
    }
    let mut files = Vec::new();
    files_under(path, &mut files);
    for file in files {
        let bytes = std::fs::read(&file).expect("文件可读");
        assert!(
            !contains(&bytes, sentinel.as_bytes()),
            "{label} 磁盘出现明文凭据：{}",
            file.display()
        );
    }
}

// ---------------------------------------------------------------- 真跑用例

/// 真 worker 子进程跑一个带签名档案的任务：凭据只进内存，磁盘与日志零明文；
/// 任务结束工作目录（及其父工作根）被删除，跨任务无残留。
#[test]
fn farm_worker_injects_credentials_in_memory_and_leaves_no_residue() {
    let temp = TempDir::new("scan");
    let site = temp.path().join("site");
    write_project(&site);
    let farm = temp.path().join("farm");
    let work_base = temp.path().join("work");
    let farm_arg = farm.to_str().expect("路径为 UTF-8");
    let work_arg = work_base.to_str().expect("路径为 UTF-8");
    let sentinel = format!(
        "POLYPACK-CRED-SENTINEL-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    let worker = Worker::spawn(
        &[
            "farm",
            "worker",
            "--dir",
            farm_arg,
            "--id",
            "isolation-worker",
            "--target",
            "tizen-tv",
            "--jobs",
            "1",
            "--idle",
            "30",
            "--work-dir",
            work_arg,
            "--credential",
            &format!("tizen-author=env:{CREDENTIAL_VAR}"),
            "--json",
        ],
        &site,
        (CREDENTIAL_VAR, &sentinel),
    );
    wait_for_hello(&farm, "isolation-worker");

    let out = poly(
        &[
            "farm",
            "submit",
            "--config",
            "polypack.toml",
            "--dir",
            farm_arg,
            "--target",
            "tizen-tv",
            "--wait",
            "60",
            "--project-dir",
            ".",
            "--json",
        ],
        &site,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "农场任务应成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_lines(&out.stdout).pop().expect("结果报告行");
    assert_eq!(report["event"], "farm-result");

    let worker_out = worker.finish();
    assert_eq!(worker_out.status.code(), Some(0), "worker 正常退出");
    let events = json_lines(&worker_out.stdout);
    let injected = events.iter().any(|event| {
        event["event"] == "warning"
            && event["message"].as_str().is_some_and(|message| {
                message.contains("内存注入 1 条凭据") && message.contains("tizen-author")
            })
    });
    assert!(
        injected,
        "worker 应报告内存注入（只含档案名，不含值）: {events:?}"
    );

    // 零明文扫描：农场目录（帧）、一次性工作目录、工程目录、两侧日志
    assert_no_plaintext(&farm, &sentinel, "农场目录");
    assert_no_plaintext(&work_base, &sentinel, "任务工作目录");
    assert_no_plaintext(&site, &sentinel, "工程目录");
    assert!(
        !String::from_utf8_lossy(&worker_out.stdout).contains(&sentinel)
            && !String::from_utf8_lossy(&worker_out.stderr).contains(&sentinel),
        "worker 日志不得出现明文凭据"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains(&sentinel)
            && !String::from_utf8_lossy(&out.stderr).contains(&sentinel),
        "主端日志不得出现明文凭据"
    );

    // 任务结束即销毁：worker 工作根随进程退出删除，工作目录父目录为空
    assert!(work_base.is_dir(), "工作父目录应已创建");
    assert_eq!(
        std::fs::read_dir(&work_base)
            .expect("工作父目录可读")
            .count(),
        0,
        "worker 退出后不得残留任何任务工作目录"
    );
}

/// `--credential` 只接受变量名；误把明文值放进 argv 时错误信息不得回显该值。
#[test]
fn credential_flag_never_echoes_argv_values() {
    let temp = TempDir::new("flag");
    let sentinel = format!(
        "PLAINTEXT-MISTAKE-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let out = poly(
        &[
            "farm",
            "worker",
            "--dir",
            temp.path().to_str().expect("路径为 UTF-8"),
            "--credential",
            &format!("tizen-author=plain:{sentinel}"),
        ],
        temp.path(),
    );
    assert_ne!(out.status.code(), Some(0), "非法 --credential 必须拒绝启动");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--credential"),
        "错误信息应点名参数: {stderr}"
    );
    assert!(
        !stderr.contains(&sentinel),
        "错误信息绝不回显 argv 中的值: {stderr}"
    );
}

/// 容器边界：本里程碑交付进程边界 + 私有一次性目录；无 docker 的宿主机按此真跑。
/// 容器部署参数（镜像 / 挂载 / 凭据注入）归 T-M8-09 部署指南，不在本任务伪造。
#[test]
fn container_boundary_is_skip_with_print_on_this_host() {
    let docker = Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    eprintln!(
        "SKIP：容器边界未接入（T-M8-07 交付进程边界 + 私有一次性工作目录）；\
         本机 docker 可用={docker}。真实隔离证据见 farm_worker_injects_credentials_in_memory_and_leaves_no_residue；\
         容器参数归 T-M8-09 部署指南。"
    );
}
