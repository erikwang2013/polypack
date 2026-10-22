//! wasmtime 组件沙箱：加载、实例化、调用 `describe` / `package`，并执行能力自检探针。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use polypack_core::{PluginGrant, codes};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

use crate::capability::{CapabilityPolicy, MAX_RESPONSE_BYTES};
use crate::error::PluginError;
use crate::limits::{EPOCH_TICK, ResourceLimits};
use crate::trust::SignaturePolicy;

#[allow(missing_docs)] // bindgen 生成的绑定不参与文档门禁
mod target_world {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "polypack-target-sandboxed",
    });
}

#[allow(missing_docs)]
mod probe_world {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "polypack-capability-probe",
    });
}

use target_world::{
    BuildInputs, CoreLib, PolypackTargetSandboxed, PolypackTargetSandboxedImports, UiBundle,
};

/// 宿主接受的插件接口版本（破坏性变更走新版本号，见 `wit/polypack-target.wit`）。
pub const TARGET_SPEC_VERSION: &str = "target-spec/v1";

/// 宿主侧构建输入（`inputs` 导入的返回值来源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInput {
    /// 应用名。
    pub name: String,
    /// 应用版本。
    pub version: String,
    /// 目标 id。
    pub target: String,
    /// UI 入口文件。
    pub entry: String,
    /// UI 文件（相对路径 → 内容）。
    pub files: Vec<(String, Vec<u8>)>,
    /// 核心库三元组。
    pub triple: String,
    /// 核心库字节。
    pub lib: Vec<u8>,
}

/// 插件声明的目标规格（`target-spec/v1`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpecInfo {
    /// 版本协商字符串。
    pub spec_version: String,
    /// 目标 id。
    pub id: String,
    /// 支持等级。
    pub tier: u8,
    /// 运行方式。
    pub runtime: String,
    /// 绑定桥。
    pub bridge: String,
    /// 打包格式。
    pub package_format: String,
    /// 打包器 id。
    pub builder: String,
    /// 可构建宿主。
    pub hosts: Vec<String>,
}

/// 插件打包产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedArtifact {
    /// 产物格式。
    pub format: String,
    /// 产物字节。
    pub bytes: Vec<u8>,
}

/// 已加载并通过 SRI 校验的插件。
pub struct Plugin {
    id: String,
    digest: String,
    component: Component,
    policy: CapabilityPolicy,
    egress: Option<Arc<Egress>>,
}

