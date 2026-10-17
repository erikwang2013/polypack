//! AUTOSAR Adaptive 打包器（T-M6-04）：静态库 + ARXML 服务清单。
//!
//! 清单从 poly-core 的 C ABI **单一来源**派生：[`EmbeddedInputs::symbols`]（CLI 由
//! `poly_core::engine::ffi::FFI_FUNCTIONS` 注入）中每个 `poly_engine_*` 符号对应
//! 一个 `SERVICE-INTERFACE`（METHOD 同名）+ 一个 `PROVIDED-SERVICE-INSTANCE`
//!（`<symbol>Port`）——服务 / 端口与导出符号一一对应，不手写第二份 ABI 清单。
//!
//! 静态库：宿主自包含 `cargo rustc --manifest-path <poly-core> --lib --crate-type
//! staticlib`（poly-core 文档给出的最小面）；`no_std` 时加 `--no-default-features`
//!（应用提供 global_allocator / panic_handler）。
//!
//! 校验三层（如实分级，不假装）：
//! 1. 结构断言（恒真跑）：服务 / 端口 / 方法与符号清单一一对应；
//! 2. `xmllint --schema`（在场真跑；缺席 → SKIP-with-print 注记）。嵌入的是本产物
//!    的 AUTOSAR r4.0 最小子集 XSD，不冒充官方全量 XSD / ECU 配置合法性；
//! 3. `nm -g --defined-only` 归档符号与清单对照（nm 在场真跑；缺席 → SKIP 注记）。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::Command;
use crate::embedded::{self, EmbeddedInputs};
use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Packager};

/// 目标 id。
const ID: &str = "autosar";

/// 产物格式。
const FORMAT: &str = "arxml";

/// 嵌入的子集 XSD 文件名。
pub const SCHEMA_FILE: &str = "polypack-adaptive.xsd";

/// 静态库文件名（宿主自包含 `libpoly_core.a` 的打包副本）。
const LIBRARY: &str = "libpolypack_engine_autosar.a";

/// AUTOSAR r4.0 命名空间。
const NS: &str = "http://autosar.org/schema/r4.0";

/// AR-PACKAGE 短名（端口引用 `/PolyPackEngine/<symbol>`）。
const PACKAGE: &str = "PolyPackEngine";

/// Adaptive 静态库 + ARXML 打包器。
pub struct AutosarAdaptive;

/// ARXML 结构检查结果（打包器自检与 verify 腿共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArxmlStructure {
    /// ARXML 文件。
    pub path: PathBuf,
    /// 子集 XSD 文件。
    pub schema: PathBuf,
    /// 服务接口短名（= C ABI 符号）。
    pub services: Vec<String>,
    /// 方法短名（与 `services` 逐位相同）。
    pub methods: Vec<String>,
    /// 提供的服务实例短名（`<symbol>Port`）。
    pub ports: Vec<String>,
    /// 服务接口引用（`/PolyPackEngine/<symbol>`）。
    pub refs: Vec<String>,
}

