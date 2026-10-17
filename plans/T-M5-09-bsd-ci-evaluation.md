# T-M5-09 BSD CI 方案评估（侦察草稿）

> 状态：草稿（未进 `docs/`；定稿与文档 diff 由 lead 合入）
> 基线：`main@5402194`；依赖：T-M5-06 / T-M5-07 入 main 后落地本方案。
> 日期：2026-10-17

## 结论先行

**采用 QEMU VM 内原生构建 + 安装冒烟（每夜 + 手动触发），不做交叉编译替代。**
新增 `.github/workflows/bsd-ci.yml`，两个 job：

- `freebsd`：`vmactions/freebsd-vm@v1`（FreeBSD 14.5 x86_64）内 `poly build --target freebsd` → `poly verify`（`pkg check`）→ `pkg install` 本地包 → 安装后的入口二进制启动（`--version` 形态）冒烟；
- `openbsd`：`vmactions/openbsd-vm@v1`（OpenBSD 7.9 x86_64）内 `poly build --target openbsd` → ports 骨架 `make package` → `pkg_add` 安装 → 入口二进制启动冒烟。

交叉编译只保留为「未来可选的每提交编译快检」，不进入本任务验收，也不替代安装冒烟。

## 1. 约束与事实（2026-10-17 核实）

| 事实 | 证据 | 影响 |
|---|---|---|
| `pkg create` / `pkg check` / `pkg install` 只在 FreeBSD 用户态存在；ports `make package` / `signify` 只在 OpenBSD 用户态存在 | 工具归 BSD base / ports 框架 | 打包 + 安装冒烟必须在 BSD VM 内原生执行 |
| rustup 有 `x86_64-unknown-freebsd` 预编译 std；**没有 OpenBSD target**（Tier 3，无官方 std 构建） | `rustup target list` 本机实测 | 交叉编译无法覆盖 OpenBSD；FreeBSD 交叉也只到「编译」，仍缺打包/安装 |
| FreeBSD 14.5 ports：`lang/rust` latest 1.99.1 / quarterly 1.98.1 | freshports + pkg 索引实测 | 满足 workspace `rust-version = 1.98` |
| OpenBSD 7.9 packages：`rust-1.94.1`；snapshot packages：`rust-1.98.1` | cdn.openbsd.org 包索引实测 | 7.9 正式包低于声明 MSRV，需 `--ignore-rust-version`（见下） |
| 当前工作区在 Rust **1.88.0** 下 `cargo check --workspace --all-targets --ignore-rust-version` 通过（exit 0，1m50s） | 本机实测（`/tmp/bsd-ci-msrv-target`） | 代码实际下限 ≤ 1.88，OpenBSD 7.9 的 1.94.1 足够；`rust-version = 1.98` 是政策声明，不是当前代码硬依赖 |
| GitHub 托管 `ubuntu-latest` 是否给 `/dev/kvm` 取决于 runner；vmactions 检测到 `/dev/kvm` 会对 x86_64 guest 用 KVM，否则 TCG | vmactions `index.js` 的 `/dev/kvm` 分支与注释 | 工作流按两种加速都成立；job 超时按 TCG 上限给足 |
| 本机 QEMU 可用（`qemu-system-x86_64` / `qemu-img`，`/dev/kvm` 在场） | 本机实测 | 执行期可做本地真跑；FreeBSD 14.5 镜像（653 MB）已在后台下载 |

## 2. 方案对比

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| A. Linux 宿主交叉编译 | 每提交可跑、最快 | FreeBSD 需自备 sysroot/linker；OpenBSD 无 rustup std；**产不出 pkg/ports 包、无法安装冒烟** | 不满足 T-M5-06/07 验收 |
| B. QEMU VM 原生构建 + 冒烟 | 真实用户态：`pkg` / ports / `signify` 全链；vmactions 提供双向同步与镜像缓存；与验收逐字对应 | 每夜成本；无 KVM 时 TCG 慢；BSD 包版本滞后（OpenBSD rust 1.94.1） | **采用** |
| C. 混合：A 做每提交编译门 + B 做每夜冒烟 | 反馈快 | 需维护两套工具链；A 对 OpenBSD 仍不可行 | 记录为后续可选增强（E3 类），不占本任务 |

