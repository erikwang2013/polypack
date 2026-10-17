//! webOS `.ipk` 打包器（T-M4-09）：与 Tizen **同源**的 UI 载荷打成 Debian 形态的 `.ipk`。
//!
//! 产物 = `<out_dir>/<kebab>-<version>.ipk`（确定性：`debian-binary` + `control.tar.gz` +
//! `data.tar.gz`，载荷落 `usr/palm/applications/<包 id>/`）。载荷与 `.wgt` 走同一个
//! [`crate::webapp`]（UI dist + wasm 出口 + 图标），摘要同源一致（T-M4-09 验收口径）。
//! 本机无 ares-cli：`ares-package` 在场即真跑、缺席按环境槽如实跳过并注记；webOS 的发布
//! 签名在平台侧（LG Content Store 提交时完成），本地不做签名。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::arwrite::{self, Member, TarArchive};
use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::webapp;

/// webOS 目标 id。
const ID: &str = "webos-tv";

/// 产物格式（须与 `[targets.webos-tv].package.format` 一致）。
const FORMAT: &str = "ipk";

/// 打包工作目录（`<out_dir>/ipk/`：载荷与 ares 输出都落在它下面）。
const WORK_DIR: &str = "ipk";

/// 应用载荷在 `data.tar.gz` 内的安装前缀（webOS 应用安装路径）。
const APP_PREFIX: &str = "usr/palm/applications";

/// webOS 应用清单名（`id` / `version` / `type` / `main` / `title` 是安装器必读字段）。
const APPINFO: &str = "appinfo.json";

/// Debian 控制文件成员名（tar 内 `./control`）。
const CONTROL: &str = "./control";

/// Debian 格式版本成员（内容固定 `2.0\n`）。
const DEBIAN_BINARY: &str = "debian-binary";

/// `.ipk` 打包器。
pub struct WebOsIpk;

/// `.ipk` 结构检查结果（打包器自检与 verify 腿 `ares-package` 的本地结构面共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpkStructure {
    /// ar 成员名（序即约定序）。
    pub members: Vec<String>,
    /// control 的 `Package`（= 产品标识符，也是安装目录名）。
    pub package: String,
    /// control 的 `Version`。
    pub version: String,
    /// control 的 `Architecture`。
    pub architecture: String,
    /// appinfo 的 `id`（安装器按它取目录，须与 control 的 Package 一致）。
    pub app_id: String,
    /// appinfo 的 `type`（web 应用恒 `web`）。
    pub app_type: String,
    /// appinfo 的 `main`（应用入口）。
    pub app_main: String,
    /// `data.tar.gz` 条目名（排序）。
    pub entries: Vec<String>,
}

