# 0007 — 外部流程与 UI 决策：分支保护、历史持久化、设备预览

- 状态（Status）：accepted
- 日期（Date）：2026-10-20
- 决策者（Deciders）：项目维护者

> **English abstract**: Three M9 items are decided without writing code. M9-G5 branch protection is an external repository-owner action: apply a ruleset with `enforce_admins=true` and archive evidence; the repository cannot fix it. M9-E6 splits build history into implementable event persistence (T-M9-16/17) and a deferred GUI (T-M9-18) because `app/` is not tracked. M9-E7 device preview starts with build-vs-buy research (T-M9-19); no device-farm or `DevicePool`/`DeviceLease` code is added here. Each item has concrete acceptance and exit criteria below.

## 背景（Context）

INITIATIVES 的三个 GA / 孵化项各有不同的阻塞类型：

- **M9-G5**：required checks 已配置，但 `enforce_admins=false`，管理员直推可以绕过 secret-scan 等必过检查；这是 GitHub 仓库设置问题，仓库内代码无法修复。
- **M9-E6**：`BuildEvent` 只经 stdout / JSON Lines 输出，没有持久历史；README 描述的 `app/` 控制台目录当前未被仓库跟踪，GUI 任务没有可构建的落点。
- **M9-E7**：`poly smoke` 能在无设备时 SKIP，但没有设备池、租约、安装启动截图与日志归档；自建农场与购买云设备农场都缺乏成本与覆盖证据。

本 ADR 为三项分别给出决策、责任方与验收 / 退出标准。它**不新增代码**：不改 `.github/` 设置或工作流、不改 `crates/poly-core/src/events.rs`、不改 `crates/poly-cli/src/farm.rs`、不实现 `DevicePool` / `DeviceLease`。

## 决策摘要

| 项目 | 结论 | 状态 | 责任方 | 下一任务 | 退出证据 |
|---|---|---|---|---|---|
| M9-G5 分支保护 | 外部仓库 owner 启用管理员同等约束 | **external** | 仓库 owner / maintainer | T-M9-55 | ruleset 链接 + 管理员直推被拒记录 |
| M9-E6 构建历史 | 先做事件持久化与查询，GUI 暂缓 | **conditional**（持久化可做） | coder / frontend | T-M9-16 / T-M9-17；T-M9-18 暂缓 | 可解析 JSONL + 查询聚合测试 |
| M9-E7 设备预览 | 先做 build-vs-buy 调研，不写农场代码 | **conditional**（调研可做） | researcher | T-M9-19；T-M9-20/21 条件启动 | 3 家供应商 + 自建 TCO 对比与结论 |

## 1. M9-G5 · 分支保护管理员绕过：外部 owner 动作

### 决策

M9-G5 **不是代码任务**，是仓库管理员的 GitHub 设置动作，状态标为 **external**。仓库内不新增工作流、不修改 required checks 集合，也不尝试用脚本绕过权限模型。

仓库 owner 需要启用一个作用于默认分支的 ruleset，至少包含：

1. 变更必须经 Pull Request（禁止直接推送）；
2. required checks 与现状一致（含 secret-scan；检查项集合不变）；
3. 禁止 force push；
4. `enforce_admins=true`，管理员同样受规则约束。

### 验收 / 退出标准

- [ ] ruleset 已启用，附设置页截图或 GitHub API 返回的可核验链接；
- [ ] `enforce_admins=true` 有独立证据（不只是口头确认）；
- [ ] 一次管理员直推被拒绝的验证记录（命令、时间、错误信息）；
- [ ] 证据链接写入 T-M2-12 记录，G5 状态改为完成或明确披露未完成；
- [ ] break-glass 流程已文档化：谁持有豁免、有效期、事后审计方式；无审计的豁免不接受；
- [ ] 未改变 required checks 集合与 CI 代码。

### 边界与风险

