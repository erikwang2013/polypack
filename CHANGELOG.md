# Changelog

本仓库的显著变更记录于此。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。
版本号自 v1.0.0 起按语义化版本递增（tag 与 GitHub Release 名称一致，见 [README](README.md#路线图)）。
Notable changes are recorded here; since v1.0.0, tags and releases follow `vX.Y.Z` semantic increments.

## [Unreleased]

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
