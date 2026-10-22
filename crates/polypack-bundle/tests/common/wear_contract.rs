//! T-M7-11 穿戴契约测试骨架：Wear / Fitbit / Zepp 每目标一份 `contract_m7_*.rs`，
//! 四段断言共用本模块（新增穿戴目标直接复制一份 `contract_m7_wearos.rs` 改标记处即可）。
//!
//! 1. **注册表 ↔ 金样**：`target / tier / runtime / format / builder / verifiers /
//!    experimental / extension / artifactName` 逐字段一致——工具链缺席也跑。
//! 2. **打包**：夹具 → 内建打包器真跑；`prepare` 返回 `None` 表示工具链缺席，
//!    打印 SKIP-with-print 后以通过结束（不红、不伪造真跑）。
//! 3. **校验器**：`structure` 钩子内调用与 `polypack verify` 同源的 `check_*_structure` /
//!    `validate_*`；失败即红（有工具的宿主上是真跑面）。
//! 4. **产物结构 ↔ 金样**：`structure` 快照逐字段比对；`requiredEntries` 是产物条目的
//!    下限（glob，`*` 匹配任意串；额外条目不算违约）。
//!
//! 金样更新**必须显式**（CI 不设置该变量，漂移即红并打印 `- 金样 / + 实际`）：
//!
//! ```text
//! POLYPACK_UPDATE_GOLDEN=1 cargo test -p polypack-bundle --test contract_m7_<目标>
//! ```
//!
//! 写入内容会出现在 `git diff` 中，review 后再提交。金样 schema：
//!
//! ```json
//! {
//!   "target": "fitbit", "tier": 6, "runtime": "none",
//!   "format": "fba", "builder": "fitbit-build",
//!   "verifiers": ["fitbit-structure-check"], "experimental": false,
//!   "extension": "fba", "artifactName": "pax-notes-1.2.3.fba",
//!   "requiredEntries": ["manifest.json", "device-*.zip"],
//!   "structure": { "…": "structure 钩子返回的目标特有字段" }
//! }
//! ```
//!
//! 新增目标：先手写上面这份最小金样（`structure` 可为 `{}`），再跑一次显式更新补齐。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use polypack_bundle::{Artifact, BundleInputs};
use polypack_targets::{Registry, TargetSpec};
use serde_json::Value;

/// 金样更新开关（显式一次；不设置即只读比对）。
pub const UPDATE_ENV: &str = "POLYPACK_UPDATE_GOLDEN";

/// 每目标一份契约测试的差异点；骨架见模块头注释。
pub struct Contract {
    /// 目标 id（内建注册表键）。
    pub target: &'static str,
    /// `tests/fixtures/` 下的金样文件名。
    pub golden: &'static str,
    /// 准备夹具 / 输入；`None` = 工具链缺席 → SKIP-with-print。
    pub prepare: fn(&Path) -> Option<BundleInputs>,
    /// 校验器断言 + 目标特有结构快照（返回金样 `structure` 对象）。
    pub structure: fn(&BundleInputs, &Artifact, &TargetSpec) -> Value,
}

/// 跑一套四段契约断言。
pub fn run(contract: Contract) {
    let spec = Registry::builtin()
        .get(contract.target)
        .unwrap_or_else(|| panic!("内建注册表缺少 {}", contract.target))
        .clone();
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(contract.golden);
    let golden = load_golden(&golden_path);

    // ① 注册表 ↔ 金样：静态声明，不依赖工具链，每次提交都跑。
    assert_header(&golden, &spec, contract.target);
    let required = value_str_list(&golden["requiredEntries"]);

    // ② 夹具 + 打包。
    let temp = Temp::new(contract.target);
    let root = temp.join("project");
    std::fs::create_dir_all(&root).expect("夹具根目录可建");
    let Some(inputs) = (contract.prepare)(&root) else {
        eprintln!(
            "SKIP：{} 工具链缺席——打包 / 校验器 / 金样比对未跑（工具就绪的宿主走真跑分支）",
            contract.target
        );
        return;
    };
    let packager = polypack_bundle::for_spec(&spec)
        .unwrap_or_else(|| panic!("{} 目标应有内建打包器", contract.target));
    assert_eq!(packager.id(), contract.target);
    assert_eq!(packager.format(), spec.package.format);
    let artifact = packager
        .package(&inputs, &spec)
        .unwrap_or_else(|err| panic!("{} 打包应真跑成功：{err}", contract.target));
    assert!(
        artifact.path.is_file(),
        "{} 产物应落盘：{}",
        contract.target,
        artifact.path.display()
    );
    assert_eq!(artifact.target, contract.target);
    assert_eq!(artifact.format, spec.package.format);
    let extension = golden["extension"]
        .as_str()
        .expect("金样 extension 应为字符串");
    assert_eq!(
        artifact.path.extension().and_then(|ext| ext.to_str()),
        Some(extension),
        "产物扩展名须与金样一致：{}",
        artifact.path.display()
    );
    let artifact_name = artifact
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("产物应有文件名");
    assert_eq!(
        artifact_name,
        golden["artifactName"].as_str().expect("金样 artifactName"),
        "产物文件名须与金样一致"
    );

    // ③ 产物结构 + 校验器（钩子内调用 check_* / validate_*）。
    let entries = polypack_bundle::zipwrite::entry_names(&artifact.path).expect("产物条目可读");
    assert_required_entries(&required, &entries);
    let structure = (contract.structure)(&inputs, &artifact, &spec);

    // ④ 金样比对（或显式更新）。
    let actual = serde_json::json!({
        "target": contract.target,
        "tier": spec.tier,
        "runtime": spec.runtime,
        "format": spec.package.format.clone(),
        "builder": spec.package.builder.clone(),
        "verifiers": spec.verify.clone(),
        "experimental": spec.experimental,
        "extension": extension,
        "artifactName": artifact_name,
        "structure": structure,
    });
    compare_or_update(&golden_path, &golden, &actual);
}