- 仓库内无法验证 owner 权限；若 owner 不执行，G5 保持 external / open，不得写成「已修复」。
- 锁死紧急修复的风险由 break-glass 承接；豁免必须可追溯，不能长期常开。
- 若组织未来启用 org-level ruleset，以 org-level 证据替代仓库级截图，验收标准不变。

## 2. M9-E6 · 构建历史：先持久化，GUI 暂缓

### 决策

1. **T-M9-16（事件持久化与轮转）与 T-M9-17（历史查询与矩阵聚合）是实现第一步**，可以在当前仓库内完成，不依赖 `app/`。
2. **T-M9-18（poly-gui 矩阵与详情视图）暂缓**，因为 `app/` 目录未被仓库跟踪；现在写 GUI 既无法在 CI 构建，也无法评审。
3. GUI 的启动条件是 `app/` 控制台骨架落地（M1 Tauri 管线跑通并被跟踪），届时复用同一 history schema，不另起数据源。

### 持久化契约（决策级）

- 位置：`.polypack/history/events.jsonl`（工程本地，不默认入库）。
- 每行一个 JSON 对象，至少含：`schema_version`、`build_id`、`ts`（RFC 3339 UTC）、`target`、`event`；可选 `duration_ms`、`artifact_digest`、`error_code`、`status`。
- 追加写，不在构建中重写整个文件；按条数与体积双上限轮转，轮转文件命名确定、可排序。
- 路径写入前脱敏，只保留相对工程根的路径；不写环境变量值、凭据、绝对宿主路径。
- 与 `poly build --json` 共用同一序列化 schema 与金样，避免 CLI / 文件 / 未来 GUI 三套字段。
- 历史写入是尽力而为：写入失败输出告警事件，但不让构建失败；显式查询历史时才报告读取错误。

### 验收 / 退出标准（T-M9-16 / T-M9-17）

- [ ] 每次构建恰好追加一条可解析记录；`poly build --json` 与文件记录字段一致；
- [ ] 超过条数或体积上限时轮转，旧文件完整、无半行 JSON；
- [ ] 路径脱敏断言：记录中不出现绝对宿主路径、凭据引用值或环境变量值；
- [ ] 查询按目标返回最近 N 次状态 / 耗时 / 摘要；空历史返回空结果且退出码 0；
- [ ] 聚合结果确定：同一历史输入二跑输出一致，有金样测试；
- [ ] 写入失败路径有稳定告警，不影响构建退出码；
- [ ] 文档写明保留期、轮转默认值与隐私说明。

### GUI 退出标准（T-M9-18，暂缓）

- [ ] `app/` 已被仓库跟踪，存在可构建的 Tauri 控制台骨架；
- [ ] Tauri command 读取同一 history schema / 金样，不复制字段定义；
- [ ] 矩阵显示全部已构建目标的最近状态、耗时、产物摘要与错误码；
- [ ] 点击进入单次构建详情；无历史时显示引导而非空白；
- [ ] 隐私说明与 CLI 文档一致，路径脱敏规则相同。

### 明确不做

- 不做多用户服务端、不做云同步、不做远程农场调度 UI（INITIATIVES M9-E6 非目标）。
- 本 ADR 不修改 `crates/poly-core/src/events.rs`、`crates/poly-cli/src/build.rs` 或新建 `app/`。

## 3. M9-E7 · 设备预览：先 build-vs-buy 调研

### 决策

M9-E7 当前只批准**调研**（T-M9-19），不批准设备农场实现。T-M9-20（协议扩展）与 T-M9-21（双目标演练）必须等调研给出 go 结论后再启动；本 ADR 不写 `DevicePool` / `DeviceLease`、不接云农场 API、不修改农场协议。

### 调研范围与验收（T-M9-19）

至少比较 3 家云设备农场（Firebase Test Lab、BrowserStack、AWS Device Farm）与自建方案，输出可审计对比：

