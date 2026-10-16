//! T-M4-08 / T-M4-09 / T-M4-10 契约测试：web 系 TV 四目标从**同一份 UI 载荷**出包。
//!
//! 真跑面（本机可真造真验）：`.wgt`（zip）/ `.ipk`（ar + tar.gz）/ 两枚应用包（zip）就地
//! 出包并过各自的**同一份**结构检查；「同源同摘要」断言四个产物的 UI 载荷摘要相等且等于
//! 从源 dist 直接算的摘要（构造性口径：同一个 [`poly_bundle::webapp::tree_digest`]）。
//! 外部见证：宿主有 python3 / ar / tar 时，用系统工具独立读回容器条目；缺席按仓库惯例
//! SKIP-with-print（见 `common` 模块），绝不伪造。

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::{BundleInputs, ProductInfo, WebAppPlatform};
use poly_targets::Registry;

/// 测试用临时目录：进程内自增序号（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "poly-bundle-webtv-{}-{tag}-{seq}",
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

/// 产品元数据（与 `poly init` 模板同形）。
fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "契约测试合成输入".to_string(),
        homepage: None,
    }
}

/// 合成 UI 载荷（dist 整树：入口 + 资源子目录）。
fn seed_dist(t: &Temp) -> PathBuf {
    let dist = t.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist");
    std::fs::write(dist.join("index.html"), "<!doctype html><html></html>").expect("写入口");
    std::fs::write(dist.join("assets/app.js"), "console.log('pax');").expect("写资源");
    dist
}

/// 合成 wasm 出口四文件（T-M3-07 随包面；打包器只做整目录拷贝分发，打包期不解析内容）。
fn seed_bridge(t: &Temp) -> PathBuf {
    let dir = t.join("bridge-src");
    std::fs::create_dir_all(&dir).expect("建 wasm 出口目录");
    for (name, bytes) in [
        ("manifest.json", &b"{\"schema\":\"wasm-sandbox/v1\"}\n"[..]),
        ("polypack-loader.js", &b"// loader\n"[..]),
        ("pax_core_wasm.js", &b"// glue\n"[..]),
        ("pax_core_wasm_bg.wasm", &[0x00, 0x61, 0x73, 0x6d][..]),
    ] {
        std::fs::write(dir.join(name), bytes).expect("写出口文件");
    }
    dir
}

/// 内建注册表里的目标（L3 web 系 TV；规格漂移在这里即红）。
fn spec(id: &str) -> poly_targets::TargetSpec {
    let spec = Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 {id}"));
    assert_eq!(spec.tier, 3, "{id} 应为 L3（tier 3）");
    spec
}

/// 用同一份 dist + wasm 出口真跑一个目标出包。
fn pack(t: &Temp, id: &str, format: &str, dist: &Path, bridge: &Path) -> poly_bundle::Artifact {
    let mut inputs = BundleInputs::new(product(), t.join(&format!("out-{id}")));
    inputs.ui_dist = Some(dist.to_path_buf());
    inputs.miniapp_bridge = Some(bridge.to_path_buf());
    let spec = spec(id);
    assert_eq!(spec.package.format, format, "{id} 声明格式");
    let packager = poly_bundle::find(id, format)
        .unwrap_or_else(|| panic!("poly-bundle 缺少 {id} 的 {format} 打包器"));
    assert_eq!(packager.id(), id);
    packager
        .package(&inputs, &spec)
        .unwrap_or_else(|err| panic!("{id} package() 真跑失败：{err}"))
}

