//! PolyPack 签名 / 公证 / 加固（M2+ 实现）。
//!
//! 凭据一律以引用出现（`poly_core::CredentialRef`），明文凭据绝不入库（ROADMAP §7）。
//! 本 crate 当前覆盖：
//! - **T-M2-11** 密钥库集成：[`secret`] 的凭据后端链（keyring / Vault trait / Env / CI 内存注入），
//!   以及 [`vault`] 的真 HTTP Vault 客户端（KV v2，`VAULT_ADDR` / `VAULT_TOKEN` / `VAULT_CACERT`）；
//! - **T-M2-09** Android 签名：[`android`] 的 apksigner 计划与执行（密钥材料只落内存与
//!   一次性临时目录，口令经环境变量注入，绝不进命令行 / 日志 / 事件流）。
//! - **T-M3-04** HarmonyOS 签名：[`harmony`] 的 hap-sign-tool `sign-app` / `verify-app`
//!   （口令只能经 argv——展示 / 日志 / 事件流统一脱敏为 `****`；应用证书链与已签名 Profile
//!   经载荷 `app_cert` / `profile` 字段注入）。

pub mod android;
pub mod error;
pub mod harmony;
pub mod ios;
pub mod secret;
pub mod vault;

pub use android::{AndroidSigner, SignPlan, SignRequest, SignedArtifact};
pub use error::{SignError, codes};
pub use harmony::{HarmonySignPlan, HarmonySignRequest, HarmonySignedArtifact, HarmonySigner};
pub use secret::{SecretBackend, SigningMaterial, default_backends, resolve};
pub use vault::HttpVaultClient;
