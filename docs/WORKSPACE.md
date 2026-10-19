# 多应用工作区（M9-E2）

> 状态：代码与测试已交付（T-M9-04/05/06）；单应用行为与缓存键保持不变。`poly retry` / `poly verify` 暂不支持 `--workspace` / `--app`。

## 1. 目录布局

```text
my-workspace/
  polypack.toml              # 根配置：schema + [workspace].members
  apps/
    notes/
      polypack.toml          # 成员自己的单应用配置
      core/
      ui/
    timer/
      polypack.toml
      core/
      ui/
```

成员 id 取目录最后一段（`notes`、`timer`），必须唯一；成员路径必须是工作区根下的相对路径，`..` / 绝对路径 / 符号链接逃逸会被拒绝。

## 2. 根配置

```toml
schema = 1

[workspace]
members = ["apps/notes", "apps/timer"]
```

根配置只负责声明成员；`[package]` / `[ui]` / `[core]` / `[targets.*]` 仍在各成员自己的 `polypack.toml` 中。没有 `[workspace]` 时完全走旧单应用路径。

## 3. 成员配置

成员配置与现有单应用配置相同，例如：

```toml
schema = 1

[package]
name = "Pax Notes"
version = "1.2.3"

[ui]
framework = "react"
entry = "src/main.tsx"

[core]
crate = "core"

[targets.linux]
format = "deb"
```

## 4. 命令

```bash
# 构建工作区全部成员（按声明序）
poly build --workspace

# 只构建一个成员
poly build --app notes

# 指定目标（与现有 --target 组合）
poly build --workspace --target linux
```

`--workspace` 与 `--app` 互斥；都不给时行为与单应用版本一致。构建产物与快照按应用隔离：

```text
<workspace>/.polypack/packages/<app>/<target>/
<workspace>/.polypack/artifacts/<app>/<target>/
<workspace>/.polypack/cache/            # 工作区共享
```

共享 core 的 compile 缓存键不含 app id（跨应用复用）；UI bundle 缓存键混入 app id（避免串包）。

> 可运行 `scripts/demo-workspace.sh` 用 `poly init` 生成两个应用并演示二跑缓存命中；用 `POLY_BIN` 指定 `poly` 路径，用 `POLY_DEMO_TARGET` 覆盖目标。

## 5. 诊断

| 错误码 | 含义 |
| --- | --- |
| `E_WS_0001` | `[workspace]` 声明非法（类型 / 空成员 / 保留路径等） |
| `E_WS_0002` | 成员目录或成员 `polypack.toml` 缺失 / 无法加载 |
| `E_WS_0003` | 成员路径逃逸工作区根（含符号链接规范化后越界） |
| `E_WS_0004` | `--app <id>` 指向未声明成员 |

## 6. 已知限制

- `poly retry` / `poly verify` 不接受 `--workspace` / `--app`；指向成员配置时使用成员本地 `.polypack`。
- 无跨应用任务图、无目标继承、无并发调度；工作区成员串行构建。
- 成员 id 取目录名，重命名目录会改变 app id 与缓存键。
- 构建历史目前写在成员根，不在工作区根聚合。