**定型理由**：本任务的验收是「CI 每夜构建 + 冒烟通过；构建日志与产物归档可追溯」，打包与安装冒烟天然要求 BSD 用户态；交叉编译连 FreeBSD 的 `pkg` 包都产不出。交叉编译唯一价值是快速编译反馈，当前 `rust-ci` 三宿主已覆盖编译面，BSD 每夜一次足够。

## 3. 落地设计（`bsd-ci.yml` 骨架）

```yaml
name: bsd-ci

on:
  schedule:
    # 每夜：与 mobile-contract（18:37）/ miniprogram-contract（18:47）/
    # tv-smoke（18:57）/ bridge（19:23）错峰
    - cron: "7 20 * * *"
  workflow_dispatch:

permissions:
  contents: read

jobs:
  freebsd:
    name: freebsd / 14.5 / qemu
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - name: FreeBSD VM（原生构建 + pkg 安装冒烟）
        uses: vmactions/freebsd-vm@v1
        with:
          release: "14.5"
          arch: x86_64
          mem: 6144
          cpu: 4
          copyback: true
          cache-after-prepare: true
          prepare: |
            set -eu
            env ASSUME_ALWAYS_YES=yes pkg bootstrap -f
            pkg update -f
            pkg install -y rust
            rustc --version
          run: |
            set -eu
            WORKSPACE=$(pwd)                    # vmactions 双向同步的仓库根
            export CARGO_TARGET_DIR=/tmp/polypack-cargo
            POLY=/tmp/polypack-cargo/debug/poly
            LOGS="$WORKSPACE/.polypack/ci-logs/freebsd"
            mkdir -p "$LOGS"
            run_stage() { name=$1; shift; "$@" >"$LOGS/$name.log" 2>&1; rc=$?; cat "$LOGS/$name.log"; return $rc; }
            run_stage rustc rustc --version
            run_stage build cargo build -p poly-cli
            run_stage init "$POLY" init /tmp/bsd-app
            cd /tmp/bsd-app
            run_stage poly-build "$POLY" build --target freebsd
            # 产物在 /tmp/bsd-app/.polypack/packages/freebsd：copback 只同步 workspace，
            # 先复制回 $WORKSPACE/.polypack/ci-artifacts/freebsd 再上传
            mkdir -p "$WORKSPACE/.polypack/ci-artifacts/freebsd"
            cp -R .polypack/packages/freebsd/. "$WORKSPACE/.polypack/ci-artifacts/freebsd/"
            # 安装冒烟按 T-M5-06 实际包名/入口接线（见「关键口径」第 2 条）
      - name: 留证据（日志 + pkg 产物；失败也留）
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: bsd-freebsd-evidence
          path: |
            .polypack/ci-logs/freebsd/**
            .polypack/ci-artifacts/freebsd/**
          include-hidden-files: true
          if-no-files-found: error
          retention-days: 14
```

OpenBSD job 同构：`vmactions/openbsd-vm@v1`、`release: "7.9"`、`prepare` 里 `pkg_add rust` + 拉取 ports 树（`https://cdn.openbsd.org/pub/OpenBSD/7.9/ports.tar.gz` → `/usr/ports`），`run` 里 `cargo build -p poly-cli --ignore-rust-version` → `poly build --target openbsd` → ports 目录 `make package` → `pkg_add` 安装 → 入口二进制启动冒烟。

**关键口径**

