# 0006 — 规划平台结论：关闭 / 条件 / NDA 门控

- 状态（Status）：accepted
- 日期（Date）：2026-10-20
- 决策者（Deciders）：项目维护者

> **后续更新（2026-10-20）**：P3 已按本决策落地为部署模式，实际命令是 `polypack deploy line-liff`（不是 `polypack deploy --target line-liff`）；P1 retired 与 P2 季度复审（2027-01-20）已写入 README / TASKS。正文保留决策时的表述。
>
> **Follow-up (2026-10-20)**: P3 landed as a deploy mode per this decision; the actual command is `polypack deploy line-liff` (not `polypack deploy --target line-liff`). P1's retired status and P2's quarterly review (2027-01-20) are recorded in README / TASKS. The body preserves the wording from decision time.

> **English abstract**: This record closes the decision phase for the eight planned platforms (M9-P1–M9-P8). Google Play Instant is closed and kept as a retired row; Snapchat Mini is closed with a quarterly review; LINE Mini App is a conditional deploy mode without a TargetSpec; Sailfish OS and Ubuntu Touch are conditional on a sponsor or committed user providing devices and accounts; PlayStation, Xbox, and Nintendo Switch stay NDA-gated with no SDK or platform code in the public repository. Evidence was reviewed on 2026-10-20. No TargetSpec, packager, SDK, generator, or README count change is made here.

## 背景（Context）

`README.md` 的 49 目标现状表与 `tools/gen-status-table.mjs` 的 `PLATFORMS` 清单目前有 8 个 `planned` 行：Google Play Instant、Snapchat Mini、LINE Mini App、Sailfish OS、Ubuntu Touch、PlayStation、Xbox、Nintendo Switch。它们没有 TargetSpec，也不在支持矩阵的已声明目标内；INITIATIVES §3 为每个平台给出了现状、可行性、阻塞与建议，但「建议」尚未成为可引用的决策记录。

本 ADR 把这 8 项收敛为可执行的结论，并记录证据复核日期与重开条件。**证据日期统一为 2026-10-20（本次复核日）**；平台状态事实沿用 [INITIATIVES](../INITIATIVES.md) §3 已归档证据，不新增厂商承诺或未经验证的 URL。

结论口径：

- **关闭（close）**：不再投入打包器；P1 保留一行 retired 说明，P2 保留行并季度复审；
- **条件（conditional）**：只在明确触发条件下启动；触发前不排期、不写代码；
- **NDA 门控（NDA-gated）**：无授权不写代码，SDK / devkit / 平台包格式不入库。

## 决策（Decision）

| 平台 | 结论 | 证据日期 | 一句话理由 | 重开条件（摘要） |
|---|---|---|---|---|
| M9-P1 Google Play Instant | **close**（retired，保留一行） | 2026-10-20 | 平台停止受理，无提交与审核路径 | Google 官方恢复受理、Instant API 与提交契约 |
| M9-P2 Snapchat Mini | **close + 季度复审** | 2026-10-20（下次复审 2027-01-20） | 官方契约不可核验，任何打包器都是猜测 | 文档 / SDK / 上传 / 审核四项同时可核验 |
| M9-P3 LINE Mini App | **conditional**（仅部署模式，不声明 TargetSpec） | 2026-10-20 | LIFF 只支持 Console 托管 URL，无本地上传包格式 | 出现接受 URL 托管的 LINE 用户；TargetSpec 仅在官方发布包格式后重开 |
| M9-P4 Sailfish OS | **conditional**（需赞助 / 设备承诺） | 2026-10-20 | 缺 Qt / Silica 宿主、Sailfish SDK 交叉工具链与 harbour 签名 | 赞助方或用户承诺设备、Jolla 账号、SDK 与维护 |
| M9-P5 Ubuntu Touch | **conditional**（需赞助 / 设备承诺） | 2026-10-20 | deb 链不能产出 `.click`，缺 Clickable / AppArmor / OpenStore 链路 | 赞助方或用户承诺设备、OpenStore 账号与维护 |
| M9-P6 PlayStation | **NDA-gated** | 2026-10-20 | 官方 SDK 需授权，公开仓库不能实现或分发 | 签署 NDA、开发者身份、devkit 与本地 SDK 齐备 |
| M9-P7 Xbox | **NDA-gated** | 2026-10-20 | GDK 需授权，公开仓库不能实现或分发 | 微软授权、NDA、devkit 与本地 GDK 齐备 |
| M9-P8 Nintendo Switch | **NDA-gated** | 2026-10-20 | NintendoSDK 需授权，公开仓库不能实现或分发 | 任天堂授权、NDA、devkit 与本地 NintendoSDK 齐备 |

## 逐平台结论