impl Plugin {
    /// 插件 id（= `[[plugins]].id`）。
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 插件制品内容摘要（`blake3-<hex>`）。
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// 计算插件制品内容摘要（SRI 口径：`blake3-<hex>`）。
pub fn content_digest(bytes: &[u8]) -> String {
    format!("blake3-{}", blake3::hash(bytes).to_hex())
}

/// 校验 `target-spec/v1`；非 v1 拒绝并给迁移提示。
pub fn validate_spec_version(version: &str) -> Result<(), PluginError> {
    if version == TARGET_SPEC_VERSION {
        return Ok(());
    }
    Err(PluginError::new(
        codes::E_PLUGIN_0005,
        format!("插件接口版本不受支持: {version}（宿主要求 {TARGET_SPEC_VERSION}）"),
    )
    .with_hint("升级插件到 target-spec/v1，或升级 polypack 以支持新版本；破坏性变更走新版本号"))
}

/// 沙箱（engine / linker / 资源上限可跨插件复用）。
pub struct Sandbox {
    engine: Engine,
    linker: Linker<SandboxState>,
    limits: ResourceLimits,
}

impl Sandbox {
    /// 装配 engine 与 linker：fuel、epoch、组件模型开启；linker 只注册本 crate 的
    /// 能力接口——不注册 WASI 文件系统 / 套接字（组件的 WASI 导入会实例化失败）。
    pub fn new(limits: ResourceLimits) -> Result<Self, PluginError> {
        limits.validate()?;
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config).map_err(|err| {
            PluginError::new(codes::E_PLUGIN_0007, format!("engine 初始化失败: {err}"))
        })?;
        let mut linker = Linker::new(&engine);
        // HasSelf：宿主状态直接实现生成的 Host trait（`&mut SandboxState` 投影）。
        PolypackTargetSandboxed::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)
            .map_err(|err| {
                PluginError::new(codes::E_PLUGIN_0007, format!("linker 初始化失败: {err}"))
            })?;
        Ok(Self {
            engine,
            linker,
            limits,
        })
    }

    /// 加载插件：SRI 校验 → 组件编译 → 冻结能力策略。
    pub fn load(
        &self,
        bytes: &[u8],
        id: &str,
        sri: Option<&str>,
        root: &Path,
        grant: &PluginGrant,
    ) -> Result<Plugin, PluginError> {
        self.load_signed(bytes, id, sri, None, root, grant)
    }

    /// 加载插件（T-M8-03）：SRI 校验 →（配置要求时）分离签名验签 → 组件编译 → 冻结能力策略。
    ///
    /// 验签先于组件编译：签名不符 / 缺失 / 信任根错误都在实例化之前拒绝。
    pub fn load_signed(
        &self,
        bytes: &[u8],
        id: &str,
        sri: Option<&str>,
        signature: Option<&SignaturePolicy<'_>>,
        root: &Path,
        grant: &PluginGrant,
    ) -> Result<Plugin, PluginError> {
        if let Some(expected) = sri {
            let actual = content_digest(bytes);
            if expected != actual {
                return Err(PluginError::new(
                    codes::E_PLUGIN_0006,
                    format!("插件 {id} 内容摘要不匹配（期望 {expected}，实际 {actual}）"),
                )
                .with_hint("重新构建插件并更新 SRI；制品被改动过的插件一律不加载"));
            }
        }
        if let Some(policy) = signature {
            crate::trust::verify(id, bytes, policy)?;
        }
        let component = Component::from_binary(&self.engine, bytes).map_err(|err| {
            PluginError::new(
                codes::E_PLUGIN_0007,
                format!("插件 {id} 不是可用的 wasm 组件: {err}"),
            )
            .with_hint("用 target-spec/v1 WIT 重新编译（见 guests/build.sh）")
        })?;
        let policy = CapabilityPolicy::new(root, grant)?;
        let egress = if grant.net.is_empty() {
            None
        } else {
            Some(Arc::new(Egress::new()?))
        };
        Ok(Plugin {
            id: id.to_string(),
            digest: content_digest(bytes),
            component,
            policy,
            egress,
        })
    }

    /// 调用插件 `describe`，并校验接口版本。
    pub fn describe(
        &self,
        plugin: &Plugin,
        input: &PackageInput,
    ) -> Result<TargetSpecInfo, PluginError> {
        self.with_instance(plugin, input, |store, bindings| {
            let spec = bindings.call_describe(store)?;
            Ok(TargetSpecInfo {
                spec_version: spec.spec_version,
                id: spec.id,
                tier: spec.tier,
                runtime: spec.runtime,
                bridge: spec.bridge,
                package_format: spec.package_format,
                builder: spec.builder,
                hosts: spec.hosts,
            })
        })
        .and_then(|spec| {
            validate_spec_version(&spec.spec_version)?;
            Ok(spec)
        })
    }

    /// 调用插件 `package`。
    pub fn package(
        &self,
        plugin: &Plugin,
        input: &PackageInput,
    ) -> Result<PackagedArtifact, PluginError> {
        self.with_instance(plugin, input, |store, bindings| {
            let bundle = UiBundle {
                entry: input.entry.clone(),
                files: input.files.clone(),
            };
            let lib = CoreLib {
                triple: input.triple.clone(),
                bytes: input.lib.clone(),
            };
            match bindings.call_package(store, &bundle, &lib)? {
                Ok(artifact) => Ok(PackagedArtifact {
                    format: artifact.format,
                    bytes: artifact.bytes,
                }),
                Err(err) => Err(wasmtime::Error::msg(format!(
                    "插件返回错误 {}: {}",
                    err.code, err.message
                ))),
            }
        })
    }

    /// 能力自检探针（测试 / 诊断用）：`case` 见 `guests/capability-probe`。
    pub fn probe(&self, plugin: &Plugin, case: &str, arg: &str) -> Result<String, PluginError> {
        let input = PackageInput {
            name: String::new(),
            version: String::new(),
            target: String::new(),
            entry: String::new(),
            files: Vec::new(),
            triple: String::new(),
            lib: Vec::new(),
        };
        self.with_probe(plugin, &input, case, arg)
    }

    fn with_probe(
        &self,
        plugin: &Plugin,
        input: &PackageInput,
        case: &str,
        arg: &str,
    ) -> Result<String, PluginError> {
        self.with_store(plugin, input, |store| {
            let bindings = probe_world::PolypackCapabilityProbe::instantiate(
                &mut *store,
                &plugin.component,
                &self.linker,
            )?;
            match bindings.call_run(store, case, arg)? {
                Ok(value) => Ok(value),
                Err(err) => Err(wasmtime::Error::msg(format!(
                    "探针返回错误 {}: {}",
                    err.code, err.message
                ))),
            }
        })
    }

    fn with_instance<T>(
        &self,
        plugin: &Plugin,
        input: &PackageInput,
        call: impl FnOnce(
            &mut Store<SandboxState>,
            &PolypackTargetSandboxed,
        ) -> Result<T, wasmtime::Error>,
    ) -> Result<T, PluginError> {
        self.with_store(plugin, input, |store| {
            let bindings =
                PolypackTargetSandboxed::instantiate(&mut *store, &plugin.component, &self.linker)?;
            call(store, &bindings)
        })
    }

    fn with_store<T>(
        &self,
        plugin: &Plugin,
        input: &PackageInput,
        call: impl FnOnce(&mut Store<SandboxState>) -> Result<T, wasmtime::Error>,
    ) -> Result<T, PluginError> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.max_memory_bytes)
            .build();
        let state = SandboxState {
            inputs: BuildInputs {
                name: input.name.clone(),
                version: input.version.clone(),
                target: input.target.clone(),
            },
            policy: plugin.policy.clone(),
            limits,
            egress: plugin.egress.clone(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state: &mut SandboxState| &mut state.limits);
        store.set_epoch_deadline(self.limits.epoch_ticks);
        store.set_fuel(self.limits.fuel).map_err(|err| {
            PluginError::new(codes::E_PLUGIN_0007, format!("fuel 设置失败: {err}"))
        })?;

        let stop = Arc::new(AtomicBool::new(false));
        let ticker = {
            let engine = self.engine.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    engine.increment_epoch();
                }
            })
        };
        let result = call(&mut store);
        stop.store(true, Ordering::Relaxed);
        let _ = ticker.join();
        result.map_err(map_runtime_error)
    }
}