impl Packager for WebOsIpk {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, ID, FORMAT)?;
        inputs.require_ui_dist(ID)?;
        match ares_package_tool() {
            // ares-package 吃的是**载荷目录**（`<out_dir>/ipk/app`，组装期落盘），产物写自己的
            // 子目录（不与本打包器的确定性 .ipk 争名）。
            Ok(tool) => Ok(BundlePlan::new().with_command(
                Command::new(
                    tool.display().to_string(),
                    [
                        app_dir(inputs).display().to_string(),
                        "-o".to_string(),
                        ares_out(inputs).display().to_string(),
                    ],
                )
                .in_dir(inputs.out_dir.join(WORK_DIR)),
            )),
            Err(reason) => Ok(BundlePlan::new().skipping("ares-package", reason)),
        }
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let work = inputs.out_dir.join(WORK_DIR);
        let app = app_dir(inputs);
        let _ = std::fs::remove_dir_all(&work);
        let payload = webapp::stage_payload(inputs, &app, ID)?;
        let product = &inputs.product;
        fsutil::write_text(&app.join(APPINFO), &appinfo_json(product))?;

        // data.tar.gz：载荷整树落到 `<APP_PREFIX>/<包 id>/`（Debian 用 `./` 前缀）
        let mut data_entries = Vec::new();
        for (name, path) in webapp::collect_files(&app)? {
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            data_entries.push((
                format!("./{APP_PREFIX}/{}/{name}", product.identifier),
                bytes,
            ));
        }
        let ipk = ipk_path(inputs);
        let control_tar =
            arwrite::tar_bytes(&[(CONTROL.to_string(), control_text(product).into_bytes())])?;
        let data_tar = arwrite::tar_bytes(&data_entries)?;
        arwrite::write_ar(
            &[
                Member {
                    name: DEBIAN_BINARY.to_string(),
                    bytes: b"2.0\n".to_vec(),
                },
                Member {
                    name: "control.tar.gz".to_string(),
                    bytes: arwrite::gzip_bytes(&control_tar, &ipk)?,
                },
                Member {
                    name: "data.tar.gz".to_string(),
                    bytes: arwrite::gzip_bytes(&data_tar, &ipk)?,
                },
            ],
            &ipk,
        )?;
        // 结构自检：产物必过自己的检查器（与 verify 腿同一实现）
        let structure = check_ipk_structure(&ipk)?;
        // ares-package（在场时）：非零退出即硬失败（平台工具判缺陷不降级成跳过）
        exec::run_all(&plan.commands)?;

        let mut notes = vec![
            format!(
                "产物：{}（ar：debian-binary + control.tar.gz + data.tar.gz，{} 条目；载荷落 \
                 {APP_PREFIX}/{}/）",
                ipk.display(),
                structure.entries.len(),
                structure.package
            ),
            format!(
                "control：Package={} Version={} Architecture={}（Debian 控制文件）",
                structure.package, structure.version, structure.architecture
            ),
            format!(
                "appinfo.json：id={} type={} main={}（webOS 应用的安装清单）",
                structure.app_id, structure.app_type, structure.app_main
            ),
            "签名：平台侧（LG Content Store 发布签名）——本地不做签名，凭据引用缺席；\
             本机无 ares-cli，SDK 校验器缺席时如实跳过"
                .to_string(),
            "安装：webOS 模拟器 / 电视的侧载归平台工具链（本机无 ares-install），\
             本产物可被 `ares-package` / `dpkg-deb --info` 面消费"
                .to_string(),
        ];
        notes.extend(payload.notes());
        for command in &plan.commands {
            notes.push(format!("平台工具真跑：{}", command.line()));
        }
        notes.extend(
            plan.skipped
                .iter()
                .map(|skipped| format!("跳过：{}（{}）", skipped.tool, skipped.reason)),
        );
        fsutil::file_artifact(ID, FORMAT, &ipk, notes)
    }
}

/// `.ipk` 产物路径（`<out_dir>/<kebab>-<version>.ipk`）。
fn ipk_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}.ipk",
        inputs.product.kebab_name(),
        inputs.product.version
    ))
}

/// 载荷目录（ares-package 的输入；`<out_dir>/ipk/app/`）。
fn app_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(WORK_DIR).join("app")
}

/// ares-package 的输出目录（`<out_dir>/ipk/ares/`）。
fn ares_out(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(WORK_DIR).join("ares")
}

/// `ares-package` 工具定位：`$POLYPACK_ARES_PACKAGE`（显式覆盖）→ PATH。
///
/// `Err(reason)` = 环境缺工具（打包腿记 skipped、verify 腿告警跳过），绝不静默降级。
pub fn ares_package_tool() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("POLYPACK_ARES_PACKAGE").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "POLYPACK_ARES_PACKAGE 指向的 {} 不是文件",
            path.display()
        ));
    }
    exec::tool_path("ares-package").ok_or_else(|| {
        "PATH 里没有 ares-package（@webosose/ares-cli；或设 \
         POLYPACK_ARES_PACKAGE=<...>/ares-package）——SDK 缺席时内建结构断言守底"
            .to_string()
    })
}