### M9-P1 · Google Play Instant — 关闭（retired，保留一行）

- **结论**：关闭。不从 49 目标表删除，改为 retired 行并保留说明；不再实现 `instant-aab`。
- **理由**：Google 自 2025-12 起停止受理 Instant Apps 发布，Play services Instant API 停用；没有开发者后台入口与审核路径，任何打包器都无法验证，投入没有回报。README 与生成器已标注「已停服」，当前也无 TargetSpec。
- **证据日期与依据**：2026-10-20 复核；事实记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P1（[English](../INITIATIVES.en.md)）。
- **重开条件**：Google 官方公开恢复 Instant Apps 受理与 Instant API，并给出可核验的提交 / 审核契约；同时出现明确用户需求。满足后先更新本 ADR 或新开 ADR，再启动 T-M9-23。
- **不做什么**：不新增 `instant-aab` TargetSpec、不写打包器、不把 PWA / Web 演示冒充 Instant。

### M9-P2 · Snapchat Mini — 关闭，季度复审

- **结论**：关闭；保留规划行并标注「季度复审」。下一次复审日为 **2027-01-20**，此后每季度一次，直到重开或明确永久关闭。
- **理由**：官方文档入口 404，`developers.snap.com` 无 Minis 文档；npm `@snapchat/minis-sdk` 3.0.5（2022）已标注 deprecating minis；没有可核验的 zip / manifest / 上传 / 审核契约。基于社区猜测实现 zip 格式会产出伪平台产物。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P2。
- **重开条件**：以下四项同时可核验——官方文档、开发者账号、上传接口、审核条款；缺一项则维持关闭。若只恢复文档而无上传与审核，仍不启动。
- **不做什么**：不实现猜测的 zip 格式、不声称 Snap 兼容；如需通用 HTML5 包，走现有 Web 静态打包路径，不挂 Snap 名义。

### M9-P3 · LINE Mini App — 条件支持（仅部署模式）

- **结论**：条件支持 `polypack deploy --target line-liff` 一类的**部署模式**：校验配置、输出 Console 人工步骤、复用 Web 构建；**不声明 TargetSpec，不生成平台包**。
- **理由**：LIFF 应用由 LINE Developers Console 以托管 URL 注册（LINE Login 渠道 → LIFF；Endpoint URL 必须 HTTPS 且不得含 fragment）；官方没有本地上传包格式或 LIFF 配置文件，也没有 CI 上传 API。把它做成 TargetSpec 会伪造不存在的平台产物。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P3。
- **重开 / 启动条件**：出现明确使用 LINE 渠道的用户，且接受「只托管 URL、无本地上传包」。启动后只做 T-M9-26 / T-M9-27。
- **升级为 TargetSpec 的条件**：LINE 官方发布可核验的本地包格式、配置契约与上传 / 审核流程；否则永远保持部署模式。

### M9-P4 · Sailfish OS — 条件支持（需赞助 / 设备承诺）

- **结论**：条件支持；无赞助或明确用户承诺则不启动。
- **理由**：现有 `linux-rpm` 链只出桌面 Tauri RPM；缺 Qt / Silica QML 宿主、Sailfish SDK 交叉工具链（armv7hl / aarch64）、harbour 打包与签名。技术上可复用 RPM 与 Linux 交叉经验，但生态小、设备与维护成本高。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P4。
- **重开条件**：赞助方或明确用户承诺提供测试设备、Jolla 账号、Sailfish SDK 访问，并承诺至少覆盖两个发布周期的维护；满足后启动 T-M9-28–T-M9-30，TargetSpec 只在打包与校验通过后声明。
- **不做什么**：不用 Tauri 桌面壳冒充 Sailfish 应用、不把 SDK 提交入库。

### M9-P5 · Ubuntu Touch — 条件支持（需赞助 / 设备承诺）

- **结论**：条件支持；无赞助或明确用户承诺则不启动。
- **理由**：现有 Linux deb 链不能产出 `.click`（格式、AppArmor、click 清单均不同）；缺 Clickable / UBports SDK、WebView 壳与 OpenStore 提交流程。技术上中等，生态与设备规模小。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P5。
- **重开条件**：赞助方或明确用户承诺提供测试设备、OpenStore 发布账号、Clickable / UBports 工具链，并承诺至少覆盖两个发布周期的维护；满足后启动 T-M9-31–T-M9-33。
- **不做什么**：不把 deb 改后缀当 `.click`、不伪造 AppArmor profile。

### M9-P6 · PlayStation — NDA 门控

