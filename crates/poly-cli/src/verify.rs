//! `poly verify --reproducible`：同输入二跑的可复现比对（T-M2-06 验收口径）。
//!
//! 口径（最懒且够用）：只比对**本机宿主可达**的目标（`spec.hosts` 含 `current_host()`）；
//! 同一份输入组装两次、落到两个不同的绝对目录
//! （`<root>/.polypack/repro/<target>/{a,b}`），各自 `package()` 后比产物摘要：
//! 文件产物的 `sha384-<base64>`（SRI），目录产物逐文件比（`sri::manifest_of`）。
//!
//! 跳过语义与 `package` 阶段一致（宿主缺打包工具 / 输入未就绪 → 告警跳过并说明，不硬失败）；
//! 但**全部目标都跳过**时以 `poly::E_VERIFY_0002` 报错——「一个都没比却报绿」是假绿。
//! 已签名的 zip 产物在比对前如实说明并跳过该件（签名会改写 zip 字节，比了也不是「输入决定
//! 产物」的复现性）；`SOURCE_DATE_EPOCH` 未设置时先告警：含时间戳的产物二跑可能不一致。
//!
//! 输入组装与 `poly build` 的 package 阶段共用同一份实现（`crate::package::assemble_inputs`），
//! 「verify 比对的两跑」才和「真实构建的一跑」口径相同；`Ok(None)` = 环境性缺失
//! （该函数已发告警并清掉暂存目录），本命令跳过该目标。

use std::path::{Path, PathBuf};

use poly_bundle::error::BundleError;
use poly_bundle::packager::{Artifact, ArtifactKind};
use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::events::{BuildEvent, EventSink};
use poly_core::{Profile, Project, codes};
use poly_targets::{Host, Registry, TargetRequest, TargetSpec, current_host};

use crate::project;

/// 签名块判定：`META-INF/` 下的 `*.SF` / `*.RSA` / `*.DSA` / `*.EC`（zip 签名约定）。
const SIGNATURE_EXTENSIONS: [&str; 4] = [".SF", ".RSA", ".DSA", ".EC"];

/// `poly verify` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 可复现比对：同一输入二跑，逐产物比 SRI 摘要（T-M2-06）。
    #[arg(long)]
    pub reproducible: bool,
    /// 目标 id（可重复；缺省 = 当前宿主可达的全部已声明目标）。
    #[arg(long = "target", value_name = "ID")]
    pub targets: Vec<String>,
}

/// 执行 `poly verify`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    if !args.reproducible {
        return Err(Diagnostic::error(
            codes::E_VERIFY_0002,
            "poly verify 目前只接入 --reproducible 校验器（bundletool / codesign 等见 T-M2-07 / T-M2-10）",
        )
        .with_hint("用法：poly verify --reproducible [--target ID]…"));
    }

    let project = project::load(&args.config)?;
    let registry = Registry::builtin();
    let Some(host) = current_host() else {
        return Err(Diagnostic::error(
            codes::E_VERIFY_0002,
            "无法识别本机宿主：复现比对只对本机宿主目标成立（交叉打包归 M2 之后）",
        ));
    };
    let hosts = vec![host];
    let request = if args.targets.is_empty() || args.targets.iter().any(|id| id == "all") {
        TargetRequest::All
    } else {
        TargetRequest::Explicit(args.targets.clone())
    };
    let job = project
        .config
        .to_job(&registry, &request, Profile::Local, &hosts)
        .map_err(|err| err.diagnostic)?;

    let mut events = project::sink(json);
    if std::env::var_os("SOURCE_DATE_EPOCH").is_none() {
        warning(
            events.as_mut(),
            "SOURCE_DATE_EPOCH 未设置：含时间戳的产物在二跑间可能不一致，比对结果会如实报出差异（设 `export SOURCE_DATE_EPOCH=<秒>` 后重跑）",
        );
    }

    let mut compared = 0usize;
    for spec in &job.targets {
        if !spec.hosts.contains(&host) {
            warning(
                events.as_mut(),
                &format!(
                    "目标 {} 未做复现比对：非本机宿主（交叉打包的复现比对归 M2 之后）",
                    spec.id
                ),
            );
            continue;
        }
        if compare(&project, spec, host, json, events.as_mut())? {
            compared += 1;
        }
    }
    if compared == 0 {
        return Err(Diagnostic::error(
            codes::E_VERIFY_0002,
            format!(
                "{} 个入选目标都没有可复现比对的产物（逐目标跳过原因见上方告警）",
                job.targets.len()
            ),
        )
        .with_hint(
            "先跑 poly build（本机宿主目标）把打包输入就绪，或补上缺失的打包工具；\
             也可用 --target 只比已就绪的目标",
        ));
    }
    Ok(0)
}