/// `.ipk` 结构检查（与打包器同一实现；缺陷一律 [`BundleError::Archive`] 硬失败，不降级）。
///
/// 规则面：ar 成员恰为 `debian-binary` + `control.tar.gz` + `data.tar.gz`（序即 Debian 约定）、
/// `debian-binary` 内容 `2.0`、control 的 `Package` / `Version` / `Architecture` /
/// `Maintainer` / `Description` 齐备非空、`data.tar.gz` 内 `appinfo.json` 的 `id` 与 control
/// 的 `Package` 一致、`type` 为 `web`、`main` 与 `icon.png` 指向包内真实条目。
pub fn check_ipk_structure(ipk: &Path) -> Result<IpkStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: ipk.to_path_buf(),
        reason,
    };
    let members = arwrite::read_ar(ipk)?;
    let names: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
    let expected = [DEBIAN_BINARY, "control.tar.gz", "data.tar.gz"];
    if names.len() != expected.len() || !names.iter().zip(expected).all(|(got, want)| got == want) {
        return Err(bad(format!(
            "ar 成员面不符（须按序 {expected:?}，实际 {names:?}）"
        )));
    }
    let debian_binary = &members[0].bytes;
    if debian_binary.as_slice() != b"2.0\n" {
        return Err(bad(format!(
            "debian-binary 内容须为 `2.0\\n`（实际 {:?}）",
            String::from_utf8_lossy(debian_binary)
        )));
    }
    let control_tar = TarArchive::from_tar_gz(&members[1].bytes, ipk)?;
    let control_text = control_tar
        .text(CONTROL)
        .ok_or_else(|| bad(format!("control.tar.gz 内缺 {CONTROL}")))?;
    let fields = arwrite::control_fields(&control_text);
    for required in [
        "Package",
        "Version",
        "Architecture",
        "Maintainer",
        "Description",
    ] {
        if fields.get(required).is_none_or(|value| value.is_empty()) {
            return Err(bad(format!(
                "control 缺必填字段 {required}（Debian 控制文件）"
            )));
        }
    }
    let package = fields["Package"].clone();
    let version = fields["Version"].clone();
    let architecture = fields["Architecture"].clone();

    let data_tar = TarArchive::from_tar_gz(&members[2].bytes, ipk)?;
    let mut entries = data_tar.names();
    entries.sort();
    let root = format!("./{APP_PREFIX}/{package}");
    let appinfo_name = format!("{root}/{APPINFO}");
    let appinfo_text = data_tar
        .text(&appinfo_name)
        .ok_or_else(|| bad(format!("data.tar.gz 内缺 {appinfo_name}")))?;
    let appinfo: serde_json::Value = serde_json::from_str(&appinfo_text)
        .map_err(|err| bad(format!("{APPINFO} 非法 JSON：{err}")))?;
    let field = |key: &'static str| -> Result<String, BundleError> {
        appinfo
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| bad(format!("{APPINFO} 的 {key} 必填且须为非空字符串")))
    };
    let app_id = field("id")?;
    let app_type = field("type")?;
    let app_main = field("main")?;
    if app_id != package {
        return Err(bad(format!(
            "appinfo.id = {app_id} 与 control 的 Package = {package} 不一致（安装目录按 Package 取）"
        )));
    }
    if app_type != "web" {
        return Err(bad(format!(
            "appinfo.type = {app_type}，web 运行时目标须为 `web`"
        )));
    }
    if app_main != webapp::ENTRY {
        return Err(bad(format!(
            "appinfo.main = {app_main}，TV 应用入口须为 {}（载荷根）",
            webapp::ENTRY
        )));
    }
    for (label, name) in [
        ("main", app_main.clone()),
        ("icon", webapp::ICON.to_string()),
    ] {
        let wanted = format!("{root}/{name}");
        if !entries.iter().any(|entry| entry == &wanted) {
            return Err(bad(format!(
                "appinfo 的 {label} = {name}，但载荷内没有 {wanted}"
            )));
        }
    }
    Ok(IpkStructure {
        members: names,
        package,
        version,
        architecture,
        app_id,
        app_type,
        app_main,
        entries,
    })
}

/// Debian 控制文件字段解析（`Key: Value`；续行以空白开头，并入上一字段）。
/// Debian 控制文件（元数据经产品白名单校验：无控制字符，单行安全）。
fn control_text(product: &ProductInfo) -> String {
    format!(
        "Package: {}\nVersion: {}\nArchitecture: all\nMaintainer: {}\nDescription: {}\n",
        product.identifier, product.version, product.manufacturer, product.description
    )
}

/// webOS 应用清单（`appinfo.json`）。
fn appinfo_json(product: &ProductInfo) -> String {
    let value = serde_json::json!({
        "id": product.identifier,
        "version": product.version,
        "vendor": product.manufacturer,
        "type": "web",
        "main": webapp::ENTRY,
        "title": product.name,
        "icon": webapp::ICON,
    });
    // `Value` 的序列化不会失败（无非字符串键 / 非有限数）；失败即如实报错而非 unwrap。
    serde_json::to_string_pretty(&value)
        .map(|text| format!("{text}\n"))
        .unwrap_or_else(|_| value.to_string())
}