/// 四目标同源出包：同一 dist → 四个产物，UI 载荷摘要逐一相等（T-M4-09 验收口径），
/// 各自过自己的结构检查（与打包器同一实现），桥随包、边界注记如实。
#[test]
fn web_tv_family_shares_one_ui_digest_and_passes_own_structure_checks() {
    let t = Temp::new("family");
    let dist = seed_dist(&t);
    let bridge = seed_bridge(&t);
    let digest = poly_bundle::webapp::tree_digest(&dist).expect("源摘要");

    // Tizen `.wgt`（T-M4-08）：zip 容器 + config.xml 在根 + 桥随包 + 签名凭据引用
    let wgt = pack(&t, "tizen-tv", "wgt", &dist, &bridge);
    assert_eq!(wgt.format, "wgt");
    assert_eq!(
        wgt.path.extension().and_then(|ext| ext.to_str()),
        Some("wgt")
    );
    let structure = poly_bundle::check_wgt_structure(&wgt.path).expect("wgt 结构检查");
    assert_eq!(structure.content_src, "index.html");
    assert_eq!(structure.app_id, "com.polypack.paxnotes.paxnotes");
    assert!(
        structure
            .entries
            .iter()
            .any(|entry| entry.starts_with("bridge/wasm-bindgen/")),
        "wasm 出口须随包：{:?}",
        structure.entries
    );
    assert!(
        wgt.notes.iter().any(|note| note.contains(&digest)),
        "{:?}",
        wgt.notes
    );
    assert!(
        wgt.notes.iter().any(|note| note.contains("tizen-author")),
        "签名按凭据引用标注：{:?}",
        wgt.notes
    );

    // webOS `.ipk`（T-M4-09）：ar 三件套 + control + appinfo + 同源摘要
    let ipk = pack(&t, "webos-tv", "ipk", &dist, &bridge);
    let structure = poly_bundle::check_ipk_structure(&ipk.path).expect("ipk 结构检查");
    assert_eq!(structure.package, "com.polypack.paxnotes");
    assert_eq!(structure.app_main, "index.html");
    assert!(
        structure
            .entries
            .iter()
            .any(|entry| entry.ends_with("bridge/wasm-bindgen/pax_core_wasm_bg.wasm")),
        "wasm 出口须随载荷：{:?}",
        structure.entries
    );
    assert!(
        ipk.notes.iter().any(|note| note.contains(&digest)),
        "{:?}",
        ipk.notes
    );
    assert!(
        ipk.notes.iter().any(|note| note.contains("平台侧")),
        "webOS 签名按平台侧标注：{:?}",
        ipk.notes
    );

    // VIDAA / Vizio（T-M4-10）：应用包 + 沙箱自检 + 同源摘要 + 无签名 / 无推送边界
    for (id, manifest) in [
        ("vidaa-tv", "manifest.json"),
        ("smartcast-tv", "appinfo.json"),
    ] {
        let app = pack(&t, id, "web-app", &dist, &bridge);
        assert_eq!(
            app.path.extension().and_then(|ext| ext.to_str()),
            Some("zip"),
            "{id} 平台未规定扩展名，如实 .zip"
        );
        let platform = WebAppPlatform::from_target(id).expect("平台分表");
        let structure = poly_bundle::check_web_app_structure(&app.path, platform)
            .unwrap_or_else(|err| panic!("{id} 沙箱自检失败：{err}"));
        assert_eq!(structure.manifest, manifest, "{id}");
        assert_eq!(structure.entry, "index.html", "{id} 入口规则");
        assert!(
            app.notes.iter().any(|note| note.contains(&digest)),
            "{id} 摘要须与其它目标同源同值：{:?}",
            app.notes
        );
        assert!(
            app.notes
                .iter()
                .any(|note| note.contains("无签名") || note.contains("无本地发布签名"))
                && app.notes.iter().any(|note| note.contains("无推送")),
            "{id} 须如实标注无签名 / 无推送：{:?}",
            app.notes
        );
    }
}

/// 外部见证（宿主有工具时）：系统 python3 / ar / tar 独立读回两种容器。
#[test]
fn system_tools_witness_the_containers() {
    let t = Temp::new("witness");
    let dist = seed_dist(&t);
    let bridge = seed_bridge(&t);

    // `.wgt`：python3 zipfile 列条目（独立于本仓 zip 读写面）
    if !common::skip_without("python3") {
        let wgt = pack(&t, "tizen-tv", "wgt", &dist, &bridge);
        let out = std::process::Command::new("python3")
            .arg("-c")
            .arg("import zipfile,sys; print('\\n'.join(zipfile.ZipFile(sys.argv[1]).namelist()))")
            .arg(&wgt.path)
            .output()
            .expect("python3 可执行");
        assert!(out.status.success(), "python3 zipfile 读回失败");
        let names = String::from_utf8_lossy(&out.stdout);
        assert!(names.contains("config.xml"), "{names}");
        assert!(names.contains("index.html"), "{names}");
    }

    // `.ipk`：`ar t` 列成员 + `tar tzf` 读 control 成员（先把成员字节落临时文件）
    if !common::skip_without_any(&["ar", "tar"]) {
        let ipk = pack(&t, "webos-tv", "ipk", &dist, &bridge);
        let out = std::process::Command::new("ar")
            .arg("t")
            .arg(&ipk.path)
            .output()
            .expect("ar 可执行");
        assert!(out.status.success(), "ar t 读回失败");
        let members = String::from_utf8_lossy(&out.stdout);
        for name in ["debian-binary", "control.tar.gz", "data.tar.gz"] {
            assert!(members.contains(name), "{members}");
        }
        let control = poly_bundle::arwrite::read_ar(&ipk.path)
            .expect("ar 解析")
            .into_iter()
            .find(|member| member.name == "control.tar.gz")
            .expect("control 成员在场");
        let control_tgz = t.join("control.tar.gz");
        std::fs::write(&control_tgz, &control.bytes).expect("写 control.tar.gz");
        let out = std::process::Command::new("tar")
            .arg("tzf")
            .arg(&control_tgz)
            .output()
            .expect("tar 可执行");
        assert!(out.status.success(), "tar tzf 读回失败");
        let names = String::from_utf8_lossy(&out.stdout);
        assert!(names.contains("control"), "{names}");
    }
}