impl Packager for AutosarAdaptive {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec)?;
        inputs.product.validate(ID)?;
        let embedded = embedded::require(inputs, ID)?;
        for symbol in &embedded.symbols {
            if !symbol_is_valid(symbol) {
                return Err(BundleError::InvalidInput {
                    packager: ID,
                    field: "embedded.symbols",
                    value: symbol.clone(),
                    reason: "C ABI 符号须符合 AUTOSAR SHORT-NAME（字母开头，字母数字 / 下划线）"
                        .to_string(),
                });
            }
        }
        let engine_dir = engine_target_dir(inputs);
        Ok(BundlePlan::new()
            .with_script(arxml_path(inputs), arxml_text(inputs, embedded))
            .with_script(arxml_parent(inputs).join(SCHEMA_FILE), SCHEMA)
            .with_command(embedded::host_staticlib_command(
                &embedded.engine_crate,
                &embedded.host_triple,
                &engine_dir,
                embedded.no_std,
            )))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let embedded = embedded::require(inputs, ID)?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        let built =
            embedded::host_staticlib_path(&engine_target_dir(inputs), &embedded.host_triple);
        if !built.is_file() {
            return Err(BundleError::MissingInput {
                packager: ID,
                field: "autosar staticlib（cargo rustc --crate-type staticlib 未产 libpoly_core.a）",
            });
        }
        let library = inputs.out_dir.join("lib").join(LIBRARY);
        fsutil::copy_file(&built, &library, false)?;

        let structure = check_arxml(&inputs.out_dir)?;
        let mut want = embedded.symbols.clone();
        want.sort();
        want.dedup();
        if structure.services != want {
            return Err(bad(
                &structure.path,
                format!(
                    "ARXML 服务 {:?} 与 C ABI 符号 {want:?} 不一一对应",
                    structure.services
                ),
            ));
        }
        let mut notes = vec![format!(
            "AUTOSAR Adaptive：无 UI、自部署；静态库 lib/{LIBRARY}；ARXML 服务 {} 个，\
             服务 / 端口与 poly_engine_* 导出符号一一对应",
            structure.services.len()
        )];
        notes.push(nm_note(&library, embedded)?);
        notes.extend(xmllint_note(&structure)?);
        if embedded.no_std {
            notes.push(
                "引擎层 no_std（--no-default-features；应用提供 global_allocator / panic_handler）"
                    .to_string(),
            );
        } else {
            notes
                .push("引擎层 std（宿主默认；需 no_std 时设 $POLYPACK_ENGINE_NO_STD）".to_string());
        }
        notes.push(
            "嵌入 XSD 为 AUTOSAR r4.0 最小子集（服务 / 端口映射），非官方全量 XSD；\
             ARXML 是自适应部署清单，不是 ECU 配置描述"
                .to_string(),
        );
        fsutil::file_artifact(ID, FORMAT, &structure.path, notes)
    }
}

/// 结构检查：ARXML 与 XSD 在场、服务 / 方法 / 端口一一对应。
pub fn check_arxml(dir: &Path) -> Result<ArxmlStructure, BundleError> {
    let parent = dir.join("autosar");
    let arxml = find_single(&parent, "arxml")?;
    let schema = parent.join(SCHEMA_FILE);
    if !schema.is_file() {
        return Err(bad(&schema, "缺少嵌入的 ARXML 子集 XSD".to_string()));
    }
    let xml = std::fs::read_to_string(&arxml).map_err(crate::error::io_err(&arxml))?;
    let mut services = Vec::new();
    let mut methods = Vec::new();
    for block in blocks(&xml, "SERVICE-INTERFACE") {
        services.push(short_name(block).ok_or_else(|| bad(&arxml, "服务缺 SHORT-NAME".into()))?);
        let method = blocks(block, "METHOD")
            .into_iter()
            .next()
            .ok_or_else(|| bad(&arxml, "服务缺 METHOD".into()))?;
        methods.push(short_name(method).ok_or_else(|| bad(&arxml, "方法缺 SHORT-NAME".into()))?);
    }
    let mut ports = Vec::new();
    let mut refs = Vec::new();
    for block in blocks(&xml, "PROVIDED-SERVICE-INSTANCE") {
        ports.push(short_name(block).ok_or_else(|| bad(&arxml, "服务实例缺 SHORT-NAME".into()))?);
        refs.push(
            inner(block, "PROVIDED-INTERFACE-TREF")
                .ok_or_else(|| bad(&arxml, "服务实例缺 PROVIDED-INTERFACE-TREF".into()))?,
        );
    }
    for (what, values) in [("服务", &services), ("端口", &ports), ("方法", &methods)] {
        if values.is_empty() {
            return Err(bad(&arxml, format!("ARXML 无{what}声明")));
        }
        let mut unique = values.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != values.len() {
            return Err(bad(&arxml, format!("ARXML {what}短名重复：{values:?}")));
        }
    }
    if services.len() != ports.len() || services.len() != methods.len() {
        return Err(bad(
            &arxml,
            format!(
                "服务 {} / 方法 {} / 端口 {} 数量不一致",
                services.len(),
                methods.len(),
                ports.len()
            ),
        ));
    }
    for index in 0..services.len() {
        let service = &services[index];
        if methods[index] != *service {
            return Err(bad(
                &arxml,
                format!("方法 {} 与服务 {service} 不同名", methods[index]),
            ));
        }
        if ports[index] != format!("{service}Port") {
            return Err(bad(
                &arxml,
                format!("端口 {} 未按 {service}Port 命名", ports[index]),
            ));
        }
        if refs[index] != format!("/{PACKAGE}/{service}") {
            return Err(bad(
                &arxml,
                format!("端口引用 {} 未指向 /{PACKAGE}/{service}", refs[index]),
            ));
        }
    }
    Ok(ArxmlStructure {
        path: arxml,
        schema,
        services,
        methods,
        ports,
        refs,
    })
}

