//! rpm 打包器（T-M5-01）。
//!
//! 产物 = `<out_dir>/<kebab>-<version>-1.<arch>.rpm`：rpmbuild 消费预布置的载荷树
//! （`SOURCES/payload`，spec 的 `%install` 只做整树拷贝，不编译），spec 带 `Requires`
//! 运行时依赖、`%files` 与 changelog；AppStream 元数据随载荷分发。
//!
//! rpmbuild 解析：PATH 优先；缺席时用 `~/.cache/polypack/tools/rpm` 的解包工具链
//! （`RPM_CONFIGDIR` + `LD_LIBRARY_PATH`，与 kotlinc 的缓存约定同源）。两者都缺时如实
//! 降级为 `rpm-payload` 目录并标注，不伪造 `.rpm`。

use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use super::{
    RPM, RPM_TARGETS, app_id, check_format, install_appstream, path_arg, source_date_epoch,
    stage_dir, tool_cache, with_epoch,
};
use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// rpm 打包器（`rpmbuild -bb`）。
pub struct Rpm;

/// 打包器自称的 L1 id。
const ID: &str = "linux";

/// Fedora 运行时依赖（Tauri WebView 链；Fedora 包名）。
pub const RPM_RUNTIME_REQUIRES: &str = "webkit2gtk4.1, gtk3, libsoup3";

/// spec changelog 的日期：固定值（rpm 要求 `%a %b %d %Y`；可复现优先于「今天」）。
const CHANGELOG_DATE: &str = "Mon Jan 01 2024";

impl Packager for Rpm {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        RPM
    }

    fn serves(&self, target_id: &str) -> bool {
        RPM_TARGETS.contains(&target_id)
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, RPM)?;
        inputs.require_bare(RPM)?;
        let stage = stage_dir(inputs, RPM);
        let spec_file = stage
            .join("SPECS")
            .join(format!("{}.spec", inputs.product.kebab_name()));
        let mut plan = BundlePlan::new().with_script(spec_file.clone(), rpm_spec(inputs)?);
        match rpmbuild_command(&stage, &spec_file) {
            Some(command) => {
                plan = plan.with_command(with_epoch(command, source_date_epoch()));
            }
            None => {
                plan = plan.skipping(
                    "rpmbuild",
                    "安装 rpm-build（Fedora：dnf install rpm-build），或把 rpm 官方工具链解包到 \
                     ~/.cache/polypack/tools/rpm",
                );
            }
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(RPM)?;
        let stage = stage_dir(inputs, RPM);
        let _ = std::fs::remove_dir_all(&stage);
        for dir in [
            "SPECS",
            "SOURCES",
            "BUILD",
            "RPMS",
            "BUILDROOT",
            "tmp",
            "db",
        ] {
            std::fs::create_dir_all(stage.join(dir)).map_err(io_err(&stage.join(dir)))?;
        }
        let payload = stage.join("SOURCES/payload");
        // Fedora 走标准 FHS 前缀（/usr/bin、/usr/lib、/usr/share）：裸产物本来就是该布局，
        // 不套 Flatpak/Snap 的根级扁平化——`/lib` 等根目录属于 filesystem 包，整目录入
        // %files 会让 dnf 事务冲突（filesystem-3.18 实测）。
        fsutil::copy_tree(&bare.root, &payload)?;
        install_appstream(&payload.join("usr/share"), &inputs.product)?;
        write_license(inputs, &payload)?;
        fsutil::write_scripts(&plan)?;
        super::normalize_modes(&payload)?;
        // 可复现：rpmbuild 按 SOURCE_DATE_EPOCH 钳制载荷 mtime，输入树也先归一。
        super::apply_epoch(&payload)?;

        if !plan.skipped.is_empty() {
            let notes = plan
                .skipped
                .iter()
                .map(|s| format!("未打包：{}（{}）", s.tool, s.reason))
                .collect();
            return fsutil::dir_artifact(&spec.id, "rpm-payload", &payload, notes);
        }
        exec::run_all(&plan.commands)?;
        let built = find_built_rpm(&stage)?;
        let out = rpm_path(inputs);
        fsutil::copy_file(&built, &out, false)?;
        let structure = check_rpm_structure(&out)?;
        fsutil::file_artifact(
            &spec.id,
            RPM,
            &out,
            vec![
                format!(
                    "结构检查：{}-{}-{}.{}，{} 个文件（Requires: {RPM_RUNTIME_REQUIRES}；rpmlint 门禁见 polypack verify）",
                    structure.name,
                    structure.version,
                    structure.release,
                    structure.arch,
                    structure.files.len()
                ),
                "安装冒烟：dnf install ./<rpm> 或 rpm -ivh <rpm>（需 root）".to_string(),
            ],
        )
    }
}

