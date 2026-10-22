# OTA / 生产热更通道（M9-E4）

> 状态：T-M9-10 调研矩阵、T-M9-11 签名清单与 T-M9-12 真实演练均已完成；`examples/updater-app` + `scripts/drill.ps1` 经 rust-ci `windows-updater-real` 在 windows-2022 run 39395255929 通过（安装 0.1.0 → 更新 0.1.1 → 回滚 0.1.0）。
> 检索日期：2026-10-20。无法直连的官方页面在表中显式标注「未直连」，其细节不作为已验证事实。

## 1. 各端更新机制矩阵

| 机制 | 更新模型 | 签名 / 校验 | 灰度 / 回滚 | 人工步骤 | 官方来源（检索 2026-10-20） |
| --- | --- | --- | --- | --- | --- |
| Tauri v2 updater（自托管静态清单） | `latest.json` 静态托管或动态 endpoint；客户端按 SemVer 比较，`platforms.<os-arch>` 给 `url` 与 `signature`（`.sig` 文件内容）；更新包为 AppImage / `.app.tar.gz` / MSI / NSIS | 强制验签；`tauri signer generate` 生成密钥对，公钥写入 `plugins.updater.pubkey`，安装前校验产物 `.sig` | 静态清单无百分比灰度（按渠道分 URL）；动态服务器可覆写版本比较以安装指定版本，官方注明可用于回滚；静态 JSON 受内置版本检查限制，默认不降级 | 一次性生成并保管私钥；开启 `createUpdaterArtifacts`；构建并上传清单与产物到 HTTPS；配置 `endpoints` | https://v2.tauri.app/plugin/updater/（已直连） |
| Google Play 应用内更新 | Play Core In-App Updates：`flexible`（后台下载）或 `immediate`（全屏强制重启）；Android 5.0 / API 21+；安装与重启由 Google Play 执行 | 由 Google Play 交付与安装；应用内更新 API 不自管签名（Play App Signing 模型，该页本次未直连） | Play Console / Developer API 的 staged rollout（`userFraction` 逐步放量、可 halt）；不支持 versionCode 降级，回滚需发布更高 versionCode（staged rollout 细节为检索摘要，官方页未直连） | 上传 AAB/APK → 审核 → 分阶段放量 → 观察 Android vitals → 提到 100%；异常时 halt 并发布更高 versionCode | https://developer.android.com/guide/playcore/in-app-updates（已直连，页面标注 2026-01-30 UTC）；https://developers.google.com/android-publisher/tracks（未直连） |
| Apple App Store 分阶段发布 | App Store 系统更新；可选 phased release，对开启自动更新的用户 7 天放量 1% / 2% / 5% / 10% / 20% / 50% / 100% | Apple 代码签名与 App Review 审核后由 App Store 分发；开发者不自管更新签名 | 可暂停最多 30 天、暂停次数不限；下架会终止该版本的分阶段发布；不支持降级，需下架旧版并提交新版本恢复受控放量；手动下载不受阶段限制 | 提交版本 → 在 Phased Release 选 7 天 → 观察 → 必要时暂停 → 用新版本恢复 | https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/（已直连） |
| 微信小程序版本管理 | 开发版 → 审核版 → 线上版；发布后客户端在启动、定期检查、长时间未使用等时机检查更新；异步下载于下次冷启动生效，`wx.getUpdateManager` 可提示重启；全量发布约 24 小时覆盖 99% 以上 | 由微信平台审核与分发；开发者不控制二进制签名或更新通道；审核未通过不能发布 | 管理后台可回退版本（开发管理权限包含发布与回退）；可设「优先使用本地版本」「最低可用版本」；本次直连页面未见百分比灰度入口 | 上传代码 → 提交审核 → 审核通过后发布 → 异常时后台回退或调高最低可用版本 | https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/role.html、.../release.html、https://developers.weixin.qq.com/miniprogram/dev/framework/runtime/update-mechanism.html（均已直连） |
| 自托管签名渠道（PolyPack） | 每渠道静态 `latest.json`（`/<channel>/latest.json`），结构与 Tauri 静态清单相同；同时保留 `/<channel>/<version>.json` 版本副本 | `polypack update manifest` 用 GPG 分离签名清单（`latest.json.sig`），`polypack update check --trust-root` 验签；产物 `signature` 字段仍是 Tauri `.sig`；凭据只以 `CredentialRef` 引用出现 | v1 单渠道、无百分比灰度；回滚把版本副本复制回 `latest.json` 并重新上传（客户端仍需允许降级或切渠道）；TUF 只作威胁模型参考，未实现 root / timestamp 等角色 | 逐平台生成清单并签名 → 上传 latest、版本副本与产物 → 保留旧版本副本 → 回滚时恢复旧清单 | https://theupdateframework.io/docs/overview/、https://theupdateframework.io/docs/security/（均已直连）；本仓库 `polypack update` |

## 2. 分类结论

- **可自更新**：Tauri v2 自托管通道，以及复用其清单格式的 PolyPack 自托管渠道。真实二进制替换与回滚依赖目标应用内嵌 Tauri updater，仓库侧只负责清单与签名。
- **仅商店更新**：Google Play 与 Apple App Store。应用内更新 API / 分阶段发布只影响提示与放量节奏，安装、签名与审核仍由商店完成。
- **平台受控**：微信小程序。版本必须经平台审核后发布，客户端更新时机由微信决定，开发者只能回退或设置最低可用版本。