/// 对单个目标做二跑比对；返回是否真的比对了（`false` = 告警跳过并已说明原因）。
fn compare(
    project: &Project,
    spec: &TargetSpec,
    host: Host,
    json: bool,
    events: &mut dyn EventSink,
) -> Result<bool, Diagnostic> {
    let Some(packager) = poly_bundle::for_spec(spec) else {
        warning(
            events,
            &format!(
                "目标 {} 未做复现比对：无内建打包器（format = {}）",
                spec.id, spec.package.format
            ),
        );
        return Ok(false);
    };

    // 每一跑一个独立绝对目录；同一目标的旧证据先清掉，比对后的产物留作证据
    let root = project.root.join(".polypack").join("repro").join(&spec.id);
    std::fs::remove_dir_all(&root).ok();

    let mut runs = Vec::new();
    for tag in ["a", "b"] {
        let out_dir = root.join(tag);
        // 输入组装与 package 阶段共用同一实现（同一份输入，两次独立落点）；
        // `None` = 环境性缺失，该函数已告警并清掉暂存目录，跳过本目标即可
        let Some(inputs) = crate::package::assemble_inputs(project, spec, host, &out_dir, events)?
        else {
            return Ok(false);
        };
        match packager.package(&inputs, spec) {
            Ok(artifact) => runs.push(artifact),
            // 宿主缺打包工具 / 输入未就绪：与 package 阶段同语义，告警跳过本目标
            Err(err @ (BundleError::ToolMissing { .. } | BundleError::MissingInput { .. })) => {
                warning(events, &format!("目标 {} 未做复现比对：{err}", spec.id));
                return Ok(false);
            }
            Err(other) => return Err(bundle_diagnostic(spec, &other)),
        }
    }

    let (a, b) = (&runs[0], &runs[1]);
    // 签名会改写 zip 字节：已签名的产物不比字节，如实说明并跳过该件（绝不假装通过）
    if let Some(entry) = signed_entry(a).or_else(|| signed_entry(b)) {
        warning(
            events,
            &format!(
                "目标 {} 未做复现比对：产物已签名（{entry}）——签名块与时间戳会改写 zip 字节，\
                 复现比对只对未签名件成立（签名位在 T-M2-09 已接入，比对请用打包暂存目录里的未签名件）",
                spec.id
            ),
        );
        return Ok(false);
    }

    match compare_artifacts(a, b) {
        Ok(digest) => {
            project::report(
                "verify",
                json,
                serde_json::json!({
                    "event": "verify-reproducible",
                    "target": spec.id,
                    "format": a.format,
                    "kind": kind_name(a.kind),
                    "digest": digest,
                    "match": true,
                }),
            );
            Ok(true)
        }
        Err(problems) => {
            project::report(
                "verify",
                json,
                serde_json::json!({
                    "event": "verify-reproducible",
                    "target": spec.id,
                    "format": a.format,
                    "kind": if a.kind == b.kind { kind_name(a.kind) } else { "mismatch" },
                    "match": false,
                    "mismatches": problems,
                }),
            );
            Err(Diagnostic::error(
                codes::E_VERIFY_0001,
                format!(
                    "目标 {} 二跑产物不一致（{} 项，输入相同即应字节一致）",
                    spec.id,
                    problems.len()
                ),
            )
            .with_hint(format!(
                "不一致项：{}；证据在 {}",
                problems.join("；"),
                root.display()
            )))
        }
    }
}

