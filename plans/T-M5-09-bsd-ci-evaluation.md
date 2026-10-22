# T-M5-09 BSD CI 方案评估（侦察草稿）

> 状态：草稿（未进 `docs/`；定稿与文档 diff 由 lead 合入）
> 基线：`main@5402194`；依赖：T-M5-06 / T-M5-07 入 main 后落地本方案。
> 日期：2026-10-17

## 结论先行

**采用 QEMU VM 内原生构建 + 安装冒烟（每夜 + 手动触发），不做交叉编译替代。**
新增 `.github/workflows/bsd-ci.yml`，两个 job：

- `freebsd`：`vmactions/freebsd-vm@v1`（FreeBSD 14.5 x86_64）内 `polypack build --target freebsd` → `polypack verify`（`pkg check`）→ `pkg install` 本地包 → 安装后的入口二进制启动（`--version` 形态）冒烟；
- `openbsd`：`vmactions/openbsd-vm@v1`（OpenBSD 7.9 x86_64）内 `polypack build --target openbsd` → ports 骨架 `make package` → `pkg_add` 安装 → 入口二进制启动冒烟。

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
            POLYPACK=/tmp/polypack-cargo/debug/polypack
            LOGS="$WORKSPACE/.polypack/ci-logs/freebsd"
            mkdir -p "$LOGS"
            run_stage() { name=$1; shift; "$@" >"$LOGS/$name.log" 2>&1; rc=$?; cat "$LOGS/$name.log"; return $rc; }
            run_stage rustc rustc --version
            run_stage build cargo build -p polypack-cli
            run_stage init "$POLYPACK" init /tmp/bsd-app
            cd /tmp/bsd-app
            run_stage polypack-build "$POLYPACK" build --target freebsd
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

OpenBSD job 同构：`vmactions/openbsd-vm@v1`、`release: "7.9"`、`prepare` 里 `pkg_add rust` + 拉取 ports 树（`https://cdn.openbsd.org/pub/OpenBSD/7.9/ports.tar.gz` → `/usr/ports`），`run` 里 `cargo build -p polypack-cli --ignore-rust-version` → `polypack build --target openbsd` → ports 目录 `make package` → `pkg_add` 安装 → 入口二进制启动冒烟。

**关键口径**

1. **构建位置**：`polypack build` 在 VM 内原生跑；`CARGO_TARGET_DIR=/tmp/polypack-cargo` 避开 vmactions 双向同步把 GB 级 target 搬回宿主。
2. **入口冒烟**：无 GUI 会话，按 T-M1-03 既有口径用「安装后的入口二进制执行（`--version` / 启动即打印版本后退出）」代替 GUI 启动；退出码 0 即通过。具体二进制名/Plist 路径由 T-M5-06/07 的打包器决定，执行期接线。
3. **日志**：`.polypack/ci-logs/<os>/<stage>.log`（工作区内，copyback 回宿主）；每阶段先落盘再上屏，失败时也能留证。
4. **缓存**：`cache-after-prepare: true` 把「装好 rust 的 VM 镜像」缓存；镜像与包安装是每夜主要固定成本。
5. **产物**：`.polypack/packages/<target>/**` + 日志一起 `upload-artifact@v4`（14 天）；`include-hidden-files: true`（`.polypack` 为隐藏目录）。
6. **Rust 版本**：FreeBSD 走包管理器 rust（≥1.98）；OpenBSD 7.9 的 1.94.1 加 `--ignore-rust-version`，并在 workflow 注释里写明「仅因发行版包滞后；树在 1.88 上 check 全绿，升级到 OpenBSD 8.0 / snapshot rust 后移除」。
7. **触发**：仅 `schedule` + `workflow_dispatch`（VM 成本不进每提交/PR）；`permissions: contents: read`；job 级 env 不引用 `runner.*`（今天 tv-smoke 的零 job 实证）。

## 4. 文档 diff（docs/ 归 lead；此处给建议文本）

### docs/ROADMAP.md §8

在「CI 宿主 × 目标层矩阵」表中把「每晚扩展」行的 QEMU 列改为「QEMU 冒烟（BSD 原生构建 + 安装）」，并在表后补一段：

