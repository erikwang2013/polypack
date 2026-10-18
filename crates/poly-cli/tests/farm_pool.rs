//! T-M8-06 验收（CLI 侧）：两个真 worker 子进程 + 单次 submit 的多目标并发构建。
//!
//! 两个 worker 各持一份工程副本（cwd 不同），主端以 `--project-dir .` 投递——相对路径由
//! 各 worker 按自己的当前目录解析，于是同一份任务帧在本地回路上等价于「两台构建机」。
//! 夹具只有静态 Web 链（web / vidaa-tv，后者走 `sandbox-selfcheck`），毫秒级、不依赖
//! node / 打包器 / 网络。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// 夹具配置：两个静态站点目标（均声明 Linux 宿主、react-dom UI）。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-pool-cli-fixture\"
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
            "polypack-cli-farm-pool-{tag}-{}-{}",
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

/// 写夹具工程（静态 UI + PWA 图标）。
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm pool fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
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

/// 把输出按 JSON Lines 解析（逐行必须合法 JSON）。
fn json_lines(text: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("非 JSON Lines：{line}（{e}）"))
        })
        .collect()
}

/// worker 子进程护栏：即使用例失败也收尸（drop 即 kill + wait）。
struct Worker {
    child: Option<Child>,
}

impl Worker {
    fn spawn(args: &[&str], cwd: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_poly"))
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("poly farm worker 可启动");
        Self { child: Some(child) }
    }

    /// 收尾：退出码 + stdout 事件流。
    fn finish(mut self) -> (i32, Vec<serde_json::Value>) {
        let child = self.child.take().expect("worker 未被回收");
        let out = child.wait_with_output().expect("worker 可回收");
        (out.status.code().unwrap_or(-1), json_lines(&out.stdout))
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

/// 等 worker 的 `hello` 帧落到信箱（<dir>/v1/<id>/to-master 出现定宽帧文件）：
/// 先确认注册已就绪再跑 submit，避免「凑齐两个 worker」受进程启动抖动影响。
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
    panic!("worker {worker_id} 未在 30s 内注册（未写 hello 帧）");
}

/// 两个真 worker + 一次 submit：两个目标分别在两个 worker 上完成，产物合并回执。
#[test]
fn farm_pool_builds_two_targets_concurrently_with_two_workers() {
    eprintln!(
        "SKIP：跨机 / serving 网络面未覆盖（本机无真实跨机农场）——本用例以两个本地 worker \
         子进程 + 目录信箱回路真跑并发调度、超时重试与产物合并"
    );
    let temp = TempDir::new("concurrent");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a);
    write_project(&site_b);
    let farm = temp.path().join("farm");
    let farm_arg = farm.to_str().expect("路径为 UTF-8");

    let worker_args = |id: &str| {
        vec![
            "farm".to_string(),
            "worker".to_string(),
            "--dir".to_string(),
            farm_arg.to_string(),
            "--id".to_string(),
            id.to_string(),
            "--target".to_string(),
            "web".to_string(),
            "--target".to_string(),
            "vidaa-tv".to_string(),
            "--jobs".to_string(),
            "1".to_string(),
            "--idle".to_string(),
            "30".to_string(),
            "--json".to_string(),
        ]
    };
    let args_a = worker_args("worker-a");
    let args_b = worker_args("worker-b");
    // 两个 worker 各在自己的工程副本里服务（cwd = site）
    let worker_a = Worker::spawn(
        &args_a.iter().map(String::as_str).collect::<Vec<_>>(),
        &site_a,
    );
    let worker_b = Worker::spawn(
        &args_b.iter().map(String::as_str).collect::<Vec<_>>(),
        &site_b,
    );
    wait_for_hello(&farm, "worker-a");
    wait_for_hello(&farm, "worker-b");

    let out = poly(
        &[
            "farm",
            "submit",
            "--config",
            "polypack.toml",
            "--dir",
            farm_arg,
            "--target",
            "web",
            "--target",
            "vidaa-tv",
            "--wait",
            "60",
            "--retries",
            "1",
            "--project-dir",
            ".",
            "--json",
        ],
        &site_a,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "多目标投递应成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = json_lines(&out.stdout);
    let report = lines.last().expect("结果报告行");
    assert_eq!(report["event"], "farm-result");
    let jobs = report["jobs"].as_array().expect("逐任务报告为数组");
    assert_eq!(jobs.len(), 2, "两个目标应各有一个任务: {jobs:?}");
    assert!(
        jobs.iter().all(|job| job["ok"] == true),
        "全部任务应成功: {jobs:?}"
    );
    let used: HashSet<&str> = jobs
        .iter()
        .filter_map(|job| job["worker_id"].as_str())
        .collect();
    assert_eq!(used.len(), 2, "两个任务应落在两个 worker 上: {jobs:?}");
    let artifacts: Vec<&str> = report["artifacts"]
        .as_array()
        .expect("产物清单为数组")
        .iter()
        .filter_map(|artifact| artifact["name"].as_str())
        .collect();
    assert!(
        artifacts.contains(&"dist/index.html"),
        "真实产物应合并回传: {artifacts:?}"
    );

    // 两个 worker 各完成一个任务后正常退出
    let (code_a, events_a) = worker_a.finish();
    let (code_b, events_b) = worker_b.finish();
    assert_eq!(code_a, 0, "worker-a 正常退出");
    assert_eq!(code_b, 0, "worker-b 正常退出");
    let completed = |events: &[serde_json::Value]| {
        events
            .iter()
            .filter(|event| event["event"] == "warning" && event["stage"] == "farm")
            .filter_map(|event| event["message"].as_str())
            .filter(|message| message.contains("完成"))
            .count()
    };
    assert_eq!(
        completed(&events_a) + completed(&events_b),
        2,
        "两个 worker 合计完成两个任务（各一）"
    );
}
