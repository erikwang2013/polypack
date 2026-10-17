//! T-M5-10 验收 real 链路（`POLY_CONTRACT_REAL=1`，Linux + cargo）：
//!
//! `poly init` → 注入应用自带 SW → **一次** `poly build --target chromeos --target chromeos-pwa`
//! （同一 JobSpec，compile / bundle 各只一次）→ deb 结构 + 安装 / 启动 / 卸载（Crostini 不可得
//! 时用 Docker `debian:bookworm-slim` 替代，SKIP-with-print）→ 拆包载荷与 site 逐文件 SRI 相等
//! （同源证据）→ `poly verify` 两轨各自通过 → 真 Chrome 三腿审计（`ui/smoke/pwa/audit.mjs`，
//! Chrome 缺席 SKIP-with-print）。
//!
//! 模板不生成 service worker（T-M5-10 决策：SW 属应用资产）；本链路在 `poly init` 后注入
//! `ui/public/sw.js` 与注册脚本，模拟「应用自带离线能力」的合法形态。`chromeos-pwa` notes 已
//! 如实写明该前提。

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{combined, command_available, init_project, poly, real_mode, write_ui};
use poly_targets::Host;

/// 在目录树中按扩展名找最新产物（契约产物落点 `.polypack/packages/<target>/`）。
fn find_artifact(root: &Path, extension: &str) -> Option<PathBuf> {
    let suffix = format!(".{extension}");
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().ends_with(&suffix))
            {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    files.pop()
}

/// deb 结构断言：control 字段 / 维护者脚本 / 数据段载荷 / AppStream。
fn assert_deb_structure(deb: &Path) {
    let info = Command::new("dpkg-deb")
        .arg("--info")
        .arg(deb)
        .output()
        .expect("跑 dpkg-deb --info");
    assert!(
        info.status.success(),
        "dpkg-deb --info 失败：{}",
        String::from_utf8_lossy(&info.stderr)
    );
    let info = String::from_utf8_lossy(&info.stdout).into_owned();
    for marker in [
        "Package: polypack-app",
        "Version: 0.1.0",
        "Architecture: amd64",
        "Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libsoup-3.0-0",
        "postinst",
        "postrm",
    ] {
        assert!(info.contains(marker), "deb control 缺 {marker}：\n{info}");
    }

    let contents = Command::new("dpkg-deb")
        .arg("--contents")
        .arg(deb)
        .output()
        .expect("跑 dpkg-deb --contents");
    assert!(
        contents.status.success(),
        "dpkg-deb --contents 失败：{}",
        String::from_utf8_lossy(&contents.stderr)
    );
    let contents = String::from_utf8_lossy(&contents.stdout).into_owned();
    for marker in [
        "./usr/bin/polypack-app",
        "./usr/lib/polypack-app/index.html",
        "./usr/lib/polypack-app/sw.js",
        "./usr/share/applications/polypack-app.desktop",
        "./usr/share/metainfo/",
        "./usr/share/icons/hicolor/128x128/apps/polypack-app.png",
    ] {
        assert!(
            contents.contains(marker),
            "deb 数据段缺 {marker}：\n{contents}"
        );
    }
}

/// 同一 JobSpec 证据：deb 载荷 `usr/lib/<product>` 与 site（去掉 `integrity.json`）逐文件
/// SRI（sha384 + 字节数）完全相等——两轨产物来自同一次编译 / UI 产物。
fn assert_same_jobspec(site: &Path, payload: &Path) {
    let site_manifest = poly_bundle::sri::manifest_of(site).expect("site SRI 清单可生成");
    let deb_manifest = poly_bundle::sri::manifest_of(payload).expect("deb 载荷 SRI 清单可生成");
    let mut site_files: BTreeMap<String, poly_bundle::sri::Entry> = site_manifest.files;
    site_files.remove("integrity.json");

    assert!(!site_files.is_empty(), "site 不应为空");
    for name in ["index.html", "manifest.webmanifest", "sw.js", "icon.svg"] {
        assert!(site_files.contains_key(name), "site 缺 {name}");
    }
    assert_eq!(
        site_files, deb_manifest.files,
        "deb 载荷与 site 不同源（同一 JobSpec 证据失败）：\n- site / + deb 差异需逐文件核对"
    );
    eprintln!(
        "同源证据：{} 个文件 SRI 相等（sha384，site 去 integrity.json）",
        site_files.len()
    );
}

