# polypack-watch（实验）

`poly init --target watchos` 生成的 watchOS 伴生模板（T-M7-05）：最小可 `xcodebuild`
构建的 SwiftUI 手表 App，含核心 API 调用桩与 WatchConnectivity 说明。

> **实验目标**：手表端 `runtime = none`——PolyPack 核心**不直接在手表上运行**。
> 真实核心调用由 iOS 伴生 App 代理：手表经 WatchConnectivity 请求 → iOS 宿主调用核心 →
> 回传结果。本模板可独立构建（watch-only）；接入 iOS 工程时移除 `WKWatchOnly` 并补
> `WKCompanionAppBundleIdentifier` 即可配对。

## 目录结构

```text
polypack.toml                 工程配置（[targets.watchos] format = "watch"）
WatchApp/
  PolyPackWatch.swift         @main SwiftUI App 入口
  ContentView.swift           手表界面（调用 CoreBridge.fetchStatus）
  CoreBridge.swift            核心 API 调用桩（TODO：接入 iOS 代理）
  WatchConnectivity.swift     WatchConnectivity 通道说明 / 骨架
  Info.plist                  watchOS App 清单（WKApplication）
PolyPackWatch.xcodeproj/      最小 Xcode 工程 + 共享 scheme
```

## 构建（macOS 宿主）

```bash
# 仅编译（无需签名）
xcodebuild -project PolyPackWatch.xcodeproj -scheme PolyPackWatch \
  -destination 'generic/platform=watchOS' build CODE_SIGNING_ALLOWED=NO

# 走 PolyPack 完整链路（package → sign → verify）
poly build --target watchos
```

`poly build` 的 package 阶段执行 `xcodebuild archive` + `ditto` 到
`.polypack/packages/watchos/export/Payload/polypack-watch.app`；sign 阶段复用 Apple
codesign 链（`apple-codesign` 凭据引用）签名该 `.app`（硬化运行时 + 安全时间戳），
verify 阶段对同一产物跑 `codesign --verify --deep --strict` + `spctl --assess`。

## Linux / Windows 宿主（R3）

Xcode 工具链只随 macOS 提供：非 macOS 宿主上 `poly` 如实报 `poly::E_BND_0001`
（ToolMissing，提示远程农场）。发起构建走远程农场 macOS worker：

```bash
poly farm submit --target watchos
```

## 凭据

工程与模板不含任何明文凭据。签名档案 `apple-codesign` 只在目标注册表 /
`polypack.toml` 中以**引用名**出现；证书与描述文件由系统钥匙库 / Vault / CI 注入
（ROADMAP §7），`POLYPACK_IOS_IDENTITY` / `POLYPACK_IOS_TEAM_ID` 是非机密配置项。
