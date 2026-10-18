//! T-M8-08 验收（core 侧）：共享 CAS 跨 worker 复用、同输入同键、缺席降级。
//!
//! 夹具与 T-M2-13 的 `farm_remote.rs` 相同（web 系静态目标，毫秒级、不依赖 node / 网络）；
//! 「不同 worker」= 内容相同的两份工程副本（绝对路径不同）+ 同一共享缓存目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use poly_cache::{CacheInputs, Digest, cache_key};
use poly_core::farm::{CacheBackend, execute_job};
use poly_core::pipeline::{self, InjectedStages};
use poly_core::{BuildEvent, EventSink, JobSpec, PolypackConfig, Project, Profile, UiEngine};
use poly_targets::{Host, Registry, TargetRequest, current_host};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-farm-cache-{tag}-{}-{}",
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

/// 夹具配置：三个静态站点目标（均 react-dom、均声明 Linux 宿主）。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-cache-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

/// 写夹具工程（内容完全相同的两份副本用于「两个构建地」）。
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
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

/// 由夹具配置构造 `JobSpec`（`hosts` = 「可发起构建的宿主」）。
fn job_spec(hosts: &[Host], targets: &[&str]) -> JobSpec {
    let config = PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法");
    let request = TargetRequest::Explicit(targets.iter().map(|s| s.to_string()).collect());
    config
        .to_job(&Registry::builtin(), &request, Profile::Ci, hosts)
        .expect("夹具目标可解析")
}

/// 工程上下文（缓存键计算的输入面）。
fn project(root: &Path) -> Project {
    Project {
        root: root.to_path_buf(),
        config: PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法"),
    }
}

/// 本机宿主（测试运行地必被识别）。
fn host() -> Host {
    current_host().expect("测试宿主必被识别")
}

/// worker 侧接收端：只记录事件文本。
#[derive(Default)]
struct Recorder {
    events: Vec<String>,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(format!("{event:?}"));
    }
}

/// 同输入（字节相同、绝对路径不同）→ 同一缓存键：§5.2 公式与 UI 支线键都成立。
#[test]
fn same_input_same_cache_key_across_build_locations() {
    let temp = TempDir::new("keys");
    let (site_a, site_b) = (temp.path().join("site-a"), temp.path().join("site-b"));
    write_project(&site_a);
    write_project(&site_b);

    // §5.2 缓存键公式直接对齐
    let inputs = |root: &Path| CacheInputs {
        sources: Digest::of_tree(&root.join("ui"), &[]).expect("UI 源树摘要"),
        lockfile: Digest::of_bytes(b""),
        target_spec: "{\"id\":\"web\",\"ui\":[\"react-dom\"]}".to_string(),
        toolchain: "rustc-fixture|linux".to_string(),
        profile: "ci".to_string(),
    };
    assert_eq!(
        cache_key(&inputs(&site_a)),
        cache_key(&inputs(&site_b)),
        "同一输入在不同构建地必须产生同一缓存键（键公式不含绝对路径）"
    );

    // 流水线实际使用的 bundle 键（对项目副本调用）同样一致
    let key_a = pipeline::bundle_key(&project(&site_a), &UiEngine::Static, "ci")
        .expect("站点 A 的 bundle 键");
    let key_b = pipeline::bundle_key(&project(&site_b), &UiEngine::Static, "ci")
        .expect("站点 B 的 bundle 键");
    assert_eq!(key_a, key_b, "不同构建地的 bundle 缓存键必须一致");
}

