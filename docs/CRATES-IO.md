# crates.io 发布审计（T-M9-45）

> 状态：审计完成，发布保持关闭。本文不授权也不执行任何上传。
> 审计日期：2026-10-20；名称可用性来自 crates.io API 当日查询结果。
> English: [CRATES-IO.en.md](CRATES-IO.en.md)

## 1. 结论

- workspace 版本已从 `0.0.0` 提升到 `1.14.0`，`Cargo.lock` 同步；`poly --version` 与最新 tag 一致。
- 9 个 crate 的 `description`、`license`、`repository`、`readme`、`keywords`、`categories` 已齐备。`readme` 继承根 `README.md`，`keywords` / `categories` 在根清单统一声明。
- workspace 内部 path 依赖已补 `version = "1.14.0"`；没有版本号时 `cargo package` 会拒绝打包（path dependency without version）。
- 所有 crate 仍为 `publish = false`。未发布、未上传、未使用 crates.io token。
- 两个名称在 crates.io 已被他人占用：`poly-cli`、`poly-core`；其余 7 个可用。发布前必须改名或取得所有权，属维护者决策。
- `cargo package --list` 9/9 通过（内容清单审计）；`cargo publish --dry-run` 因 `publish = false` 被拒绝，这是预期的发布闸门，不是元数据错误。

## 2. 发布顺序

| 顺序 | crate | 内部依赖 | 名称状态（2026-10-20） |
| --- | --- | --- | --- |
| 1 | `poly-targets` | — | 可用 |
| 2 | `poly-cache` | — | 可用 |
| 3 | `poly-toolchain` | — | 可用 |
| 4 | `poly-core` | `poly-cache`、`poly-targets`（可选 `poly-toolchain`） | 已占用（0.0.1） |
| 5 | `poly-bundle` | `poly-targets` | 可用 |
| 6 | `poly-sign` | `poly-core` | 可用 |
| 7 | `poly-publish` | `poly-core` | 可用 |
| 8 | `poly-plugin` | `poly-core`、`poly-sign`、`poly-targets` | 可用 |
| 9 | `poly-cli` | 全部内部 crate | 已占用（0.5.0） |

## 3. 必需元数据

| 字段 | 来源 | 当前值 / 状态 |
| --- | --- | --- |
| `version` | `[workspace.package]` | `1.14.0` |
| `description` | 各 crate `Cargo.toml` | 9/9 已填写 |
| `license` | `[workspace.package]` | `Apache-2.0`（crates.io 必需） |
| `repository` | `[workspace.package]` | `https://github.com/erikwang2013/polypack` |
| `readme` | `[workspace.package]` | 根 `README.md`，随每个包归档 |
| `keywords` | `[workspace.package]` | `polypack`、`packaging`、`build-tool`、`cross-platform`、`mobile` |
| `categories` | `[workspace.package]` | `development-tools::build-utils`、`command-line-utilities` |
| `publish` | 各 crate `Cargo.toml` | `false`（9/9，保持） |
| `documentation` / `homepage` | 未设置 | crates.io 非必需 |

## 4. 当前阻塞项

| 阻塞项 | 证据 | 状态 / 解除条件 |
| --- | --- | --- |
| 版本为 `0.0.0` | 根清单原值；`poly --version` 会输出 `0.0.0` | 已解除：提升为 `1.14.0` |
| path 依赖无 `version` | `cargo package` 报 “all dependencies must have a version specified when packaging” | 已解除：workspace 依赖补 `1.14.0` |
| `publish = false` | 9/9 crate 均设置 | 开放：维护者批准发布计划后单独提交移除 |
| `poly-cli` 名称占用 | crates.io 现有 0.5.0，owner 非本仓库 | 开放：改名为 `polypack-cli`（可用）或取得所有权 |
| `poly-core` 名称占用 | crates.io 现有 0.0.1，owner 非本仓库 | 开放：改名为 `polypack-engine`（可用）或取得所有权 |
| 无 crates.io token | 仓库无 `CARGO_REGISTRY_TOKEN`，CI 无发布 secret | 开放：维护者配置 secret；不得写入仓库 |

## 5. 命令

审计命令，不需要 token、不上传：

```bash
# 内容清单（逐个 crate；CI 在 Linux 上只跑这一步）
cargo package --list -p poly-targets --locked
cargo package --list -p poly-cache --locked
cargo package --list -p poly-toolchain --locked
cargo package --list -p poly-core --locked
cargo package --list -p poly-bundle --locked
cargo package --list -p poly-sign --locked
cargo package --list -p poly-publish --locked
cargo package --list -p poly-plugin --locked
cargo package --list -p poly-cli --locked

# 对照当前闸门（预期失败：publish = false）
cargo publish --dry-run -p poly-targets
```

正式发布（维护者批准并移除 `publish = false` 后执行；按 §2 顺序，每步等 crates.io 索引可见）：

```bash
cargo publish -p poly-targets
cargo publish -p poly-cache
cargo publish -p poly-toolchain
cargo publish -p poly-core
cargo publish -p poly-bundle
cargo publish -p poly-sign
cargo publish -p poly-publish
cargo publish -p poly-plugin
cargo publish -p poly-cli
```

## 6. 发布检查清单

| # | 检查项 | 命令 | 通过标准 |
| --- | --- | --- | --- |
| 1 | 内容与元数据 | `cargo package --list -p <crate> --locked` | 9/9 成功，清单只含源码、测试、README 与 Cargo 文件 |
| 2 | 名称决策 | 维护者评审 §4 | `poly-cli` / `poly-core` 改名或所有权有书面结论 |
| 3 | 移除发布闸门 | 单独提交删除 `publish = false` | 9/9 crate 可发布，且 CI 仍全绿 |
| 4 | 测试 | `cargo test --workspace --no-fail-fast` | 全绿 |
| 5 | Lint | `cargo clippy --workspace --all-targets -- -D warnings` | 全绿 |
| 6 | 版本一致 | `cargo run -p poly-cli -- --version` | 输出 `1.14.0`，与 `[workspace.package].version` 一致 |
| 7 | 拓扑 dry-run | `cargo publish --dry-run -p <crate>` | 按 §2 顺序全部通过 |
| 8 | 正式发布 | `cargo publish -p <crate>` | 按 §2 顺序成功，索引可见后才继续 |
| 9 | 安装验证 | `cargo install poly-cli --version 1.14.0 --locked` | 安装 bin `poly`，`poly --version` 输出 `1.14.0` |

## 7. 版本与 tag 策略

- `[workspace.package].version` 是唯一版本源；9 个 crate 与 workspace 内部依赖的 `version` 必须在同一提交内一起提升。
- tag 使用 `vX.Y.Z`，与 `CHANGELOG.md` 的 `[X.Y.Z]` 段落一致；tag 打在与发布构建相同的提交上。
- crates.io 版本不可覆盖：只允许 yank，不允许重发同一版本；漏发内容必须提升 patch 版本。
- 发布顺序遵循 §2 的依赖图；预发布版本（`-alpha.N` 等）需维护者单独决策，不随常规发布自动使用。
