//! T-M2-01 / T-M2-02 真跑（`POLY_CONTRACT_REAL=1`）：真生成 Kotlin + Swift 绑定 →
//! 真用 NDK 交叉编译 aarch64 / x86_64 的 `.so` → 真组装 AAR → 真用 kotlinc 编 `classes.jar`。
//!
//! 默认（mock 模式）/ 缺工具一律打印 SKIP 原因后返回，绝不红——与 M1/M2 既有契约测试同一
//! 纪律（`crates/poly-cli/tests/contract_m1.rs`、`crates/poly-bundle/tests/ios_real_run.rs`）。
//! 无工具面的结构断言（UDL / 摘要 / AAR 布局 / 缓存）在 `bridge.rs`，每宿主都真跑。
//!
//! 夹具是**带 uniffi 依赖的独立 workspace**（`tests/fixtures/bridge_core/`，见其 Cargo.toml
//! 里的 license 说明）；真跑把它复制到临时目录，产物绝不落进仓库树。
//!
//! shortcut: 断言「绑定签名 ↔ UDL」「绑定 extern 符号 ↔ NDK 编出的 .so 符号表」，
//! 不做设备上的 `System.loadLibrary` 实跑（无 JVM/模拟器）；上真机时补一条仪器化测试。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use poly_cache::Cache;
use poly_core::bridge::{self, BridgeLang, aar};
use poly_core::config::PolypackConfig;
use poly_core::events::HumanSink;
use poly_core::plan::Project;

/// 夹具核心库基础名（= `tests/fixtures/bridge_core` 的 `[lib] name`）。
const LIB: &str = "pax_core";
/// UDL namespace（= 绑定包名 `uniffi.<ns>`）。
const NS: &str = "pax";
/// UDL 里声明的两个函数：UDL ↔ 两端绑定 ↔ `.so` 符号表三处都必须出现。
const FUNCS: [&str; 2] = ["greet", "add"];

// ---------------------------------------------------------------- 支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程根；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-bridge-real-{tag}-{}-{}",
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
        // CI 留产物通道：`POLY_KEEP_TEMP=1` 时保留目录（bridge-real 的 upload-artifact 消费）。
        // 此前 RAII 先删、「留产物」步在其后运行——自落地起空转（2026-10-16 排查实证：日志里
        // 产物确在 /tmp/polypack-bridge-real-*，上传时两个成功 run 均 0 产物）。
        // 默认（本机）行为不变：照旧自清，产物绝不落进仓库树。
        if std::env::var("POLY_KEEP_TEMP").as_deref() == Ok("1") {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 真跑开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 前置不满足 → 打印原因并返回 false（SKIP 纪律：缺工具不红）。
fn gate(what: &str) -> bool {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLY_CONTRACT_REAL=1（{what}）");
        return false;
    }
    true
}

/// 把夹具复制成临时工程（真跑产物不落仓库树）。
fn staged_project(tag: &str) -> (TempDir, Project) {
    let temp = TempDir::new(tag);
    let core = temp.path().join("core");
    copy_tree(&fixture_dir(), &core);
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    let project = Project {
        root: temp.path().to_path_buf(),
        config,
    };
    (temp, project)
}

/// 仓库里的夹具目录。
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bridge_core")
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

/// 读文本产物。
fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("读 {} 失败: {err}", path.display()))
}

/// 在单个 `.so` 上跑 `readelf -h` 取 `Machine:` 行（工具缺失即 None）。
fn elf_machine(so: &Path) -> Option<String> {
    // LC_ALL=C：readelf 的表头随 locale 翻译（中文宿主上是「机器：」），不钉死就永远解析不到
    let out = Command::new("readelf")
        .env("LC_ALL", "C")
        .arg("-h")
        .arg(so)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|line| line.contains("Machine:"))
        .map(str::to_string)
}

