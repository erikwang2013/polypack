# 0009 — LINE LIFF 部署模式：`polypack deploy line-liff`

- 状态（Status）：accepted
- 日期（Date）：2026-10-20
- 决策者（Deciders）：项目维护者

> **English abstract**: LINE LIFF apps are registered as hosted URLs in the LINE Developers Console; there is no local package format, LIFF config file, or CI upload API. This record implements M9-P3 as a deploy mode, not a TargetSpec: `polypack deploy line-liff --dir <dir> --config line-liff.toml` validates the config and static directory, prints manual Console steps, and never touches the network. It does not change the 49-target table or its tier counts. ADR 0006 records the platform verdict; this ADR covers the implemented contract.

## 背景（Context）

[ADR 0006](0006-platform-decisions.md) 将 M9-P3 LINE Mini App 定为**条件支持（仅部署模式）**：LIFF 应用由 LINE Developers Console 以托管 URL 注册（LINE Login 渠道 → LIFF；Endpoint URL 必须 HTTPS 且不得含 fragment），官方没有本地上传包格式、LIFF 配置文件或 CI 上传 API。把它声明为 TargetSpec 会产出一个平台不存在的「包」，因此打包链不接入。

出现使用 LINE 渠道的用户后，重复的人工检查仍然是真实成本：Endpoint 是否 HTTPS、是否含 fragment、频道 / LIFF ID / scopes 是否齐全。本 ADR 记录一个只做本地校验与指引的部署模式，作为该条件下的最小可用交付。

## 决策（Decision）

新增 `polypack deploy` 子命令，首个模式为 `line-liff`：

```bash
polypack deploy line-liff --dir <dist> --config line-liff.toml
```

- **不是 TargetSpec**：不改 `crates/polypack-targets/src/builtin.rs`，不进入 `TargetRegistry`，不生成任何平台产物，也不改变 49 目标表与 L1–L7 计数。
- **配置**：`line-liff.toml`，`schema = 1`；必填 `liff_id`、`endpoint`、`channel_id`、`scopes`；未知字段直接拒绝。
- **校验**：`schema` 必须为 1；必填字段非空且不含空白 / 控制字符；`channel_id` 必须为数字；`endpoint` 必须为 HTTPS、带主机名、不含 fragment；`scopes` 非空且每项非空；`--dir` 必须是已存在的目录。
- **输出**：列出配置摘要与 5 条 LINE Developers Console 人工步骤；`--json` 输出单个 `deploy_dry_run` 事件，显式带 `dry_run=true`、`network=false`、`target_spec=false`。
- **始终 dry-run**：命令不访问网络、不上传、不写任何文件；真正的静态托管与 Console 注册由人工完成。没有官方 CI 上传 API，因此不实现 `--publish` 一类选项。
- **错误码**：配置 / 目录输入错误统一为 `polypack::E_DEPLOY_0001`，提示指向配置字段规则。

落地位置：`crates/polypack-cli/src/deploy.rs`、`crates/polypack-cli/src/main.rs`、`crates/polypack-core/src/diagnostics.rs`；端到端测试 `crates/polypack-cli/tests/deploy.rs`；使用说明 [LINE-DEPLOY](../LINE-DEPLOY.md)（[English](../LINE-DEPLOY.en.md)）。

## 配置示例（Config example）

```toml
schema = 1
liff_id = "1657038298-AbCdEfgh"
endpoint = "https://example.com/liff/"
channel_id = "1657038298"
scopes = ["profile", "openid"]
```

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 声明 `line-liff` TargetSpec 并生成「静态包 + LIFF 配置」 | 官方没有本地上传包格式或 LIFF 配置文件；产物无法上传或审核，属于伪造平台产物 |
| 只写文档，不提供命令 | Endpoint / fragment / 必填字段的检查继续靠人工，容易在发版时漏项 |
| 复用通用静态托管命令 | 通用命令不校验 LIFF 专属规则（HTTPS、无 fragment、channel / liff ID、scopes），也无法输出 Console 步骤 |
| 调用 Console / LINE API 自动注册 | 官方没有公开的 LIFF 创建或上传 API；伪造自动化会误导用户 |

## 影响（Consequences）

- M9-P3 在仓库内的部分（T-M9-26 / T-M9-27）已交付；真实 Console 注册与静态托管仍是人工 / 外部步骤，不标为自动验证。
- 49 目标表中 P3 行保留「条件支持 · 部署模式」标注，仍不声明 TargetSpec；P1 / P2 的 retired / closed 状态不受影响。
- Endpoint 规则由 `schema = 1` 固定；LINE 规则变化时提升 schema 或新开 ADR，不静默放宽校验。
- 若 LINE 未来发布可核验的本地包格式、配置契约与上传 / 审核流程，另开 ADR 再评估升级为 TargetSpec。

## 参考（References）

- [ADR 0006 — 规划平台结论](0006-platform-decisions.md)
- [LINE-DEPLOY 使用说明](../LINE-DEPLOY.md)（[English](../LINE-DEPLOY.en.md)）
- [INITIATIVES M9-P3](../INITIATIVES.md)（[English](../INITIATIVES.en.md)）
- `crates/polypack-cli/src/deploy.rs`、`crates/polypack-cli/tests/deploy.rs`