/// 比对二跑产物：一致 → `Ok(摘要)`；不一致 → `Err(不一致项清单)`。
///
/// 文件产物比整串 `sha384-<base64>`；目录产物逐文件比 SRI 摘要与文件集
/// （缺失 / 摘要不符 / 清单外新增），汇总摘要取「路径 + 摘要」行的 SRI（稳定序）。
fn compare_artifacts(a: &Artifact, b: &Artifact) -> Result<String, Vec<String>> {
    match (a.kind, b.kind) {
        (ArtifactKind::File, ArtifactKind::File) => {
            let digest_a = match digest_of_file(&a.path) {
                Ok(digest) => digest,
                Err(err) => return Err(vec![err]),
            };
            let digest_b = match digest_of_file(&b.path) {
                Ok(digest) => digest,
                Err(err) => return Err(vec![err]),
            };
            if digest_a == digest_b {
                Ok(digest_a)
            } else {
                Err(vec![format!(
                    "{} 与 {} 内容不同：a {digest_a}，b {digest_b}",
                    a.path.display(),
                    b.path.display()
                )])
            }
        }
        (ArtifactKind::Directory, ArtifactKind::Directory) => {
            let manifest_a = match poly_bundle::sri::manifest_of(&a.path) {
                Ok(manifest) => manifest,
                Err(err) => return Err(vec![format!("读取 a 清单失败：{err}")]),
            };
            let manifest_b = match poly_bundle::sri::manifest_of(&b.path) {
                Ok(manifest) => manifest,
                Err(err) => return Err(vec![format!("读取 b 清单失败：{err}")]),
            };
            let mut problems = Vec::new();
            for (path, entry) in &manifest_a.files {
                match manifest_b.files.get(path) {
                    None => problems.push(format!("b 缺失: {path}")),
                    Some(found) if found.integrity != entry.integrity => problems.push(format!(
                        "摘要不符: {path}（a {}，b {}）",
                        entry.integrity, found.integrity
                    )),
                    Some(_) => {}
                }
            }
            for path in manifest_b.files.keys() {
                if !manifest_a.files.contains_key(path) {
                    problems.push(format!("b 清单外新增: {path}"));
                }
            }
            if !problems.is_empty() {
                return Err(problems);
            }
            let canonical: String = manifest_a
                .files
                .iter()
                .map(|(path, entry)| format!("{path} {}\n", entry.integrity))
                .collect();
            Ok(poly_bundle::sri::integrity_of(canonical.as_bytes()))
        }
        (kind_a, kind_b) => Err(vec![format!(
            "产物形态不一致：a {:?}，b {:?}",
            kind_a, kind_b
        )]),
    }
}

/// 文件产物的 SRI 摘要（失败返回人读原因，进不一致清单）。
fn digest_of_file(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(poly_bundle::sri::integrity_of(&bytes)),
        Err(err) => Err(format!("{} 读取失败：{err}", file_label(path))),
    }
}

/// zip 产物里的签名块条目（`META-INF/*.SF|*.RSA|*.DSA|*.EC`）；非 zip / 未签名 → `None`。
fn signed_entry(artifact: &Artifact) -> Option<String> {
    if artifact.kind != ArtifactKind::File {
        return None;
    }
    let names = poly_bundle::zipwrite::entry_names(&artifact.path).ok()?;
    names.into_iter().find(|name| {
        let upper = name.to_ascii_uppercase();
        upper.starts_with("META-INF/")
            && SIGNATURE_EXTENSIONS
                .iter()
                .any(|extension| upper.ends_with(extension))
    })
}

