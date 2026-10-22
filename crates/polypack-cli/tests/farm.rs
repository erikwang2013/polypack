//! T-M2-13 验收（CLI 侧）：`polypack farm worker` / `polypack farm submit` 的端到端回环。
//!
//! 夹具工程只有 `web` 目标（无 native 桌面目标 → compile 支线跳过、UI 内建静态打包），
//! 全部用例都在毫秒级，不依赖 node / 打包器 / 网络。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 夹具配置：web 目标（`react-dom` 适配器 → 静态打包），无 native 桌面目标。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-cli-fixture\"
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
            "polypack-cli-farm-{tag}-{}-{}",
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

/// 写夹具工程；`icon` 为假时不放图标资产（打包阶段必然失败 → 失败回传用例）。
fn write_project(root: &Path, icon: bool) {
    std::fs::create_dir_all(root.join("ui")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm cli fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    if icon {
        std::fs::create_dir_all(root.join("ui").join("public")).expect("创建 public");
        std::fs::write(
            root.join("ui").join("public").join("icon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
        )
        .expect("写 icon.svg");
    }
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
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
        let child = Command::new(env!("CARGO_BIN_EXE_polypack"))
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("polypack farm worker 可启动");
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

/// 农场事件里的一条告警文本（按 stage 过滤）。
fn farm_messages(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["event"] == "warning" && event["stage"] == "farm")
        .filter_map(|event| event["message"].as_str().map(str::to_string))
        .collect()
}

/// 产物清单（`[{name, bytes, digest}]`）。
fn artifacts(report: &serde_json::Value) -> Vec<(String, String)> {
    report["artifacts"]
        .as_array()
        .expect("产物清单为数组")
        .iter()
        .map(|artifact| {
            (
                artifact["name"].as_str().expect("产物名").to_string(),
                artifact["digest"].as_str().expect("产物摘要").to_string(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------- 回环

/// 回环：worker 注册 → 主端投递 → 同一 DAG 执行 → 产物回传并落盘；worker 侧事件流可读。
#[test]
fn farm_loopback_builds_and_returns_artifacts() {
    let temp = TempDir::new("loopback");
    let project = temp.path().join("app");
    write_project(&project, true);
    let farm = temp.path().join("farm");

    // worker 不带 `--target`：缺省声明「本机宿主可达的全部已声明目标」（web 三宿主皆可达）
    let worker = Worker::spawn(
        &[
            "farm",
            "worker",
            "--dir",
            farm.to_str().expect("路径为 UTF-8"),
            "--id",
            "cli-loopback",
            "--jobs",
            "1",
            "--idle",
            "30",
            "--json",
        ],
        &project,
    );
    let out = polypack(
        &[
            "farm",
            "submit",
            "--config",
            "polypack.toml",
            "--dir",
            farm.to_str().expect("路径为 UTF-8"),
            "--target",
            "web",
            "--wait",
            "60",
            "--json",
        ],
        &project,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "投递应成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 1, "结果报告为单行 JSON");
    assert_eq!(lines[0]["event"], "farm-result");
    let names: Vec<String> = artifacts(&lines[0])
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        names.contains(&"dist/index.html".to_string()),
        "真实产物应回传: {names:?}"
    );
    assert!(
        names.contains(&"dist/manifest.webmanifest".to_string()),
        "PWA manifest 应回传: {names:?}"
    );
    assert!(
        project.join("dist").join("index.html").is_file(),
        "产物确实落盘"
    );

    let (code, events) = worker.finish();
    assert_eq!(code, 0, "worker 正常退出");
    let messages = farm_messages(&events);
    assert!(
        messages.iter().any(|message| message.contains("已注册")),
        "注册事件: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| message.contains("缓存命中")),
        "完成事件带缓存统计: {messages:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["event"] == "cached" && event["hit"] == false),
        "首个构建应为缓存未命中: {events:?}"
    );
}

/// 失败回传：worker 端错误码 + 修复提示经 `polypack::E_FARM_0003` 上抛，退出码 1，不吞错。
#[test]
fn farm_loopback_returns_worker_failure_with_code_and_hint() {
    let temp = TempDir::new("failure");
    let project = temp.path().join("app");
    write_project(&project, false); // 缺图标资产 → bundle 阶段 `polypack::E_WEB_0002`
    let farm = temp.path().join("farm");

    let worker = Worker::spawn(
        &[
            "farm",
            "worker",
            "--dir",
            farm.to_str().expect("路径为 UTF-8"),
            "--target",
            "web",
            "--jobs",
            "1",
            "--idle",
            "30",
            "--json",
        ],
        &project,
    );
    let out = polypack(
        &[
            "farm",
            "submit",
            "--config",
            "polypack.toml",
            "--dir",
            farm.to_str().expect("路径为 UTF-8"),
            "--target",
            "web",
            "--wait",
            "60",
        ],
        &project,
    );
    assert_eq!(out.status.code(), Some(1), "worker 失败必须让主端失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("polypack::E_FARM_0003"),
        "主端错误码（回传包装）: {stderr}"
    );
    assert!(
        stderr.contains("polypack::E_WEB_0002"),
        "远端错误码须内嵌: {stderr}"
    );
    assert!(stderr.contains("修复提示"), "修复提示须一路保留: {stderr}");

    let (code, events) = worker.finish();
    assert_eq!(code, 0, "任务失败不杀 worker（生命周期正常退出）");
    let failed = events
        .iter()
        .find(|event| event["event"] == "failed")
        .expect("失败事件");
    assert_eq!(failed["code"], "polypack::E_WEB_0002");
    assert!(failed["hint"].is_string(), "失败事件带修复提示: {failed}");
}

/// R3 宿主阻断：农场投递**不**按发起地过滤目标——`--target ios` / `--target tvos` /
/// `--target watchos`（Linux 发起，声明宿主 = macOS）应保留进任务并等 macOS worker 注册
/// （此处无 worker → 等待注册超时），绝不报「目标不可达」。
/// 宿主准入由 master 对 worker 的能力判据把关（语义见 core 侧 `farm_remote.rs` 的 mock 契约）。
#[test]
fn farm_submit_keeps_foreign_host_target_for_worker_admission() {
    let temp = TempDir::new("foreign-host");
    let project = temp.path().join("app");
    write_project(&project, true);
    let farm = temp.path().join("farm");

    // ios（T-M2-13 既有）、tvos（T-M4-07：同一投递语义，声明宿主同为 macOS）与
    // watchos（T-M7-06：xcodebuild 链同走 R3 远程农场 macOS worker）
    for target in ["ios", "tvos", "watchos"] {
        let out = polypack(
            &[
                "farm",
                "submit",
                "--config",
                "polypack.toml",
                "--dir",
                farm.to_str().expect("路径为 UTF-8"),
                "--target",
                target,
                "--wait",
                "0",
            ],
            &project,
        );
        assert_eq!(
            out.status.code(),
            Some(1),
            "{target}：无 worker 时等待注册超时必须失败"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("polypack::E_FARM_0002"),
            "{target} 应停在「等待 worker 注册」: {stderr}"
        );
        assert!(
            !stderr.contains("E_TGT_0002"),
            "{target}：投递侧不得按发起地判「目标不可达」: {stderr}"
        );
    }
}

/// 跨机缓存：两个构建地点共用同一对象存储后端 → 第二处全命中（产物摘要一致）。
#[test]
fn farm_shared_cache_reuses_across_build_locations() {
    let temp = TempDir::new("shared-cache");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a, true);
    write_project(&site_b, true);
    let farm = temp.path().join("farm");
    let store = temp.path().join("shared-store");

    let mut digests = Vec::new();
    let mut messages = Vec::new();
    for (site, id) in [(&site_a, "worker-a"), (&site_b, "worker-b")] {
        let worker = Worker::spawn(
            &[
                "farm",
                "worker",
                "--dir",
                farm.to_str().expect("路径为 UTF-8"),
                "--id",
                id,
                "--target",
                "web",
                "--jobs",
                "1",
                "--idle",
                "30",
                "--cache",
                store.to_str().expect("路径为 UTF-8"),
                "--json",
            ],
            site,
        );
        let out = polypack(
            &[
                "farm",
                "submit",
                "--config",
                "polypack.toml",
                "--dir",
                farm.to_str().expect("路径为 UTF-8"),
                "--target",
                "web",
                "--wait",
                "60",
                "--cache",
                store.to_str().expect("路径为 UTF-8"),
                "--json",
            ],
            site,
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "投递应成功: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        digests.push(artifacts(&json_lines(&out.stdout)[0]));
        let (code, events) = worker.finish();
        assert_eq!(code, 0, "worker 正常退出");
        messages.extend(farm_messages(&events));
    }

    assert!(
        messages
            .iter()
            .any(|message| message.contains("缓存命中 0/1")),
        "首个构建地点未命中并写入共享后端: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("缓存命中 1/1")),
        "第二个构建地点跨机全命中: {messages:?}"
    );
    assert_eq!(
        digests[0], digests[1],
        "同一输入 → 同一产物摘要（跨机对账）"
    );
    assert!(
        site_b.join("dist").join("index.html").is_file(),
        "命中即还原产物到新地点"
    );
}
