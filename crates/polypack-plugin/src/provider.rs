//! wasm 插件即目标 provider（T-M8-05，ROADMAP 原则 6）。
//!
//! [`PluginProvider`] 把沙箱里的 `target-spec/v1` 插件接到 [`TargetProvider`]：
//! `describe` 在加载期执行一次并转成宿主 [`TargetSpec`]；`package` 把通用输入
//! 原样交给插件，产物以**内联字节**返回（调用侧落盘），与内建 provider 同契约。

use polypack_core::codes;
use polypack_targets::provider::{PackageArtifact, PackageInput, ProviderError, TargetProvider};
use polypack_targets::spec::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

use crate::error::PluginError;
use crate::sandbox::{Plugin, Sandbox, TargetSpecInfo};

/// 第三方 wasm 插件 provider（describe 结果在加载期固化）。
pub struct PluginProvider {
    sandbox: Sandbox,
    plugin: Plugin,
    spec: TargetSpec,
}

impl PluginProvider {
    /// 执行一次插件 `describe` 并转成宿主声明。
    ///
    /// `probe` 是 describe 的输入快照（插件可据 `name` / `version` / `target` 声明目标）；
    /// 之后每次 `package` 使用调用侧的新输入。
    pub fn load(
        sandbox: Sandbox,
        plugin: Plugin,
        probe: &crate::sandbox::PackageInput,
    ) -> Result<Self, ProviderError> {
        let info = sandbox
            .describe(&plugin, probe)
            .map_err(|err| describe_error(&err))?;
        let spec = target_spec(&info, plugin.id())?;
        Ok(Self {
            sandbox,
            plugin,
            spec,
        })
    }

    /// 插件 id（`[[plugins]].id`）。
    pub fn plugin_id(&self) -> &str {
        self.plugin.id()
    }

    /// 插件制品摘要（SRI 口径）。
    pub fn digest(&self) -> &str {
        self.plugin.digest()
    }
}

impl TargetProvider for PluginProvider {
    fn describe(&self) -> Result<TargetSpec, ProviderError> {
        Ok(self.spec.clone())
    }

    fn package(&self, input: &PackageInput<'_>) -> Result<PackageArtifact, ProviderError> {
        if input.target != self.spec.id {
            return Err(package_contract_error(format!(
                "插件 {} 收到目标 {} 的输入（声明为 {}）",
                self.plugin.id(),
                input.target,
                self.spec.id
            )));
        }
        let plugin_input = crate::sandbox::PackageInput {
            name: input.name.to_string(),
            version: input.version.to_string(),
            target: input.target.to_string(),
            entry: input.entry.to_string(),
            files: input.files.to_vec(),
            triple: input.triple.to_string(),
            lib: input.lib.to_vec(),
        };
        let artifact = self
            .sandbox
            .package(&self.plugin, &plugin_input)
            .map_err(|err| package_error(&err))?;
        if artifact.format != self.spec.package.format {
            return Err(package_contract_error(format!(
                "插件 {} 声明格式 {}，实际返回 {}",
                self.plugin.id(),
                self.spec.package.format,
                artifact.format
            )));
        }
        Ok(PackageArtifact::inline(
            self.spec.id.clone(),
            artifact.format,
            artifact.bytes,
        ))
    }
}

/// `TargetSpecInfo` → 宿主 `TargetSpec`。
///
/// WIT 只暴露宿主可移植字段：`ui` 留空、`signing` / `verify` / `publish` 未声明；
/// 这些差异由契约测试显式记录，不臆造插件能力。
fn target_spec(info: &TargetSpecInfo, plugin_id: &str) -> Result<TargetSpec, ProviderError> {
    let mut hosts = Vec::with_capacity(info.hosts.len());
    for host in &info.hosts {
        hosts.push(parse_host(host)?);
    }
    Ok(TargetSpec {
        id: info.id.clone(),
        tier: info.tier,
        hosts,
        runtime: parse_runtime(&info.runtime)?,
        bridge: parse_bridge(&info.bridge)?,
        ui: Vec::new(),
        package: PackageSpec {
            format: info.package_format.clone(),
            builder: info.builder.clone(),
        },
        signing: None,
        verify: Vec::new(),
        publish: Vec::new(),
        experimental: false,
        notes: Some(format!(
            "第三方 wasm 插件 {plugin_id}（target-spec/v1；ui / signing / verify / publish 未声明）"
        )),
    })
}

fn parse_host(value: &str) -> Result<Host, ProviderError> {
    match value {
        "win" | "windows" => Ok(Host::Win),
        "linux" => Ok(Host::Linux),
        "macos" | "darwin" => Ok(Host::Macos),
        "bsd" | "freebsd" | "openbsd" | "netbsd" => Ok(Host::Bsd),
        other => Err(contract_error(format!("插件声明了未知宿主 {other:?}"))),
    }
}

fn parse_runtime(value: &str) -> Result<Runtime, ProviderError> {
    match value {
        "native" => Ok(Runtime::Native),
        "wasm" => Ok(Runtime::Wasm),
        "staticlib" => Ok(Runtime::Staticlib),
        "html5" => Ok(Runtime::Html5),
        "none" => Ok(Runtime::None),
        other => Err(contract_error(format!("插件声明了未知 runtime {other:?}"))),
    }
}

fn parse_bridge(value: &str) -> Result<Bridge, ProviderError> {
    match value {
        "uniffi" => Ok(Bridge::Uniffi),
        "napi" => Ok(Bridge::Napi),
        "wasm-bindgen" => Ok(Bridge::WasmBindgen),
        "cffi" => Ok(Bridge::Cffi),
        "tauri-ipc" => Ok(Bridge::TauriIpc),
        "html5" => Ok(Bridge::Html5),
        "none" => Ok(Bridge::None),
        other => Err(contract_error(format!("插件声明了未知 bridge {other:?}"))),
    }
}

fn contract_error(message: String) -> ProviderError {
    ProviderError::Describe {
        code: codes::E_PLUGIN_0005.as_str(),
        message,
        hint: Some("核对插件 target-spec/v1 的枚举取值（hosts / runtime / bridge）".to_string()),
    }
}

fn package_contract_error(message: String) -> ProviderError {
    ProviderError::Package {
        code: codes::E_PLUGIN_0005.as_str(),
        message,
        hint: Some("核对插件 provider 契约：目标 id 与产物格式须与 describe 声明一致".to_string()),
    }
}

fn describe_error(err: &PluginError) -> ProviderError {
    ProviderError::Describe {
        code: err.code.as_str(),
        message: err.message.clone(),
        hint: err.hint.clone(),
    }
}

fn package_error(err: &PluginError) -> ProviderError {
    ProviderError::Package {
        code: err.code.as_str(),
        message: err.message.clone(),
        hint: err.hint.clone(),
    }
}