- [ ] API 与 CI 集成能力：认证、任务提交、状态轮询、产物导出、并发与配额；
- [ ] 覆盖矩阵：Android 手机、Android TV、wearOS / 其他目标中至少覆盖 E7 关注的两类目标；
- [ ] 价格模型：按分钟 / 按设备 / 订阅的当前价格，标注来源与抓取日期；
- [ ] 数据与隐私：日志 / 截图保留期、存储位置、删除接口、访问控制；
- [ ] 自建 TCO：设备采购或租用、托管、供电网络、维护人力、镜像与清理；
- [ ] 成本模型：Android + TV 各 1 台的一次演练成本，以及月度稳定态成本；
- [ ] 明确结论：buy / self-host / hybrid，附风险与不选理由。

### 退出 / 升级标准

- **Go**：调研结论支持购买或混合，且成本、覆盖、隐私均可接受；此时才启动 T-M9-20，协议扩展需保持与农场 v1 兼容。
- **No-go**：维持人工矩阵与 `poly smoke` 的 SKIP-with-print，E7 关闭或继续观察；不得用不完整调研启动采购。
- **安全边界**：任何预览只跑夹具应用，不使用真实用户数据；截图与日志默认保留 ≤ 30 天；凭据不进入农场协议帧。
- **重开条件**：商店预览 / 真机回归成为发布阻塞，或供应商价格 / 覆盖发生显著变化。

### 明确不做

- 不做完整设备农场产品、不做无人值守审核、不承诺所有目标都有设备。
- 不修改 `crates/poly-cli/src/farm.rs`、`crates/poly-cli/src/smoke.rs`、`docs/FARM.md`。
- 不在调研前采购设备或签署云服务合同。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| G5 用 CI 脚本或本地钩子绕过管理员权限 | 无法约束 GitHub 服务端的管理员直推；只有 ruleset / org policy 有效 |
| E6 先做 GUI，后补持久化 | `app/` 未跟踪，GUI 无法构建；没有历史数据源，视图只能是 mock |
| E6 直接用 SQLite | 查询能力强但首版过重；JSONL 足够矩阵聚合，后续可迁移 |
| E7 直接自建设备农场 | 设备、网络、维护成本高且无覆盖证据；先调研再决定 |
| E7 直接买最大套餐 | 未验证使用率与目标覆盖，容易为闲置容量付费 |
| 三项继续停留在「建议」而不记录 | 触发条件出现时重复讨论；责任方与验收标准不清晰 |

## 影响（Consequences）

- G5 在 owner 执行前保持外部阻塞；项目文档必须如实披露，不能把 required checks 的存在等同于管理员受约束。
- E6 的持久化先行为未来 GUI 提供稳定数据源；GUI 延后不影响 CLI 自动化，但可视化需求继续以 `--json` + 外部工具满足。
- E7 的调研成本低、决策价值高；在 go 之前不会增加设备采购与协议复杂度。
- 三项都不改变现有构建、签名、农场与插件行为；没有代码回归面。
- 若 G5 长期不执行，审计叙事会保留一个已知缺口；应在 GA / 发布说明中披露。

## 参考（References）

- [INITIATIVES](../INITIATIVES.md) §2 M9-E6 / M9-E7、§5 M9-G5（[English](../INITIATIVES.en.md)）
- [ROADMAP](../ROADMAP.md) §13 E6 / E7（[English](../ROADMAP.en.md)）
- [TASKS](../TASKS.md) T-M2-12（`enforce_admins=false` 记录，[English](../TASKS.en.md)）
- [`crates/poly-core/src/events.rs`](../../crates/poly-core/src/events.rs)（`BuildEvent`）
- [`crates/poly-cli/src/farm.rs`](../../crates/poly-cli/src/farm.rs)、[FARM](../FARM.md)（农场协议）
- [`crates/poly-cli/src/smoke.rs`](../../crates/poly-cli/src/smoke.rs)（现有冒烟与 SKIP 行为）
