# 0001 — 双语文档（中文主文档 + 英文镜像）

- 状态（Status）：accepted
- 日期（Date）：2026-10-19（回填）
- 决策者（Deciders）：项目维护者

> **English abstract**: Chinese is the authoritative documentation language; user-facing docs ship English mirrors. Any change to architecture, features, flows, or the platform matrix updates both sides in the same commit, and `tools/check-docs.mjs` enforces structural parity (heading counts, table rows, task IDs) across seven zh/en pairs.

## 背景（Context）

项目以中文为工作语言，同时通过 `README.en.md` 面向英文读者。中英文档若各自演化，平台矩阵、任务状态与规划会悄悄漂移。ROADMAP §11 已把双语同步列为提交检查项。

## 决策（Decision）

1. 中文文档为权威版本；英文文档为用户可读镜像，覆盖七对：README、ROADMAP、TASKS、FARM、EXPERIMENTAL-TARGETS、FARM-DEPLOY、GA。
2. 影响架构 / 功能 / 流程 / 平台矩阵的改动，按 ROADMAP §11 在同一次提交内成对更新。
3. 结构一致性由 `tools/check-docs.mjs` 在 CI（`.github/workflows/docs-check.yml`）校验：H2 / H3 标题数、表格行数、TASKS 任务 ID 集合，以及生成表、49 目标现状表与 experimental 三处一致。
4. 不做逐句翻译比对；语义质量由评审负责。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 仅中文 | 英文读者没有入口，且 `README.en.md` 已是既有承诺 |
| 仅英文 | 与项目工作语言及既有文档体系不符 |
| 全文机器翻译 / 逐句比对 | 成本高；结构对账已能拦住大部分漂移 |

## 影响（Consequences）

- 每次文档改动需成对提交，评审面变大。
- `check-docs.mjs` 只做结构检查，发现不了两侧语义漂移；内容仍需人工核对。
- 新增文档对必须登记进 `PAIRS`（`tools/check-docs.mjs`），否则不受门禁保护。

## 参考（References）

- [ROADMAP](../ROADMAP.md) §11（[English](../ROADMAP.en.md) §11）
- [tools/check-docs.mjs](../../tools/check-docs.mjs)
- [docs-check 工作流](../../.github/workflows/docs-check.yml)