/// rpm 产物路径（测试按同一函数推导）。
pub(crate) fn rpm_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}-1.{}.rpm",
        inputs.product.kebab_name(),
        inputs.product.version,
        rpm_arch()
    ))
}

/// rpm 架构名（x86_64 → x86_64、aarch64 → aarch64；测试同口径派生）。
pub fn rpm_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "i686",
        "arm" => "armv7hl",
        "riscv64" => "riscv64",
        other => other,
    }
}

/// spec 文本：预布置载荷 + `%files` + changelog（rpm 要求 changelog 带日期与版本）。
fn rpm_spec(inputs: &BundleInputs) -> Result<String, BundleError> {
    let product = &inputs.product;
    let id = app_id(product)?;
    let kebab = product.kebab_name();
    let binary = product.binary_name();
    let summary = summary_line(product)?;
    let mut out = String::new();
    out.push_str(&format!("Name: {kebab}\n"));
    out.push_str(&format!("Version: {}\n", product.version));
    out.push_str("Release: 1\n");
    out.push_str(&format!("Summary: {summary}\n"));
    out.push_str("License: Apache-2.0\n");
    if let Some(homepage) = &product.homepage {
        out.push_str(&format!("URL: {homepage}\n"));
    }
    out.push_str(&format!("BuildArch: {}\n", rpm_arch()));
    out.push_str(&format!("Requires: {RPM_RUNTIME_REQUIRES}\n"));
    out.push_str(&format!("\n%description\n{}\n", product.description.trim()));
    // 宏路径必须加引号：rpm 展开后直接进 shell，临时目录含 `(` 等字符时不 quoting
    // 会把 `/tmp/...ThreadId(10)/...` 解析成语法错误（TMPDIR 元字符复现法）。
    out.push_str(
        "\n%install\n\
         mkdir -p \"%{buildroot}\"\n\
         cp -a \"%{_sourcedir}/payload/.\" \"%{buildroot}/\"\n",
    );
    // rpm 内建 %clean 用未引号化的 %{buildroot}：TMPDIR 含 `(` / `&` 等元字符时
    // shell 直接语法错误（测试路径 `ThreadId(10)` 实证），此处以引号版覆盖。
    out.push_str("\n%clean\nrm -rf \"%{buildroot}\"\n");
    out.push_str("\n%files\n");
    out.push_str(&format!("/usr/bin/{binary}\n"));
    // 只声明产品自己的资源目录：`/lib`（或 /usr/lib）属于 filesystem 包，
    // 整目录入 %files 会让 dnf 事务冲突（filesystem-3.18 实测）。
    // 含空格路径须整条加引号（rpm 4.18 实证：反斜杠转义与中段引号都按空白拆词，
    // `/usr/lib/Pax\ Notes` 会被误读成 "Notes" 条目 → File must begin with "/"）。
    out.push_str(&format!("\"/usr/lib/{}\"\n", product.name));
    out.push_str(&format!("/usr/share/applications/{kebab}.desktop\n"));
    out.push_str("/usr/share/icons/hicolor\n");
    out.push_str(&format!("/usr/share/metainfo/{id}.metainfo.xml\n"));
    out.push_str(&format!("/usr/share/doc/{kebab}/copyright\n"));
    // 裸产物随附的运行时依赖说明：漏列即 rpmbuild 报 Installed (but unpackaged)。
    out.push_str(&format!(
        "/usr/share/doc/{kebab}/RUNTIME-DEPENDENCIES.txt\n"
    ));
    out.push_str(&format!(
        "\n%changelog\n* {CHANGELOG_DATE} PolyPack <noreply@polypack.invalid> - {}-1\n- PolyPack build\n",
        product.version
    ));
    Ok(out)
}

