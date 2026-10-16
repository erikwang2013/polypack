//! T-M3-08 验收（CLI 侧）：体积预算（`[budget].size`）门禁——骑在 `package` 节点的**逐产物**
//! 判定上（判定面 = `Artifact.bytes`，与目标 / 格式无关，故四端代码包共用一条判据）。
//!
//! 断言面：
//! - 未配 `[budget]` → 门禁关闭（缺省不拦、也不发判定事件）；
//! - 超预算 + `warn`（缺省策略）→ 构建仍绿；事件流发 `budgeted`（带 `poly::E_BUDGET_0001`），
//!   终端报告同屏可定位；
//! - 超预算 + `fail` → `package` 节点失败（DAG 语义：sign / verify 不再执行）、退出码 1，
//!   stderr 给错误码、目标、格式与差值；
//! - 在预算内 → 事件如实报「在预算内」且**不带**错误码（预算是报告，不是只有越界才说话）。
//!
//! 宿主腿选快应用：`plan` 零外部命令（T-M3-06），且 `runtime = wasm` 目标在 `compile` /
//! `bundle` 支线上都是 Skip——整条 DAG 在无 Taro / 无交叉工具链的三宿主上都能跑到出货口。
//! 真实体积数据（真 wasm-opt 裁剪前后）在 `poly-core/tests/wasm_real.rs`，本文件只钉门禁语义。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-budget-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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

/// 按 JSON Lines 解析 stdout。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

/// `poly init .` 模板工程。
fn init_project(tag: &str) -> TempDir {
    let temp = TempDir::new(tag);
    let out = poly(&["init", "."], temp.path());
    assert!(
        out.status.success(),
        "poly init 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    temp
}

/// 覆写 `[budget]` 段（追加在文件尾 = 新表；模板其余内容不动，幂等重写）。
fn write_budget(root: &Path, size: &str, policy: Option<&str>) {
    let path = root.join("polypack.toml");
    let base = std::fs::read_to_string(&path).expect("配置可读");
    let mut text = base
        .split("\n[budget]")
        .next()
        .expect("split 至少一节")
        .to_string();
    text.push_str(&format!("\n[budget]\nsize = {size}\n"));
    if let Some(policy) = policy {
        text.push_str(&format!("policy = \"{policy}\"\n"));
    }
    std::fs::write(&path, text).expect("配置可写");
}

/// `poly build --target quickapp`。
fn build(root: &Path, json: bool) -> Output {
    let mut args = vec!["build", "--target", "quickapp"];
    if json {
        args.push("--json");
    }
    poly(&args, root)
}

/// 事件流里的 `budgeted` 事件（必须恰一条：逐产物判定）。
fn budgeted(events: &[serde_json::Value]) -> &serde_json::Value {
    let found: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event["event"] == "budgeted")
        .collect();
    assert_eq!(found.len(), 1, "应有恰一条预算判定事件：{events:?}");
    found[0]
}

/// 事件流里的某事件（无则失败）。
fn event<'a>(events: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    events
        .iter()
        .find(|event| event["event"] == name)
        .unwrap_or_else(|| panic!("应有 {name} 事件：{events:?}"))
}

