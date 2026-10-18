# 0002 — 单一 Cargo workspace（不拆 crate / 仓库）

- 状态（Status）：accepted
- 日期（Date）：2026-10-19（回填）
- 决策者（Deciders）：项目维护者

> **English abstract**: All Rust code lives in one Cargo workspace (`members = ["crates/*"]`) rather than separate repositories. Nine crates carry the pipeline stages with a one-way dependency graph and shared workspace metadata/lints. The no_std engine for embedded targets is a feature split inside `poly-core`, not a new crate.

## 背景（Context）

流水线横跨调度、工具链、打包、签名、发布、缓存与插件等阶段，需要稳定的内部契约与统一的依赖方向；M0 已按 9 个 crate 建立 workspace 骨架（ROADMAP §9 M0）。嵌入式目标又要求 no_std 引擎层，需要决定是新增 crate 还是分层。

## 决策（Decision）

1. 根 `Cargo.toml` 定义唯一 workspace：`resolver = "3"`、`members = ["crates/*"]`，当前 9 个 crate。
2. 元数据统一继承：版本、edition 2024、MSRV、license 与 repository 由 `[workspace.package]` 提供，各 crate 使用 `version.workspace = true` 等。
3. 依赖方向单向（GA §5.1）：`poly-core` 依赖 `poly-cache` / `poly-targets`；`poly-sign` / `poly-publish` / `poly-plugin` 依赖 `poly-core`；`poly-cli` 依赖其余全部；无环。
4. 嵌入式 no_std 分层在 `poly-core` 内用 feature 拆分，不新增 crate（ROADMAP §9 M6）。
5. 全仓共用 `[workspace.lints]`：`unsafe_code = "deny"`、`missing_docs = "warn"`，clippy 拒绝 `dbg!` / `todo!` / `unimplemented!`。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 按阶段拆成多个仓库 | 跨仓改动不能原子提交，版本与锁文件需分别维护 |
| 单一单体 crate | 无法按阶段约束依赖方向，也难隔离 no_std 与 std 依赖 |
| 为 no_std 引擎新增 crate | M6 明确选择 `poly-core` 内 feature 拆分 |

## 影响（Consequences）

- 任一 crate 的依赖变更会更新全仓 `Cargo.lock`；CI 以 workspace 为单位跑测试（GA §5.5 `rust-ci`）。
- 各 crate 均为 `publish = false`，不独立发版，随仓库整体演进。
- feature 拆分把 std / no_std 双面复杂度留在 `poly-core`，新增 API 需同时守住两个面（GA §5.2）。

## 参考（References）

- [Cargo.toml](../../Cargo.toml)
- [crates/](../../crates/)
- [ROADMAP](../ROADMAP.md) §4.1、§9 M0 / M6（[English](../ROADMAP.en.md)）
- [GA](../GA.md) §5.1–5.2
- 提交：`f60a5dd`（M0 地基，workspace 骨架）