/// `nm -D` 的动态符号（工具缺失或失败即 None）。
fn dynamic_symbols(so: &Path) -> Option<String> {
    let out = Command::new("nm").arg("-D").arg(so).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 归档条目名（升序）。
fn entry_names(aar: &Path) -> Vec<String> {
    let file = std::fs::File::open(aar).expect("打开 AAR");
    let mut zip = zip::ZipArchive::new(file).expect("AAR 是合法 zip");
    (0..zip.len())
        .map(|index| {
            zip.by_index(index)
                .expect("读条目")
                .name()
                .expect("条目名")
                .into_owned()
        })
        .collect()
}

// ---------------------------------------------------------------- 真跑

/// 真跑三段：单处 UDL → Kotlin + Swift 绑定 → kotlinc 编 `classes.jar` → NDK 交叉编译
/// arm64-v8a / x86_64 → 组装 AAR。
///
/// 合在一段里跑：每段都要先把夹具的 uniffi 依赖真编译一遍（分钟级），合成一段只付一次。
/// 段内的工具缺失按 SKIP 打印，不掩盖前段已得的断言（缺 kotlinc 不阻断 NDK 段，反之亦然）。
#[test]
fn real_generates_bindings_and_cross_compiles_and_assembles_aar() {
    if !gate("uniffi 生成 / kotlinc / NDK 交叉编译") {
        return;
    }
    let (_temp, project) = staged_project("real");
    let root = project.root.clone();
    let mut sink = HumanSink { out: Vec::new() };

    // —— 段一：一份 UDL → 两端绑定（引擎取夹具自带的 src/bin/uniffi-bindgen.rs） ——
    let outputs = bridge::generate(
        &project,
        &[BridgeLang::Kotlin, BridgeLang::Swift],
        &mut sink,
    )
    .expect("真生成两端绑定");
    let kotlin = root
        .join("bridge/kotlin")
        .join(format!("uniffi/{NS}/{NS}.kt"));
    assert!(kotlin.is_file(), "缺 Kotlin 绑定 {}", kotlin.display());
    // Swift 三件套以核心库名命名（write_config 把 module_name / ffi_module_name 钉死），
    // 落点与 T-M2-03 的 `bridge_dir(out_dir) = out_dir/bridge/swift` 一致
    for name in [
        format!("{LIB}.swift"),
        format!("{LIB}FFI.h"),
        format!("{LIB}FFI.modulemap"),
    ] {
        let path = root.join("bridge/swift").join(&name);
        assert!(path.is_file(), "缺 Swift 绑定 {}", path.display());
    }
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].lang, BridgeLang::Kotlin);
    assert_eq!(outputs[1].lang, BridgeLang::Swift);

    let kt = read_text(&kotlin);
    let swift = read_text(&root.join("bridge/swift").join(format!("{LIB}.swift")));
    assert!(
        kt.contains(&format!("package uniffi.{NS}")),
        "Kotlin 包名取 UDL namespace"
    );
    // 库名被钉死为 lib<pax_core>.so：不钉的话 uniffi 的默认名与 AAR 里的 .so 不一致，
    // 且只在设备上 `System.loadLibrary` 时才炸
    assert!(
        kt.contains(&format!("return \"{LIB}\"")),
        "Kotlin 的 findLibraryName 必须指向 {LIB}"
    );
    // UDL ↔ 两端绑定：接口定义的每个函数都要在两端绑定的公开签名里落地。
    // Kotlin 侧生成的是反引号转义形式（`fun `greet`(`），故先去反引号再断言
    let kt_plain = kt.replace('`', "");
    for func in FUNCS {
        assert!(
            kt_plain.contains(&format!("fun {func}(")),
            "Kotlin 绑定缺 {func}"
        );
        assert!(
            swift.contains(&format!("func {func}(")),
            "Swift 绑定缺 {func}"
        );
    }

    // —— 段二：Kotlin 绑定 → classes.jar（kotlinc + JNA 不在场则跳过该段） ——
    let kotlin_dir = root.join("bridge/kotlin");
    let classes_jar = match bridge::kotlinc::Kotlinc::discover() {
        Some(compiler) => {
            let kt_sources = collect_kt(&kotlin_dir);
            assert!(!kt_sources.is_empty(), "没有可编译的绑定源码");
            let jar = root
                .join("bridge/android")
                .join(format!("{LIB}.classes.jar"));
            compiler
                .compile(&kt_sources, &jar)
                .expect("kotlinc 真编译绑定");
            // classes.jar 必须真有绑定类（不是空壳）
            let names = entry_names(&jar);
            assert!(
                names
                    .iter()
                    .any(|name| name.starts_with(&format!("uniffi/{NS}/"))),
                "classes.jar 里应有 uniffi/{NS}/ 下的绑定类，实际：{names:?}"
            );
            eprintln!(
                "真跑：kotlinc 编译 {} 个源文件 → {}",
                kt_sources.len(),
                jar.display()
            );
            Some(jar)
        }
        None => {
            eprintln!(
                "SKIP：无 kotlinc（$POLYPACK_KOTLINC / PATH / ~/.cache/polypack/tools/kotlin + JNA）"
            );
            None
        }
    };

    // —— 段三：NDK 交叉编译两条 ABI → AAR ——
    let Some(ndk) = android_prerequisites() else {
        return;
    };
    let core_dir = root.join("core");
    let mut libs = Vec::new();
    for abi in [aar::ABIS[0], aar::ABIS[2]] {
        let so = aar::build_lib(&core_dir, abi, &ndk, aar::DEFAULT_MIN_SDK, &mut sink)
            .expect("NDK 真交叉编译");
        let bytes = std::fs::metadata(&so).expect("读 .so").len();
        assert!(bytes > 0, "{} 是空文件", so.display());
        // 架构断言：readelf 不在场时按缺工具跳过（Windows 宿主）
        match elf_machine(&so) {
            Some(machine) => match abi.triple {
                "aarch64-linux-android" => assert!(machine.contains("AArch64"), "{machine}"),
                "x86_64-linux-android" => assert!(machine.contains("X86-64"), "{machine}"),
                other => panic!("未覆盖的 triple: {other}"),
            },
            None => eprintln!("SKIP：无 readelf，跳过 {} 的架构断言", so.display()),
        }
        // 绑定 ↔ .so 的真契约：Kotlin 声明的 extern 符号必须在 .so 的动态符号表里
        if let Some(symbols) = dynamic_symbols(&so) {
            let symbol = format!("uniffi_{LIB}_fn_func_{}", FUNCS[0]);
            assert!(
                symbols.contains(&symbol),
                "{} 缺导出符号 {symbol}（绑定与核心库会版本错配）",
                so.display()
            );
            eprintln!("真跑：{} 导出 {symbol}", so.display());
        }
        libs.push((abi, so));
    }

    // 真 AAR：清单 + jni/<abi>/lib<pax_core>.so + classes.jar + 随附绑定源码
    let out = root.join("bridge/android").join(format!("{LIB}.aar"));
    let manifest = aar::manifest_xml(NS, aar::DEFAULT_MIN_SDK, "1.0");
    aar::assemble(&out, &manifest, &libs, classes_jar.as_deref(), &kotlin_dir).expect("真组装 AAR");
    let names = entry_names(&out);
    assert_eq!(names[0], "AndroidManifest.xml");
    for abi in ["arm64-v8a", "x86_64"] {
        assert!(
            names
                .iter()
                .any(|name| name == &format!("jni/{abi}/lib{LIB}.so")),
            "{abi} 的 .so 不在 AAR 里：{names:?}"
        );
    }
    assert_eq!(
        names.iter().any(|name| name == "classes.jar"),
        classes_jar.is_some(),
        "有 classes.jar 就得进 AAR，没有就不许伪造"
    );
    eprintln!(
        "真跑：AAR {} 字节 → {}",
        std::fs::metadata(&out).expect("AAR 元数据").len(),
        out.display()
    );

    // 二跑缓存：同输入 → 同键，命中即字节一致（`poly bridge aar` 的缓存路径）
    let interface = bridge::interface_digest(&bridge::interface_source(&project).expect("UDL"))
        .expect("接口摘要");
    let abis: Vec<aar::Abi> = libs.iter().map(|(abi, _)| *abi).collect();
    let key =
        aar::cache_key(&project, &abis, &interface, "ndk-cargo", "local").expect("AAR 缓存键");
    let cache = Cache::from_root(root.join(".polypack/cache"));
    let bytes = std::fs::read(&out).expect("读 AAR");
    cache.store(&key, &bytes).expect("AAR 入库");
    let restored = cache.lookup(&key).expect("查缓存").expect("缓存命中");
    assert_eq!(restored, bytes, "命中的字节必须与产物一致");
    // 换 ABI 集合 → 键变（缓存不会把两 ABI 的产物发成一条 ABI 的）
    let single =
        aar::cache_key(&project, &[aar::ABIS[0]], &interface, "ndk-cargo", "local").expect("键");
    assert_ne!(key, single);
}

/// NDK 段的前置：rustup 的 Android triple 与 NDK 根，缺任一即打印 SKIP 并返回 `None`。
fn android_prerequisites() -> Option<PathBuf> {
    for abi in [aar::ABIS[0], aar::ABIS[2]] {
        let installed = Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .is_ok_and(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|line| line.trim() == abi.triple)
            });
        if !installed {
            eprintln!(
                "SKIP：缺 rustup target {}（rustup target add {}）",
                abi.triple, abi.triple
            );
            return None;
        }
    }
    match aar::ndk_root() {
        Some(ndk) => Some(ndk),
        None => {
            eprintln!("SKIP：无 Android NDK（$ANDROID_NDK_HOME / $POLYPACK_NDK_HOME / <sdk>/ndk）");
            None
        }
    }
}

/// 递归收集 `.kt` 源文件（升序）。
fn collect_kt(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("读绑定目录").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "kt") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
