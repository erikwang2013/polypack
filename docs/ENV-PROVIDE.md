# 缺失环境槽输入提供指南（ENV-PROVIDE）

> 状态：用户入口文档（2026-10-21，基线 `main` v1.15.0）。本页说明**只有你能提供**的输入怎么交：凭据、授权、硬件 / 宿主、人类试用。
> 真实槽位状态与证据仍以 [ENV-SLOTS](ENV-SLOTS.md) 为准；**提供输入不会自动关闭槽位**，关闭仍需 §4.2 的真实执行证据与复核。
> English: [ENV-PROVIDE.en.md](ENV-PROVIDE.en.md)

## 1. 先选入口

| 你要提供 | 入口 | 红线 |
| --- | --- | --- |
| 密码 / token / 登录态 / 私钥文件 | [GitHub Actions Secrets](https://github.com/erikwang2013/polypack/settings/secrets/actions)（[新建](https://github.com/erikwang2013/polypack/settings/secrets/actions/new)）或 `node tools/env-provide.mjs --set-secret NAME` | 不贴 issue / 聊天 / 日志 / 提交 |
| 非机密配置（Team ID、CLI 路径、设备 IP） | [GitHub Actions Variables](https://github.com/erikwang2013/polypack/settings/variables/actions)（[新建](https://github.com/erikwang2013/polypack/settings/variables/actions/new)）或 `--set-var NAME=VALUE` | 不把机密伪装成变量 |
| 厂商授权 / 许可证 / NDA | `node tools/env-provide.mjs --open ENV-XX` 打开官方门户；用[授权登记](https://github.com/erikwang2013/polypack/issues/new?template=env-authorization.yml)记录批准编号 | 不把许可证文件、密钥、NDA 正文贴进 issue |
| 设备 / 宿主 / KVM / 局域网 | [硬件 / 宿主申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) | 不在公开 issue 写地址、门禁、电话 |
| 真人外部试用 | [试用报名](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml)；协议见 [HUMAN-TRIAL](HUMAN-TRIAL.md) | 不写姓名、邮箱、公司等敏感身份 |

Secrets 页面需要仓库 admin 权限；打不开时把**名称与来源**交给 maintainer，由其在设置页上传，不要改走 issue 贴值。

## 2. 凭据：GitHub Secrets / Variables

1. 打开 [Secrets](https://github.com/erikwang2013/polypack/settings/secrets/actions) 或 [Variables](https://github.com/erikwang2013/polypack/settings/variables/actions)，点 **New repository secret / variable**。
2. 名称必须与 §4 表逐字一致（大小写敏感）；名称错了 workflow 读到的就是空串。
3. 本地用 `node tools/env-provide.mjs`（读取 `tools/env-providers.json`，只打印名称与链接，不打印值）：

```sh
node tools/env-provide.mjs --slot ENV-02        # 该槽缺哪些名称、设置页链接、gh 命令、授权门户
node tools/env-provide.mjs --set-secret POLYPACK_APPLE_APP_PASSWORD   # TTY 隐藏输入，经 gh secret set 上传
node tools/env-provide.mjs --set-secret POLYPACK_GARMIN_DEVKEY_B64 --file developer_key.der  # 文件 → base64
node tools/env-provide.mjs --set-var POLYPACK_IOS_TEAM_ID=TEAMID1234  # 非机密变量
node tools/env-provide.mjs --check --slot ENV-02 --strict             # 只查名称；缺失时退出码非 0
```

4. `--file` 只用于清单要求文件型 secret 的项（如 ENV-14 的 `.der` devkey；Apple `.p12` 在清单登记名称后同理）；证书 / 私钥原始文件不进仓库、不进 issue。Apple 证书按官方流程导入 macos-14 钥匙串或以文件型 secret 提供。
5. 值只出现在隐藏输入或 `gh` 的 stdin；`--dry-run` 只打印将执行的名称与链接。脚本不在当前检出时，直接用第 1 步的直链。
6. 上传后不要截图值；需要确认时只回报名称与上传时间。

## 3. 授权：官方门户 + 登记编号

授权是组织动作，agent 不能代批。用 `node tools/env-provide.mjs --open ENV-XX` 打开该槽在 `tools/env-providers.json` 中登记的官方门户（下表列出主要入口，`--dry-run` 只打印 URL；主机 NDA 暂无槽，只能点表中链接）。批准后用[授权登记表](https://github.com/erikwang2013/polypack/issues/new?template=env-authorization.yml)记录**非机密**编号；许可证文件 / 密钥仍按 §2 走 secret。

| 提供方 | 官方入口（https） | 关联槽位 | 登记什么（非机密） |
| --- | --- | --- | --- |
| Apple Developer | [Programs](https://developer.apple.com/programs/) · [Account](https://developer.apple.com/account) | ENV-02 / ENV-03 | Team ID、证书 Common Name、Program 批准号 |
| BlackBerry QNX | [developers.blackberry.com](https://developers.blackberry.com/) | ENV-08 | 许可证 / entitlement ID |
| Wind River | [VxWorks](https://www.windriver.com/products/vxworks) | ENV-09 | Workbench 许可证 / 支持账号 ID |
| Garmin Connect IQ | [developer.garmin.com/connect-iq](https://developer.garmin.com/connect-iq/) | ENV-14 | 开发者账号 ID / SDK 协议引用 |
| Roku | [developer.roku.com](https://developer.roku.com/) | ENV-05 | 开发者账号 ID / 设备开发者模式确认 |
| 微信 | [公众平台](https://mp.weixin.qq.com/) · [开发者工具](https://developers.weixin.qq.com/miniprogram/dev/devtools/download.html) | ENV-04 | appid / 管理员审批引用 |
| 支付宝 | [open.alipay.com](https://open.alipay.com/) | ENV-04 | appid / 账号 ID |
| 抖音 | [developer.open-douyin.com](https://developer.open-douyin.com/) | ENV-04 | appid / 资质审批引用 |
| Firebase Test Lab | [firebase.google.com/docs/test-lab](https://firebase.google.com/docs/test-lab) | ENV-20（可选 provider） | GCP project ID |
| BrowserStack | [App Automate](https://www.browserstack.com/app-automate) | ENV-20（可选 provider） | 账号 / 计划 ID |
| AWS Device Farm | [aws.amazon.com/device-farm](https://aws.amazon.com/device-farm/) | ENV-20（可选 provider） | AWS 账号 ID / project ARN 的非机密部分 |
| PlayStation Partners | [partners.playstation.net](https://partners.playstation.net/) | 主机 NDA（暂无 ENV 槽） | partner ID / NDA 引用 |
| Xbox / ID@Xbox | [xbox.com/developers/id](https://www.xbox.com/en-US/developers/id) | 主机 NDA（暂无 ENV 槽） | 发行商 / ID@Xbox 批准引用 |
| Nintendo Developer | [developer.nintendo.com](https://developer.nintendo.com/) | 主机 NDA（暂无 ENV 槽） | 开发者账号 / NDA 引用 |
| Zepp OS | [developer.zepp.com](https://developer.zepp.com/) | ENV-13 | 开发者账号 ID |

授权编号本身若属保密内容，在表单里写 `confidential — sent via private channel` 并选择私有联系偏好；**不要**把保密编号贴进公开 issue。

## 4. 逐槽位：缺什么、去哪里交

下表只列**非 CLOSED** 槽位。名称与 [ENV-SLOTS](ENV-SLOTS.md) 执行卡一致；没有列出的名称不要自造。

| 槽位 | 需要的 Secrets | 需要的 Variables | 授权 / 官方入口 | 硬件 / 人工入口 | 文档锚点 |
| --- | --- | --- | --- | --- | --- |
| <a id="env-02"></a>ENV-02 | `POLYPACK_APPLE_APP_PASSWORD` | `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID`, `POLYPACK_APPLE_ID` | [Apple](https://developer.apple.com/programs/)（Team ID / 证书 CN；证书导入钥匙串） | 无（macos-14 托管 runner） | [§ENV-02](ENV-SLOTS.md#slot-env-02) |
| <a id="env-03"></a>ENV-03 | —（Apple 凭据在 worker 钥匙串） | `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID` | [Apple](https://developer.apple.com/programs/) | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（macOS 宿主 + 共享农场目录） | [§ENV-03](ENV-SLOTS.md#slot-env-03) |
| <a id="env-04"></a>ENV-04 | `POLYPACK_WECHAT_DEVTOOLS_LOGIN`, `POLYPACK_ALIPAY_DEVTOOLS_LOGIN`, `POLYPACK_DOUYIN_DEVTOOLS_LOGIN` | `POLYPACK_WECHAT_DEVTOOLS_CLI`, `POLYPACK_ALIPAY_DEVTOOLS_CLI`, `POLYPACK_DOUYIN_DEVTOOLS_CLI`, `POLYPACK_DEVTOOLS_LOGIN_FILE` | [微信](https://mp.weixin.qq.com/) · [支付宝](https://open.alipay.com/) · [抖音](https://developer.open-douyin.com/) | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（macOS GUI 扫码 / 2FA 宿主） | [§ENV-04](ENV-SLOTS.md#slot-env-04) |
| <a id="env-05"></a>ENV-05 | `ROKU_DEV_PASSWORD` | `ROKU_DEV_TARGET` | [Roku](https://developer.roku.com/)（开发者账号） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（Roku + 同 LAN 宿主） | [§ENV-05](ENV-SLOTS.md#slot-env-05) |
| <a id="env-06"></a>ENV-06 | — | — | 无（OpenBSD 开源） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（KVM self-hosted runner / OpenBSD 主机） | [§ENV-06](ENV-SLOTS.md#slot-env-06) |
| <a id="env-08"></a>ENV-08 | — | `POLYPACK_QNX_SDP`, `POLYPACK_QNX_QEMU`, `POLYPACK_QNX_QEMU_ARGS` | [BlackBerry QNX](https://developers.blackberry.com/)（商业许可证） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（self-hosted runner + 许可证服务器） | [§ENV-08](ENV-SLOTS.md#slot-env-08) |
| <a id="env-09"></a>ENV-09 | — | `WR_CC`, `POLYPACK_VXWORKS_TRIPLE` | [Wind River VxWorks](https://www.windriver.com/products/vxworks)（商业许可证） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（self-hosted runner + Workbench SDK） | [§ENV-09](ENV-SLOTS.md#slot-env-09) |
| <a id="env-10"></a>ENV-10 | — | `POLYPACK_WEAR_FORM_APK`, `POLYPACK_WEAR_FORM_PACKAGE` | 无（模拟器）；物理设备需 ADB 授权 | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（KVM runner 或 Wear OS 设备） | [§ENV-10](ENV-SLOTS.md#slot-env-10) |
| <a id="env-13"></a>ENV-13 | — | — | [Zepp OS](https://developer.zepp.com/)（开发者账号） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（GUI 模拟器宿主） | [§ENV-13](ENV-SLOTS.md#slot-env-13) |
| <a id="env-14"></a>ENV-14 | `POLYPACK_GARMIN_DEVKEY_B64`（`.der` 经 `--file`） | `CIQ_HOME`, `CONNECTIQ_HOME`, `CIQ_MONKEYC`, `CIQ_DEVKEY`（runner 本地路径） | [Garmin Connect IQ](https://developer.garmin.com/connect-iq/) | 无（CI 腿；工作流待建） | [§ENV-14](ENV-SLOTS.md#slot-env-14) |
| <a id="env-19"></a>ENV-19 | — | — | 无（人类流程） | [试用报名](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml) · [协议](HUMAN-TRIAL.md) | [§ENV-19](ENV-SLOTS.md#slot-env-19) |
| <a id="env-20"></a>ENV-20 | —（provider 适配层未接线，勿自造名称） | — | [Firebase](https://firebase.google.com/docs/test-lab) · [BrowserStack](https://www.browserstack.com/app-automate) · [AWS](https://aws.amazon.com/device-farm/)（可选云账号） | [硬件申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)（Android + Android TV 或云农场） | [§ENV-20](ENV-SLOTS.md#slot-env-20) |

## 5. 硬件 / 宿主

- 入口：[硬件 / 宿主申请](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml)。槽位下拉覆盖 ENV-03 / 05 / 06 / 08 / 09 / 10 / 13 / 20。
- 表单只收**非机密**信息：设备型号、用途、request type（设备借用 / 寄送、宿主访问、KVM runner、同 LAN、云农场）、粗粒度时区、GitHub 联系人。
- **收货地址、电话、门禁、序列号等只在 maintainer 建立的私有通道提交**；公开 issue 不写。
- 提交后 maintainer 在 issue 回复安排方式；**未真正寄出 / 未拿到访问前，不得声称硬件已提供**，槽位保持 OPEN / PARTIAL。

## 6. 人类试用（ENV-19）

- 入口：[试用报名](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml)；协议与同意条款见 [HUMAN-TRIAL](HUMAN-TRIAL.md)。
- 表单收集：操作系统、Rust / 移动 / 发布背景、90 分钟可用时间、录屏 / 录音同意、联系偏好。
- **不收集**姓名、邮箱、电话、公司、住址等敏感身份；报告只用 P1 / P2 编号，公开前脱敏。
- 少于 3 份真实报告不得关闭 ENV-19；内部演练或 AI 角色扮演不算。

## 7. `env-provide` 命令（预期行为）

脚本由 `tools/env-provide.mjs` 提供，读取 `tools/env-providers.json`；本页只描述行为，值永不进入 argv / 日志。

| 命令 | 预期行为 |
| --- | --- |
| `--list [--json]` | 列出全部槽位的 secrets / variables 名称与是否已配置；`gh` 不可用时显示 unknown |
| `--check [--slot ENV-XX] [--strict]` | 只检查名称是否存在；`--strict` 在缺失时退出非 0 |
| `--slot ENV-XX` | 打印该槽缺失名称、Settings 直链、`gh` 命令与授权门户 |
| `--open ENV-XX [--dry-run]` | 用系统浏览器打开该槽官方门户；`--dry-run` 只打印 URL |
| `--set-secret NAME [--file PATH]` | TTY 隐藏输入，或把 `PATH` 转 base64；确认后经 `gh secret set` 从 stdin 上传 |
| `--set-var NAME=VALUE` | 上传**非机密**仓库变量；值会出现在命令行，机密禁用 |
| `--repo OWNER/REPO` | 覆盖默认目标 `erikwang2013/polypack` |

## 8. 安全与“已提供”的判定

- 任何 secret / token / 私钥 / 许可证文件都只经 Secrets 或 `--set-secret`；issue、聊天、日志、截图、提交一律禁止。
- 发现泄露：立即在 GitHub / 厂商门户轮换或吊销，再按 [ENV-SLOTS](ENV-SLOTS.md) §4.2 重新取证。
- 授权登记编号、Team ID、appid、project ID 是非机密引用；不要用它们代替凭据。
- `--check` 全绿只代表名称存在，**不代表值正确、权限足够或真实腿已跑**；槽位关闭以 ENV-SLOTS §4.2 证据为准。
- 本页所有链接使用 https 与官方域名；深层路径不确定时只链接官方落地页。

## 9. 参考

- [ENV-SLOTS](ENV-SLOTS.md) · [HUMAN-TRIAL](HUMAN-TRIAL.md) · [DEVICE-PREVIEW](DEVICE-PREVIEW.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [GA](GA.md)
- 英文版：[ENV-PROVIDE.en.md](ENV-PROVIDE.en.md)