1. **构建位置**：`poly build` 在 VM 内原生跑；`CARGO_TARGET_DIR=/tmp/polypack-cargo` 避开 vmactions 双向同步把 GB 级 target 搬回宿主。
2. **入口冒烟**：无 GUI 会话，按 T-M1-03 既有口径用「安装后的入口二进制执行（`--version` / 启动即打印版本后退出）」代替 GUI 启动；退出码 0 即通过。具体二进制名/Plist 路径由 T-M5-06/07 的打包器决定，执行期接线。
3. **日志**：`.polypack/ci-logs/<os>/<stage>.log`（工作区内，copyback 回宿主）；每阶段先落盘再上屏，失败时也能留证。
4. **缓存**：`cache-after-prepare: true` 把「装好 rust 的 VM 镜像」缓存；镜像与包安装是每夜主要固定成本。
5. **产物**：`.polypack/packages/<target>/**` + 日志一起 `upload-artifact@v4`（14 天）；`include-hidden-files: true`（`.polypack` 为隐藏目录）。
6. **Rust 版本**：FreeBSD 走包管理器 rust（≥1.98）；OpenBSD 7.9 的 1.94.1 加 `--ignore-rust-version`，并在 workflow 注释里写明「仅因发行版包滞后；树在 1.88 上 check 全绿，升级到 OpenBSD 8.0 / snapshot rust 后移除」。
7. **触发**：仅 `schedule` + `workflow_dispatch`（VM 成本不进每提交/PR）；`permissions: contents: read`；job 级 env 不引用 `runner.*`（今天 tv-smoke 的零 job 实证）。

## 4. 文档 diff（docs/ 归 lead；此处给建议文本）

### docs/ROADMAP.md §8

在「CI 宿主 × 目标层矩阵」表后补一行/一段：

```markdown
| 每晚扩展 | — | 真实开发者工具 CLI | 模拟器冒烟 | QEMU 冒烟（FreeBSD pkg / OpenBSD ports 原生构建 + 安装） | — |

BSD 两目标由 [`.github/workflows/bsd-ci.yml`](../.github/workflows/bsd-ci.yml) 在 QEMU VM 内**原生构建 + 安装冒烟**（每夜 + 手动触发），不做交叉编译替代：`pkg` / ports 工具链只在 BSD 用户态存在；构建日志与产物随作业归档 14 天。
```

### docs/ROADMAP.en.md §8

```markdown
| nightly extension | — | real devtools CLIs | emulator smoke | QEMU smoke (FreeBSD pkg / OpenBSD ports native build + install) | — |

The two BSD targets run **native builds plus install smoke** in QEMU VMs via [`.github/workflows/bsd-ci.yml`](../.github/workflows/bsd-ci.yml) (nightly + manual dispatch); no cross-compilation substitute — the `pkg` and ports toolchains only exist in BSD userland. Build logs and artifacts are retained with the job for 14 days.
```

### README.md / README.en.md 状态行

- 「七工作流三宿主全绿」→「八工作流三宿主全绿（新增 BSD 每夜 QEMU 构建 + 安装冒烟）」；
- `poly release --target fedora,freebsd`（rpm/pkg 自建仓库：产物上传 + 元数据刷新与签名 + 回执归档）加入可用命令清单（T-M5-11 完成后）。
- 英文对应：`Eight workflows green across three hosts (plus a nightly BSD QEMU build + install-smoke workflow)`；`poly release --target fedora,freebsd` (rpm/pkg self-hosted repos: artifact upload + metadata refresh/signing + receipt archival).

### docs/TASKS.md 进度行（lead）

M5 交付时补：`+ M5 新增一工作流（bsd-ci 每夜 QEMU 原生构建 + 安装冒烟）`。

## 5. 验收与证据计划

- **本地 actionlint**：`/home/wwwroot/go/bin/actionlint .github/workflows/*.yml` 全绿（新文件必须过）。
- **本地真跑（执行期）**：
  - FreeBSD：本机 QEMU + FreeBSD 14.5 cloud-init 镜像（已在后台下载），SSH 进 VM 跑与 workflow 相同的 `run` 脚本，留 `run.log` / 包产物；
  - OpenBSD：优先官方 7.9 安装镜像 autoinstall；镜像带宽不许可时按纪律 **SKIP-with-print**（打印原因与替代证据：CI 工作流 + actionlint + 命令级推演），绝不静默。
- **CI 真跑**：workflow 落地后由 lead 合并到 main，等待首个 schedule / 手动 dispatch；日志链接与 artifacts 作为最终证据（本 agent 不能 push，无法自行触发远端）。
- **追溯性**：workflow run URL + `bsd-<os>-evidence` artifact（日志 + 包）14 天。

