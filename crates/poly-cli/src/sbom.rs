//! M9-E1：产物 SBOM（SPDX 2.3 / CycloneDX 1.5）与 in-toto / SLSA 风格 provenance。
//!
//! 每个产物生成 `<artifact>.spdx.json`、`<artifact>.cdx.json`、`<artifact>.intoto.jsonl`。
//! 依赖图来自 `cargo metadata --format-version 1 --locked --offline`，逐包按 name / version /
//! source 与 `Cargo.lock` 对齐。时间戳固定为 0（1970-01-01T00:00:00Z），包与 JSON 键均排序，
//! 故同一输入二跑字节一致。provenance 是自产、可离线复核的构建来源说明：**不是 SLSA 认证**，
//! 不接 Sigstore / Rekor，也不声明任何 SLSA 等级。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use poly_cache::Digest;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// 依赖图包数上限（避免清单无界膨胀）。
const MAX_PACKAGES: usize = 2000;
/// 固定时间戳（确定性：不读时钟、不读 SOURCE_DATE_EPOCH）。
const CREATED: &str = "1970-01-01T00:00:00Z";
/// 源树摘要排除项（构建产物 / 缓存 / VCS / 依赖目录）。
const TREE_EXCLUDES: [&str; 4] = ["target", ".polypack", ".git", "node_modules"];

/// SBOM 生成错误（消息含可执行的下一步）。
#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// 与 `Cargo.lock` 对齐后的依赖包。
struct Package {
    name: String,
    version: String,
    source: Option<String>,
}

/// 一次 SBOM 生成的项目级输入。
pub struct Inputs<'a> {
    /// PolyPack 工程根（`polypack.toml` 所在目录）。
    pub project_root: &'a Path,
    /// `[core].crate`：核心 crate 相对工程根的路径。
    pub core_crate: &'a str,
    /// `[package].name`。
    pub name: &'a str,
    /// `[package].version`。
    pub version: &'a str,
    /// 目标 id。
    pub target: &'a str,
    /// 目标声明的序列化文本（材料摘要的输入）。
    pub target_spec: &'a str,
    /// 构建 profile（`local` / `ci`）。
    pub profile: &'a str,
    /// 触发本次生成的人类可读命令（provenance invocation）。
    pub invocation: &'a str,
}

/// 与产物无关的生成上下文：依赖图、材料摘要、工具链身份只算一次。
pub struct Context {
    name: String,
    version: String,
    target: String,
    target_spec: String,
    profile: String,
    invocation: String,
    toolchain: String,
    builder: String,
    packages: Vec<Package>,
    lockfile_sha256: String,
    lockfile_blake3: String,
    source_tree_blake3: String,
}

/// 三件套的落点与产物摘要。
#[derive(Debug)]
pub struct Outputs {
    /// SPDX 2.3 JSON 路径。
    pub spdx: PathBuf,
    /// CycloneDX 1.5 JSON 路径。
    pub cdx: PathBuf,
    /// provenance JSONL 路径。
    pub provenance: PathBuf,
    /// 产物 blake3（文件字节 / 目录树）。
    pub artifact_blake3: String,
}

impl Context {
    /// 装载依赖图与材料摘要（`cargo metadata` / `rustc` 均离线执行）。
    pub fn load(inputs: Inputs<'_>) -> Result<Self, Error> {
        let manifest = manifest_path(inputs.project_root, inputs.core_crate)?;
        let metadata = cargo_metadata(&manifest)?;
        let root = metadata["workspace_root"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("cargo metadata 输出缺 workspace_root"))?;
        let lock_path = root.join("Cargo.lock");
        let lock_bytes = std::fs::read(&lock_path)
            .map_err(|err| Error::new(format!("读 Cargo.lock 失败：{err}")))?;
        let packages = metadata_packages(
            &metadata,
            &lock_packages(&String::from_utf8_lossy(&lock_bytes)),
        )?;
        if packages.len() > MAX_PACKAGES {
            return Err(Error::new(format!(
                "依赖图有 {} 个包，超过 SBOM 上限 {MAX_PACKAGES}",
                packages.len()
            )));
        }
        let tree = Digest::of_tree(inputs.project_root, &TREE_EXCLUDES)
            .map_err(|err| Error::new(format!("工程源树摘要失败：{err}")))?;
        Ok(Self {
            name: inputs.name.to_string(),
            version: inputs.version.to_string(),
            target: inputs.target.to_string(),
            target_spec: inputs.target_spec.to_string(),
            profile: inputs.profile.to_string(),
            invocation: inputs.invocation.to_string(),
            toolchain: rustc_version(),
            builder: builder_identity(),
            packages,
            lockfile_sha256: sha256_hex(&lock_bytes),
            lockfile_blake3: Digest::of_bytes(&lock_bytes).to_string(),
            source_tree_blake3: tree.to_string(),
        })
    }