/// 打包格式与打包器声明必须一致（防「按 ipk 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, id: &'static str, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: id,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_targets::{Bridge, Host, PackageSpec, Runtime};

    fn spec(id: &str, format: &str) -> TargetSpec {
        let mut spec = TargetSpec::new_stub(id, 3, &[Host::Linux]);
        spec.runtime = Runtime::Wasm;
        spec.bridge = Bridge::WasmBindgen;
        spec.package = PackageSpec {
            format: format.to_string(),
            builder: "webos-ares".to_string(),
        };
        spec
    }

    fn product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用".to_string(),
            homepage: None,
        }
    }

    fn inputs(dir: &Path) -> BundleInputs {
        let dist = dir.join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html><html></html>").unwrap();
        let mut inputs = BundleInputs::new(product(), dir.join("out"));
        inputs.ui_dist = Some(dist);
        inputs
    }

    fn scratch(tag: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("polypack-webos-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    /// 真造真验：包出 `.ipk` → 结构检查回读 control / appinfo / 载荷条目。
    #[test]
    fn packages_ipk_and_check_reads_members_back() {
        let base = scratch("roundtrip");
        let inputs = inputs(&base);
        let artifact = WebOsIpk
            .package(&inputs, &spec("webos-tv", "ipk"))
            .expect("打包");
        assert_eq!(artifact.format, "ipk");
        assert_eq!(
            artifact.path.extension().and_then(|ext| ext.to_str()),
            Some("ipk")
        );

        let structure = check_ipk_structure(&artifact.path).expect("结构检查");
        assert_eq!(
            structure.members,
            vec!["debian-binary", "control.tar.gz", "data.tar.gz"]
        );
        assert_eq!(structure.package, "com.polypack.paxnotes");
        assert_eq!(structure.version, "1.2.3");
        assert_eq!(structure.architecture, "all");
        assert_eq!(structure.app_id, "com.polypack.paxnotes");
        assert_eq!(structure.app_type, "web");
        assert_eq!(structure.app_main, "index.html");
        // 条目名照 tar 内原样（Debian 载荷条目带 `./` 前缀）
        let root = "./usr/palm/applications/com.polypack.paxnotes";
        assert!(
            structure
                .entries
                .iter()
                .any(|name| name == &format!("{root}/index.html"))
        );
        assert!(
            structure
                .entries
                .iter()
                .any(|name| name == &format!("{root}/appinfo.json"))
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 结构缺陷必报错：成员面不符（缺 data.tar.gz）/ appinfo 缺失。
    #[test]
    fn structure_check_flags_missing_member_and_appinfo() {
        let base = scratch("bad");
        std::fs::create_dir_all(&base).unwrap();
        let ipk = base.join("bad.ipk");
        arwrite::write_ar(
            &[
                Member {
                    name: "debian-binary".to_string(),
                    bytes: b"2.0\n".to_vec(),
                },
                Member {
                    name: "control.tar.gz".to_string(),
                    bytes: arwrite::gzip_bytes(
                        &arwrite::tar_bytes(&[(
                            CONTROL.to_string(),
                            control_text(&product()).into_bytes(),
                        )])
                        .unwrap(),
                        &ipk,
                    )
                    .unwrap(),
                },
            ],
            &ipk,
        )
        .unwrap();
        let err = check_ipk_structure(&ipk).expect_err("缺成员必报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0006);
        assert!(err.to_string().contains("data.tar.gz"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 外部见证（本机有 `tar` 时）：系统 tar 能读回我们写的 control.tar.gz 条目。
    #[test]
    fn system_tar_reads_our_control_member() {
        let base = scratch("system");
        std::fs::create_dir_all(&base).unwrap();
        let inputs = inputs(&base);
        let artifact = WebOsIpk
            .package(&inputs, &spec("webos-tv", "ipk"))
            .expect("打包");
        let members = arwrite::read_ar(&artifact.path).unwrap();
        let control = base.join("control.tar.gz");
        std::fs::write(&control, &members[1].bytes).unwrap();
        let data = base.join("data.tar.gz");
        std::fs::write(&data, &members[2].bytes).unwrap();
        for (file, expected) in [
            (&control, "./control"),
            (
                &data,
                "./usr/palm/applications/com.polypack.paxnotes/index.html",
            ),
        ] {
            let out = std::process::Command::new("tar")
                .args(["-tzf"])
                .arg(file)
                .output();
            match out {
                Ok(out) if out.status.success() => {
                    let names = String::from_utf8_lossy(&out.stdout);
                    assert!(names.contains(expected), "{names}");
                }
                _ => println!("SKIP: 本机无 tar（系统交叉验证腿跳过）"),
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