### 5.1 本地 QEMU 真跑实录（FreeBSD，2026-10-17）

环境：qemu-system-x86_64 + KVM（`-cpu host -m 4096 -smp 4`），FreeBSD 14.5-RELEASE-p2
cloud-init amd64 镜像（华为云镜像站，解压后 virtio 根盘扩至 13G）；VM 内 `rust-1.98.1`
来自 FreeBSD quarterly pkg（与仓库声明 MSRV 完全一致），crates.io 走 rsproxy sparse。
源码 = `git archive`（当时 `main@9595262`），构建目录 `/root/polypack-target`。

| 检查 | 命令 | 结果 |
|---|---|---|
| 原生构建 | `cargo build -p poly-cli --locked` | 绿（6bba6c4 冷编译 1m20s；9595262 增量 14.29s） |
| 单测 | `cargo test -p poly-cli --test release` | 7/7 绿（原生 FreeBSD） |
| lint | `cargo clippy -p poly-cli --all-targets -- -D warnings` | 绿（原生 FreeBSD，clippy 0.1.98） |
| 冒烟 | `poly init /tmp/bsd-app2` → `poly doctor --json` | rc=0；`fedora` 在目标矩阵，`freebsd` 缺（T-M5-06 未入 main，`poly build --target freebsd` 如实 `E_TGT_0001`） |

日志与 machine 信息归档在本地 `/tmp/bsd-ci-evidence/freebsd-local/`（不入仓）。
注意：`git archive` 保留提交 mtime，可能早于既有构建产物导致 cargo 误判 fresh；
本地复跑前须 `find <src> -exec touch {} +`。CI 的 `actions/checkout` 会刷新 mtime，不复现。

**OpenBSD 本地真跑：SKIP-with-print。** 官方安装介质不可直接当 SSH 可用镜像；
vmactions 镜像资产 `openbsd-7.9.qcow2.zst`（595 MB）实测直连 GitHub 75 KB/s、
gh-proxy 96 KB/s，需 ~1.7 小时且可能中断，按纪律不消耗该带宽。替代证据：草稿 workflow
（actionlint 绿）+ T-M5-07 ports 单测/verify 已在 main 真跑；OpenBSD 侧安装冒烟待 CI
首次 dispatch 取证。FreeBSD 的 `.pkg` 安装冒烟同样待 T-M5-06 落 main 后接线。

## 6. 风险与未决

| 风险 | 缓解 |
|---|---|
| OpenBSD 7.9 的 rust 1.94.1 未来可能撑不住新代码 | `--ignore-rust-version` 下失败即红（真实可移植性信号）；升级 OpenBSD 8.0 / snapshot rust 后移除 flag |
| 无 KVM 的 runner 上 TCG 超时 | `timeout-minutes: 60`；`cache-after-prepare` 缓存装好 rust 的镜像；必要时后续换 larger runner |
| T-M5-06/07 的产物落点/包名/冒烟入口尚未定 | 执行期以它们的实际实现接线；本方案只固定「VM 内原生 + 安装后入口执行」口径 |
| ports 树下载 ~ 每夜固定成本 | 放进 `prepare` 并 `cache-after-prepare: true`，随 VM 镜像缓存 |
| `poly build` 的 UI 支线缺 vite 时走内建静态打包（Warning） | 冒烟只验证包安装与入口执行，不把 UI 引擎降级当失败；日志保留 Warning |

## 附：T-M5-11 执行期计划（摘要）

`poly release --target fedora,freebsd`：新增 `crates/poly-cli/src/release.rs` + `Command::Release`；逐目标定位 `.polypack/packages/<target>` 产物 → 调 T-M5-05 仓库元数据/签名抽象写入 `<repo>` → 回执 `.polypack/release/<target>/<version>-<digest16>.json`；`--json` 发 `published` 事件（含 receipt 路径）；二跑按「产物集合清单摘要」命中即 no-op；失败走新 `E_RELEASE_*` 稳定码 + 修复提示。复用 M1/M2 `BuildEvent`/JSON Lines 事件面与回执归档口径，不新起发布 DAG。
