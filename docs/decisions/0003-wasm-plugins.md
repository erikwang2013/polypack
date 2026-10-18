# 0003 — WASM 插件沙箱与 `target-spec/v1` WIT

- 状态（Status）：accepted
- 日期（Date）：2026-10-19（回填）
- 决策者（Deciders）：项目维护者

> **English abstract**: Third-party target plugins are WebAssembly components described by the `polypack-target` WIT world and run in a wasmtime sandbox with no filesystem or network access by default. Capabilities are granted explicitly per plugin; artifacts pass SRI and optional GPG verification before instantiation. The interface is versioned `target-spec/v1`, and built-in and plugin targets share one `TargetProvider` contract.

## 背景（Context）

目标是新增平台 ≤ 2 人日，且新增平台不改核心代码路径（ROADMAP §1、§2 原则 3 / 6）。第三方打包逻辑不可信，必须在宿主进程边界内隔离；为此需要一套稳定的插件接口与制品校验链。

## 决策（Decision）

1. 插件是 WASM 组件；接口由 `crates/poly-plugin/wit/polypack-target.wit` 的 `world polypack-target` 描述（`inputs` / `describe` / `package`）。
2. 运行时为 wasmtime，版本钉死 `=49.0.3`，只启用 `component-model`、`cranelift`、`runtime`、`std`（根 `Cargo.toml`）。
3. 默认零能力：无文件系统、无网络；`fs_read` / `fs_write` / `net` 按插件显式授权，越权返回 `E_PLUGIN_0001` / `0002`。
4. 资源上限：fuel / epoch / 线性内存，配置只能收紧不能放开；超限中止插件调用，不影响宿主进程（GA §3.3）。
5. 制品加载顺序固定：SRI（`blake3-<hex>`）→ GPG 分离验签 → 组件解析 → 实例化（GA §3.2）。
6. 接口版本为 `target-spec/v1`；破坏性变更发新版本号，宿主拒绝非 v1 插件并返回 `E_PLUGIN_0005`（GA §1.2）。
7. 内建目标与插件统一走 `TargetProvider` / `ProviderRegistry`，CLI 无 builtin 特判（GA §5.4）。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 仅内建打包器 | 与「插件优先」原则和 ≤ 2 人日目标不符 |
| 宿主内动态库插件 | 与不可信代码隔离的目标冲突，ABI 与内存安全边界不可控 |
| 默认放开 WASI 能力 | 与默认零能力的安全模型冲突 |

## 影响（Consequences）

- wasmtime 带来较重的编译依赖；版本锁死，升级需单独验证组件 ABI。
- 插件制品的 SRI 与签名必须随制品同步更新；只改 SRI 不能绕过验签。
- 模板 WIT 是宿主 WIT 的手工副本，升级接口需同步两处，当前无自动比对门禁（GA §2.5）。
- `E_PLUGIN_0001`–`0010` 错误码成为对外契约（GA §3.4）。

## 参考（References）

- [ROADMAP](../ROADMAP.md) §4.5（[English](../ROADMAP.en.md) §4.5）
- [GA](../GA.md) §1.1–1.2、§2.5、§3
- [crates/poly-plugin](../../crates/poly-plugin/)
- [WIT 契约](../../crates/poly-plugin/wit/polypack-target.wit)
- [插件模板](../../templates/plugin-target/)
- 提交：`00a0695`（沙箱 + `polypack-target` world v1）、`97e9a2a`（模板与 `poly plugin new/digest`）