/// 农场二跑：worker A 写共享 CAS，内容相同的 worker B 全部命中（≥ 90%，对齐 M1 口径）。
#[test]
fn second_farm_run_hits_shared_cache_at_least_90pct() {
    let temp = TempDir::new("second-run");
    let shared = temp.path().join("shared-cas");
    std::fs::create_dir_all(&shared).expect("共享目录视为已挂载");
    let (site_a, site_b) = (temp.path().join("worker-a"), temp.path().join("worker-b"));
    write_project(&site_a);
    write_project(&site_b);
    let spec = job_spec(&[host()], &["web", "chromeos-pwa", "vidaa-tv"]);
    let mut events = Recorder::default();

    // 一跑（worker A）：共享后端为空 → 全部未命中，构建后写入
    let first = execute_job(
        &site_a,
        &spec,
        &CacheBackend::Shared(shared.clone()),
        InjectedStages::default(),
        &mut events,
    )
    .expect("worker A 构建");
    assert_eq!(first.cache.hits, 0, "首跑不该命中: {:?}", first.cache);
    assert!(first.cache.misses > 0, "首跑应写入共享后端: {:?}", first.cache);

    // 二跑（worker B，不同工程副本）：农场一次 submit 的逐目标任务各执行一次，
    // 统计全部任务的缓存查询（对齐 M1 口径：命中数 / cached 事件总数）
    let backend = CacheBackend::Shared(shared);
    let (mut hits, mut misses) = (0u64, 0u64);
    for target in ["web", "chromeos-pwa", "vidaa-tv"] {
        let outcome = execute_job(
            &site_b,
            &job_spec(&[host()], &[target]),
            &backend,
            InjectedStages::default(),
            &mut events,
        )
        .unwrap_or_else(|err| panic!("worker B 构建 {target}: {err:?}"));
        assert_eq!(
            outcome.artifacts, first.artifacts,
            "同一输入 → 同一产物摘要（跨 worker 对账）"
        );
        hits += outcome.cache.hits;
        misses += outcome.cache.misses;
    }
    let total = hits + misses;
    assert!(total >= 3, "二跑应有逐任务缓存查询: {hits} / {total}");
    assert!(
        hits as f64 / total as f64 >= 0.9,
        "农场二跑命中率 {:.3} < 0.90（{hits} / {total}）",
        hits as f64 / total as f64
    );
    assert_eq!(misses, 0, "全部任务的查询都应命中: {hits} / {total}");
    assert!(
        site_b.join("dist").join("index.html").is_file(),
        "命中即还原产物到新构建地"
    );
}

/// 共享对象存储缺席（未挂载 / 路径被占用）→ 告警并降级为本地 CAS，构建不失败。
#[test]
fn missing_shared_cache_degrades_to_local_with_warning() {
    let temp = TempDir::new("degrade");
    let site = temp.path().join("worker");
    write_project(&site);
    // 用一个普通文件占住共享根路径：创建目录必失败（等价于「未挂载 / 不可用」）
    let blocked = temp.path().join("not-mounted");
    std::fs::write(&blocked, b"not a directory").expect("占位文件");
    let missing = blocked.join("shared");
    let spec = job_spec(&[host()], &["web"]);
    let mut events = Recorder::default();
    let backend = CacheBackend::Shared(missing.clone());

    // 一跑：共享目录缺席 → 不应报错，本地 CAS 照常写入
    let first = execute_job(
        &site,
        &spec,
        &backend,
        InjectedStages::default(),
        &mut events,
    )
    .expect("共享缓存缺席不得让构建失败");
    assert_eq!(first.cache.hits, 0, "首跑不该命中: {:?}", first.cache);
    assert!(
        events
            .events
            .iter()
            .any(|event| event.contains("降级为工程本地缓存")),
        "缺席应发出降级告警: {:?}",
        events.events
    );

    // 二跑：本地后备命中，农场照常复用
    let second = execute_job(
        &site,
        &spec,
        &backend,
        InjectedStages::default(),
        &mut events,
    )
    .expect("降级后二跑仍应成功");
    assert!(
        second.cache.hit_rate() >= 0.9,
        "降级后本地二跑命中率 {:.3} < 0.90（{} / {}）",
        second.cache.hit_rate(),
        second.cache.hits,
        second.cache.hits + second.cache.misses
    );
}
