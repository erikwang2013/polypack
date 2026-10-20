//! T-M8-08 验收（CLI 侧）：`poly farm --cache <共享目录>` 跨 worker 进程复用。
//!
//! 两个 worker 进程各在自己的工程副本里服务（cwd 不同、绝对路径不同），共用同一共享缓存
//! 目录：首跑 worker-a 写共享 CAS，二跑 worker-b 命中，且两侧 `cached` 事件的键完全相同。
//! 夹具只有静态 Web 链，毫秒级、不依赖 node / 网络。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use poly_cache::{ObjectStore, S3Config, S3Env, S3Store};
use serde_json::Value;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// S3 前缀兜底清理：断言 panic 时也删除本测试写入的对象（唯一前缀，不碰他测对象）。
struct S3PrefixCleanup(String);

impl Drop for S3PrefixCleanup {
    fn drop(&mut self) {
        let Ok(config) = S3Config::from_uri(&self.0, S3Env::from_env()) else {
            return;
        };
        let Ok(store) = S3Store::new(&config) else {
            return;
        };
        let Ok(entries) = store.entries() else {
            return;
        };
        for entry in entries {
            let _ = store.remove(&entry.key);
        }
    }
}

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-farm-cache-{tag}-{}-{}",
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
    std::fs::write(
        root.join("polypack.toml"),
        "schema = 1\n\n[package]\nname = \"farm-cache-cli-fixture\"\nversion = \"0.1.0\"\n\n[ui]\nframework = \"react\"\nentry = \"ui/index.html\"\n\n[core]\ncrate = \"core\"\n",
    )
    .expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm cache fixture</title>\n  </head>\n  <body></body>\n</html>\n",
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

/// 输出按 JSON Lines 解析。
fn json_lines(text: &[u8]) -> Vec<Value> {
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

    fn finish(mut self) -> (i32, Vec<Value>) {
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

/// 等 worker 的 `hello` 帧落到信箱。
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

/// 一轮「起 worker → submit 一个 web 任务 → 收 worker 事件流」。
fn run_round(farm: &Path, id: &str, site: &Path, cache: &str) -> (i32, Vec<Value>) {
    let farm_arg = farm.to_str().expect("路径为 UTF-8");
    let worker = Worker::spawn(
        &[
            "farm", "worker", "--dir", farm_arg, "--id", id, "--target", "web", "--jobs", "1",
            "--idle", "30", "--cache", cache, "--json",
        ],
        site,
    );
    wait_for_hello(farm, id);
    let submit = poly(
        &[
            "farm",
            "submit",
            "--config",
            "polypack.toml",
            "--dir",
            farm_arg,
            "--target",
            "web",
            "--wait",
            "60",
            "--retries",
            "0",
            "--project-dir",
            ".",
            "--cache",
            cache,
            "--json",
        ],
        site,
    );
    assert_eq!(
        submit.status.code(),
        Some(0),
        "submit 应成功: {}",
        String::from_utf8_lossy(&submit.stderr)
    );
    worker.finish()
}

/// 事件流中的 `cached` 键（bundle 支线一次查询）。
fn cached_keys(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["event"] == "cached")
        .filter_map(|event| event["key"].as_str().map(str::to_string))
        .collect()
}

/// 任务完成事件里的缓存命中报告（`cache hits N/M` 的中文形态）。
fn hit_reports(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["event"] == "warning" && event["stage"] == "farm")
        .filter_map(|event| event["message"].as_str())
        .filter(|message| message.contains("缓存命中"))
        .map(str::to_string)
        .collect()
}

#[test]
fn shared_cache_hits_across_worker_processes_with_same_key() {
    let temp = TempDir::new("shared");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a);
    write_project(&site_b);
    let shared = temp.path().join("shared-cache");
    std::fs::create_dir_all(&shared).expect("共享目录视为已挂载");
    let shared_arg = shared.to_str().expect("路径为 UTF-8");
    let farm = temp.path().join("farm");

    // 首跑：worker-a 在自己的副本里构建，写入共享 CAS
    let (code_a, events_a) = run_round(&farm, "worker-a", &site_a, shared_arg);
    assert_eq!(code_a, 0, "worker-a 正常退出");
    assert!(
        hit_reports(&events_a)
            .iter()
            .any(|message| message.contains("0/1")),
        "首跑应为未命中: {:?}",
        hit_reports(&events_a)
    );

    // 二跑：worker-b 在另一份副本（绝对路径不同）里构建，应从共享 CAS 全部命中
    let (code_b, events_b) = run_round(&farm, "worker-b", &site_b, shared_arg);
    assert_eq!(code_b, 0, "worker-b 正常退出");
    assert!(
        hit_reports(&events_b)
            .iter()
            .any(|message| message.contains("1/1")),
        "二跑应跨 worker 命中共享 CAS: {:?}",
        hit_reports(&events_b)
    );

    // 同输入 → 同一缓存键（两个独立进程、两份工程副本）
    let keys_a = cached_keys(&events_a);
    let keys_b = cached_keys(&events_b);
    assert!(!keys_a.is_empty() && !keys_b.is_empty(), "应有 cached 事件");
    assert_eq!(keys_a, keys_b, "不同 worker 上同一输入必须产生同一缓存键");
}

/// ENV-17 真实 S3 腿：MinIO / 真实桶作为两 worker 的共享缓存，二跑必须 1/1 命中。
///
/// 未设置 `POLYPACK_S3_REAL=1` 时打印 SKIP 并返回（本地默认）；开启后缺 URI / 端点 /
/// 凭据直接失败，不静默跳过。每次运行使用独立前缀并在 Drop 中清理。
#[test]
fn shared_cache_hits_across_worker_processes_on_s3() {
    if std::env::var("POLYPACK_S3_REAL").as_deref() != Ok("1") {
        println!(
            "SKIP: 真实 S3 环境槽未启用（设置 POLYPACK_S3_REAL=1 与 \
             POLYPACK_S3_TEST_URI=s3://<bucket>[/<prefix>]）"
        );
        return;
    }
    let base = std::env::var("POLYPACK_S3_TEST_URI")
        .expect("POLYPACK_S3_REAL=1 时必须设置 POLYPACK_S3_TEST_URI");
    let temp = TempDir::new("s3");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a);
    write_project(&site_b);
    // 独立前缀：首跑 0/1、二跑 1/1 只可能来自本次运行写入桶的对象。
    let cache = format!(
        "{}/{}-{}",
        base.trim_end_matches('/'),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let _cleanup = S3PrefixCleanup(cache.clone());
    let farm = temp.path().join("farm");

    let (code_a, events_a) = run_round(&farm, "worker-a", &site_a, &cache);
    assert_eq!(code_a, 0, "worker-a 正常退出");
    assert!(
        hit_reports(&events_a)
            .iter()
            .any(|message| message.contains("0/1")),
        "首跑应为未命中并写入桶: {:?}",
        hit_reports(&events_a)
    );

    let (code_b, events_b) = run_round(&farm, "worker-b", &site_b, &cache);
    assert_eq!(code_b, 0, "worker-b 正常退出");
    assert!(
        hit_reports(&events_b)
            .iter()
            .any(|message| message.contains("1/1")),
        "二跑应从 S3 共享桶全命中: {:?}",
        hit_reports(&events_b)
    );

    let keys_a = cached_keys(&events_a);
    let keys_b = cached_keys(&events_b);
    assert!(!keys_a.is_empty() && !keys_b.is_empty(), "应有 cached 事件");
    assert_eq!(keys_a, keys_b, "不同 worker 上同一输入必须产生同一缓存键");
}