/// Crostini 不可得时的替代面：Docker `debian:bookworm-slim` 真跑 dpkg 安装 / 启动 / 卸载。
/// Docker 或缺镜像 → SKIP-with-print（不隐式拉镜像，避免夜间网络抖动）。
fn crostini_surrogate(deb: &Path) {
    if !command_available("docker", &["--version"]) {
        eprintln!("SKIP Crostini 安装冒烟：本机无 Docker，Crostini 容器不可得（替代面未执行）");
        return;
    }
    let image = Command::new("docker")
        .args(["image", "inspect", "debian:bookworm-slim"])
        .output();
    if !image.is_ok_and(|out| out.status.success()) {
        eprintln!(
            "SKIP Crostini 安装冒烟：本机无 debian:bookworm-slim 镜像（docker pull 后重跑；替代面未执行）"
        );
        return;
    }
    let mount = format!("{}:/pkg.deb:ro", deb.display());
    let out = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &mount,
            "debian:bookworm-slim",
            "sh",
            "-c",
            // 精简镜像无 GUI 运行库；--force-depends 只替代依赖解析，dpkg 安装 / 启动 / 卸载真跑。
            "dpkg -i --force-depends /pkg.deb && dpkg -s polypack-app && /usr/bin/polypack-app && dpkg -r polypack-app",
        ])
        .output()
        .expect("跑 docker 替代面");
    let log = combined(&out);
    assert!(
        out.status.success(),
        "Docker 替代面（dpkg -i / 启动 / dpkg -r）失败：\n{log}"
    );
    for marker in [
        "Setting up polypack-app (0.1.0)",
        "Status: install ok installed",
        "polypack-app-core 0.1.0",
        "Removing polypack-app",
    ] {
        assert!(log.contains(marker), "替代面输出缺 {marker}：\n{log}");
    }
    eprintln!("Crostini 替代面：debian:bookworm-slim 内 dpkg -i → 启动 → dpkg -r 均通过");
}

/// 真 Chrome 三腿审计：Node ≥ 22 + Chrome 在场时实跑；Chrome 缺席由脚本 SKIP-with-print。
fn chrome_leg(site: &Path) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("仓根可达")
        .to_path_buf();
    let script = root.join("ui/smoke/pwa/audit.mjs");
    assert!(
        script.is_file(),
        "缺少真浏览器审计脚本 {}（T-M5-10 交付物）",
        script.display()
    );
    if !command_available("node", &["--version"]) {
        eprintln!("SKIP PWA 真浏览器腿：本机无 node（CDP 脚本未执行）");
        return;
    }
    let out = Command::new("node")
        .arg(&script)
        .arg("--site")
        .arg(site)
        .arg("--json")
        .output()
        .expect("跑 PWA 真浏览器审计");
    let log = combined(&out);
    assert!(out.status.success(), "PWA 真浏览器审计失败：\n{log}");
    let record: serde_json::Value = serde_json::from_str(log.trim())
        .unwrap_or_else(|err| panic!("审计脚本输出不是 JSON（{err}）：\n{log}"));
    match record["status"].as_str() {
        Some("pass") => {
            assert_eq!(
                record["installability"]["errors"].as_array().map(Vec::len),
                Some(0)
            );
            assert_eq!(record["offline"]["ok"], serde_json::Value::Bool(true));
            eprintln!(
                "PWA 真浏览器腿：manifest / 安装性 / 离线（断网重载）三腿 PASS（Chrome `Page.getInstallabilityErrors` 空）"
            );
        }
        Some("skip") => eprintln!(
            "SKIP PWA 真浏览器腿：{}",
            record["reason"].as_str().unwrap_or("Chrome 缺席")
        ),
        Some(other) => panic!("PWA 真浏览器审计未通过（status={other}）：\n{log}"),
        None => panic!("审计输出缺 status：\n{log}"),
    }
}