/// `xmllint` 可执行（缺席 → `None`，调用方按 SKIP-with-print 注记）。
pub fn xmllint_tool() -> Option<PathBuf> {
    exec::tool_path("xmllint")
}

/// `xmllint --noout --schema <xsd> <arxml>` 命令（打包器与 verify 腿同一构造）。
pub fn xmllint_command(arxml: &Path, schema: &Path) -> Command {
    Command::new(
        "xmllint",
        [
            "--noout".to_string(),
            "--schema".to_string(),
            schema.display().to_string(),
            arxml.display().to_string(),
        ],
    )
}

/// 嵌入的 AUTOSAR r4.0 最小子集 XSD（仅本清单用到的元素 / 顺序）。
pub const SCHEMA: &str = r###"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://autosar.org/schema/r4.0"
           xmlns="http://autosar.org/schema/r4.0"
           elementFormDefault="qualified">
  <xs:element name="AUTOSAR">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="AR-PACKAGES">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="AR-PACKAGE" maxOccurs="unbounded">
                <xs:complexType>
                  <xs:sequence>
                    <xs:element name="SHORT-NAME" type="xs:string"/>
                    <xs:element name="ELEMENTS">
                      <xs:complexType>
                        <xs:sequence>
                          <xs:element name="SERVICE-INTERFACE" maxOccurs="unbounded">
                            <xs:complexType>
                              <xs:sequence>
                                <xs:element name="SHORT-NAME" type="xs:string"/>
                                <xs:element name="METHODS">
                                  <xs:complexType>
                                    <xs:sequence>
                                      <xs:element name="METHOD" maxOccurs="unbounded">
                                        <xs:complexType>
                                          <xs:sequence>
                                            <xs:element name="SHORT-NAME" type="xs:string"/>
                                          </xs:sequence>
                                        </xs:complexType>
                                      </xs:element>
                                    </xs:sequence>
                                  </xs:complexType>
                                </xs:element>
                              </xs:sequence>
                            </xs:complexType>
                          </xs:element>
                          <xs:element name="PROVIDED-SERVICE-INSTANCE" maxOccurs="unbounded">
                            <xs:complexType>
                              <xs:sequence>
                                <xs:element name="SHORT-NAME" type="xs:string"/>
                                <xs:element name="PROVIDED-INTERFACE-TREF">
                                  <xs:complexType>
                                    <xs:simpleContent>
                                      <xs:extension base="xs:string">
                                        <xs:attribute name="DEST" type="xs:string" use="required"/>
                                      </xs:extension>
                                    </xs:simpleContent>
                                  </xs:complexType>
                                </xs:element>
                              </xs:sequence>
                            </xs:complexType>
                          </xs:element>
                        </xs:sequence>
                      </xs:complexType>
                    </xs:element>
                  </xs:sequence>
                </xs:complexType>
              </xs:element>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
      <xs:anyAttribute namespace="##other" processContents="lax"/>
    </xs:complexType>
  </xs:element>
</xs:schema>
"###;