/// 静态声明逐字段对拍：漂移时打印 `- 金样 / + 注册表`。
fn assert_header(golden: &Value, spec: &TargetSpec, target: &str) {
    let actual = serde_json::json!({
        "target": target,
        "tier": spec.tier,
        "runtime": spec.runtime,
        "format": spec.package.format.clone(),
        "builder": spec.package.builder.clone(),
        "verifiers": spec.verify.clone(),
        "experimental": spec.experimental,
    });
    let expected = serde_json::json!({
        "target": golden["target"],
        "tier": golden["tier"],
        "runtime": golden["runtime"],
        "format": golden["format"],
        "builder": golden["builder"],
        "verifiers": golden["verifiers"],
        "experimental": golden["experimental"],
    });
    if expected == actual {
        return;
    }
    let mut diff = Vec::new();
    diff_json("$", &expected, &actual, &mut diff);
    panic!(
        "注册表声明与金样漂移（- 金样 / + 注册表）：\n{}\n\
         契约字段变更请同步修改 tests/fixtures 中的金样（更新门只用于产物结构）",
        diff.join("\n")
    );
}

/// 金样 `requiredEntries` 的 glob 下限：每个模式至少命中一个实际条目。
fn assert_required_entries(required: &[String], entries: &[String]) {
    let missing: Vec<&String> = required
        .iter()
        .filter(|pattern| !entries.iter().any(|entry| glob_match(pattern, entry)))
        .collect();
    assert!(
        missing.is_empty(),
        "产物缺少金样要求的条目（requiredEntries）：{missing:?}\n实际条目：{entries:?}"
    );
}

/// 金样 ↔ 实际快照：相等即通过；漂移且显式更新门开启 → 写盘（git diff 可见）；
/// 否则红并打印路径级 diff。
fn compare_or_update(path: &Path, golden: &Value, actual: &Value) {
    // `requiredEntries` 是比对模式元数据（产物条目下限），不进快照逐字段比对。
    let mut expected = golden.clone();
    expected
        .as_object_mut()
        .expect("金样应为 JSON 对象")
        .remove("requiredEntries");

    let mut diff = Vec::new();
    diff_json("$", &expected, actual, &mut diff);
    if diff.is_empty() {
        return;
    }
    if update_golden() {
        let mut next = actual.clone();
        next["requiredEntries"] = golden["requiredEntries"].clone();
        let text = serde_json::to_string_pretty(&next).expect("金样序列化");
        std::fs::write(path, format!("{text}\n")).expect("写金样");
        eprintln!(
            "UPDATE-GOLDEN：已按实测更新 {}（显式 {}；改动见 git diff）",
            path.display(),
            UPDATE_ENV
        );
        return;
    }
    panic!(
        "金样漂移（- 金样 / + 实际）：\n{}\n\
         金样：{}\n显式更新：{}={} cargo test -p polypack-bundle --test contract_m7_<目标>",
        diff.join("\n"),
        path.display(),
        UPDATE_ENV,
        "1"
    );
}

/// 显式更新门：`POLYPACK_UPDATE_GOLDEN=1`（或 `true`）。
fn update_golden() -> bool {
    matches!(std::env::var(UPDATE_ENV).as_deref(), Ok("1") | Ok("true"))
}

fn load_golden(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!(
            "读取金样 {} 失败：{err}\n新增目标先手写最小金样（schema 见 common/wear_contract.rs 头注释）",
            path.display()
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("金样 {} 非法 JSON：{err}", path.display()))
}

fn value_str_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("金样字段应为数组：{value}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("数组元素应为字符串：{item}"))
                .to_string()
        })
        .collect()
}

/// 路径级 diff：对象递归，数组 / 标量整体打印。
fn diff_json(prefix: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let mut keys: Vec<&String> = expected.keys().chain(actual.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = format!("{prefix}.{key}");
                match (expected.get(key), actual.get(key)) {
                    (Some(want), Some(got)) => diff_json(&child, want, got, out),
                    (Some(_), None) => out.push(format!("- {child}（金样有、实际无）")),
                    (None, Some(_)) => out.push(format!("+ {child}（金样未声明）")),
                    (None, None) => {}
                }
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!("- {prefix}: {expected}\n+ {prefix}: {actual}")),
    }
}

/// 最小 glob：`*` 匹配任意串（含空串）。契约金样只需要这一种通配。
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut resume = 0usize;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if let Some(at) = star {
            p = at + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|ch| *ch == '*')
}

/// 测试临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "polypack-wear-contract-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        Self(dir)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 夹具文件写入（父目录自动创建）。
pub fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("夹具父目录可建");
    }
    std::fs::write(path, contents).expect("夹具文件可写");
}