    /// 为单个产物生成三件套；产物不存在 / 不可读时返回明确错误。
    pub fn generate(&self, artifact: &Path, out_dir: &Path) -> Result<Outputs, Error> {
        let id = identify(artifact)?;
        let spdx = self.spdx_json(&id);
        let cdx = self.cdx_json(&id);
        let provenance = self.provenance_json(&id);
        std::fs::create_dir_all(out_dir)
            .map_err(|err| Error::new(format!("创建 SBOM 目录失败：{err}")))?;
        let spdx_path = out_dir.join(format!("{}.spdx.json", id.name));
        let cdx_path = out_dir.join(format!("{}.cdx.json", id.name));
        let provenance_path = out_dir.join(format!("{}.intoto.jsonl", id.name));
        write_json(&spdx_path, &spdx)?;
        write_json(&cdx_path, &cdx)?;
        let line = serde_json::to_string(&provenance)
            .map_err(|err| Error::new(format!("provenance 序列化失败：{err}")))?;
        std::fs::write(&provenance_path, format!("{line}\n"))
            .map_err(|err| Error::new(format!("写 provenance 失败：{err}")))?;
        Ok(Outputs {
            spdx: spdx_path,
            cdx: cdx_path,
            provenance: provenance_path,
            artifact_blake3: id.blake3,
        })
    }

    fn spdx_json(&self, art: &ArtifactId) -> Value {
        let mut checksums = Vec::new();
        if let Some(sha256) = &art.sha256 {
            checksums.push(json!({"algorithm": "SHA256", "checksumValue": sha256}));
        }
        checksums.push(json!({"algorithm": "BLAKE3", "checksumValue": art.blake3}));
        let mut packages = vec![spdx_package(
            "SPDXRef-Artifact".to_string(),
            &art.name,
            &self.version,
            Some(Value::Array(checksums)),
        )];
        for (index, pkg) in self.packages.iter().enumerate() {
            let mut package = spdx_package(
                format!("SPDXRef-Package-{index}"),
                &pkg.name,
                &pkg.version,
                None,
            );
            package["externalRefs"] = json!([purl_ref(&purl(pkg))]);
            packages.push(package);
        }
        json!({
            "spdxVersion": "SPDX-2.3", "dataLicense": "CC0-1.0", "SPDXID": "SPDXRef-DOCUMENT",
            "name": format!("{}-{}-{}", self.name, self.version, art.name),
            "documentNamespace": format!("https://polypack.dev/spdx/{}",
                art.sha256.as_deref().unwrap_or(&art.blake3)),
            "creationInfo": {"created": CREATED,
                "creators": [format!("Tool: poly-{}", env!("CARGO_PKG_VERSION"))]},
            "documentDescribes": ["SPDXRef-Artifact"], "packages": packages,
        })
    }

    fn cdx_json(&self, art: &ArtifactId) -> Value {
        let components: Vec<Value> = self
            .packages
            .iter()
            .enumerate()
            .map(|(index, pkg)| {
                let package_url = purl(pkg);
                json!({"type": "library", "bom-ref": format!("{package_url}#{index}"),
                    "name": pkg.name, "version": pkg.version, "purl": package_url})
            })
            .collect();
        let mut hashes = Vec::new();
        if let Some(sha256) = &art.sha256 {
            hashes.push(json!({"alg": "SHA-256", "content": sha256}));
        }
        hashes.push(json!({"alg": "BLAKE3", "content": art.blake3}));
        json!({
            "bomFormat": "CycloneDX", "specVersion": "1.5",
            "serialNumber": deterministic_urn(art), "version": 1,
            "metadata": {
                "timestamp": CREATED,
                "tools": {"components": [{"type": "application", "name": "poly",
                    "version": env!("CARGO_PKG_VERSION")}]},
                "component": {"type": "application", "bom-ref": self.project_purl(),
                    "name": self.name, "version": self.version,
                    "purl": self.project_purl(), "hashes": hashes},
            },
            "components": components,
        })
    }

