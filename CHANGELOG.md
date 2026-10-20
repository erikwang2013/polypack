# Changelog

本仓库的显著变更记录于此。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。
版本号自 v1.0.0 起按语义化版本递增（tag 与 GitHub Release 名称一致，见 [README](README.md#路线图)）。
Notable changes are recorded here; since v1.0.0, tags and releases follow `vX.Y.Z` semantic increments.

## [1.13.1] — 2026-10-20

### Added
- **ENV-15 keyring 信任根载体（已关闭）**：新增 Linux CLI E2E `plugin_trust_root_keyring`（隔离 Secret Service 播种 → `source = "keyring"` → `plugin_granted` / `packaged` → 删除后 `E_PLUGIN_0010`）与 rust-ci `keyring-real` 作业（dbus-run-session + gnome-keyring，SKIP 即失败）；run 39409593528 通过。

*ENV-15 keyring trust-root carrier (closed): adds the Linux CLI E2E `plugin_trust_root_keyring` (isolated Secret Service seeding → `source = "keyring"` → `plugin_granted` / `packaged` → `E_PLUGIN_0010` after deletion) and the rust-ci `keyring-real` job (dbus-run-session + gnome-keyring, SKIP fails); run 39409593528 passed.*
- **ENV-16 Vault 信任根载体（已关闭）**：新增 Linux CLI E2E `plugin_trust_root_vault`（生产 `HttpVaultClient` 播种 KV v2 → `source = "vault"` → 正负路径）与 `HttpVaultClient::delete_payload`；rust-ci `vault-real` 用官方 HashiCorp 源启动 dev Vault，SKIP 即失败；run 39414212765 通过。

*ENV-16 Vault trust-root carrier (closed): adds the Linux CLI E2E `plugin_trust_root_vault` (production `HttpVaultClient` seeds KV v2 → `source = "vault"` → positive/negative paths) and `HttpVaultClient::delete_payload`; rust-ci `vault-real` starts a dev Vault from the official HashiCorp repository and fails on SKIP; run 39414212765 passed.*
- **ENV-17 S3 / 农场二跑（CI 已接线，PARTIAL）**：新增 rust-ci `s3-real`，本地 MinIO 上跑 `s3_real` 的 PUT / GET / LIST / DELETE 与 `farm_cache` 的两 worker 共享桶二跑（首跑 0/1、二跑 1/1），SKIP 即失败；MinIO server 因 `dl.min.io` 410 改由归档官方 tag 源码构建，`mc` 使用官方 release 并核对 SHA-256；run 39425835979 首跑通过。

*ENV-17 S3 / farm second run (CI wired, PARTIAL): adds rust-ci `s3-real`, running `s3_real` PUT / GET / LIST / DELETE and the two-worker farm shared-bucket rerun (0/1 then 1/1) on local MinIO, failing on SKIP; the MinIO server is built from an archived official tag because `dl.min.io` returns 410, and `mc` comes from the official release with its SHA-256 verified; the first run, 39425835979, passed.*

### Fixed
- **wasmtime 49.0.4**：修复 RUSTSEC-2026-0378（组件模型流载荷类型 panic）与 RUSTSEC-2026-0379（aarch64 Cranelift `br_table` 跳转错误）；精确版本钉从 `=49.0.3` 提升到 `=49.0.4`，`Cargo.lock` 同步，`cargo deny check advisories` 通过。

*wasmtime 49.0.4: fixes RUSTSEC-2026-0378 (component-model stream payload panic) and RUSTSEC-2026-0379 (aarch64 Cranelift `br_table` mis-jump); the exact pin moves from `=49.0.3` to `=49.0.4`, `Cargo.lock` is synced, and `cargo deny check advisories` passes.*

## [1.13.0] — 2026-10-20

### Added
- **M9-P3 LINE LIFF 部署模式**：新增 `poly deploy line-liff --dir <dir> --config line-liff.toml`（部署模式，非 TargetSpec）：校验 `schema=1`、`liff_id` / `endpoint` / `channel_id` / `scopes`、Endpoint 必须 HTTPS 且不含 fragment，输出 5 条 Console 人工步骤；始终 dry-run，不触网、不上传、不生成平台包；错误码 `poly::E_DEPLOY_0001`；新增 ADR 0009 与双语 `docs/LINE-DEPLOY`，8 个集成测试通过。

*M9-P3 LINE LIFF deploy mode: new `poly deploy line-liff --dir <dir> --config line-liff.toml` (a deploy mode, not a TargetSpec) validating `schema=1`, `liff_id` / `endpoint` / `channel_id` / `scopes`, and an HTTPS Endpoint without a fragment, then printing 5 manual Console steps; always a dry run with no network, upload, or platform package; error code `poly::E_DEPLOY_0001`; new ADR 0009 and bilingual `docs/LINE-DEPLOY`; 8 integration tests pass.*
- **M9-E3 实测（T-M9-09）**：`scripts/bench-release-changed.sh` 用 5 目标夹具测量 `--changed`：首跑 5 次后端发布 / 8366 ms，二跑 1 次发布 + 4 次 `release-skipped` / 1549 ms（调用数 5→1，墙钟 −81%；集成测试另一次测量 11743→3163 ms，−73%）；CI 在 Linux 上运行同一夹具并断言调用数 5→1，双语 `docs/RELEASE-CHANGED` 记录口径与限制。

*M9-E3 measurement (T-M9-09): `scripts/bench-release-changed.sh` measures `--changed` on a five-target fixture: the first run makes 5 backend publishes in 8366 ms, the second makes 1 publish + 4 `release-skipped` in 1549 ms (5→1 calls, −81% wall clock; a separate integration-test measurement was 11743→3163 ms, −73%); CI runs the same fixture on Linux and asserts 5→1 calls, with method and limits documented in both `docs/RELEASE-CHANGED` languages.*
- **M9-E6 GUI（T-M9-18）**：新增 Tauri v2 桌面视图 `app/`，通过 `history_records` command 读取 `.polypack/history/events.jsonl`，提供目标矩阵、单目标最近构建详情与空状态，读取时再次脱敏路径；`bundle.active=false`，不生成安装包；6 项测试与 clippy 通过，真实窗口启动按环境门控披露；双语 `docs/GUI`。

*M9-E6 GUI (T-M9-18): new Tauri v2 desktop view in `app/` that reads `.polypack/history/events.jsonl` via the `history_records` command, with a target matrix, per-target recent-build detail, and an empty state, re-redacting paths on read; `bundle.active=false` so no installer is produced; 6 tests and clippy pass, and real window launch is disclosed as environment-gated; bilingual `docs/GUI`.*
- **M9-E7 设备预览（T-M9-19/20）**：`preview/v1` 的 `DevicePool` / `DeviceLease` / `CleanupPolicy` 协议与 `poly preview --dry-run`（确定性计划，不租设备、不读凭据、不触网）；四家供应商调研矩阵与 build-vs-buy 建议；T-M9-21 真机安装 / 启动 / 截图仍为外部环境项。

*M9-E7 device preview (T-M9-19/20): the `preview/v1` `DevicePool` / `DeviceLease` / `CleanupPolicy` protocol and `poly preview --dry-run` (deterministic plan; no device lease, credentials, or network); a four-vendor research matrix and build-vs-buy guidance; T-M9-21 real-device install / launch / screenshot remains external.*
- **M9-G1 crates.io 审计（T-M9-45）**：workspace 版本 `0.0.0` → `1.12.0`（`poly --version` 一致），9 个 crate 元数据齐备、内部 path 依赖补版本，`cargo package --list` 9/9 通过并接入 Linux CI；`poly-cli` / `poly-core` 名称已被占用、全部 crate 仍 `publish = false`、无 token，实际发布保持关闭；双语 `docs/CRATES-IO`。

*M9-G1 crates.io audit (T-M9-45): workspace version `0.0.0` → `1.12.0` (`poly --version` agrees), metadata complete for all nine crates, versions added to internal path dependencies, and `cargo package --list` 9/9 wired into Linux CI; `poly-cli` / `poly-core` names are taken, every crate remains `publish = false`, and no token exists, so actual publishing stays closed; bilingual `docs/CRATES-IO`.*
- **M9-E4 T-M9-12 更新 / 回滚演练**：新增 `examples/updater-app/`（真实 Tauri v2 样例，含 `--smoke-update` 模式）、`examples/updater-app/scripts/drill.ps1`（一次性 Tauri / GPG 密钥、0.1.0 / 0.1.1 NSIS 安装器与 updater 签名、`poly update manifest` / `check`、本地 HTTP 渠道、安装 → 更新 → 恢复旧清单 → 回滚、证据归档与 `POLY_UPDATER_DRILL_0001..0009`），以及 rust-ci `windows-updater-real` 作业（windows-2022，禁止把 SKIP / 失败当通过，归档证据）；windows-2022 run 39395255929 首跑通过，Linux 侧同时验证了编译、清单签名链路与 workflow 语法。

*M9-E4 T-M9-12 update / rollback drill: adds `examples/updater-app/` (a real Tauri v2 sample with a `--smoke-update` mode), `examples/updater-app/scripts/drill.ps1` (throwaway Tauri / GPG keys, 0.1.0 and 0.1.1 NSIS installers and their updater signatures, `poly update manifest` / `check`, a local HTTP channel, install → update → restore the old manifest → roll back, evidence archiving, and `POLY_UPDATER_DRILL_0001..0009`), and the rust-ci `windows-updater-real` job (windows-2022, never treats SKIP / failure as success, uploads evidence); its first windows-2022 run, 39395255929, passed, while Linux also validated compilation, the manifest-signing path, and workflow syntax.*

### Changed
- **M9 平台决策收口（2026-10-20）**：Google Play Instant 标记 retired（保留行，不声明 TargetSpec）；Snapchat Mini 标记 closed 并按季度复审（下次 2027-01-20）；P4 / P5 为条件支持、P6–P8 为 NDA 门控，均链接 ADR 0006；49 目标表行数与 L1–L7 计数不变。

*M9 platform decisions closed out (2026-10-20): Google Play Instant marked retired (row kept, no TargetSpec claim); Snapchat Mini marked closed with quarterly review (next 2027-01-20); P4 / P5 are conditional and P6–P8 NDA-gated, all linking ADR 0006; the 49-target row count and L1–L7 counts are unchanged.*
- **M9-G5 分支保护关闭**：GitHub API 核实 `enforce_admins.enabled = true` 与 9 个必需检查上下文（含 3 个 secret-scan 上下文，2026-10-20）；ENV-18 由 `OPEN` 改为 `CLOSED`，TASKS T-M2-12 / T-M9-55 同步。

*M9-G5 branch protection closed: the GitHub API confirms `enforce_admins.enabled = true` and 9 required check contexts (including 3 secret-scan contexts, verified 2026-10-20); ENV-18 moved from `OPEN` to `CLOSED`, with TASKS T-M2-12 / T-M9-55 synced.*
- **工作区选择器扩展到 `retry` / `verify`**：`poly retry` 与 `poly verify` 接受 `--workspace` / `--app <id>`，与 `poly build` 共用解析与 `E_WS_0001..0004`；单应用路径不变，`docs/WORKSPACE` 双语同步。

*Workspace selectors extended to `retry` / `verify`: `poly retry` and `poly verify` accept `--workspace` / `--app <id>`, sharing resolution and `E_WS_0001..0004` with `poly build`; the single-app path is unchanged, and bilingual `docs/WORKSPACE` is synced.*
- **内部重构（无行为变更）**：`crates/poly-cli/src/publish.rs`（约 1000 行）拆为 `publish/{args,prepare,metadata,tests}.rs`，`publish.rs` 降至约 260 行；命令、回执与错误码不变。

*Internal refactor (no behavior change): `crates/poly-cli/src/publish.rs` (~1000 lines) is split into `publish/{args,prepare,metadata,tests}.rs`, leaving `publish.rs` at ~260 lines; commands, receipts, and error codes are unchanged.*

### Fixed
- **Windows MSYS gpg 路径兼容**：`poly-sign` 按 PATH 定位 gpg；命中 Git for Windows 的 MSYS gpg 时把 `--homedir` / `--import` / `--passphrase-file` / 签名载荷等路径参数转为 `/c/...`，并同步转换 `gpgconf --kill` 的 home。`poly update manifest` / `poly update check` / 插件验签在未安装 Gpg4win 的 Windows 上不再因路径解释失败；原生 Gpg4win 与 Linux/macOS 行为不变。

*Windows MSYS gpg path compatibility: `poly-sign` resolves gpg via PATH; when it finds Git for Windows' MSYS gpg, it converts path arguments (`--homedir` / `--import` / `--passphrase-file` / the signing payload) to `/c/...` and does the same for `gpgconf --kill`. `poly update manifest` / `poly update check` / plugin verification no longer fail on path interpretation on Windows without Gpg4win; native Gpg4win and Linux/macOS behavior are unchanged.*

## [1.12.0] — 2026-10-20

### Added
- **M9 决策记录（2026-10-20）**：新增 ADR 0004（去中心化签名插件索引）、ADR 0005（GA 仅自托管农场）、ADR 0006（8 个规划平台结论：关闭 / 条件 / NDA 门控）、ADR 0007（G5 外部动作 / E6 历史持久化优先 / E7 设备预览调研）；`docs/INITIATIVES` 双语状态同步。

*M9 decision records (2026-10-20): ADRs 0004 (signed decentralized plugin index), 0005 (self-hosted-only farm for GA), 0006 (eight planned-platform verdicts: close / conditional / NDA-gated), and 0007 (G5 external action / E6 persistence-first / E7 device-preview research); INITIATIVES statuses synced in both languages.*
- **M9 分发与流程**：`scripts/install.sh`（三宿主 Release 资产 + 强制 SHA256 + 可选 minisign 严格验签）、双语 `docs/INSTALL`、release 工作流的 secret-gated 签名与 install-smoke、双语 `docs/HUMAN-TRIAL` 试用协议、ENV-SLOTS 的 Owner/Evidence 列与 `tools/check-env-slots.mjs` 门禁（接入 docs-check）。T-M9-45（crates.io）、T-M9-49（真人试用）、T-M9-51（逐槽真实关闭）仍为外部项。

*M9 distribution and process: `scripts/install.sh` (three-host release assets + mandatory SHA-256 + optional strict minisign verification), bilingual `docs/INSTALL`, secret-gated release signing plus an install-smoke job, the bilingual `docs/HUMAN-TRIAL` protocol, and the ENV-SLOTS Owner/Evidence columns with the `tools/check-env-slots.mjs` gate wired into docs-check. T-M9-45 (crates.io), T-M9-49 (human trials), and T-M9-51 (per-slot real closure) remain external.*
- **M9-G4 MSI 真样本**：`examples/windows-msi-app/`（真实 Tauri v2 样例）、`crates/poly-bundle/tests/windows_msi_real.rs`（门控 `POLY_MSI_REAL=1`，安装 / 启动 / 卸载 / 残留检查，错误码 `POLY_MSI_REAL_0001..0006`）与 rust-ci `windows-msi-real` 作业；run 39308849058 在 windows-2022 真跑通过，ENV-01 已 `CLOSED`。

*M9-G4 MSI real sample: `examples/windows-msi-app/` (a real Tauri v2 sample), `crates/poly-bundle/tests/windows_msi_real.rs` (gated by `POLY_MSI_REAL=1`; install / launch / uninstall / residue checks; codes `POLY_MSI_REAL_0001..0006`), and the rust-ci `windows-msi-real` job; run 39308849058 passed on windows-2022, closing ENV-01.*
- **M9-E3 增量发布**：`poly release --changed` 比较回执 `cache_key`（当前等于 `manifest_digest`）并回退 `manifest_digest`，命中时发独立 `release-skipped` 事件、不写仓库 / 不重写回执 / 不递增 `runs`；`--since` 返回 `E_PLAN_0002`。T-M9-09 的 CI 示例与节时实测待补。

*M9-E3 incremental release: `poly release --changed` compares the receipt `cache_key` (currently equal to `manifest_digest`) with a `manifest_digest` fallback; a hit emits a distinct `release-skipped` event without writing the repo, rewriting the receipt, or incrementing `runs`; `--since` returns `E_PLAN_0002`. The T-M9-09 CI example and measured savings are pending.*
- **M9-E6 构建历史**：`HistoryRecord` 写入 `.polypack/history/events.jsonl`（路径脱敏为项目相对路径、产物 SHA-256 / 目录摘要、5 MiB / 10000 条双上限轮转并保留最新），`poly history [--target ID] [--limit N] [--json]` 按目标查询；T-M9-18 GUI 因 `app/` 未跟踪延后。

*M9-E6 build history: `HistoryRecord` is persisted to `.polypack/history/events.jsonl` (project-relative path redaction, artifact SHA-256 / directory digest, rotation at 5 MiB / 10,000 records keeping the newest), with `poly history [--target ID] [--limit N] [--json]` for per-target queries; the T-M9-18 GUI is deferred because `app/` is not tracked.*
- **M9-E5 商店元数据**：`store/metadata/<platform>/<locale>.toml`（`schema = 1`，微信 / 支付宝两套规则），校验返回全部稳定 `metadata.*` 违规码；`poly publish --metadata-dir` / `--check-metadata` / `--metadata-locale` 接入，`--materials` 与 `--metadata-dir` 互斥；材料按 key 排序并以 blake3(canonical JSON) 摘要写入回执 `materials_digest`；双语作者指南 `docs/STORE-METADATA.md`。

*M9-E5 store metadata: `store/metadata/<platform>/<locale>.toml` (`schema = 1`, WeChat / Alipay rule sets); validation reports every stable `metadata.*` violation; `poly publish --metadata-dir` / `--check-metadata` / `--metadata-locale` are wired, `--materials` and `--metadata-dir` are mutually exclusive; materials are key-sorted and their blake3(canonical JSON) digest is recorded as receipt `materials_digest`; bilingual author guide in `docs/STORE-METADATA.md`.*
- **M9-E2 多应用工作区**：根 `polypack.toml` 可选 `[workspace].members`，成员目录各自持有 `polypack.toml`；`poly build --workspace` / `--app <id>`；`JobSpec.app_id` 可省略且单应用序列化不变；共享 core compile 键跨应用一致、UI bundle 键按 app 隔离；产物 / 快照落 `.polypack/packages/<app>/<target>/` 与 `.polypack/artifacts/<app>/<target>/`；新增 `E_WS_0001..0004`、ADR 0008、`docs/WORKSPACE.md` 与 `scripts/demo-workspace.sh`。
- **M9-E4 OTA（部分）**：`poly update manifest` 生成并 GPG 签名的确定性 Tauri v2 `latest.json`（保留版本副本供回滚），`poly update check` 做 schema + 可选信任根验签；机制矩阵与回滚手册见 `docs/OTA.md`。真实 Tauri 安装 / 回滚演练仍为环境门控。

*M9-E2 multi-app workspace: optional root `[workspace].members` with per-member `polypack.toml`; `poly build --workspace` / `--app <id>`; `JobSpec.app_id` optional with unchanged single-app serialization; shared core compile keys stay identical while UI bundle keys are per-app; outputs/snapshots land in `.polypack/packages/<app>/<target>/` and `.polypack/artifacts/<app>/<target>/`; new `E_WS_0001..0004`, ADR 0008, `docs/WORKSPACE.md`, and `scripts/demo-workspace.sh`.*
*M9-E4 OTA (partial): `poly update manifest` generates and GPG-signs a deterministic Tauri v2 `latest.json` (version copies retained for rollback), and `poly update check` validates the schema plus optional trust-root verification; the mechanism matrix and rollback runbook are in `docs/OTA.md`. The real Tauri install / rollback drill remains environment-gated.*

## [1.11.0] — 2026-10-19

### Added
- **缺口收口（2026-10-19）**：`poly release` 的 `--credential` 可重复并支持 `TARGET=REF` 按目标凭据（裸 `REF` 仍为全局缺省；未知/重复映射报 `E_PLAN_0002`；dry-run 逐目标报告 `credential_given`）；Garmin 内建 `.iq` 打包器（`crates/poly-bundle/src/packagers/garmin.rs`：解析工程、环境槽 `monkeyc` / `CIQ_DEVKEY`、`.iq` 结构自检；`iq-structure-check` 已接入 `poly verify`）；`default_backends()` 按环境装配 Vault 信任根载体；新增 `.github/workflows/release.yml`（`v*` tag 构建三宿主二进制 + `SHA256SUMS.txt`）；新增双语 `docs/INITIATIVES.md`（E1–E7、8 个规划平台、开放决策与 55 个候选任务 / 226 人日）与 `docs/ENV-SLOTS.md`（19 个环境槽的关闭协议）。

*Gap closure (2026-10-19): repeatable `poly release --credential` with `TARGET=REF` per-target refs (a bare `REF` stays the global default; unknown/duplicate mappings fail with `E_PLAN_0002`; dry-run reports `credential_given` per target); the built-in Garmin `.iq` packager (`crates/poly-bundle/src/packagers/garmin.rs`: project resolution, `monkeyc` / `CIQ_DEVKEY` environment slots, `.iq` structure check; `iq-structure-check` wired into `poly verify`); `default_backends()` assembles the Vault trust-root carrier from the environment; a new `.github/workflows/release.yml` (three-host binaries + `SHA256SUMS.txt` on `v*` tags); and the bilingual `docs/INITIATIVES.md` (E1–E7, the 8 planned platforms, open decisions, and 55 candidate tasks / 226 person-days) plus `docs/ENV-SLOTS.md` (close-out protocol for 19 environment slots).*

### Changed
- keyring 3.6.3 → 4.2.0（`v1` 兼容层）→ zbus 5.19；zbus 5 仍以独立 crate 携带 `zvariant` / `zvariant_derive` / `zbus_names`，RUSTSEC-2026-0370/0371/0372 豁免保留（上游标注 unmaintained，非漏洞；升级路径 zbus 6）；文档同步：GA 规划平台数 13→8、README 源码安装 / 预编译二进制说明、TASKS M2 全 ✅、ROADMAP 任务数 116/283。

*keyring 3.6.3 → 4.2.0 (`v1` compatibility layer) → zbus 5.19; zbus 5 still ships `zvariant` / `zvariant_derive` / `zbus_names` as standalone crates, so the RUSTSEC-2026-0370/0371/0372 exemptions remain (upstream marks them unmaintained, not vulnerable; the upgrade path is zbus 6); documentation sync: GA planned-platform count 13→8, README source-install / prebuilt-binary guidance, all M2 TASKS entries ✅, ROADMAP task count 116/283.*

### Fixed
- 文档更正：GA 缺口清单的规划平台数由 13 改为 8（与 49 目标现状表中的 8 行「规划中」一致）；README 快速开始改用仓库内安装路径（`cargo install --path crates/poly-cli`），注明 crates.io 发布待办（所有 workspace crate 均 `publish = false`），并把过时的 M1 命令说明更新为当前命令集（`poly release` 面向 deb / rpm / pkg / pkgsrc 自托管仓库，小程序提审走 `poly publish`）；TASKS 中 T-M2-10 / T-M2-13 标题补 ✅（保留原注记）。

*Documentation corrections: the GA gap-list planned-platform count changed from 13 to 8 (matching the 8 "planned" rows in the 49-target status table); the README quick start now uses the in-repo install path (`cargo install --path crates/poly-cli`), notes that crates.io publication is pending (all workspace crates set `publish = false`), and replaces the stale M1 command note with the current command set (`poly release` targets deb / rpm / pkg / pkgsrc self-hosted repos; mini-program submission goes through `poly publish`); and the T-M2-10 / T-M2-13 headings now carry ✅ (their original notes preserved).*

## [1.10.0] — 2026-10-18

### Added
- **M8 生态 GA 交付**（2026-10-18，13/13）：wasmtime 插件沙箱（默认无文件系统/网络，目录与出网域名显式授权，fuel/epoch/内存三道上限，逃逸负向集全绿）；WIT world `polypack-target` 与 `target-spec/v1` 版本协商（非 v1 拒绝并给迁移提示）；插件 SRI 校验 + GPG 分离签名链（`CredentialRef` 信任根、验签先于实例化，`E_PLUGIN_0009/0010`）与 `poly scan` 的 wasm 制品脱敏规则；独立模板仓库 `templates/plugin-target`（WIT/componentize/契约测试/自带 CI）与 `poly plugin new|digest`（生成→构建→沙箱加载→describe/package 端到端 14.4 min ≤ 30 min）；内建目标统一 `TargetProvider`/`ProviderRegistry`（内建即官方插件，无 `if builtin` 特判，L1–L5 打包器源码零改动、Tizen 统一入口字节 == 基线）；农场 v1（并发调度/worker 池/超时重试 + v0 worker 兼容、0700 一次性工作目录、凭据仅内存注入、共享目录 CAS 与无可选后端降级）；发布流水线 GA（指数退避、Idempotency-Key、SubmitJournal、85009=accepted、回执 v2、`poly retry --from publish`）；49 目标现状表生成与漂移门禁；双语 GA 文档、技术审查报告与「1 天新增目标」演练（独立开发者代理，38 min 完成脚手架→自定义出包，SRI 负向 `E_PLUGIN_0006`）。

*M8 ecosystem GA delivery (13/13): the wasmtime plugin sandbox (no filesystem/network by default, explicit directory/domain grants, fuel/epoch/memory limits, escape-negative suite green); the `polypack-target` WIT world with `target-spec/v1` negotiation (non-v1 rejected with a migration hint); plugin SRI verification plus a GPG detached-signature chain (`CredentialRef` trust root, verification before instantiation, `E_PLUGIN_0009/0010`) and redacted wasm-artifact rules in `poly scan`; the standalone `templates/plugin-target` repo (WIT/componentize/contract test/own CI) and `poly plugin new|digest` (generate→build→sandbox load→describe/package end to end in 14.4 min ≤ 30 min); built-in targets unified behind `TargetProvider`/`ProviderRegistry` (built-ins are official plugins, no `if builtin` special-casing, L1–L5 packager sources untouched, unified Tizen entry bytes == baseline); farm v1 (concurrent scheduling/worker pool/timeout retries + v0-worker compatibility, 0700 one-shot work directories, in-memory-only credentials, shared-directory CAS with degradation when no optional backend exists); the publishing GA (exponential backoff, Idempotency-Key, SubmitJournal, 85009=accepted, receipt v2, `poly retry --from publish`); the generated 49-target status table with a drift gate; and the bilingual GA docs, technical review and "add a target in one day" rehearsal (an independent developer agent completed scaffolding→custom packaging in 38 min; negative SRI yields `E_PLUGIN_0006`).*

### Fixed
- M8 集成期真跑修复：Windows 农场隔离测试字段 dead_code（`-D warnings`）；cargo-deny 放行 wasmtime 全系 `Apache-2.0 WITH LLVM-exception`；依赖方向白名单加入 `poly-plugin → poly-core/poly-sign/poly-targets`（无环）；macOS gpg-agent socket 104 字节上限（签名测试改用短 `/tmp` GNUPGHOME）；macOS 农场超时重试窗口 120 ms→2 s；Windows WGT zip 元数据差异（基线哈希按平台）；宿主 `providers` 消费 `[[plugins]].sri/signature/trust_root` 并拒绝验签失败插件；模板契约断言单一来源（`EXPECTED_FORMAT`/`EXPECTED_PACKAGE`），README 与 `poly plugin new` 输出补齐 SRI、产物复制与最小宿主工程说明。

*Fixed during M8 integration across real runs: the Windows dead_code in the farm-isolation test under `-D warnings`; cargo-deny allowance for wasmtime's `Apache-2.0 WITH LLVM-exception`; the dependency-direction whitelist for `poly-plugin → poly-core/poly-sign/poly-targets` (acyclic); the 104-byte gpg-agent socket limit on macOS (signature tests now use a short `/tmp` GNUPGHOME); the macOS farm retry window (120 ms → 2 s); Windows WGT zip-metadata differences (per-platform baseline hashes); host `providers` now consuming `[[plugins]].sri/signature/trust_root` and refusing failed signatures; and the template contract's single-source assertions (`EXPECTED_FORMAT`/`EXPECTED_PACKAGE`) with README/`poly plugin new` guidance for SRI, artifact copying and the minimal host project.*

## [1.9.0] — 2026-10-18

### Added
- **M7 穿戴交付**（2026-10-18，11/11）：`@polypack/adapter-wear`（圆/方安全区、紧凑 tokens、capabilities，焦点环复用共享内核）；Wear OS 目标（`wearos`，复用 Android 链 + watch 清单变体）与表单冒烟双轨（`wear-smoke` 夜跑 input/swipe/back、`wear-release-smoke` 发版门含安装→启动→卸载生命周期与截图/构建摘要归档）；watchOS SwiftUI 伴生模板（`poly init --target watchos`）与 watch 打包/签名链（复用 Apple 签名与 R3 农场判定，实验）；Fitbit `.fba` 与 Zepp `.zab` 打包器（runtime=none，结构/金样/确定性）；Garmin Monkey C 子集转译 spike（结论=继续受限档，实验）；实验目标能力/缺口/替代方案双语新章（watchOS W1–W7 / Garmin G1–G10 + Wear beta 边界）；穿戴契约测试与金样（统一四段骨架 + 显式 `POLYPACK_UPDATE_GOLDEN` 门）。**十一工作流三宿主全绿（新增 `wear-smoke` / `wear-release-smoke`）。**

*M7 wearables delivery (11/11): adapter-wear with round/square safe areas; the Wear OS target reusing the Android chain plus the dual form-smoke tracks (nightly wear-smoke and a blocking wear-release-smoke gate with install→launch→uninstall lifecycle and screenshot/build-summary archives); the watchOS SwiftUI companion template and watch packaging/signing chain (experimental); Fitbit/Zepp packagers with structure goldens; the Garmin Monkey C subset spike (constrained-mode decision); the bilingual capability/gap/alternatives chapter for experimental targets; and the wearable contract suite with explicit golden updates. Eleven workflows green on three hosts (new wear-smoke / wear-release-smoke).*

### Fixed
- 穿戴链集成修复：Fitbit/Zepp 缺工程改报 `MissingInput`（环境槽 SKIP-with-print），`poly build --target all` 不再因用户未提供该平台工程硬失败；watchOS 归档来源断言按逻辑路径归一（Windows 反斜杠假红）。

*Fixed during M7 integration: Fitbit/Zepp now report MissingInput (environment-slot SKIP) when the platform project is absent, so `--target all` no longer hard-fails; the watchOS archive-source assertion is normalized for Windows backslashes.*

## [1.8.0] — 2026-10-18

### Added
- **M6 嵌入式与车机交付**（2026-10-18，13/13）：`poly-core` no_std 引擎层（feature 拆分不新增 crate；`thumbv7em-none-eabihf` 构建 + std/no_std 同缓存键）与 `poly_engine_*` C ABI（单源头文件，gcc/clang 真编译、nm 无 mangling）；L6 六目标——Android Automotive（复用 Android 链 + OEM 占位渠道）、QNX（SDP 探测 + 库/镜像；无 SDP SKIP-with-print）、AUTOSAR Adaptive（ARXML 与 C ABI 一一对应 + xmllint 真校验）、VxWorks（库/镜像 kit + §5.3 无绝对路径）、Zephyr（west 模块模板 + 两板真构建 + cortex_m3 QEMU 真跑）、FreeRTOS（mps2-an385 真跑，内核按 tag+sha256 现拉不入库）；`poly targets` 派生 CI 矩阵与共享 QEMU 冒烟运行器（`Failed{stage,code,hint}`、日志/result.json 归档）；TargetSpec 派生 L6 状态表生成器与漂移校验；L6 体积预算 warn/fail 与 experimental 三处一致校验。**九工作流三宿主全绿（新增 `embedded-ci`）。**

*M6 embedded & automotive delivery (13/13): the no_std engine layer and poly_engine_* C ABI with a single-source header; six L6 targets (Android Automotive, QNX, AUTOSAR, VxWorks, Zephyr, FreeRTOS) with real builds/QEMU where toolchains exist and honest SKIP-with-print elsewhere; the TargetSpec-derived CI matrix and shared QEMU smoke runner; the generated L6 status table with drift checks; L6 size budgets and the three-way experimental validator. Nine workflows green on three hosts (new embedded-ci).*

### Fixed
- 集成期真跑修复（三宿主 CI 实证）：`--target all` 遇 CI 缺 nightly `rust-src` / 目标 std 时硬失败 → 引擎静态库构建前预检并降级为环境槽 SKIP-with-print；头文件唯一性扫描只认含 `poly_engine_abi_version` 的头（FreeRTOS 模板辅助声明不再误判）；`EngineBuild.packager` 与统一 `EmbeddedInputs`（含 `prebuilt`）对齐；AAOS 目标补 adapter-native 能力矩阵行；`CARGO_HOME` remap 改为无条件登记（CI 新建 CARGO_HOME 时 `registry` 预检为假，静态库残留绝对路径触发 §5.3 硬失败）；Zephyr `qemu_x86` 引擎从 i686 改 **i586**（i686 ABI 强制 SSE2，qemu32 无 → Invalid opcode）并在嵌入式 wrapper 关闭 blake3 全部 x86 SIMD 运行时分派（本机容器 QEMU 三步 PASS + SMOKE PASS 实证）；供应链豁免 RUSTSEC-2026-0370/0371/0372（zbus 4.x unmaintained，非漏洞，keyring Linux 凭据面，升级路径 zbus 5）。

*Fixed during M6 integration across the three-host CI: embedded toolchain preflight for missing nightly rust-src / target std (now SKIP-with-print), a scope-correct header-uniqueness scan, the unified EmbeddedInputs/EngineBuild fields, the AAOS capability-matrix row, an unconditional CARGO_HOME path remap (fresh CI homes triggered §5.3), the Zephyr qemu_x86 engine moved to i586 with blake3 x86 SIMD disabled (qemu32 lacks SSE2), and supply-chain exemptions for RUSTSEC-2026-0370/0371/0372 (zbus 4.x unmaintained, not vulnerabilities).*

## [1.7.0] — 2026-10-17

### Added
- **M5 发行版与 BSD 交付**（2026-10-17，11/11）：deb/rpm 成熟化（维护者脚本、spec `%files`、lintian/rpmlint 门禁）与 L5 ubuntu/fedora/mint（apt/dnf 真装装卸、双跑字节一致）；AppImage 成熟化（AppStream/desktop/hicolor、`SOURCE_DATE_EPOCH`、`--reproducible`）与 Flatpak / Snap（最小权限声明 + lint 腿）；自托管仓库元数据与 GPG 签名（deb Packages/Release/InRelease、rpm repodata/repomd，凭据全引用、apt 真跑 + 篡改拒绝、dnf 容器真跑、双跑幂等）；BSD 三目标——FreeBSD pkg（`pkg create` 显式 plist、pkg 签名仓库、VM 内 `pkg install`/`check -s` 与二跑幂等）、OpenBSD ports（signify + portcheck）、NetBSD/DragonFly pkgsrc（`Makefile`+`PLIST`+`DESCR`，bmake 腿缺环境 SKIP）；ChromeOS 双轨（deb + PWA 审计三腿，Chrome 151 真跑）；`poly release` 多目标编排（回执归档、`Published` 事件、稳定错误码 E_RELEASE_0001..0005）；`Host::Bsd` 原生宿主；新增 `bsd-ci` 工作流（FreeBSD 14.5 QEMU 原生构建 + 安装冒烟真跑；OpenBSD 7.9 尽力腿，托管 runner 无 KVM 时按 SKIP-with-print 留痕；夜跑 20:07 UTC）。**八工作流三宿主全绿。**

*M5 distributions & BSD delivery (11/11): matured deb/rpm with lint gates and real apt/dnf installs; matured AppImage plus Flatpak/Snap with least-privilege manifests; self-hosted repo metadata + GPG signing proven by real apt consumption and tamper rejection; FreeBSD pkg / OpenBSD ports / NetBSD pkgsrc targets with a native FreeBSD VM signed-repo install smoke; the ChromeOS dual track with a real Chrome 151 PWA audit; the multi-target `poly release` orchestration with receipts and stable error codes; the `Host::Bsd` native host; and the new bsd-ci workflow (FreeBSD QEMU native build + install smoke for real; the OpenBSD leg records SKIP-with-print when the image cannot boot under TCG on KVM-less runners). Eight workflows green on three hosts.*

### Fixed
- 跨平台真跑修复（M5 集成期，三宿主 CI 实证）：Taro 宿主关闭 webpack 软链解析（`file:..` 依赖致 `react` 解析逃逸出宿主 `node_modules`）；rpm `%files` 含空格路径整条加引号并补 `RUNTIME-DEPENDENCIES.txt`、自定义 `%clean` 引号化 buildroot（含括号 TMPDIR 不再语法错误）；Roku 发射器与 quickapp lint 的 node 路径剥 Windows `\\?\` verbatim 前缀；release 回执路径断言与 CLI 输出同按 canonicalize 形态（macOS `/private/var`、Windows 8.3）；gpg 真跑测试在 macOS 落短路径 `/tmp`（agent socket 104 字节上限），Windows MSYS gpg 按环境槽 SKIP。

*Fixed during M5 integration across the three-host CI: webpack symlink resolution in the Taro host, rpm %files quoting + the missing doc entry + a quoted %clean, Windows verbatim prefixes handed to node, canonicalized receipt-path assertions, and gpg test temp-path portability.*

## [1.6.0] — 2026-10-17

### Added
- **M4 TV 家族交付**（2026-10-17，14/15；T-M4-15 条件任务未触发）：`@polypack/adapter-tv` v0——10-foot tokens（单源派生）+ 几何推导焦点遍历图 + 连通性检查器 + 焦点记忆 / 分层 Back / 数字键（焦点内核冻结，50/50）；Android TV / Fire TV（Leanback 清单，复用 M2 同一条 Android 链零第二实现；设备冒烟 6/6 走查 + 「无指针完成核心任务」+ 焦点环像素实证）；tvOS（iOS 链按 ApplePlatform 描述符参数化复用 + RN-tvOS 宿主；**macos-14 真跑首证：XCFramework→archive→IPA**）+ 远程农场 R3 扩臂；Tizen `.wgt` / webOS `.ipk`（确定性容器手写读写 + 结构硬失败）/ VIDAA / Vizio SmartCast 应用包（四目标同源同摘要 sha384 单值）；Roku SceneGraph 子集受限渲染器 spike + 决策（**继续受限档**：`experimental` 维持、官方侧载协议逐字取证、真机侧载为 T-M4-15 前置）；鸿蒙 TV（deviceTypes 加 `tv` 按 SDK19 枚举真文件实证）；遥控器冒烟套件（Android TV 真机 + web 替代腿（如实标注），14 场景 / 69 键步 fail 0）+ `tv-smoke` 新工作流（mock 每提交 / 夜跑 18:57 错峰）。**七工作流三宿主全绿。**

*M4 TV-family delivery (14/15; conditional T-M4-15 not triggered): adapter-tv with the frozen focus kernel, Android TV / Fire TV on the shared Android chain with real device-walk evidence, tvOS reusing the iOS chain via an ApplePlatform descriptor with a first real macos-14 run, Tizen/webOS/VIDAA/Vizio web-tier packagers on one same-source digest, the Roku subset-renderer spike with a "continue in constrained mode" decision, HarmonyOS TV adaptation, and the remote-control smoke suite plus its new workflow. Seven workflows green on three hosts.*

### Fixed
- chrome ozone 根因（DISPLAY 在场探 X11 后端致 DevTools 口卡死 >90s → `--ozone-platform=headless` 2s 就绪；CI 无 DISPLAY 不受影响，此旗标为确定化）；adapter-native 能力矩阵补 android-tv / fire-tv 行（RN 承载哨兵实测抓出）；registry / 内建计数随 L3 电视族八目标聚合（19/20）。

*Fixed: the chrome ozone hang on desktop hosts with DISPLAY set; the RN-host capability-matrix rows the sentinel caught for android-tv / fire-tv; and the L3 count aggregation to eight TV targets.*

## [1.5.0] — 2026-10-17

### Added
- **M3 鸿蒙与小程序交付**（2026-10-17）：RNOH 宿主模板 + NAPI 桥出口（`poly bridge ohos`——OHOS aarch64 真交叉编译自包含 `libpax.so` 6.2MB + node 真装载对拍同字面量）与 RNOH 承载分支（双宿主结构快照逐字节一致）；HAP 打包（hvigorw 真跑，增量三跑字节一致）与 hap-sign-tool 签名/校验（凭据全引用、口令经 argv 全链脱敏）；adapter-taro v0 + Taro 四端管线（weapp / alipay / tt 代码包 + 快应用真 build 出 `.rpk`；真编译 + 结构断言 + 金样 + 二跑缓存命中）；wasm 沙箱出口（`poly bridge wasm`：生成期/加载期两道白名单闸 + loader 适配层随包）与 wasm-opt 裁剪（−51%）+ `[budget]` 体积门禁（warn/fail 双态入 DAG）；三平台 publish 客户端（微信 B 协议 + 支付宝网关 v1 + 抖音，四步零分叉驱动 / 断点续走 / 回执永久归档）与单文件载具 + quickapp 渠道备位 + `poly publish` 提审薄层（真 rpk dry-run 幂等）；小程序契约测试（devtools CLI 校验腿）与 `miniprogram-contract` workflow（mock 每提交 / real 每夜 18:47 错峰）。**终版全量验证（fmt / clippy `-D warnings` / workspace 测试）全绿；六工作流三宿主全绿。**

*M3 HarmonyOS & mini-program delivery — RNOH host + NAPI outlet, HAP packager + hap-sign-tool chain, adapter-taro + four-target Taro pipeline (incl. real .rpk), wasm sandbox outlet with a two-gate allowlist + wasm-opt trimming (−51%) + the `[budget]` gate, three publish clients on a zero-fork driver with a single-file carrier, devtools contract tests + the sixth workflow. Full-battery verification and all six workflows green on three hosts.*

### Fixed
- **CI 逐环修复与如实更正**（2026-10-16/17）：devtools 契约登录态路径断言兼容 Windows Debug 转义（windows-2022 实证）；`cargo-deny` 与 `cargo-audit` 双配置豁免 RUSTSEC-2023-0071（rsa Marvin 计时侧信道——上游无修复版、仅本地签名无解密面；audit 腿此前被 deny 挡在前面掩盖）；`probes` 计时用例边距加固（重载假红实证）；`bridge-real` NDK 安装 retry 与留产物空转修复（RAII 先删 → `POLY_KEEP_TEMP` 通道；非空回执后翻 `error`）；清单统一（poly-publish / base64 进根 `workspace.dependencies`）；跨平台路径形态三修（quickapp lint 腿 root 去 Windows verbatim 前缀——node `createRequire` 不认该前缀；publish 定位器断言形态归一——mac `/private/var`、win 8.3 短名，均三宿主 CI 实证）。更正：v1.4.0 记「`schedule` 触发历史为 0」实为观察窗口过短——首夜即已触发（best-effort 延迟约 3 小时），mobile-contract 真实模式经 schedule 全绿。

*CI hardening ring by ring (Windows path assertion, dual-config RUSTSEC-2023-0071 exemption, probes timing margins, NDK retry + artifact keep-temp); manifest unification; cross-platform path-shape fixes (Windows verbatim prefix, macOS `/private/var` short/long names); and an honest correction — the `schedule` trigger does fire (fired ~3 h late on its first night).*

## [1.4.0] — 2026-10-16

### Added
- **M2 收口：设备走查闭环 + sign/verify 入 DAG**（2026-10-16）：RN 宿主设备走查真跑（emulator）——六走查 touch 6/6、none 6/6（焦点序列与计数逐条命中 `expectations.json`）+ 焦点环差分像素采样双态实证（T-M2-04/05 升 ✅；自带漂移哨兵与 adb 驱动口径）；`poly build` 内联 sign 阶段（凭据门控三级语义：未声明静默 / 环境缺→告警跳过 / 非法→硬失败）与 verify 校验器注册表入 DAG（T-M2-10「verify 阶段接入」落地）；真签实证（Secret Service 一次性密钥 → apksigner v2+v3 独立复核，钥匙库事后清空）；T-M2-11 双真环境闭环（真 Vault dev KV v2 读写往返——audit 日志 token 仅 hmac；真 keyring 隔离总线——Deepin 补丁版根因实证）；真生成器 × Apple 编译首证（真 uniffi 产物 + 真调用面过 `swiftc -typecheck`，macos-14 真模式）与 simulator 调用对拍（`simctl` 真跑 `paxAdd(-5,3)=-2` 与 i32 回绕）；windows msi / macos dmg 真检查器落地（MSI 真机执行首次全通：windows-2022 真跑 `candle`+`light`+`msiexec` 载荷字节核对）。**工作区 384 测试全绿（另 ui/ 42）；五工作流三宿主绿；真实模式经手动 dispatch 全绿实证（mobile-contract 双新腿 + bridge 全链）。**

*M2 closure — device walks proven on emulator (6/6 in both forms, plus ring pixel evidence), sign/verify stages wired into the DAG with real apksigner evidence, Vault/keyring real closures, first real proof of generator × Apple compile and simulator call parity, MSI/DMG real checkers with MSI real execution green on windows-2022. All workflows green; real mode evidenced via manual dispatch.*

### Fixed
- **首夜真跑根因三修**（2026-10-16）：msi 链接 LGHT0311（WixUI 本地化 wxl 钉死 DB 码页 1252 与中文描述冲突——UI 腿退出，`Product/@Codepage=65001` 保全任意 Unicode；`exec` 失败证据补 stdout 回落）；Swift 调用面文件须为 `main.swift`（多文件编译顶层表达式规则——两处同修）；`bridge.yml` 的 `setup-android` v3→v4（v3 默认包含已下架死包 `tools`——两次真派实证，对齐 mobile-contract 同款修复）。
- **如实发现**：`schedule` 事件历史触发数为 0——两个 workflow 的「每夜真跑」此前从未发生（原生 API 实证；仓库/YAML/账号侧均已排查，平台侧成因待查）；真实模式今后以手动 dispatch 达成，台账已注记。

*First-real-run root causes fixed (WiX code page LGHT0311, swiftc `main.swift` rule, setup-android v3→v4); noted honestly: the `schedule` trigger has never fired — real mode is now evidenced via manual dispatch.*

## [1.3.0] — 2026-10-15

### Added
- **M2 移动与桥生态交付**（2026-10-15）：uniffi 桥生成器（UDL 单源 → Kotlin/Swift 绑定、接口摘要入缓存键、`poly bridge`）；Kotlin/JNI AAR（双 ABI 真交叉编译，emulator 三层实证：loadLibrary + dlsym 真调用 + Kotlin→JNA→Rust 全链）；Swift XCFramework + iOS IPA 链（macos-14 真跑直出未签名 IPA 并留档）；Android 链（AAPT2 免 Gradle 直出 AAB/APK、bundletool 校验、emulator 冷启动冒烟、跨绝对目录逐字节可复现）；apksigner 签名（v2+v3 真验、临时密钥零残留）与凭据层（keyring/Vault/Env + 全通道脱敏）；远程农场协议 v0（发起地/构建地分离、共享对象存储跨构建点缓存复用、`E_FARM_0002` 准入语义）；secret-scan 双检（全历史 823 文件零命中 + CI 三宿主）；`poly verify --reproducible` / `poly retry --from sign`；adapter-native v0（RN 宿主 + 焦点基线）；移动契约 CI（mobile-contract：mock 每提交 + nightly 真列并留档）与 bridge 真跑 nightly；FARM 双语文档。**355 测试、五工作流三宿主全绿、真模式两腿全绿（IPA/AAB 留档）。**

*M2 mobile & bridge ecosystem delivered — uniffi bridge, AAR, Android AAPT2 chain, iOS IPA chain, signing & credentials, farm v0, secret-scan, verify/retry, adapter-native, mobile-contract CI. 355 tests; five workflows green on three hosts; real-mode dispatch green with archived unsigned IPA and AAB.*

### Fixed
- **CI 三宿主全绿收敛**（2026-10-15）：测试与宿主工具解耦（夹具入库 + 显式 SKIP 体系）、`.gitattributes` 强制 LF 检出、显式 linux 目标整文件门控、`EXE_SUFFIX`/路径分隔符/`getopt` 参数序等跨宿主修复——**windows 腿首次全绿**；随后真模式七轮 dispatch 逐层根因（SDK 死包 `tools`、`lipo` 输出目录执行序、上传基准 `$TMPDIR`、`exportArchive` 无 Team 语义、骨架绑定引用写死、取件竞态）全部修复并经真机判决，含三处"静默假绿"通道的点亮（aapt2 `warn:` 前缀、SKIP 掩盖失败、错误显示丢 stderr）。

*Three-host CI convergence (windows leg green for the first time) and real-mode hardening: every root cause fixed with local reproduction — including three silent-false-green channels lit up.*

## [1.2.0] — 2026-10-15

### Added
- **M1 桌面与 Web 交付**（2026-10-15）：`poly init` 示例应用脚手架（React 19 + vite 7 真构建）；`poly build` 多端构建（真 triple 探测、内容寻址缓存 **二跑 100% 命中**、DAG 双支线并行）与**真实打包**（Linux 真出 deb（含 Pax 派生六尺寸图标）/ AppDir / Web 静态包 + SRI 清单）；`poly dev` 整页重载热预览（Chrome 真浏览器证据）；`poly clean` 与保留策略（LRU）；五态 CLI 表情（Pax 情绪映射）；英文版图示合入 README.en。**200 测试全绿、clippy `-D warnings` 零告警。**

*M1 Desktop & Web delivered: poly init/build/dev/clean, the five-packager matrix, and four-target contracts — 200 tests green.*

## [1.1.0] — 2026-10-15

### Added
- **M0 地基交付**（2026-10-15）：9 crate Rust 工作区（依赖方向约束测试）；`poly doctor`（6 探测器 + L1 覆盖矩阵，人读与 `--json`）；`polypack.toml` schema（serde + JSON Schema 导出，严格模式）与 TargetSpec 注册表（L1 七目标）；诊断框架（稳定错误码 + BuildEvent 事件流）；三宿主 CI 工作流（远端执行待账单恢复）；供应链检查（cargo-deny 四项通过 + cargo-audit 0 漏洞）。**56 测试全绿、clippy `-D warnings` 零告警。**
- Pax 单形态动效：五种表情各自带专属动态（哭=泪滴下落、惊恐=发抖加汗滴、疑惑=问号浮动、眨眼=睁闭眼循环、笑=浮动），联系表逐格动画且相位错开
- Pax 全身化：联系表改为全身像（六腿 + 包裹 + 双钳）；`pax-mark.svg` 徽章补蟹腿；图示右下角角标同步更新
- 校验器前置交付：`tools/render-diagrams.mjs --check`（图示漂移）、`tools/check-docs.mjs`（结构级双语校验）与最小 CI（`.github/workflows/docs-check.yml`）
- 规划缺口审计落地：供应链检查（T-M0-11）、Pax 品牌落地任务（T-M1-14）、`--target all` 语义、版本策略（§4.6）、构建时限预算（§5.4）、清理与保留策略（§5.5 / T-M1-15）、扩展方向孵化池（§13，E1–E8）
- `LICENSE`（Apache-2.0）、`SECURITY.md`

*Per-form motion for Pax, full-body expression sheet, drift checkers + minimal CI, license and security policy.*

## [1.0.0] — 2026-10-14

### Added
- 双语 README（简介 / 50+ 平台矩阵 / UI 组件策略 / Pax 品牌约定）
- [`docs/ROADMAP.md`](docs/ROADMAP.md)：L1–L7 分级策略、49 平台逐项工程明细、M0–M8 验收标准、风险登记册
- [`docs/TASKS.md`](docs/TASKS.md)：113 任务 / 279.5 人日（角色 / 依赖 / 估算 / 交付物 / 验收）
- 四张架构类 SVG（架构 / 请求周期 / 功能 / 生命周期）+ Pax 六形态资产（动画版 / 五表情 / 图标 / 联系表）
- 两条零依赖流水线：`tools/render-diagrams.mjs`（JSON IR → SVG，含 Pax 角标）、`tools/build-pax.mjs`（Pax 全形态，`--check` 防漂移）

*Initial release — documentation & planning baseline.*