fn arxml_text(inputs: &BundleInputs, embedded: &EmbeddedInputs) -> String {
    let mut symbols = embedded.symbols.clone();
    symbols.sort();
    symbols.dedup();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(
        "<!-- Generated by PolyPack T-M6-04 from the poly_engine_* C ABI single source. -->\n",
    );
    xml.push_str(&format!(
        "<AUTOSAR xmlns=\"{NS}\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:schemaLocation=\"{NS} {SCHEMA_FILE}\">\n"
    ));
    xml.push_str("  <AR-PACKAGES>\n    <AR-PACKAGE>\n");
    xml.push_str(&format!("      <SHORT-NAME>{PACKAGE}</SHORT-NAME>\n"));
    xml.push_str(&format!(
        "      <!-- {} v{}: 无 UI、自部署 -->\n      <ELEMENTS>\n",
        xml_escape(&inputs.product.name),
        xml_escape(&inputs.product.version)
    ));
    for symbol in &symbols {
        xml.push_str("        <SERVICE-INTERFACE>\n");
        xml.push_str(&format!("          <SHORT-NAME>{symbol}</SHORT-NAME>\n"));
        xml.push_str("          <METHODS>\n            <METHOD>\n");
        xml.push_str(&format!(
            "              <SHORT-NAME>{symbol}</SHORT-NAME>\n"
        ));
        xml.push_str("            </METHOD>\n          </METHODS>\n");
        xml.push_str("        </SERVICE-INTERFACE>\n");
    }
    for symbol in &symbols {
        xml.push_str("        <PROVIDED-SERVICE-INSTANCE>\n");
        xml.push_str(&format!(
            "          <SHORT-NAME>{symbol}Port</SHORT-NAME>\n"
        ));
        xml.push_str(&format!(
            "          <PROVIDED-INTERFACE-TREF DEST=\"SERVICE-INTERFACE\">/{PACKAGE}/{symbol}\
             </PROVIDED-INTERFACE-TREF>\n"
        ));
        xml.push_str("        </PROVIDED-SERVICE-INSTANCE>\n");
    }
    xml.push_str("      </ELEMENTS>\n    </AR-PACKAGE>\n  </AR-PACKAGES>\n</AUTOSAR>\n");
    xml
}

fn check_format(spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format == FORMAT {
        Ok(())
    } else {
        Err(BundleError::InvalidInput {
            packager: ID,
            field: "package.format",
            value: spec.package.format.clone(),
            reason: format!("AUTOSAR 打包器只服务 format = {FORMAT}"),
        })
    }
}

fn nm_note(library: &Path, embedded: &EmbeddedInputs) -> Result<String, BundleError> {
    let Some(found) = embedded::nm_engine_symbols(library) else {
        return Ok(
            "nm 不在场 / 失败：归档符号对照 SKIP-with-print（ARXML 结构断言仍一一对应）"
                .to_string(),
        );
    };
    let mut want = embedded.symbols.clone();
    want.sort();
    want.dedup();
    if found != want {
        return Err(bad(
            library,
            format!("归档导出符号 {found:?} 与 C ABI 清单 {want:?} 不一致"),
        ));
    }
    Ok(format!(
        "nm 真跑：归档 {} 个 poly_engine_* 导出符号与清单一一对应",
        found.len()
    ))
}

fn xmllint_note(structure: &ArxmlStructure) -> Result<Vec<String>, BundleError> {
    let Some(tool) = xmllint_tool() else {
        return Ok(vec![
            "xmllint 不在场：ARXML schema 校验 SKIP-with-print（结构断言已过：服务 / 端口 / \
             方法与 C ABI 一一对应）"
                .to_string(),
        ]);
    };
    let mut command = xmllint_command(&structure.path, &structure.schema);
    command.program = tool.display().to_string();
    exec::run(&command)?;
    Ok(vec![
        "xmllint 真跑：ARXML 通过嵌入的 AUTOSAR r4.0 子集 XSD 校验".to_string(),
    ])
}

fn arxml_parent(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("autosar")
}

fn arxml_path(inputs: &BundleInputs) -> PathBuf {
    arxml_parent(inputs).join(format!("{}.arxml", inputs.product.kebab_name()))
}

fn engine_target_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("autosar/engine-target")
}

fn find_single(dir: &Path, ext: &str) -> Result<PathBuf, BundleError> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(crate::error::io_err(dir))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == ext))
        .collect();
    found.sort();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(bad(dir, format!("缺少 .{ext} 产物"))),
        _ => Err(bad(dir, format!(".{ext} 产物不唯一：{found:?}"))),
    }
}

fn blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let from = &rest[start + open.len()..];
        let Some(end) = from.find(&close) else {
            break;
        };
        out.push(&from[..end]);
        rest = &from[end + close.len()..];
    }
    out
}

fn short_name(block: &str) -> Option<String> {
    inner(block, "SHORT-NAME")
}

fn inner(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let open_start = xml.find(&open)?;
    let start = open_start + xml[open_start..].find('>')? + 1;
    let rest = &xml[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim().to_string())
}

fn symbol_is_valid(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn bad(path: &Path, reason: String) -> BundleError {
    BundleError::Archive {
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(test)]
#[path = "autosar_tests.rs"]
mod tests;
