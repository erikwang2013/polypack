//! T-M2-13 验收（core 侧）·远程面：跨机缓存复用与 iOS mock 契约（`farm.rs` 的拆分）。
//!
//! 与 `farm.rs` 同一套夹具（web 目标、毫秒级、不依赖 node / 打包器 / 网络）；这里只放
//! 「构建地不在本机」的两个用例：共享对象存储后端与 macOS mock worker 的协议契约。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use poly_cache::Digest;
use poly_core::farm::{
    ArtifactRecord, CacheBackend, DirTransport, FarmFrame, FarmMaster, FarmTransport, JobOutcome,
    PROTOCOL, Requirement, WorkerCaps, execute_job,
};
use poly_core::pipeline::InjectedStages;
use poly_core::{BuildEvent, EventSink, JobSpec, PolypackConfig, Profile};
use poly_targets::{Host, Registry, TargetRequest, current_host};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-farm-remote-{tag}-{}-{}",
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

/// 夹具配置：web 目标（`react-dom` 适配器 → 静态打包），无 native 桌面目标。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

/// 写夹具工程（`ui/index.html` + `ui/public/icon.svg`：静态打包后产物含入口与 PWA 图标）。
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    std::fs::write(
        root.join("ui").join("public").join("icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    )
    .expect("写 icon.svg");
}

/// 由夹具配置构造 `JobSpec`（`hosts` = 「可发起构建的宿主」）。
fn job_spec(hosts: &[Host], targets: &[&str]) -> JobSpec {
    let config = PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法");
    let request = TargetRequest::Explicit(targets.iter().map(|id| id.to_string()).collect());
    config
        .to_job(&Registry::builtin(), &request, Profile::Ci, hosts)
        .expect("夹具目标可解析")
}

/// worker 侧接收端：只记录事件文本，不输出（保持测试 stdout 干净）。
#[derive(Default)]
struct Recorder {
    events: Vec<String>,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(format!("{event:?}"));
    }
}

/// 本机宿主（测试运行地必被识别）。
fn host() -> Host {
    current_host().expect("测试宿主必被识别")
}

// ---------------------------------------------------------------- 跨机缓存

#[test]
fn shared_object_store_reuses_cache_across_build_locations() {
    // 「跨机」= 两个构建地点（内容相同的两个工程目录）共用一个对象存储后端：
    // 第二个地点不得重跑 bundle，而是从共享后端还原产物（缓存键公式不含绝对路径）。
    let temp = TempDir::new("cross-cache");
    let store = temp.path().join("shared-store");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a);
    write_project(&site_b);
    let spec = job_spec(&[host()], &["web"]);
    let mut events = Recorder::default();

    let first = execute_job(
        &site_a,
        &spec,
        &CacheBackend::Shared(store.clone()),
        InjectedStages::default(),
        &mut events,
    )
    .expect("站点 A 构建");
    assert_eq!(
        first.cache.hits, 0,
        "首个构建地点不该命中: {:?}",
        first.cache
    );
    assert!(
        first.cache.misses > 0,
        "首个构建地点应写入共享后端: {:?}",
        first.cache
    );

    let second = execute_job(
        &site_b,
        &spec,
        &CacheBackend::Shared(store),
        InjectedStages::default(),
        &mut events,
    )
    .expect("站点 B 构建");
    assert!(
        second.cache.hits > 0,
        "第二个构建地点应跨机命中: {:?}",
        second.cache
    );
    assert_eq!(
        second.cache.misses, 0,
        "全部阶段都应命中: {:?}",
        second.cache
    );
    assert!(
        site_b.join("dist").join("index.html").is_file(),
        "命中即还原产物到新地点"
    );
    assert_eq!(
        first.artifacts, second.artifacts,
        "同一输入 → 同一产物摘要（跨机对账）"
    );
}

// ---------------------------------------------------------------- iOS mock 契约