#[test]
fn budget_gate_reports_and_gates_end_to_end() {
    let temp = init_project("gate");
    let root = temp.path();

    // ① 未配 `[budget]`：门禁关闭——构建照常出货，不发判定事件
    let out = build(root, true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "无预算配置的构建必须为绿：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    event(&events, "packaged");
    assert!(
        !events.iter().any(|event| event["event"] == "budgeted"),
        "未配 [budget] 不得发判定事件：{events:?}"
    );

    // ② 人为超预算（size = 1）+ warn（缺省策略）：构建仍绿，两处报告都可定位
    write_budget(root, "1", None);
    let out = build(root, true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "warn 策略不得拦构建：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let packaged_bytes = event(&events, "packaged")["bytes"]
        .as_u64()
        .expect("产物体积");
    assert!(packaged_bytes > 1, "合成超预算需产物大于 1 字节");
    let verdict = budgeted(&events);
    assert_eq!(verdict["target"], "quickapp");
    assert_eq!(verdict["format"], "quickapp");
    assert_eq!(verdict["limit"], 1);
    assert_eq!(verdict["policy"], "warn");
    assert_eq!(
        verdict["bytes"], packaged_bytes,
        "判定面 = Packaged 事件的同一份字节（无二次统计）"
    );
    assert_eq!(
        verdict["code"], "poly::E_BUDGET_0001",
        "事件流里的越界必须带可定位错误码"
    );

    // 终端报告（人读 sink）：同一次判定的人读形态，错误码同屏给出
    let human = build(root, false);
    assert_eq!(human.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("预算：") && stdout.contains("poly::E_BUDGET_0001"),
        "人读输出应含预算告警与错误码：{stdout}"
    );

    // ③ 超预算 + fail：package 节点失败 → 退出 1，DAG 的后续层（sign / verify）不再执行
    write_budget(root, "1", Some("fail"));
    let out = build(root, true);
    assert_eq!(
        out.status.code(),
        Some(1),
        "fail 策略必须非零退出；stdout：{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    for needle in ["poly::E_BUDGET_0001", "quickapp", "超体积预算", "字节"] {
        assert!(stderr.contains(needle), "stderr 缺 {needle:?}：{stderr}");
    }
    let events = json_lines(&out);
    // 事件流照常给出越界判定（含错误码），随后是 package 节点的 Failed
    assert_eq!(budgeted(&events)["code"], "poly::E_BUDGET_0001");
    let failed = event(&events, "failed");
    assert_eq!(failed["stage"], "package");
    assert_eq!(failed["code"], "poly::E_BUDGET_0001");
    assert!(
        !events
            .iter()
            .any(|event| event["event"] == "signed" || event["event"] == "verified"),
        "package 失败后 sign / verify 不得执行：{events:?}"
    );

    // ④ fail 策略下在预算内：照常出货，判定事件不带错误码
    write_budget(root, "100000000000", Some("fail"));
    let out = build(root, true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "在预算内不得触发 fail：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let verdict = budgeted(&events);
    assert_eq!(verdict["limit"], 100_000_000_000u64);
    assert!(
        verdict["bytes"].as_u64().expect("体积") < 100_000_000_000,
        "判定须与上限可比：{verdict}"
    );
    assert!(
        verdict.get("code").is_none(),
        "在预算内不得给错误码（未越界不是错误）：{verdict}"
    );
    let human = build(root, false);
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("在预算内"),
        "人读输出应如实报「在预算内」：{stdout}"
    );
}

// ---------------------------------------------------------------- real 模式（POLY_CONTRACT_REAL=1）

/// 真跑开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 真链路夹具：poly-core 的真 uniffi 核心库（`greet` / `add`，UDL 是唯一接口来源）。
fn core_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../poly-core/tests/fixtures/bridge_core")
}

/// 递归复制目录（夹具只有几个小文件）。
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("建目标目录");
    for entry in std::fs::read_dir(from).expect("读夹具目录").flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().expect("文件类型").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("复制夹具文件");
        }
    }
}

/// wasm 真链路前置：真 cargo `wasm32-unknown-unknown` + 真 wasm-bindgen CLI + 真 wasm-opt
/// （裁剪腿）；缺一即打印 NOTE 后跳过（不红，同 M1/M2/M3 契约测试纪律）。
fn wasm_chain_ready() -> bool {
    use poly_core::bridge::wasm;
    if wasm::WasmBindgen::discover().is_none() {
        eprintln!(
            "NOTE：未找到 wasm-bindgen CLI（$POLYPACK_WASM_BINDGEN / PATH / \
             ~/.cache/polypack/tools/wasm-bindgen）——真链路跳过"
        );
        return false;
    }
    if poly_core::bridge::wasm_opt::WasmOpt::discover().is_none() {
        eprintln!(
            "NOTE：未找到 wasm-opt（$POLYPACK_WASM_OPT / PATH / ~/.cache/polypack/tools/wasm-opt）\
             ——裁剪腿无从真跑，跳过"
        );
        return false;
    }
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|line| line.trim() == wasm::WASM_TRIPLE)
        });
    if !installed {
        eprintln!(
            "NOTE：未装 {}（rustup target add {}）——wasm 出口真链路跳过",
            wasm::WASM_TRIPLE,
            wasm::WASM_TRIPLE
        );
        return false;
    }
    true
}