/// `Summary`：描述首行、去句末点（rpmlint 的 summary-ended-with-dot 是 error）。
fn summary_line(product: &ProductInfo) -> Result<String, BundleError> {
    let line = product.description.lines().next().unwrap_or("").trim();
    let summary = line.trim_end_matches(['.', '。']).trim();
    if summary.is_empty() {
        return Err(BundleError::InvalidInput {
            packager: RPM,
            field: "product.description",
            value: product.description.clone(),
            reason: "rpm Summary 不能为空".to_string(),
        });
    }
    if summary.contains(':') || summary.contains('%') {
        return Err(BundleError::InvalidInput {
            packager: RPM,
            field: "product.description",
            value: summary.to_string(),
            reason: "rpm Summary 不能含 `:` / `%`（spec 语义字符）".to_string(),
        });
    }
    Ok(summary.to_string())
}

/// 许可证文件（`/usr/share/doc/<name>/copyright`），`%files` 显式收录。
fn write_license(inputs: &BundleInputs, payload: &Path) -> Result<(), BundleError> {
    fsutil::write_text(
        &payload.join(format!(
            "usr/share/doc/{}/copyright",
            inputs.product.kebab_name()
        )),
        &format!(
            "Upstream: {}\nName: {}\nLicense: Apache-2.0\n",
            inputs.product.homepage.as_deref().unwrap_or("(未提供)"),
            inputs.product.name
        ),
    )
}

/// rpmbuild 命令：PATH 优先，缓存工具链（`~/.cache/polypack/tools/rpm`）兜底。
fn rpmbuild_command(stage: &Path, spec_file: &Path) -> Option<Command> {
    let topdir = format!("_topdir {}", path_arg(stage));
    let tmppath = format!("_tmppath {}", path_arg(&stage.join("tmp")));
    let dbpath = format!("_dbpath {}", path_arg(&stage.join("db")));
    let common = [
        "--define".to_string(),
        topdir,
        "--define".to_string(),
        tmppath,
        "--define".to_string(),
        dbpath,
        "--define".to_string(),
        "_buildhost polypack".to_string(),
        // 可复现：BUILDTIME 取 SOURCE_DATE_EPOCH（未设置时该宏被 rpm 忽略）。
        "--define".to_string(),
        "use_source_date_epoch_as_buildtime 1".to_string(),
        "-bb".to_string(),
        path_arg(spec_file),
    ];
    if exec::tool_present("rpmbuild") {
        return Some(Command::new("rpmbuild", common));
    }
    let root = tool_cache("rpm")?;
    let program = root.join("usr/bin/rpmbuild");
    if !program.is_file() {
        return None;
    }
    let libs = [
        root.join("usr/lib/x86_64-linux-gnu"),
        root.join("usr/lib/aarch64-linux-gnu"),
        root.join("usr/lib"),
    ];
    let ld_path = libs
        .iter()
        .filter(|dir| dir.is_dir())
        .map(|dir| path_arg(dir))
        .collect::<Vec<_>>()
        .join(":");
    let mut args = common.to_vec();
    // 缓存工具链的安装后脚本（brp-*）依赖宿主缺少的辅助工具：预布置载荷不需要它们。
    args.extend(
        [
            "--define",
            "__os_install_post %{nil}",
            "--define",
            "__arch_install_post %{nil}",
            "--define",
            "debug_package %{nil}",
        ]
        .iter()
        .map(|arg| arg.to_string()),
    );
    Some(
        Command::new(path_arg(&program), args)
            .with_env("RPM_CONFIGDIR", path_arg(&root.join("usr/lib/rpm")))
            .with_env("LD_LIBRARY_PATH", ld_path),
    )
}