/// real：同一 JobSpec 双轨出包 + deb 结构 / 同源证据 / verify / Chrome / Crostini 替代面。
#[test]
fn chromeos_dual_track_real_artifacts() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真实链路设 POLY_CONTRACT_REAL=1 运行");
        return;
    }
    if poly_targets::current_host() != Some(Host::Linux) {
        eprintln!(
            "SKIP：ChromeOS 双轨 real 腿只在 Linux 宿主执行（当前 {:?}）",
            poly_targets::current_host()
        );
        return;
    }
    if !command_available("cargo", &["--version"]) {
        eprintln!("SKIP：宿主无 cargo，compile 支线无法执行");
        return;
    }

    let dir = init_project("real");
    write_ui(&dir);

    // 一次构建、两个 target = 同一 JobSpec（compile / bundle 各一次）。
    let build = poly(
        &[
            "build",
            "--target",
            "chromeos",
            "--target",
            "chromeos-pwa",
            "--json",
        ],
        &dir,
    );
    let build_log = combined(&build);
    assert!(
        build.status.success(),
        "poly build 双轨失败（{}）：\n{build_log}",
        build.status
    );
    assert!(
        build_log.contains(r#""event":"planned","targets":["chromeos","chromeos-pwa"]"#),
        "构建计划应同时含两轨（同一 JobSpec）：\n{build_log}"
    );
    assert_eq!(
        build_log.matches(r#""event":"compiled""#).count(),
        1,
        "同一 JobSpec 的 compile 应只跑一次：\n{build_log}"
    );
    assert_eq!(
        build_log.matches(r#""event":"ui_bundled""#).count(),
        1,
        "同一 JobSpec 的 UI bundle 应只跑一次：\n{build_log}"
    );

    let deb = find_artifact(&dir.join(".polypack/packages/chromeos"), "deb")
        .unwrap_or_else(|| panic!("未发现 chromeos deb 产物（工程 {}）", dir.display()));
    let site = dir.join(".polypack/packages/chromeos-pwa/site");
    assert!(
        site.join("index.html").is_file() && site.join("integrity.json").is_file(),
        "未发现 chromeos-pwa 静态站点（应含 index.html + integrity.json）"
    );
    // build 注入对照：入口必须有 manifest link。
    let index = std::fs::read_to_string(site.join("index.html")).expect("site 入口可读");
    assert!(
        index.contains("rel=\"manifest\""),
        "Web 管线未注入 manifest link：\n{index}"
    );

    let mut log = format!(
        "T-M5-10 ChromeOS 双轨契约 real 模式\n工程：{}\ndeb：{}\nsite：{}\n\n{build_log}\n",
        dir.display(),
        deb.display(),
        site.display()
    );

    // (a) deb 结构（dpkg-deb 缺席时如实跳过深检，不红）。
    let mut payload = None;
    if command_available("dpkg-deb", &["--version"]) {
        assert_deb_structure(&deb);
        let extract = dir.join(".evidence/deb-root");
        let _ = std::fs::remove_dir_all(&extract);
        std::fs::create_dir_all(&extract).expect("deb 解包目录可建");
        let status = Command::new("dpkg-deb")
            .arg("-x")
            .arg(&deb)
            .arg(&extract)
            .status()
            .expect("跑 dpkg-deb -x");
        assert!(status.success(), "dpkg-deb -x 失败");
        payload = Some(extract.join("usr/lib/polypack-app"));
        log.push_str("OK deb：control / postinst + postrm / AppStream / 数据段载荷齐备\n");
    } else {
        eprintln!("SKIP deb 结构深检：本机无 dpkg-deb（真实链路证据不完整，如实注记）");
    }

    // (b) 同一 JobSpec 证据。
    if let Some(payload) = &payload {
        assert_same_jobspec(&site, payload);
        log.push_str("OK 同源：deb 载荷与 site 逐文件 SRI 相等\n");
    }

    // (c) verify 两轨各自通过（build 内已跑同一实现；此处证明独立命令可用）。
    for target in ["chromeos", "chromeos-pwa"] {
        let verify = poly(&["verify", "--target", target, "--json"], &dir);
        let verify_log = combined(&verify);
        assert!(
            verify.status.success(),
            "poly verify --target {target} 失败：\n{verify_log}"
        );
        assert!(
            verify_log.contains(r#""event":"verified""#),
            "verify 未给出 Verified 事件（{target}）：\n{verify_log}"
        );
        log.push_str(&format!("OK verify {target}\n{verify_log}\n"));
    }

    // (d) 真 Chrome 三腿（Chrome 缺席 SKIP-with-print）。
    chrome_leg(&site);

    // (e) Crostini 替代面（Docker 真跑 dpkg）。
    crostini_surrogate(&deb);

    std::fs::write(dir.join("chromeos-contract-real.log"), &log).expect("契约日志可写");
    eprintln!(
        "OK ChromeOS 双轨 real：deb + PWA 产物齐备，日志 {}（工程保留供 CI 留档）",
        dir.join("chromeos-contract-real.log").display()
    );
}