    fn provenance_json(&self, art: &ArtifactId) -> Value {
        let mut subject_digests = serde_json::Map::new();
        if let Some(sha256) = &art.sha256 {
            subject_digests.insert("sha256".into(), Value::String(sha256.clone()));
        }
        subject_digests.insert("blake3".into(), Value::String(art.blake3.clone()));
        let materials = vec![
            json!({"uri": "Cargo.lock", "digest": {"sha256": self.lockfile_sha256,
                "blake3": self.lockfile_blake3}}),
            json!({"uri": "source-tree", "digest": {"blake3": self.source_tree_blake3}}),
            json!({"uri": format!("target:{}", self.target),
                "digest": {"sha256": sha256_hex(self.target_spec.as_bytes())}}),
            json!({"uri": "toolchain:rustc",
                "digest": {"sha256": sha256_hex(self.toolchain.as_bytes())}}),
            json!({"uri": format!("profile:{}", self.profile),
                "digest": {"sha256": sha256_hex(self.profile.as_bytes())}}),
        ];
        json!({
            "_type": "https://in-toto.io/Statement/v1",
            "subject": [{"name": art.name, "digest": subject_digests}],
            "predicateType": "https://slsa.dev/provenance/v1",
            "predicate": {
                "buildDefinition": {
                    "buildType": "https://polypack.dev/build/v1",
                    "externalParameters": {"artifact": art.name,
                        "invocation": self.invocation, "target": self.target},
                    "internalParameters": {"builder": self.builder, "toolchain": self.toolchain,
                        "polypackNotice": "SLSA-style provenance generated by PolyPack; not a SLSA certification"},
                    "resolvedDependencies": materials,
                },
                "runDetails": {
                    "builder": {"id": self.builder},
                    "metadata": {
                        "invocationId": Digest::of_bytes(format!("{}\0{}",
                            self.invocation, art.name).as_bytes()).to_string(),
                        "startedOn": CREATED, "finishedOn": CREATED,
                    },
                },
            },
        })
    }

    fn project_purl(&self) -> String {
        format!("pkg:generic/polypack@{}", self.version.replace('+', "%2B"))
    }
}

/// 对一组产物路径计算确定性集合摘要（blake3 小写 hex）。
///
/// 输入顺序无关：按产物名 + 摘要排序后逐项混入名字 / sha256 / blake3 / 字节数。
/// 空集合或任一产物不存在均报错；release 回执的 `sbom_digest` 可传入本函数。
pub fn digest_for_artifacts(paths: &[PathBuf]) -> Result<String, Error> {
    if paths.is_empty() {
        return Err(Error::new("没有可摘要的产物（路径集合为空）"));
    }
    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        ids.push(identify(path)?);
    }
    ids.sort_by(|a, b| (&a.name, &a.blake3, &a.sha256).cmp(&(&b.name, &b.blake3, &b.sha256)));
    let mut input = b"polypack-sbom-artifacts/v1".to_vec();
    for id in &ids {
        for text in [&id.name, &id.blake3, id.sha256.as_deref().unwrap_or("")] {
            input.extend_from_slice(&(text.len() as u64).to_le_bytes());
            input.extend_from_slice(text.as_bytes());
        }
        input.push(u8::from(id.sha256.is_some()));
        input.extend_from_slice(&id.bytes.to_le_bytes());
    }
    Ok(Digest::of_bytes(&input).to_string())
}

/// 产物身份：文件名、sha256（文件）/ blake3、字节数。
struct ArtifactId {
    name: String,
    sha256: Option<String>,
    blake3: String,
    bytes: u64,
}

fn identify(path: &Path) -> Result<ArtifactId, Error> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::new(format!("产物路径没有可用文件名：{}", path.display())))?;
    if path.is_file() {
        let bytes = std::fs::read(path).map_err(|err| Error::new(format!("产物不可读：{err}")))?;
        return Ok(ArtifactId {
            name,
            sha256: Some(sha256_hex(&bytes)),
            blake3: Digest::of_bytes(&bytes).to_string(),
            bytes: bytes.len() as u64,
        });
    }
    if path.is_dir() {
        let tree = Digest::of_tree(path, &[]).map_err(|err| Error::new(err.to_string()))?;
        let (bytes, _) = crate::project::dir_size(path)
            .map_err(|err| Error::new(format!("目录不可读：{err}")))?;
        return Ok(ArtifactId {
            name,
            sha256: None,
            blake3: tree.to_string(),
            bytes,
        });
    }
    Err(Error::new(format!(
        "产物不存在：{}（先跑 poly build 生成产物再生成 SBOM）",
        path.display()
    )))
}

