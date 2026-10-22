//! T-M1-10 验收：`polypack dev` 热预览 —— 静态托管 + 注入脚本 + 变更后重打包并递增版本号。
//!
//! 端口不解析自输出（人读文本），改为先借一个空闲端口（bind 0 → 读端口 → 释放）再传入；
//! HTTP 走 `TcpStream` 手写 HTTP/1.0 请求（dev 服只用请求行，响应 `Connection: close`）。
//! 首轮就绪含一次核心 release 编译，故轮询给到 60s（本宿主 ≈1s，冷启动慢机更久）；
//! 改动 → 重打包由 dev 侧 400ms 轮询驱动，检测给 30s。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- 测试支架

/// 临时工程目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("polypack-{tag}-{}", std::process::id()));
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

/// 借一个空闲端口：bind 0 读端口后立刻释放（dev 的端口无人读文本可解析）。
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("借端口可 bind");
    listener.local_addr().expect("本地地址可取").port()
}

/// dev 子进程守卫：断言失败（panic 展开）也保证 kill + reap，不留监听进程。
struct DevServer {
    child: Child,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
}

impl DevServer {
    fn spawn(root: &Path, port: u16) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_polypack"))
            .args(["dev", "--port", &port.to_string(), "--host", "127.0.0.1"])
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("polypack 可执行");
        let stdout = drain(child.stdout.take().expect("stdout 已接通管道"));
        let stderr = drain(child.stderr.take().expect("stderr 已接通管道"));
        Self {
            child,
            stdout,
            stderr,
        }
    }

    /// 子进程输出快照（失败诊断用）。
    fn transcript(&self) -> String {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            self.stdout.lock().expect("锁未毒化"),
            self.stderr.lock().expect("锁未毒化")
        )
    }

    /// 终止并回收（并发地等端口监听消失）。
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 后台抽干管道（防管道写满阻塞子进程），缓冲可随时读取。
fn drain(mut pipe: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let sink = Arc::new(Mutex::new(String::new()));
    let buffer = sink.clone();
    std::thread::spawn(move || {
        let mut raw = [0u8; 4096];
        while let Ok(read) = pipe.read(&mut raw) {
            if read == 0 {
                break;
            }
            if let Ok(mut text) = buffer.lock() {
                text.push_str(&String::from_utf8_lossy(&raw[..read]));
            }
        }
    });
    sink
}

/// 极简 HTTP/1.0 GET：读到最后（响应带 `Connection: close`）→（状态码, 正文）。
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, body.to_string()))
}

/// 等 dev 就绪（`/__polypack_version` 有响应）并返回首个版本号。
fn wait_ready(dev: &DevServer, port: u16) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some((200, body)) = http_get(port, "/__polypack_version")
            && let Ok(version) = body.trim().parse()
        {
            return version;
        }
        assert!(
            Instant::now() < deadline,
            "polypack dev 60s 内未就绪：\n{}",
            dev.transcript()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 等版本号严格超过 `base`（每次重建 +1；检测上限 30s）。
fn wait_version_above(dev: &DevServer, port: u16, base: u64) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some((200, body)) = http_get(port, "/__polypack_version")
            && let Ok(version) = body.trim().parse::<u64>()
            && version > base
        {
            return version;
        }
        assert!(
            Instant::now() < deadline,
            "UI 改动 30s 内未触发重建（版本号未超过 {base}）：\n{}",
            dev.transcript()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

// ---------------------------------------------------------------- 契约

/// 静态托管 + 注入脚本 + `/__polypack_status`；改 UI 源文件后版本号 +1 且首页返回改动内容。
#[test]
fn dev_serves_injected_index_and_reloads_on_ui_change() {
    let project = TempDir::new("dev");
    let init = Command::new(env!("CARGO_BIN_EXE_polypack"))
        .arg("init")
        .current_dir(project.path())
        .output()
        .expect("polypack 可执行");
    assert!(
        init.status.success(),
        "polypack init 失败：{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let port = free_port();
    let mut dev = DevServer::spawn(project.path(), port);
    let first = wait_ready(&dev, port);
    assert!(first >= 1, "dev 就绪时版本号从 1 起：{first}");

    // 首页：200 + 注入的 dev 脚本（轮询 /__polypack_status：变更即整页重载）
    let (code, index) = http_get(port, "/").expect("GET / 有响应");
    assert_eq!(code, 200, "GET / 状态码：{index}");
    assert!(
        index.contains("__polypack_status"),
        "GET / 的 HTML 应含注入的 dev 脚本：{index}"
    );

    // 状态端点：build / ui.engine / core.state 三键是状态条的数据契约
    let (code, status) = http_get(port, "/__polypack_status").expect("GET /__polypack_status 有响应");
    assert_eq!(code, 200);
    let status: serde_json::Value = serde_json::from_str(&status).expect("状态端点为 JSON");
    assert!(status["build"].is_u64(), "status.build：{status}");
    assert!(
        status["ui"]["engine"].is_string(),
        "status.ui.engine：{status}"
    );
    assert!(
        status["core"]["state"].is_string(),
        "status.core.state：{status}"
    );

    // 改 UI 源文件 → 重打包 → 版本 +1 → 首页含改动内容（整页重载闭环）
    let index_path = project.path().join("ui/index.html");
    let mut html = std::fs::read_to_string(&index_path).expect("读 ui/index.html");
    html.push_str("\n<!-- polypack-dev-reload-marker -->\n");
    std::fs::write(&index_path, html).expect("写 ui/index.html");

    let second = wait_version_above(&dev, port, first);
    assert!(second > first, "版本号应严格递增：{first} → {second}");
    // 注入脚本读的是状态载荷的 version（缺了它浏览器永整页不重载——回归点）
    let (code, status) = http_get(port, "/__polypack_status").expect("GET /__polypack_status 有响应");
    assert_eq!(code, 200);
    let status: serde_json::Value = serde_json::from_str(&status).expect("状态端点为 JSON");
    assert_eq!(
        status["version"],
        serde_json::json!(second),
        "status.version 应与 /__polypack_version 同步：{status}"
    );
    let (code, index) = http_get(port, "/").expect("GET / 有响应");
    assert_eq!(code, 200);
    assert!(
        index.contains("polypack-dev-reload-marker"),
        "重打包后的首页应含改动内容：{index}"
    );

    // 退出：进程回收 + 端口不再有人监听
    dev.stop();
    assert!(
        dev.child.try_wait().expect("可回收").is_some(),
        "dev 进程应已回收"
    );
    // 不用 bind 断言端口空闲：TIME_WAIT 会让无 SO_REUSEADDR 的 bind 报 EADDRINUSE
    // （宿主实测），改断言「连接被拒绝 = 无监听者」。
    let mut refused = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_err() {
            refused = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        refused,
        "dev 退出后仍有进程在监听 {port}：\n{}",
        dev.transcript()
    );
}
