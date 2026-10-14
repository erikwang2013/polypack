//! T-M1-01 / T-M1-13 验收（CLI 侧）：`poly build` 真实端到端与二跑缓存契约。
//!
//! 一次真实构建（compile 支线真编 release + bundle 支线真出 UI 产物）后重跑同一构建：
//! 首跑不得虚报命中，二跑命中率 ≥ 90%；`--json` 的 stdout 必须是纯 JSON Lines；核心
//! 产物按 `core/target/<host-triple>/release/<crate>` 落盘且可执行。注册表 ↔ 金样比对与
//! real 模式四端链路见 tests/contract_m1.rs，此处不重复。
//!
//! 本测试真编 crate：宿主上 hello-world 级 crate 冷启动数秒、慢机可达 ~1 分钟，故不做时长断言。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-{tag}-{}-{}",
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

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 按 JSON Lines 解析 stdout（逐行必须合法 JSON，否则 panic）。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("--json 的 stdout 混入非 JSON 行：{line}（{err}）"))
        })
        .collect()
}

/// 事件流里的 cached 事件计（命中数, 总数）——字段契约见 contract_m1.rs。
fn cached_counts(events: &[serde_json::Value]) -> (usize, usize) {
    let cached: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event["event"] == "cached")
        .collect();
    let hits = cached
        .iter()
        .filter(|event| event["hit"] == serde_json::json!(true))
        .count();
    (hits, cached.len())
}

/// 从 crate 清单取 `[package]` 段字段（`name` / `version`）——模板零依赖，无需 toml 解析。
fn package_field(manifest: &str, field: &str) -> String {
    manifest
        .lines()
        .skip_while(|line| line.trim() != "[package]")
        .find_map(|line| line.trim().strip_prefix(&format!("{field} = \"")))
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("crate 清单缺少 [package].{field}"))
}

/// 宿主 triple（与 `poly build` 同源：`rustc -vV` 的 host 行）。
fn host_triple() -> String {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("rustc 可执行（poly build 的 compile 支线已依赖它）");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc -vV 含 host 行")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------- 契约

/// 同一输入二跑：首跑 0 命中、二跑 ≥ 90% 命中；stdout 纯 JSON Lines；核心产物可执行自报版本。
#[test]
fn identical_rebuild_hits_cache_and_core_binary_runs() {
    let project = TempDir::new("build-cache");
    let init = poly(&["init"], project.path());
    assert!(
        init.status.success(),
        "poly init 失败：{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let first = poly(&["build", "--json"], project.path());
    assert!(
        first.status.success(),
        "poly build 失败（{}）：{}",
        first.status,
        String::from_utf8_lossy(&first.stderr)
    );
    let first_events = json_lines(&first);
    let (first_hits, first_total) = cached_counts(&first_events);
    assert_eq!(
        first_hits, 0,
        "空缓存首跑不得虚报命中（{first_hits} / {first_total}）"
    );
    assert!(
        first_total >= 2,
        "compile 与 bundle 两支线各发一条 cached 事件：{first_total}"
    );
    // 首跑必须在算键前补好锁文件：否则构建自己生成的 Cargo.lock 会改掉下一次的键（假失效）
    assert!(
        project.path().join("core/Cargo.lock").is_file(),
        "首跑后核心 crate 应有 Cargo.lock（缓存键的稳定输入）"
    );

    let second = poly(&["build", "--json"], project.path());
    assert!(
        second.status.success(),
        "二跑 poly build 失败（{}）：{}",
        second.status,
        String::from_utf8_lossy(&second.stderr)
    );
    let (second_hits, second_total) = cached_counts(&json_lines(&second));
    assert!(second_total > 0, "二跑事件流应含 cached 事件");
    let rate = second_hits as f64 / second_total as f64;
    assert!(
        rate >= 0.9,
        "二跑缓存命中率 {rate:.2} < 0.90（{second_hits} / {second_total}）；T-M1-13 验收"
    );

    // --json 的 stdout 已被逐行解析：再断言无人读总结行混入
    assert!(
        !String::from_utf8_lossy(&first.stdout).contains("构建完成"),
        "--json 的 stdout 不得混入人读总结行"
    );

    // 核心产物：路径 = <root>/core/target/<host-triple>/release/<crate 名>，执行即自报名 + 版本
    let manifest = std::fs::read_to_string(project.path().join("core/Cargo.toml"))
        .expect("读生成的 core/Cargo.toml");
    let name = package_field(&manifest, "name");
    let version = package_field(&manifest, "version");
    let artifact = project
        .path()
        .join("core/target")
        .join(host_triple())
        .join("release")
        .join(&name);
    assert!(artifact.is_file(), "核心产物缺失：{}", artifact.display());
    let run = Command::new(&artifact).output().expect("核心产物可执行");
    assert!(run.status.success(), "核心产物退出码 {}", run.status);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        format!("{name} {version}"),
        "核心产物应自报 crate 名与版本"
    );

    // 人读模式：构建总结行存在，且其后只剩结束表情行（T-M1-14 的收尾；--json 无此两行）
    let human = poly(&["build"], project.path());
    assert!(human.status.success(), "人读模式 poly build 应成功");
    let stdout = String::from_utf8_lossy(&human.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let summary = lines
        .iter()
        .position(|line| line.contains("构建完成"))
        .unwrap_or_else(|| panic!("人读模式应有构建总结行：{stdout}"));
    let tail = &lines[summary + 1..];
    assert_eq!(tail.len(), 1, "构建总结后应恰好一行结束表情：{stdout}");
    assert!(
        tail[0].starts_with('('),
        "结束表情行应以面容开头：{}",
        tail[0]
    );
}