fn spdx_package(id: String, name: &str, version: &str, checksums: Option<Value>) -> Value {
    let mut package = json!({"SPDXID": id, "name": name, "versionInfo": version,
        "downloadLocation": "NOASSERTION", "filesAnalyzed": false,
        "licenseConcluded": "NOASSERTION", "licenseDeclared": "NOASSERTION",
        "copyrightText": "NOASSERTION"});
    if let Some(checksums) = checksums {
        package["checksums"] = checksums;
    }
    package
}

fn purl(pkg: &Package) -> String {
    format!("pkg:cargo/{}@{}", pkg.name, pkg.version.replace('+', "%2B"))
}

fn purl_ref(purl: &str) -> Value {
    json!({"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl",
        "referenceLocator": purl})
}

fn deterministic_urn(art: &ArtifactId) -> String {
    let key = art.sha256.as_deref().unwrap_or(&art.blake3);
    let h = sha256_hex(format!("{}:{key}", art.name).as_bytes());
    format!(
        "urn:uuid:{}-{}-4{}-a{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn manifest_path(root: &Path, core_crate: &str) -> Result<PathBuf, Error> {
    let local = root.join(core_crate).join("Cargo.toml");
    for candidate in [local, root.join("Cargo.toml")] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(Error::new(format!(
        "工程 {} 下找不到 {}/Cargo.toml 或 Cargo.toml（SBOM 需要 Cargo 依赖图）",
        root.display(),
        core_crate
    )))
}

fn cargo_metadata(manifest: &Path) -> Result<Value, Error> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|err| Error::new(format!("无法执行 cargo metadata（{cargo}）：{err}")))?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "cargo metadata --format-version 1 --locked 失败（{}）：{}",
            manifest.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| Error::new(format!("cargo metadata 输出不是合法 JSON：{err}")))
}

/// `Cargo.lock` 的 `[[package]]` 键集合（name / version / source）。
fn lock_packages(text: &str) -> BTreeMap<(String, String, Option<String>), ()> {
    let mut out = BTreeMap::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines().map(str::trim).chain(["[[package]]"]) {
        if line == "[[package]]" {
            let field = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            if let (Some(name), Some(version)) = (field("name"), field("version")) {
                out.insert((name, version, field("source")), ());
            }
            fields.clear();
        } else if let Some((key, value)) = line.split_once(" = ") {
            fields.push((key.to_string(), value.trim_matches('"').to_string()));
        }
    }
    out
}

fn metadata_packages(
    metadata: &Value,
    lock: &BTreeMap<(String, String, Option<String>), ()>,
) -> Result<Vec<Package>, Error> {
    let array = metadata["packages"]
        .as_array()
        .ok_or_else(|| Error::new("cargo metadata 输出缺 packages 数组"))?;
    let mut packages = Vec::with_capacity(array.len());
    for pkg in array {
        let name = pkg["name"]
            .as_str()
            .ok_or_else(|| Error::new("cargo metadata 的包缺 name"))?;
        let version = pkg["version"]
            .as_str()
            .ok_or_else(|| Error::new(format!("cargo metadata 的包 {name} 缺 version")))?;
        let source = pkg["source"].as_str().map(str::to_string);
        if !lock.contains_key(&(name.to_string(), version.to_string(), source.clone())) {
            return Err(Error::new(format!(
                "Cargo.lock 与 cargo metadata 不一致：缺 {name} {version}"
            )));
        }
        packages.push(Package {
            name: name.into(),
            version: version.into(),
            source,
        });
    }
    packages
        .sort_by(|a, b| (&a.name, &a.version, &a.source).cmp(&(&b.name, &b.version, &b.source)));
    packages.dedup_by(|a, b| a.name == b.name && a.version == b.version && a.source == b.source);
    Ok(packages)
}

fn builder_identity() -> String {
    std::env::var("POLYPACK_BUILDER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("poly {}", env!("CARGO_PKG_VERSION")))
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "rustc (unknown)".to_string())
}

fn write_json(path: &Path, value: &Value) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| Error::new(format!("JSON 序列化失败：{err}")))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|err| Error::new(format!("写 SBOM 失败（{}）：{err}", path.display())))
}
