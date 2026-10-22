# LINE LIFF 部署模式（M9-P3）

> 状态：`polypack deploy line-liff` 已交付（T-M9-26/27，2026-10-20）；命令始终 dry-run，不触网、不上传、不生成平台包。决策见 [ADR 0009](decisions/0009-line-deploy.md)，平台结论见 [ADR 0006](decisions/0006-platform-decisions.md)。

## 1. 定位

LIFF 应用由 LINE Developers Console 以**托管 URL** 注册（LINE Login 渠道 → LIFF），官方没有本地上传包格式、LIFF 配置文件或 CI 上传 API。因此本功能是**部署模式，不是 TargetSpec**：

- 不进入 `TargetRegistry`，不改变 49 目标表与 L1–L7 计数；
- 不产出「平台包」，`polypack deploy line-liff` 只校验配置与静态目录；
- 真正的上传 / 发布由人工在 Console 完成，命令不调用 LINE API。

## 2. 配置

在工程根放置 `line-liff.toml`（也可用 `--config` 指向其他路径）：

```toml
schema = 1
liff_id = "1657038298-AbCdEfgh"
endpoint = "https://example.com/liff/"
channel_id = "1657038298"
scopes = ["profile", "openid"]
```

- `schema`：固定为 `1`；规则变化时提升版本，不静默放宽。
- `liff_id`：Console 生成的 LIFF ID，非空且不含空白。
- `endpoint`：LIFF 的 Endpoint URL，必须 HTTPS、有主机名、不含 fragment。
- `channel_id`：LINE Login 渠道 ID，只允许数字。
- `scopes`：至少一项，每项非空（如 `profile`、`openid`）。

未知字段会被拒绝，避免拼写错误被静默忽略。

## 3. 命令与校验

```bash
# 先产出 Web 静态目录（按工程实际命令）
polypack build --target web

# 校验并打印 Console 人工步骤
polypack deploy line-liff --dir dist --config line-liff.toml

# 机器可读事件
polypack deploy line-liff --dir dist --config line-liff.toml --json
```

校验失败统一返回 `polypack::E_DEPLOY_0001`，覆盖：`schema` 非 1、必填字段缺失或为空、`channel_id` 非数字、Endpoint 非 HTTPS / 缺主机名 / 含 fragment、`scopes` 为空、`--dir` 不存在或不是目录。

`--json` 输出单个事件：

```json
{"event":"deploy_dry_run","mode":"line-liff","dir":"dist","liff_id":"1657038298-AbCdEfgh","channel_id":"1657038298","endpoint":"https://example.com/liff/","scopes":["profile","openid"],"dry_run":true,"network":false,"target_spec":false,"steps":["…"]}
```

## 4. Console 人工步骤

1. 将 `--dir` 目录的内容部署到 `endpoint`（HTTPS；URL 不得含 fragment）。
2. 打开 LINE Developers Console → Provider → Channel `<channel_id>` → LINE Login → LIFF → Add。
3. Endpoint URL 填 `endpoint`；Scopes 填配置中的列表；LIFF app name 按应用填写。
4. 保存后打开 Console 给出的 LIFF URL，确认页面加载正常。
5. 上传 / 发布由人工完成；本命令不调用 LINE API、不触网。

## 5. 边界与未做事项

- **无网络**：命令不访问 LINE 或任何远端；`network=false` 是固定契约，不是开关。
- **无上传 API**：官方未提供 LIFF 创建 / 上传 API，故不实现自动注册或 `--publish`。
- **无 TargetSpec**：不新增 `crates/polypack-targets/src/builtin.rs` 条目；若 LINE 未来发布可核验的本地包格式与上传 / 审核流程，另开 ADR 再评估。
- **人工演练**：真实 Console 注册需要 LINE 账号与 HTTPS 托管，属外部步骤；仓库内只验证配置规则、dry-run 输出与错误码。

相关文件：`crates/polypack-cli/src/deploy.rs`、`crates/polypack-cli/tests/deploy.rs`、[ADR 0009](decisions/0009-line-deploy.md)。