/// 产物形态名（JSON 报告的 `kind` 字段）。
fn kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::File => "file",
        ArtifactKind::Directory => "directory",
    }
}

/// 文件名（报告里比全路径短，且两跑路径必然不同）。
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// `BundleError` → 诊断（错误码原样透传，`poly::E_BND_*`）。
fn bundle_diagnostic(spec: &TargetSpec, err: &BundleError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        Code::new(err.code()),
        format!("目标 {} 打包失败：{err}", spec.id),
    );
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

/// 告警（跳过原因如实进事件流）。
fn warning(events: &mut dyn EventSink, message: &str) {
    events.emit(&BuildEvent::Warning {
        stage: "verify".to_string(),
        message: message.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 目录产物比对的三类不一致都要点名（缺失 / 摘要不符 / 清单外新增），一致时给汇总摘要。
    #[test]
    fn directory_comparison_names_every_kind_of_difference() {
        let root = std::env::temp_dir().join(format!("polypack-verify-cmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir_a, dir_b) = (root.join("a"), root.join("b"));
        for dir in [&dir_a, &dir_b] {
            std::fs::create_dir_all(dir).expect("暂存目录可建");
        }
        std::fs::write(dir_a.join("index.html"), b"one").expect("写 a");
        std::fs::write(dir_b.join("index.html"), b"one").expect("写 b");
        let artifact = |path: &Path| {
            poly_bundle::fsutil::dir_artifact("web", "static-site", path, Vec::new())
                .expect("构造目录产物")
        };
        let (a, b) = (artifact(&dir_a), artifact(&dir_b));

        let digest = compare_artifacts(&a, &b).expect("同输入二跑应一致");
        assert!(digest.starts_with("sha384-"), "{digest}");

        std::fs::write(dir_b.join("index.html"), b"two").expect("改写 b");
        let problems = compare_artifacts(&a, &b).expect_err("内容不同应报不一致");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("摘要不符: index.html"), "{problems:?}");

        std::fs::write(dir_b.join("index.html"), b"one").expect("还原 b");
        std::fs::write(dir_b.join("extra.txt"), b"x").expect("b 多写一件");
        let problems = compare_artifacts(&a, &b).expect_err("清单外新增应报不一致");
        assert!(
            problems[0].contains("b 清单外新增: extra.txt"),
            "{problems:?}"
        );

        std::fs::remove_file(dir_b.join("extra.txt")).expect("清掉多余件");
        std::fs::write(dir_a.join("only-a.txt"), b"y").expect("a 多写一件");
        let problems = compare_artifacts(&a, &b).expect_err("b 缺件应报不一致");
        assert!(problems[0].contains("b 缺失: only-a.txt"), "{problems:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 文件产物（apk / ipa / 安装包）比整串 SRI：一致给摘要，不一致两名摘要都要报出。
    #[test]
    fn file_comparison_reports_both_digests() {
        let root =
            std::env::temp_dir().join(format!("polypack-verify-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("暂存目录可建");
        let (path_a, path_b) = (root.join("a.apk"), root.join("b.apk"));
        std::fs::write(&path_a, b"same").expect("写 a");
        std::fs::write(&path_b, b"same").expect("写 b");
        let artifact = |path: &Path| Artifact {
            target: "android".to_string(),
            format: "apk".to_string(),
            path: path.to_path_buf(),
            bytes: 0,
            kind: ArtifactKind::File,
            notes: Vec::new(),
        };

        assert!(
            compare_artifacts(&artifact(&path_a), &artifact(&path_b))
                .expect("同内容应一致")
                .starts_with("sha384-")
        );
        std::fs::write(&path_b, b"other").expect("改写 b");
        let problems = compare_artifacts(&artifact(&path_a), &artifact(&path_b))
            .expect_err("内容不同应报不一致");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("a.apk"), "{problems:?}");
        assert!(problems[0].contains("b.apk"), "{problems:?}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