- **结论**：NDA 门控；无授权不写代码。
- **理由**：L7 规划目标；官方 SDK 需 Sony 授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。公开仓库只能保留插件接口与 mock。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P6。
- **重开条件**：签署 NDA、获得授权开发者身份、取得 devkit、能在本地安装官方 SDK，四项齐备。之后仅以 `target-spec/v1` 插件形态接入（T-M9-34 / T-M9-35），产物格式由授权文档定义。
- **不做什么**：不在公开仓库实现或分发 Sony SDK、不描述未公开的包格式。

### M9-P7 · Xbox — NDA 门控

- **结论**：NDA 门控；无授权不写代码。
- **理由**：L7 规划目标；GDK 需 Microsoft 授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。公开仓库只能保留插件接口与 mock。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P7。
- **重开条件**：签署 NDA、获得微软授权开发者身份、取得 devkit、能在本地安装 GDK，四项齐备。之后仅以 `target-spec/v1` 插件形态接入（T-M9-36 / T-M9-37）。
- **不做什么**：不在公开仓库实现或分发 GDK、不描述未公开的包格式。

### M9-P8 · Nintendo Switch — NDA 门控

- **结论**：NDA 门控；无授权不写代码。
- **理由**：L7 规划目标；NintendoSDK 需任天堂授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。公开仓库只能保留插件接口与 mock。
- **证据日期与依据**：2026-10-20 复核；记录见 [INITIATIVES](../INITIATIVES.md) §3 M9-P8。
- **重开条件**：签署 NDA、获得任天堂授权开发者身份、取得 devkit、能在本地安装 NintendoSDK，四项齐备。之后仅以 `target-spec/v1` 插件形态接入（T-M9-38 / T-M9-39）。
- **不做什么**：不在公开仓库实现或分发 NintendoSDK、不描述未公开的包格式。

## 全局边界（Scope）

- 本 ADR **不新增或修改任何 TargetSpec、packager、SDK、CLI 代码**；不新增 `crates/polypack-targets/src/builtin.rs` 条目。
- 本 ADR **不修改** `tools/gen-status-table.mjs` 的 `PLATFORMS` / `TOTAL` / `TIER_COUNTS`，也不修改 README 49 目标表与分级计数。P1 的 retired 行落地、P2 的复审标注与相关计数由 T-M9-22 与集成 owner 在后续提交执行，并统一跑 `node tools/check-docs.mjs`。
- 条件与 NDA 门控平台在触发前保持 `planned` / 候选状态，不进入 M0–M8 排期，不产生版本承诺。
- 所有平台结论不改变支持矩阵中已声明目标的任何字段；已声明目标仍以 TargetSpec 为单一事实源。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 保留全部 8 个规划项并继续标注「规划中」 | 矩阵不诚实；已停服 / 无契约平台会被误读为待实现 |
| 立即从 49 表删除全部关闭项 | 会造成计数与生成表漂移，且用户仍会搜索；P1 保留 retired 行更清晰 |
| 把 LINE / Sailfish / Ubuntu Touch 直接声明为 TargetSpec | 缺少可核验的包格式或工具链，会产出伪平台产物 |
| 主机平台用公开逆向 / 社区 SDK 实现 | 违反「不支持未获厂商授权的侧载」原则，法律与安全风险不可接受 |
| 为每个平台单独开 ADR | 8 项同属「规划平台收敛」，合并且便于统一重开条件与证据日期 |

## 影响（Consequences）

- 规划清单从「8 个待实现」变为「1 个 retired + 1 个季度复审 + 3 个条件 + 3 个 NDA 门控」，矩阵语义更诚实。
- P1 的 retired 行需要生成器与 README 计数同步；在落地前保持现状，避免本文档与生成表冲突。
- P2 需要季度复审记录；若长期无契约，可在后续 ADR 中转为永久关闭。
- P3 的部署模式会进入官方支持面，但必须与 TargetSpec 明确区分；文档与命令命名是主要风险。
- P4 / P5 在无赞助时不会消耗工程时间；一旦触发，需要设备、账号与维护承诺，成本可预期。
- P6–P8 保持零 SDK 文件入库；任何授权环境的工作都只能以插件与 mock 形态出现。

## 参考（References）

- [INITIATIVES](../INITIATIVES.md) §3 M9-P1–M9-P8（[English](../INITIATIVES.en.md)）
- [ROADMAP](../ROADMAP.md) §3.1（分级总览）、§11（文档同步，[English](../ROADMAP.en.md)）
- [README](../../README.md) 49 目标现状表（[English](../../README.en.md)）
- [`tools/gen-status-table.mjs`](../../tools/gen-status-table.mjs)（`planned` 清单与计数）
- [GA](../GA.md) §7.1（规划平台与环境槽对账）
- [ADR 0003](0003-wasm-plugins.md)（插件形态与 `target-spec/v1`）