```markdown
| 每晚扩展 | — | 真实开发者工具 CLI | 模拟器冒烟 | QEMU 冒烟（BSD 原生构建 + 安装） | — |

BSD 原生构建腿由 [`bsd-ci`](../.github/workflows/bsd-ci.yml)（每夜 20:07 UTC + 手动）执行：
FreeBSD 14.5 / OpenBSD 7.9 QEMU VM 内 `polypack build` 原生出包并做安装冒烟（FreeBSD 走
`pkg install` + `pkg check -s`，OpenBSD 走 `make package` + `pkg_add` / `pkg_check`）；
FreeBSD 腿同时真跑自托管 pkg 仓库签名（`polypack release --target freebsd`）与二跑幂等断言。
构建日志、签名仓库、回执与包随作业归档 14 天。CI 工作流共八个：`rust-ci` / `docs-check` /
`secret-scan` / `mobile-contract` / `miniprogram-contract` / `tv-smoke` / `bridge` / `bsd-ci`。
```

### docs/ROADMAP.en.md §8

```markdown
| nightly extension | — | real devtools CLIs | emulator smoke | QEMU smoke (BSD native build + install) | — |

The BSD native-build leg runs in [`bsd-ci`](../.github/workflows/bsd-ci.yml) (nightly 20:07 UTC +
manual dispatch): inside FreeBSD 14.5 / OpenBSD 7.9 QEMU VMs it runs a native `polypack build` plus an
install smoke (`pkg install` + `pkg check -s` on FreeBSD; `make package` + `pkg_add` / `pkg_check`
on OpenBSD). The FreeBSD leg also exercises self-hosted pkg-repo signing via
`polypack release --target freebsd` and asserts second-run idempotency. Build logs, the signed repo,
receipts, and packages are retained with the job for 14 days. Eight CI workflows total: `rust-ci` /
`docs-check` / `secret-scan` / `mobile-contract` / `miniprogram-contract` / `tv-smoke` / `bridge` /
`bsd-ci`.
```

### README.md / README.en.md 状态行

- 「七工作流三宿主全绿」→「八工作流三宿主全绿（新增 `bsd-ci`：FreeBSD 14.5 / OpenBSD 7.9 QEMU 原生构建 + 安装冒烟，每夜）」；
- 可用命令清单在 `polypack clean` 后补：`` `polypack release`（自托管 rpm/deb/pkg 仓库元数据 + 签名 + 回执归档，`--target fedora,freebsd`） ``。
- 英文对应：`Eight workflows green across three hosts (new bsd-ci: native FreeBSD 14.5 / OpenBSD 7.9 QEMU builds + install smoke, nightly)`；`polypack release` (self-hosted rpm/deb/pkg repo metadata + signing + receipt archival, `--target fedora,freebsd`).

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
| 原生构建 | `cargo build -p polypack-cli --locked` | 绿（6bba6c4 冷编译 1m20s；9595262 增量 14.29s） |
| 单测 | `cargo test -p polypack-cli --test release` | 7/7 绿（原生 FreeBSD） |
| lint | `cargo clippy -p polypack-cli --all-targets -- -D warnings` | 绿（原生 FreeBSD，clippy 0.1.98） |
| 冒烟 | `polypack init /tmp/bsd-app2` → `polypack doctor --json` | rc=0（首轮；`freebsd` 当时因 T-M5-06 未入 main 如实 `E_TGT_0001`，最终安装冒烟见 5.2） |

日志与 machine 信息归档在本地 `/tmp/bsd-ci-evidence/freebsd-local/`（不入仓）。
注意：`git archive` 保留提交 mtime，可能早于既有构建产物导致 cargo 误判 fresh；
本地复跑前须 `find <src> -exec touch {} +`。CI 的 `actions/checkout` 会刷新 mtime，不复现。

### 5.2 本地 QEMU 真跑实录（FreeBSD pkg 安装冒烟，T-M5-09 / T-M5-11 freebsd 腿）

环境同 5.1；源码同步到 worktree HEAD（T-M5-05/06 已 merge，含 `Host::Bsd` 修复），
VM 内 cargo 增量构建 `polypack-cli`。凭据为一次性未加密 PEM RSA 私钥（CI/冒烟专用，
`key_alias=rsa`，`store_password` 非空但不参与 pkg repo 签名）。

| 检查 | 命令 | 结果 |
|---|---|---|
| 出包 | `polypack build --target freebsd`（`polypack init` 模板应用） | 绿；`.pkg` 207,802 B，15 个载荷文件；`pkg info -F` 列出全部 `/usr/local` 路径 |
| 发布 | `polypack release --target freebsd --credential env:POLYPACK_PKG_CRED --json` | 绿；`event=published`、`cache_hit=false`、`runs=1`、receipt `.polypack/release/freebsd/0.1.0-ee90c20b7c65ea97.json` |
| 仓库 | `pkg repo` + `openssl pkey -pubout` | `meta.conf` / `data.pkg` / `packagesite.pkg` / `polypack-repo.pub` 齐全 |
| 客户端 | `pkg update -f`；`pkg install -y polypack-app`；`pkg check -s` | 全绿；签名仓库 1 package processed，安装 15 文件，check rc=0 |
| 负向验签 | 换成另一把 PUBKEY 后 `pkg update -f` | `signature verification failure` / `Invalid signature, removing repository`（pkg 仍返回 0，属 pkg(8) 行为） |
| 二跑 | 再次 `polypack release --target freebsd ... --json` | `cache_hit=true`、`runs=2`；仓库文件 sha256 与 mtime 完全不变，`first_unix` 保留 |