## 3. 清单 schema 与签名

`latest.json` 的必需键为 `version`、`platforms.<target>.url`、`platforms.<target>.signature`；`notes` 与 `pub_date` 可选。`platforms` 键取 Tauri 的 `OS-ARCH` 形式（如 `linux-x86_64`）。

```json
{
  "notes": "fix crash on launch",
  "platforms": {
    "linux-x86_64": {
      "signature": "<产物 .sig 文件内容>",
      "url": "https://updates.example.com/stable/Pax%20Notes_1.2.3_x64.AppImage"
    }
  },
  "pub_date": "2026-10-20T00:00:00Z",
  "version": "1.2.3"
}
```

### 3.1 规范形式

- 顶层键与平台键按字典序；两空格缩进；末尾固定一个换行。
- `pub_date` 必须是 RFC 3339；优先 `--pub-date`，其次 `SOURCE_DATE_EPOCH`，都未给才用当前时间。相同输入二跑字节一致。
- 产物 URL 由 `--base-url` 与百分号编码后的文件名拼接；生产环境必须 HTTPS。

### 3.2 签名与验签

- 清单签名复用 `polypack-sign` 的 `GpgSigner`（`RepoSigner::detach_sign`，SHA-256，ASCII armor），输出 `<manifest>.sig`。
- 凭据只接受 `env:VAR` / `keyring[:条目名]` / `vault[:条目名]` 引用；明文口令与私钥不进入命令行、日志或事件流。
- 验签走 `ArtifactVerifier`，信任根载荷为 base64(JSON) `{"public_key":"<base64>"}`。
- Tauri 原生校验各平台产物的 `.sig`（Tauri 密钥对），与本清单的 GPG 签名是两层，不能互相替代。

### 3.3 命令

```bash
# 生成并签名（默认写 <package-dir>/<channel>/latest.json 与 <version>.json）
polypack update manifest --channel stable --version 1.2.3 --notes "fix crash" \
  --base-url https://updates.example.com/stable --platform linux --arch x86_64 \
  --package-dir .polypack/packages/linux-reference \
  --credential env:POLYPACK_UPDATE_KEY --pub-date 2026-10-20T00:00:00Z

# 先做结构检查，再用信任根验签
polypack update check --manifest latest.json \
  --signature latest.json.sig --trust-root env:POLYPACK_UPDATE_TRUST
```

## 4. 回滚设计

1. 每次生成都保留 `<channel>/<version>.json` 与 `.sig`，发布时与 `latest.json` 一并上传；旧版本副本不覆盖。
2. 回滚时把目标旧版本的 `<version>.json` 复制为 `latest.json`，连同其 `.sig` 上传。签名覆盖的是同一份字节，无需重新签名。
3. 客户端必须允许降级：Tauri 静态清单默认执行内置版本检查，旧版本不会自动安装；需切到独立回滚渠道，或由动态更新服务器覆写版本比较。
4. 回滚后在下一次发布中提升补丁号，避免版本号回退造成后续更新判定混乱。
5. 仓库内已提供真实演练装置：`examples/updater-app` 的 `--smoke-update` 模式与 `scripts/drill.ps1` 在 windows-2022 上完成「安装 0.1.0 → 更新 0.1.1 → 恢复 0.1.0 清单 → 回滚」；回滚腿使用样例内演练专用的「版本不同即可」比较器，生产默认仍只接受更新版本。Linux 侧只验证编译与清单链路；真实链路由 rust-ci `windows-updater-real` 作业产出，run 39395255929 已通过。

## 5. 环境门控与未验证项

- **Tauri 二进制更新 / 回滚**：已由 `examples/updater-app`、`examples/updater-app/scripts/drill.ps1` 与 rust-ci `windows-updater-real` 作业覆盖（一次性 Tauri 密钥、真实 NSIS 安装 / 更新 / 回滚、证据归档）；windows-2022 run 39395255929 已通过。`polypack update check` 本身仍只验证清单结构与 GPG 签名。
- **Google Play staged rollout / Play App Signing**：官方页在本次环境不可达，表中相应细节来自检索摘要，标为未直连；已直连的只有 In-App Updates 的 flexible / immediate 流程。
- **Apple 降级**：官方文档未提供版本降级；只能暂停分阶段发布或用新版本恢复。
- **微信百分比灰度**：本次直连的版本管理与更新机制页面未列出百分比灰度入口，故不写入矩阵承诺。

## 6. 来源

- Tauri v2 Updater：https://v2.tauri.app/plugin/updater/（2026-10-20 直连）
- Google Play In-App Updates：https://developer.android.com/guide/playcore/in-app-updates（2026-10-20 直连，页面更新于 2026-01-30 UTC）
- Google Play Developer API Tracks（staged rollout）：https://developers.google.com/android-publisher/tracks（2026-10-20 未直连）
- Apple App Store Connect 分阶段发布：https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/（2026-10-20 直连）
- 微信小程序版本与回退权限：https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/role.html（2026-10-20 直连）
- 微信小程序发布流程：https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/release.html（2026-10-20 直连）
- 微信小程序更新机制：https://developers.weixin.qq.com/miniprogram/dev/framework/runtime/update-mechanism.html（2026-10-20 直连）
- TUF 概览与安全模型：https://theupdateframework.io/docs/overview/、https://theupdateframework.io/docs/security/（2026-10-20 直连）
