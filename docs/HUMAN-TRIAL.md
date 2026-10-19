# 人类外部开发者试用协议

> 状态：协议草案（T-M9-48），可由维护者直接执行；执行 ≥ 3 名真实试用者（T-M9-49）是外部人工活动，不能在 CI 或由 agent 代跑。
> 目标：验证「1 天内新增自定义目标并出包」对外部开发者是否可行，并按严重度回写问题。
> English: [HUMAN-TRIAL.en.md](HUMAN-TRIAL.en.md)

## 1. 范围与产出

- 适用：未参与本仓开发的外部开发者，只依据 [GA](GA.md) §2 与 [README](../README.md) 从零操作。
- 产出：每人一份匿名报告（模板见 §7）、一份汇总 issue、修复清单与文档回写 PR。
- 非目标：正式可用性实验室、眼动 / 生理测量、与试用无关的个人数据采集。
- 本工作只交付协议与模板；内部彩排与真实试用必须由维护者另行执行并留痕。

## 2. 招募标准

| 维度 | 要求 |
| --- | --- |
| 身份 | 未参与 PolyPack 仓库开发，未阅读内部设计文档 |
| 经验 | 至少 1 名 Rust 背景、1 名移动 / 前端背景、1 名跨平台发布背景 |
| 环境 | Linux / macOS / Windows 至少各 1 台干净机器；允许预装 Rust |
| 时间 | 能连续投入 90 分钟，并同意 think-aloud 记录 |
| 排除 | 与维护者有直接汇报关系，或参与本里程碑评审者 |

样本目标 3–5 人；少于 3 份真实报告不得关闭 T-M9-49。

## 3. 知情同意

试用开始前必须逐条确认并留痕（电子或纸质）：

- 目的与流程：90 分钟任务、think-aloud、屏幕与语音记录（仅在同意时）。
- 自愿与退出：可随时停止，无需说明理由，不影响任何权益或报酬。
- 数据最小化：只收集任务相关命令、耗时、卡点原文与环境版本；不收集账号、密钥、客户数据。
- 匿名化：报告使用编号（P1、P2…），不写姓名、公司、邮箱；引用前去除路径中的用户名。
- 记录保留：原始录屏 / 录音在报告归档后 90 天内删除；匿名报告可公开。
- 报酬：如有，金额与方式在开始前书面说明，不因负面反馈扣减。
- 联系方式与投诉渠道：开始前提供维护者联系人与数据处理联系人。

## 4. 90 分钟任务脚本

| 阶段 | 分钟 | 任务 | 观察点 |
| --- | --- | --- | --- |
| 准备 | 10 | 检查环境、签署同意书、说明 think-aloud | 是否理解目标与记录方式 |
| 脚手架 | 20 | `poly init` 或 `poly plugin new pocket-notes`，再跑 `poly plugin check` | 文档入口、首次报错、等待时间 |
| 自定义目标 | 35 | 按 GA §2 实现 describe / package 并声明自定义目标 | 规格理解、插件模板、调试循环 |
| 出包 | 15 | `poly build --config <app>/polypack.toml --target pocket-notes`，再跑 `poly verify` | 命令可发现性、产物位置、失败信息 |
| 复盘 | 10 | 按 §6 分级、补充匿名报告、开放提问 | 真实卡点与改进建议 |

主流程命令示例（以 GA §2 与试用工程为准）：

```bash
poly plugin new pocket-notes
poly plugin check pocket-notes
poly build --config <app>/polypack.toml --target pocket-notes
poly verify --target pocket-notes
```

## 5. 观察与 think-aloud

- 让试用者在执行前说出意图，在执行后说出结果是否符合预期。
- 主持人只在卡住超过 2 分钟、涉及安全 / 隐私，或试用者主动求助时介入。
- 记录时间戳、逐字引用、命令序列、错误码、打开的文档页面与停留时长。
- 不先给解决方案；用「你现在想完成什么？」「你期望发生什么？」追问。
- 区分「文档缺失」「命令失败」「概念误解」三类卡点，避免混为一谈。
- 试用结束后 10 分钟内补记主持人的观察，不依赖事后回忆。

## 6. 问题分级

| 级别 | 定义 | 示例 | 处理时限 |
| --- | --- | --- | --- |
| `blocker` | 无法完成脚手架 → 自定义目标 → 出包核心路径，且无可行绕行 | 模板无法构建、目标无法加载、出包必失败 | 发布前修复或明确披露 |
| `friction` | 可完成，但额外耗时超过 10 分钟或文档误导 | 命令参数难找、错误信息不指向修复 | 下个文档批次处理 |
| `suggestion` | 体验改进，不阻塞任务 | 输出措辞、示例顺序、补全提示 | 进入排期评审 |

每条问题必须附：步骤、原文引用、期望 / 实际、是否阻塞、证据位置（时间戳或截图编号）。

## 7. 匿名化报告模板

```yaml
participant: P1
date: YYYY-MM-DD
environment:
  os: "Linux / macOS / Windows + version"
  rust: "rustc --version"
  poly: "poly --version"
timeline:
  start: "HH:MM"
  package_built: "HH:MM"
  total_minutes: 0
tasks:
  scaffold: pass # pass | fail
  custom_target: pass # pass | fail
  package: pass # pass | fail
issues:
  - severity: blocker # blocker | friction | suggestion
    step: "phase and command"
    quote: "verbatim user words, redacted"
    expected: "what the user expected"
    actual: "what happened"
    workaround: "none or description"
consent:
  recording: false
  anonymized_publication: true
notes: "facilitator observations, max 10 lines"
```

报告只写编号，不写姓名 / 公司 / 邮箱；路径中的用户名、主机名、token 一律替换为 `<redacted>`。原始材料按 §3 的保留期删除。

## 8. 外部性与 T-M9-49 边界

- 本文交付的是 T-M9-48（协议、同意、脚本、分级与报告模板），不代表已完成内部彩排或真实试用。
- T-M9-49（≥ 3 名人类外部开发者试用）是外部 / 人工活动，不能在 CI、脚本或由 agent 模拟执行。
- 不得用内部演练、AI 角色扮演或问卷占位冒充真实报告；没有真实签署的同意与匿名报告，就不得把 T-M9-49 标为完成。
- 协调者回写时：附匿名报告链接、为 `blocker` 建 issue、更新 GA §7.2；在满 3 份真实报告前保持 [ENV-SLOTS](ENV-SLOTS.md) ENV-19 为 `OPEN`。
