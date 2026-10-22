//! pkgsrc 骨架结构检查（T-M5-08）：打包器自检与 verify 腿 `pkgsrc-check` 共用。

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::BundleError;

use super::payload_entries;

/// pkgsrc 骨架结构检查结果（打包器自检与 verify 腿 `pkgsrc-check` 的本地结构面共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgsrcStructure {
    /// `DISTNAME`（`<kebab>-<version>`，也是 `.tgz` 基名）。
    pub distname: String,
    /// 从 `DISTNAME` 拆出的包名（kebab）。
    pub name: String,
    /// 从 `DISTNAME` 拆出的版本。
    pub version: String,
    /// `PLIST` 文件条目（不含 `@` 注解）。
    pub entries: Vec<String>,
}

/// 检查 pkgsrc 骨架：Makefile 必填项 / DESCR 非空 / PLIST 与 `files/payload/` 双向一致。
///
/// 打包器 `package()` 与 verify 腿共用本函数（产物缺陷硬失败，绝不降级成跳过）。
pub fn check_pkgsrc_structure(port: &Path) -> Result<PkgsrcStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: port.to_path_buf(),
        reason,
    };
    let makefile = std::fs::read_to_string(port.join("Makefile"))
        .map_err(|err| bad(format!("缺 Makefile：{err}")))?;
    let mut fields = BTreeMap::new();
    for name in [
        "DISTNAME",
        "CATEGORIES",
        "MAINTAINER",
        "COMMENT",
        "LICENSE",
        "NO_CHECKSUM",
        "NO_CONFIGURE",
        "NO_BUILD",
    ] {
        let value = make_var(&makefile, name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("Makefile 缺必填项 {name}")))?;
        fields.insert(name, value);
    }
    for target in ["do-extract:", "do-install:"] {
        if !makefile.lines().any(|line| line.trim() == target) {
            return Err(bad(format!(
                "Makefile 缺 {target} 目标（预置载荷无解包 / 安装步骤）"
            )));
        }
    }
    if !makefile
        .lines()
        .any(|line| line.trim() == r#".include "../../mk/bsd.pkg.mk""#)
    {
        return Err(bad(
            "Makefile 缺 `.include \"../../mk/bsd.pkg.mk\"`（bmake 不会加载 pkgsrc 基础设施）"
                .to_string(),
        ));
    }
    if let Some(bad_field) = ["DISTNAME", "CATEGORIES", "MAINTAINER", "COMMENT"]
        .iter()
        .find(|name| fields[*name].contains(['\t', '\r']))
    {
        return Err(bad(format!("Makefile 的 {bad_field} 含控制字符")));
    }
    let distname = fields["DISTNAME"];
    let Some((name, version)) = split_distname(distname) else {
        return Err(bad(format!(
            "DISTNAME 须为 <kebab>-<version> 且版本以数字开头：{distname:?}"
        )));
    };
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(bad(format!("DISTNAME 包名须为小写 kebab：{name:?}")));
    }
    if version.contains(['/', ' ', '\t']) {
        return Err(bad(format!("DISTNAME 版本含非法字符：{version:?}")));
    }

    let descr = std::fs::read_to_string(port.join("DESCR"))
        .map_err(|err| bad(format!("缺 DESCR：{err}")))?;
    if descr.trim().is_empty() {
        return Err(bad("DESCR 为空".to_string()));
    }

    let plist_text = std::fs::read_to_string(port.join("PLIST"))
        .map_err(|err| bad(format!("缺 PLIST：{err}")))?;
    let mut entries = Vec::new();
    for line in plist_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('@') {
            // pkgsrc 的 @comment / @pkgdir / @mode 等注解本生成器不产出，只核文件条目。
            continue;
        }
        if line.starts_with('/') || line.split('/').any(|part| part == "..") {
            return Err(bad(format!(
                "PLIST 条目不是 ${{PREFIX}} 下的相对路径：{line}"
            )));
        }
        entries.push(line.to_string());
    }
    if entries.is_empty() {
        return Err(bad("PLIST 没有任何文件条目".to_string()));
    }
    if !entries.iter().any(|entry| entry == &format!("bin/{name}")) {
        return Err(bad(format!(
            "PLIST 缺可执行 bin/{name}（DISTNAME 与实际载荷不一致）"
        )));
    }

    let payload_root = port.join("files/payload");
    let payload =
        payload_entries(&payload_root).map_err(|err| bad(format!("载荷树不可读：{err}")))?;
    for entry in &entries {
        if !payload.contains_key(entry) {
            return Err(bad(format!(
                "PLIST 条目 {entry} 在 files/payload/ 下没有对应文件"
            )));
        }
    }
    for path in payload.keys() {
        if !entries.iter().any(|entry| entry == path) {
            return Err(bad(format!(
                "载荷文件 {path} 未进 PLIST（安装树与 packing list 不一致）"
            )));
        }
    }

    Ok(PkgsrcStructure {
        distname: distname.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        entries,
    })
}

/// 拆 `DISTNAME`：最后一个 `-` 且后缀以数字开头（`pax-notes-1.2.3` → `pax-notes` / `1.2.3`）。
fn split_distname(distname: &str) -> Option<(&str, &str)> {
    let (name, version) = distname.rsplit_once('-')?;
    let numeric = version.starts_with(|ch: char| ch.is_ascii_digit());
    (!name.is_empty() && numeric).then_some((name, version))
}

/// 解析 Makefile 的 `NAME = value` / `NAME=value`（取首个匹配；`+=` 不当作必填项来源）。
fn make_var<'a>(makefile: &'a str, name: &str) -> Option<&'a str> {
    makefile.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim())
    })
}
