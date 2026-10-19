# 0008 — 多应用工作区（monorepo）

- 状态（Status）：accepted
- 日期（Date）：2026-10-20
- 决策者（Deciders）：项目维护者

> **English abstract**: A root `polypack.toml` may declare an optional `[workspace] members = [...]` list; each member directory owns its own `polypack.toml`. Member ids are directory basenames (unique, relative, canonicalized under the workspace root). `poly build --workspace` builds all members in declaration order, `--app <id>` builds one, and the absence of both flags preserves the single-app path byte for byte. `JobSpec.app_id` is optional and omitted for single-app jobs. The shared core compile cache key excludes the app id, while the UI bundle key includes it; multi-app outputs land under `.polypack/packages/<app>/<target>/` and `.polypack/artifacts/<app>/<target>/`. `poly retry` / `poly verify` do not yet accept workspace selectors.

## 背景（Context）

现有 `PolypackConfig` 是单应用模型：根目录一份 `polypack.toml`，`Project` 只有一个 root。大型团队常需要在一个仓库中维护多个共享同一 Rust core 的应用，分别打包、发布与缓存。直接复制仓库会失去共享 core 的缓存收益；把每个应用做成独立仓库则增加同步成本。

## 决策（Decision）

1. 根 `polypack.toml` 增加可选 `[workspace] members = ["apps/a", "apps/b"]`；成员目录各自持有完整 `polypack.toml`。没有 `[workspace]` 时行为与旧版完全一致。
2. 成员 id = 目录最后一段；要求唯一、相对路径、不得逃逸工作区根（含符号链接规范化检查）。违规返回稳定诊断 `E_WS_0001..E_WS_0004`。
3. `poly build --workspace` 按声明序构建全部成员；`poly build --app <id>` 只构建指定成员；两者互斥。缺省（都不给）走单应用路径。
4. `JobSpec.app_id: Option<String>` 可省略；单应用序列化不出现该字段，旧缓存键与产物路径不变。
5. 缓存：共享 core 的 compile 键不含 app id（跨应用复用）；UI bundle 键混入 app id（避免串包）。工作区共享 `<workspace>/.polypack/cache`；产物落 `<workspace>/.polypack/packages/<app>/<target>/`，快照落 `<workspace>/.polypack/artifacts/<app>/<target>/`。
6. 明确限制：`poly retry` / `poly verify` 暂不接受 `--workspace` / `--app`；无跨应用任务图、无目标继承；历史记录写在成员根而非工作区根。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 外部脚本循环调用 `poly build` | 无法表达共享缓存与统一工作区根；错误聚合与幂等性靠脚本 |
| 每个应用独立仓库 | 共享 core 的版本同步成本高，违背 monorepo 初衷 |
| 单一根配置内嵌多个 `[apps.*]` 表 | 与现有单应用 schema 冲突大，成员配置无法独立演进 |
| 引入 Nx / Turborepo 级任务图 | 首版过重，且与 PolyPack 的 TargetSpec/JobSpec 模型重复 |

## 影响（Consequences）

- 单应用用户零迁移；`[workspace]` 是纯增量。
- 成员 id 依赖目录名，重命名目录会改变 app id 与缓存键；需要稳定 id 时后续可加显式 `id` 字段（新 ADR）。
- `retry` / `verify` 的工作区支持是已知缺口，需在后续任务补齐。
- 共享 core 的 compile 缓存可跨应用命中；UI bundle 缓存按应用隔离，避免同名产物串包。
- 工作区构建目前串行；并发调度留给后续农场/任务图工作。

## 参考（References）

- [WORKSPACE](../WORKSPACE.md)（[English](../WORKSPACE.en.md)）
- [INITIATIVES](../INITIATIVES.md) M9-E2
- `crates/poly-core/src/workspace.rs`
- `crates/poly-cli/tests/workspace.rs`