/// **mock 契约（显式标注：非真实 IPA）**。
///
/// iOS 真实链路被宿主阻断：`ios.hosts = [Macos]`，Linux 上既不能编译也不能打包，
/// 真实由 macOS 上的 ios 打包器（T-M2-06/08）承担。本用例验证的是协议面：
/// Linux 主端发起 iOS 目标 → macOS worker 注册（能力声明 triple/host/targets）→
/// 任务帧携带 `JobSpec`（含 ios 目标与 `CredentialRef` 引用、无内联明文）→ 产物回传。
/// mock worker 写出的 `MockApp.ipa` 明确标注为 mock，绝不冒充真实产物。
#[test]
fn ios_target_routes_to_macos_worker_via_mock_contract() {
    let temp = TempDir::new("ios-mock");
    let farm = temp.path().join("farm");
    let spec = job_spec(&[Host::Macos], &["ios"]);
    assert_eq!(spec.targets[0].id, "ios");
    assert_eq!(spec.targets[0].package.format, "ipa");
    assert_eq!(spec.secrets.len(), 1, "签名凭据只以引用形式随任务走");
    assert_eq!(
        spec.secrets[0].name, "apple-codesign",
        "凭据 = 引用名（凭据在构建地拉取）"
    );
    assert_eq!(spec.secrets[0].source, poly_core::CredentialSource::Keyring);

    const MOCK_IPA: &[u8] = b"MOCK CONTRACT - NOT A REAL IPA\n";
    let worker_farm = farm.clone();
    let worker_project = temp.path().join("ios-app");
    std::fs::create_dir_all(&worker_project).expect("创建构建地目录");
    let mock = std::thread::spawn(move || -> Vec<String> {
        let mut transport = DirTransport::worker(&worker_farm);
        transport
            .send(&FarmFrame::Hello {
                protocol: PROTOCOL.to_string(),
                worker: WorkerCaps {
                    worker_id: "macos-mock-worker".to_string(),
                    triple: "aarch64-apple-darwin".to_string(),
                    host: Host::Macos,
                    targets: vec!["ios".to_string()],
                },
            })
            .expect("注册帧");
        let Some(FarmFrame::HelloAck { accepted: true, .. }) =
            transport.recv(Duration::from_secs(10)).expect("收注册结果")
        else {
            panic!("macOS worker 的注册应被接受");
        };
        let Some(FarmFrame::Job {
            job_id,
            project_dir,
            spec,
        }) = transport.recv(Duration::from_secs(10)).expect("收任务帧")
        else {
            panic!("应收到 job 帧");
        };
        // mock 产物：显式标注，文件名与内容都不是真实 IPA
        let artifact = Path::new(&project_dir).join("MockApp.ipa");
        std::fs::write(&artifact, MOCK_IPA).expect("写 mock 产物");
        transport
            .send(&FarmFrame::Result {
                job_id: job_id.clone(),
                ok: true,
                error: None,
                artifacts: vec![ArtifactRecord {
                    name: "MockApp.ipa（mock，非真实 IPA）".to_string(),
                    digest: Digest::of_bytes(MOCK_IPA),
                    bytes: MOCK_IPA.len() as u64,
                }],
            })
            .expect("回传结果");
        let _ = transport.recv(Duration::from_secs(10));
        vec![
            format!("job={job_id}"),
            format!("target={}", spec.targets[0].id),
            format!("secrets={}", spec.secrets.len()),
        ]
    });

    let mut master = FarmMaster::new(&farm);
    let require = Requirement {
        hosts: vec![Host::Macos],
        targets: vec!["ios".to_string()],
    };
    let caps = master
        .accept_worker(&require, Duration::from_secs(10))
        .expect("macOS worker 应被接受");
    assert_eq!(caps.host, Host::Macos);
    assert_eq!(caps.triple, "aarch64-apple-darwin");
    let outcome = master
        .submit("ios-job", &worker_project, &spec, Duration::from_secs(30))
        .expect("结果回传");
    let _ = master.farewell();
    let seen = mock.join().expect("mock worker 线程");

    assert_eq!(
        seen,
        vec!["job=ios-job", "target=ios", "secrets=1"],
        "worker 侧所见帧内容"
    );
    let JobOutcome::Done { artifacts } = outcome else {
        panic!("mock 契约应成功: {outcome:?}")
    };
    assert_eq!(artifacts.len(), 1);
    assert!(
        artifacts[0].name.contains("mock"),
        "产物必须是显式 mock 标注: {}",
        artifacts[0].name
    );
    assert_eq!(artifacts[0].digest, Digest::of_bytes(MOCK_IPA));
    assert!(
        worker_project.join("MockApp.ipa").is_file(),
        "mock 产物落盘于构建地"
    );
}