**真跑发现并修复的 T-M5-06 缺陷**：`pkg create -M` 不会扫描 `-r` 根目录；无 `files{}` / `-p`
plist 时产出 371 B 空包（`pkg info -F` 与打包器自检均假绿，安装 0 文件）。修复：打包器
从同一载荷表生成 `<name>-<version>.plist` 并以 `pkg create -p` 传入（commit 5a191d3，
polypack-bundle 104 unit + 集成全绿，clippy 绿）。修复后 `pkg install` 真实落 15 个文件。

证据归档：本地 `/tmp/bsd-ci-evidence/freebsd-pkg/`（ci-logs / repo / release / pkg /
manifest / plist / 负向验签日志）；另有 `/tmp/bsd-ci-evidence/freebsd-local/`（5.1）。

**多目标编排**：FreeBSD VM 内 `polypack release --target fedora,freebsd --dry-run --json`
一次调用输出两条 `release-dry-run` 事件（`repo_kind=rpm` / `pkg`，各自 receipt 锚点）；
rpm/pkg 真腿证据分别在宿主 `/tmp/release-e2e/` 与 VM 上述归档。注意当前 CLI 的
`--credential` 是单一全局引用，而 rpm 走 GPG、pkg 走未加密 PEM，异构目标同次真发布的
凭据需分次给（或后续加 per-target 凭据语法）——已报 lead 作为待集成项。

**OpenBSD 本地真跑：SKIP-with-print。** 官方安装介质不可直接当 SSH 可用镜像；
vmactions 镜像资产 `openbsd-7.9.qcow2.zst`（595 MB）实测直连 GitHub 75 KB/s、
gh-proxy 96 KB/s，需 ~1.7 小时且可能中断，按纪律不消耗该带宽。替代证据：落地 workflow
（commit 6f6bb16，actionlint v1.7.12 绿）+ T-M5-07 ports 单测/verify 已在 main 真跑；
OpenBSD 侧 `make package` / `pkg_add` 冒烟待 CI 首次 dispatch 取证（失败按 SKIP-with-print
打 `::warning::`，不伪造安装）。

## 6. 风险与未决

| 风险 | 缓解 |
|---|---|
| OpenBSD 7.9 的 rust 1.94.1 未来可能撑不住新代码 | `--ignore-rust-version` 下失败即红（真实可移植性信号）；升级 OpenBSD 8.0 / snapshot rust 后移除 flag |
| 无 KVM 的 runner 上 TCG 超时 | `timeout-minutes: 90`；`cache-after-prepare` 缓存装好 rust 的镜像；必要时后续换 larger runner |
| T-M5-07 生成的 ports Makefile 未 `.include <bsd.port.mk>`（本地静态检查发现），`make package` 可能无 target | workflow 已按 SKIP-with-print 如实注记并归档 make 日志；待 T-M5-07 owner 补 include 后 OpenBSD 安装冒烟转真跑（本地无 OpenBSD 环境，未代改） |
| ports 树下载 ~ 每夜固定成本 | 放进 `prepare` 并 `cache-after-prepare: true`，随 VM 镜像缓存 |
| `polypack build` 的 UI 支线缺 vite 时走内建静态打包（Warning） | 冒烟只验证包安装与入口执行，不把 UI 引擎降级当失败；日志保留 Warning |

## 附：T-M5-11 执行期计划（摘要）

`polypack release --target fedora,freebsd`：新增 `crates/polypack-cli/src/release.rs` + `Command::Release`；逐目标定位 `.polypack/packages/<target>` 产物 → 调 T-M5-05 仓库元数据/签名抽象写入 `<repo>` → 回执 `.polypack/release/<target>/<version>-<digest16>.json`；`--json` 发 `published` 事件（含 receipt 路径）；二跑按「产物集合清单摘要」命中即 no-op；失败走新 `E_RELEASE_*` 稳定码 + 修复提示。复用 M1/M2 `BuildEvent`/JSON Lines 事件面与回执归档口径，不新起发布 DAG。
