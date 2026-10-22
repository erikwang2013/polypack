# 商店元数据管线（M9-E5）

> 状态：已实现（T-M9-13/14/15）。`store/metadata/<platform>/<locale>.toml` 由 `polypack publish --metadata-dir` 校验并确定性打包为提审材料；`--check-metadata` 可单独校验不发布。截图只存路径，不读图片内容。

## 1. 目录与 schema

```text
store/metadata/
  wechat/
    default.toml
    zh-CN.toml      # 可选；请求 zh-CN 时优先，缺失回退 default 并告警
  alipay/
    default.toml
```

每个文件必须声明 `schema = 1`。未知版本、缺失 `default.toml`、重复 locale 都会报稳定错误，不静默降级。截图路径相对 platform 目录（如 `screenshots/home.png`），扩展名按平台规则校验。

## 2. 平台规则

长度按平台口径折算：CJK / 全角字符计 2，其余计 1。

| 字段 | 微信（wechat） | 支付宝（alipay） |
| --- | --- | --- |
| title | 必填，4–30 | 必填，3–20 |
| short_description | 必填，4–120 | 必填，10–32 |
| full_description | 必填，4–2000 | 必填，20–500 |
| keywords | 必填，1–10 项，每项 1–20 | 必填，1–10 项，每项 1–20 |
| screenshots | 0–10 个路径，png/jpg/jpeg/gif | 必填，2–5 个路径，png/jpg/jpeg |
| privacy_url / support_url | 必填，http(s) 绝对 URL | 必填，http(s) 绝对 URL |

规则来源与检索日期见 `crates/polypack-publish/src/metadata.rs` 模块文档；平台规则变化时升 `schema` 并同步该表。

## 3. 完整示例

```toml
# store/metadata/wechat/default.toml
schema = 1
title = "Pax Notes"
short_description = "离线优先的打包笔记"
full_description = "Pax Notes 是一个离线优先的笔记应用，支持多端打包与本地优先的数据存储。"
keywords = ["笔记", "离线", "打包"]
screenshots = ["screenshots/home.png", "screenshots/editor.png"]
privacy_url = "https://example.com/privacy"
support_url = "https://example.com/support"
```

## 4. 命令

```bash
# 只校验，不读取产物 / 凭据，不触网
polypack publish --check-metadata --metadata-dir store/metadata --channel wechat-open-platform

# 校验并作为提审材料发布
polypack publish --metadata-dir store/metadata --channel wechat-open-platform \
  --appid wx0000000000000000 --version 1.2.3 --credential env:WECHAT_KEY --package dist
```

`--materials <file>` 与 `--metadata-dir` 互斥；同时给出会被 clap 拒绝。`--metadata-locale` 可指定请求的 locale，缺失时回退 `default` 并输出告警。

## 5. 错误码

| 错误码 | 含义 |
| --- | --- |
| `metadata.default_missing` | platform 目录缺 `default.toml` |
| `metadata.locale_duplicate` | 归一化后 locale 重复 |
| `metadata.schema_missing` / `metadata.schema_unsupported` | 缺 `schema` 或不支持 |
| `metadata.field_missing` / `metadata.field_empty` | 必填字段缺失 / 为空 |
| `metadata.field_too_short` / `metadata.field_too_long` | 字段长度越界 |
| `metadata.keywords_too_few` / `metadata.keywords_too_many` | 关键词数量越界 |
| `metadata.keyword_empty` / `metadata.keyword_too_long` | 单个关键词非法 |
| `metadata.screenshots_required` / `metadata.screenshots_too_few` / `metadata.screenshots_too_many` | 截图数量不符 |
| `metadata.screenshot_path_invalid` / `metadata.screenshot_path_missing` / `metadata.screenshot_extension` | 截图路径越界 / 不存在 / 扩展名不符 |
| `metadata.url_invalid` | `privacy_url` / `support_url` 非 http(s) 绝对 URL |

校验返回全部违规，不短路。材料对象按 key 排序，摘要为 blake3(canonical JSON)，同输入跨进程 / 跨平台一致；回执的 `materials_digest` 记录该摘要。