/// 插件调用的宿主状态。
struct SandboxState {
    inputs: BuildInputs,
    policy: CapabilityPolicy,
    limits: StoreLimits,
    egress: Option<Arc<Egress>>,
}

// `types` 接口只有数据类型，无宿主函数；bindgen 仍要求空实现。
impl target_world::polypack::target_spec::types::Host for SandboxState {}

impl PolypackTargetSandboxedImports for SandboxState {
    fn inputs(&mut self) -> BuildInputs {
        self.inputs.clone()
    }
}

impl target_world::polypack::target_spec::filesystem::Host for SandboxState {
    fn read(
        &mut self,
        path: String,
    ) -> Result<Vec<u8>, target_world::polypack::target_spec::types::Error> {
        self.policy.read(&path).map_err(to_wit_error)
    }

    fn write(
        &mut self,
        path: String,
        contents: Vec<u8>,
    ) -> Result<u32, target_world::polypack::target_spec::types::Error> {
        self.policy.write(&path, &contents).map_err(to_wit_error)
    }
}

impl target_world::polypack::target_spec::network::Host for SandboxState {
    fn fetch(
        &mut self,
        url: String,
    ) -> Result<String, target_world::polypack::target_spec::types::Error> {
        let authorized = self.policy.authorize_url(&url).map_err(to_wit_error)?;
        let egress = self
            .egress
            .as_ref()
            .ok_or_else(|| to_wit_error(deny_fetch("出网传输未接线")))?;
        egress.fetch(&authorized).map_err(to_wit_error)
    }
}

// 探针 world 与基础 world 导入的是同一组 WIT 接口 id，linker 中由 target_world 的
// Host 实现统一服务，无需为 probe_world 再实现一份（否则是死代码）。

fn to_wit_error(err: PluginError) -> target_world::polypack::target_spec::types::Error {
    target_world::polypack::target_spec::types::Error {
        code: err.code.to_string(),
        message: err.message,
    }
}

fn deny_fetch(reason: &str) -> PluginError {
    PluginError::new(codes::E_PLUGIN_0002, format!("出网拒绝：{reason}"))
}

fn map_runtime_error(err: wasmtime::Error) -> PluginError {
    if let Some(trap) = err.downcast_ref::<Trap>() {
        match trap {
            Trap::OutOfFuel => {
                return PluginError::new(
                    codes::E_PLUGIN_0003,
                    "插件触发 fuel 上限（疑似无限循环）",
                )
                .with_hint("确认插件没有死循环；必要时在宿主侧收紧/放宽 fuel 预算");
            }
            Trap::Interrupt => {
                return PluginError::new(codes::E_PLUGIN_0003, "插件触发 epoch 超时")
                    .with_hint("插件执行超过墙钟上限；检查阻塞等待或死循环");
            }
            _ => {}
        }
    }
    PluginError::new(codes::E_PLUGIN_0007, format!("插件执行失败: {err}"))
}

/// 出网传输（显式授权域名后才创建；重定向禁用，避免越过白名单）。
struct Egress {
    agent: ureq::Agent,
}

impl Egress {
    fn new() -> Result<Self, PluginError> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            // 非 2xx 交回响应，由 fetch 统一判定；重定向一律不跟随。
            .http_status_as_error(false)
            .max_redirects(0)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::Specific(Arc::new(system_roots()?)))
                    .build(),
            )
            .build();
        Ok(Self {
            agent: config.into(),
        })
    }

    fn fetch(&self, url: &crate::capability::AuthorizedUrl) -> Result<String, PluginError> {
        let mut response = self.agent.get(&url.url).call().map_err(|err| {
            PluginError::new(
                codes::E_PLUGIN_0007,
                format!("出网请求失败（{}:{}）: {err}", url.host, url.port),
            )
        })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(PluginError::new(
                codes::E_PLUGIN_0007,
                format!("出网响应非 2xx（{}:{}）: {status}", url.host, url.port),
            ));
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|err| {
                PluginError::new(
                    codes::E_PLUGIN_0007,
                    format!("读取出网响应失败（{}:{}）: {err}", url.host, url.port),
                )
            })
    }
}

fn system_roots() -> Result<Vec<ureq::tls::Certificate<'static>>, PluginError> {
    let native = rustls_native_certs::load_native_certs();
    if native.certs.is_empty() {
        return Err(PluginError::new(
            codes::E_PLUGIN_0007,
            "系统信任库没有可用根证书（HTTPS 出网无从校验）：装 ca-certificates 后重跑",
        ));
    }
    Ok(native
        .certs
        .iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect())
}