/// real：真 wasm 出口（真 wasm-bindgen 生成 → 真 wasm-opt `-Oz` 裁剪）→ 快应用真打包。
///
/// 钉住 T-M3-08 的接口契约：**裁剪时机 = 出口目录生成后、随包拷贝前**，故随包条目与优化后
/// 字节逐字节一致；wasm-opt 前后对比同时落在事件流（裁剪报告）与产物元数据（清单 `size` 块）。
#[test]
fn real_wasm_opt_trim_and_packaged_bytes_match() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真链路设 POLY_CONTRACT_REAL=1 运行");
        return;
    }
    if !wasm_chain_ready() {
        return;
    }
    let temp = TempDir::new("real-trim");
    let root = temp.path();
    copy_tree(&core_fixture(), &root.join("core"));
    std::fs::write(
        root.join("polypack.toml"),
        "[package]\nname = \"wasm-accept\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\n\
         entry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");

    // ① 真 wasm 出口：体积报告进事件流（裁剪前后对比）与产物元数据（清单 size 块）
    let out = poly(&["bridge", "wasm", "--json"], root);
    assert!(
        out.status.success(),
        "poly bridge wasm 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let message = events
        .iter()
        .filter(|event| event["event"] == "warning" && event["stage"] == "bridge")
        .filter_map(|event| event["message"].as_str())
        .find(|message| message.contains("wasm-opt 裁剪"))
        .unwrap_or_else(|| panic!("事件流应发 wasm-opt 裁剪报告：{events:?}"))
        .to_string();
    eprintln!("证据：{message}");

    let exit = root.join("bridge/wasm-bindgen");
    let manifest_text = std::fs::read_to_string(exit.join("manifest.json")).expect("清单可读");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("清单是合法 JSON");
    let size = &manifest["size"];
    assert_eq!(size["wasm_opt"]["applied"], true, "真裁剪：{size}");
    assert_eq!(size["wasm_opt"]["args"], "-Oz");
    let before = size["wasm_opt"]["before"].as_u64().expect("before");
    let after = size["wasm_opt"]["after"].as_u64().expect("after");
    assert!(before > after, "真裁剪必须体积下降：{before} → {after}");
    assert_eq!(size["wasm_bytes"].as_u64(), Some(after));
    assert!(
        message.contains(&before.to_string()) && message.contains(&after.to_string()),
        "事件流与元数据必须是同一份前后对比：{message}"
    );
    let trimmed = std::fs::read(exit.join("pax_core_wasm_bg.wasm")).expect("读裁剪后的 wasm");
    assert_eq!(trimmed.len() as u64, after, "落盘字节 = 元数据 after");

    // ② 快应用真打包：随包条目 = 出口目录（「在场即拷」整目录），字节逐一对上
    let out = poly(&["build", "--target", "quickapp", "--json"], root);
    assert!(
        out.status.success(),
        "poly build --target quickapp 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let packed = root.join(".polypack/packages/quickapp/miniapp/src/bridge/wasm-bindgen");
    let packaged_wasm = packed.join("pax_core_wasm_bg.wasm");
    assert!(
        packaged_wasm.is_file(),
        "随包 wasm 应在 {}（暂存目录：{:?}）",
        packaged_wasm.display(),
        std::fs::read_dir(root.join(".polypack/packages/quickapp")).map(|entries| entries
            .flatten()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>())
    );
    assert_eq!(
        std::fs::read(&packaged_wasm).expect("读随包 wasm"),
        trimmed,
        "随包字节必须与 wasm-opt 优化后字节逐字节一致"
    );
    assert_eq!(
        std::fs::read(packed.join("manifest.json")).expect("读随包清单"),
        manifest_text.as_bytes(),
        "体积元数据（清单）随代码包分发"
    );
    eprintln!(
        "证据：随包 wasm {} 字节 == 出口裁剪后字节（before {before} → after {after}）；工程 {}",
        after,
        root.display()
    );
}
