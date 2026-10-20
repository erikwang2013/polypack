# 增量发布：`poly release --changed`

`poly release --changed` 在多目标仓库里只重发变更过的目标。判定键来自产物集合的
清单摘要（`manifest_digest` / `cache_key`）；命中既有回执的目标输出独立的
`release-skipped` 事件并以 0 退出，不触仓库后端、不重写回执、`runs` 不递增。

## 判定与事件

- 清单摘要：产物按名排序后混入名字、字节数与 SHA256（顺序无关、内容绑定）。
- 回执键：`<工程根>/.polypack/release/<target>/<version>-<digest16>.json`。
- 命中条件：回执的 `cache_key`（旧回执降级为 `manifest_digest`）、仓库根与仓库格式
  三者一致。
- 未命中（新产物 / 新目标 / 换仓库）照常发布，并在 `--changed` 路径写入 `cache_key`。
- 首跑用普通 `poly release` 留下的回执没有 `cache_key`：紧随其后的 `--changed` 会按
  `manifest_digest` 降级命中（`key_source: "manifest_digest"`，仍然跳过）；之后的
  `--changed` 回执带 `cache_key`（`key_source: "cache_key"`）。

## 5 目标夹具与命令

夹具用 5 个目标（`linux`、`web`、`windows`、`macos`、`android`）并把格式统一覆盖为
`deb`，每个目标在 `.polypack/packages/<target>/` 下放一份合法 deb：

```toml
[package]
name = "release-bench"
version = "0.1.0"

[targets.linux]
format = "deb"
# web / windows / macos / android 同上
```

两跑使用同一命令；首跑没有回执，因此 5 个目标全部发布，二跑只发布变更目标：

```bash
poly release --changed \
  --target linux,web,windows,macos,android \
  --repo .polypack/repo \
  --credential env:RELEASE_GPG_KEY \
  --json
```

改一个产物（例如重新打 `android` 的 deb）后再次执行同一命令，stdout 为 5 行 JSON
事件：1 行 `published` + 4 行 `release-skipped`，退出码 0。

## JSON `release-skipped` 契约

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `event` | string | 固定 `"release-skipped"`，与 `"published"` 严格区分 |
| `target` | string | 目标 id |
| `version` | string | 工程版本 |
| `repo` | string | 仓库根的绝对路径 |
| `repo_kind` | string | `deb` / `rpm` / `pkg` / `pkgsrc` |
| `manifest_digest` | string | 本次产物集合的清单摘要 |
| `cache_key` | string | 本次变更判定键（当前与清单摘要同源） |
| `receipt` | string | 命中的回执路径 |
| `artifact_count` | number | 产物数量 |
| `runs` | number | 回执中既有的执行次数（跳过不递增） |
| `key_source` | string | `cache_key` 或旧回执降级的 `manifest_digest` |
| `reason` | string | 固定 `"unchanged"` |
| `note` | string | 人读说明（不参与机器判定） |

## 实测节省

`scripts/bench-release-changed.sh` 生成夹具并跑两轮（Linux，debug 构建，一次测量）：

| 轮次 | published（= 后端调用） | release-skipped | 墙钟 |
| --- | --- | --- | --- |
| 首跑（5 个目标全发） | 5 | 0 | 8366 ms |
| 二跑（只改 1 个产物） | 1 | 4 | 1549 ms |
| 节省 | 80% | — | 81% |

后端调用数是确定性闸门（每行 `published` 对应一次仓库发布；`release-skipped` 不触
后端），脚本断言 5 → 1。墙钟受 gpg / 文件系统缓存影响，只报告不设阈值；同一夹具在
Rust 集成测试中的另一次测量为 11743 ms → 3163 ms（73%）。

## CI 接线

`rust-ci` 的既有 Linux 腿加一个聚焦步骤（不新开作业，复用已编译的测试二进制）：

```yaml
- name: release --changed 5 目标实测（Linux 聚焦）
  if: runner.os == 'Linux'
  run: |
    cargo test -p poly-cli --test release five_target_changed -- --nocapture 2>&1 \
      | tee /tmp/release-changed.log
    grep -q "first_published=5 second_published=1" /tmp/release-changed.log
```

测试缺 gpg 时打印 `SKIP` 并返回；`grep` 保证这条 Linux 腿真的跑到了测量而不是静默
跳过。`cargo test --workspace` 本身也已覆盖该测试。

## `--since` 的限制

`poly release` 不构建，回执也没有按目标的源码 / 包路径映射，无法用 git diff 稳健判定
目标是否变更。因此 `--since <REF>` 在本模式返回 `poly::E_PLAN_0002` 而不是静默忽略；
请改用 `--changed`（按回执判定），或先用 git diff 预筛 `--target`。