/// `RPMS/<arch>/*.rpm` 的单件定位（排序取首件；多件视为构建异常）。
fn find_built_rpm(stage: &Path) -> Result<PathBuf, BundleError> {
    let mut found = Vec::new();
    collect_rpms(&stage.join("RPMS"), &mut found);
    found.sort();
    match found.len() {
        0 => Err(BundleError::Archive {
            path: stage.to_path_buf(),
            reason: "rpmbuild 成功退出但 RPMS/ 下没有 .rpm".to_string(),
        }),
        1 => Ok(found.remove(0)),
        count => Err(BundleError::Archive {
            path: stage.to_path_buf(),
            reason: format!("RPMS/ 下有 {count} 份 .rpm（预期单件）"),
        }),
    }
}

fn collect_rpms(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rpms(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rpm") {
            out.push(path);
        }
    }
}

pub use super::rpm_header::{RpmStructure, check_rpm_structure};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packagers::linux::{appstream_metainfo, check_appstream_metainfo};

    #[test]
    fn arch_is_an_rpm_architecture() {
        assert!(
            ["x86_64", "aarch64", "i686", "armv7hl", "riscv64"].contains(&rpm_arch()),
            "未映射的架构 {}",
            rpm_arch()
        );
    }

    /// spec 必须带 Depends 等价物、维护者需要的 changelog 与 `%files` 载荷条目。
    #[test]
    fn spec_has_rpm_required_sections() {
        let inputs = crate::inputs::BundleInputs::new(
            ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "Pax 示例笔记应用。".to_string(),
                homepage: Some("https://example.com".to_string()),
            },
            std::env::temp_dir(),
        );
        let spec = rpm_spec(&inputs).unwrap();
        assert!(spec.contains("Name: pax-notes\n"));
        assert!(spec.contains("Requires: webkit2gtk4.1, gtk3, libsoup3\n"));
        assert!(spec.contains("%install\n"));
        assert!(spec.contains("/usr/bin/pax-notes\n"));
        assert!(spec.contains("\"/usr/lib/Pax Notes\"\n"));
        assert!(spec.contains("/usr/share/metainfo/com.polypack.paxnotes.metainfo.xml\n"));
        assert!(spec.contains("/usr/share/doc/pax-notes/RUNTIME-DEPENDENCIES.txt\n"));
        assert!(spec.contains("%changelog\n* Mon Jan 01 2024"));
        assert!(!spec.contains("%prep"), "预布置载荷不需要 %prep");
    }

    #[test]
    fn summary_rejects_spec_metacharacters() {
        let mut product = ProductInfo {
            name: "Pax".to_string(),
            version: "1.0.0".to_string(),
            identifier: "com.polypack.pax".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "a %b".to_string(),
            homepage: None,
        };
        assert!(summary_line(&product).is_err());
        product.description = "   ".to_string();
        assert!(summary_line(&product).is_err());
    }

    /// 元数据生成与校验共用同一函数：生成的 metainfo 必须过 check。
    #[test]
    fn generated_metainfo_passes_structure_check() {
        let product = ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用。".to_string(),
            homepage: Some("https://example.com".to_string()),
        };
        let base = std::env::temp_dir().join(format!("polypack-rpm-meta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let path = base.join("com.polypack.paxnotes.metainfo.xml");
        fsutil::write_text(&path, &appstream_metainfo(&product).unwrap()).unwrap();
        check_appstream_metainfo(&path).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
