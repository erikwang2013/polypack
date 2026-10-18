# 决策记录（ADR）

本目录存档 PolyPack 的架构与流程决策，回答「为什么这么选」，避免理由只散落在提交、讨论和记忆里。对应 ROADMAP §13 孵化项 E8。

## 命名

`NNNN-kebab-title.md`：四位零填充编号，标题小写、连字符分隔，编号递增且不复用。例如 `0003-wasm-plugins.md`。

## 状态

| 状态 | 含义 |
|---|---|
| `proposed` | 已提出，尚未定稿 |
| `accepted` | 已采纳，作为当前依据 |
| `superseded` | 已被后续 ADR 取代；正文保留，注明后继编号 |

已采纳的 ADR 不回改正文；需要改变决策时新开一条 ADR，并把旧条目标为 `superseded`。

## 语言约定

中文为权威正文；每条 ADR 顶部附一段英文摘要（English abstract），供英文读者快速了解结论。暂不为每条 ADR 维护英文全文镜像；需要时再按 ROADMAP §11 的双语约定扩充。

## 模板

复制 [template.md](template.md)，编号取当前最大值 + 1。

## 索引

| 编号 | 决策 | 状态 |
|---|---|---|
| [0001](0001-bilingual-docs.md) | 双语文档：中文为主，英文镜像 + 结构校验 | accepted |
| [0002](0002-single-rust-workspace.md) | 单一 Cargo workspace，不拆 crate / 仓库 | accepted |
| [0003](0003-wasm-plugins.md) | WASM 插件沙箱与 `target-spec/v1` WIT | accepted |
