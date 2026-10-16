# Apple tvOS（RN-tvOS）RN 宿主骨架（T-M4-06）

`@polypack/adapter-native` 的 tvOS 承载宿主：装配层注册组件、接遥控器 / Back 事件源，10-foot
蒙皮由 `createAdapter({ target: "tvos" })` 绑定（页面 `platformStyle()` 零参直取，T-M4-06）。
打包链复用 M2 Xcode 链（T-M4-07，`poly build --target tvos`），本目录的 `ios/` 是 Xcode 工程位。

## 铺开布局（与 android / harmony 宿主同规矩：模板按工程根铺开）

```text
<工程根>/                 ← 本目录内容（index.js / app.json / package.json / metro.config.js …）
├── node_modules/        ← JS 侧 npm 依赖（react-native 实为 RN-tvOS 别名，见下「依赖链」）
├── index.js             ← RN JS 入口（装配 adapter + 遥控器事件源）
├── app.json             ← AppRegistry 键 PaxApp + polypack.pointer（tvos 恒 "none"）
├── ios/                 ← Xcode 工程位（RN-tvOS 的 *.xcodeproj + App-tvOS target 铺设点；
│                           未就位时打包链回退骨架工程，T-M4-07）
└── polypack.toml
```

## 装配链

```text
AppRegistry(PaxApp) → host/tvos/index.js
  ├─ createAdapter({ target: "tvos", runtime: RN, host: { pointer: "none" } })
  │    └─ platformStyle() = 10-foot 蒙皮（值口径单源 = ui/adapters/tv/src/tokens.js）
  ├─ bindPrimitives(..., "tvos", createElement)   # 无剔除项：hasTVPreferredFocus 原生支持
  ├─ createExampleApp(RN, adapter) → App / onKey（方向键遍历 + Back 焦点记忆）
  ├─ adapter.mount({ appKey: "PaxApp" }, TvKeyBridge)   # 事件桥包住 App（hook 须在组件内）
  ├─ useTVEventHandler → onKey → console `POLYPACK_KEY {…}`（走查通道）
  └─ BackHandler("hardwareBackPress") → onKey("Back")   # 遥控器 menu 键
```

## 键事件约定（T-M4-06）

- **方向 / 确认**：`useTVEventHandler`（`react-native` 具名导出，RN-tvOS）——事件名是字符串
  `eventType`（原生常量 `RCTTVRemoteHandlerConstants`：`up/down/left/right/select/menu/playPause/
  pageUp/pageDown/longSelect/…`）。`index.js` 的 `EVENTTYPE_TO_KERNEL` 映射五向：
  up/down/left/right/select（= 内核 DPAD_* 键名），**只消费 Down**（`eventKeyAction`：0=Down /
  1=Up——一次物理按键派发两帧，全收则中心键激活双计，同 android ACTION_DOWN / RNOH keyAction 过滤）。
- **Back = 遥控器 menu 键**：走核心 `BackHandler('hardwareBackPress')`——RN-tvOS 的
  `BackHandler.ios.js` 在 `Platform.isTV` 下正是对 `eventType === 'menu'` 的封装。**menu 不入
  `EVENTTYPE_TO_KERNEL`**（两路都接会双计；同 harmony 宿主「Back 不走键表」）。menu 键默认被
  Apple 指南吞掉（RN issue #18930），须 `TVEventControl.enableTVMenuKey()` 开闸（`TvKeyBridge`
  挂载时开、卸载对偶关）。
- 走查通道契约（与 `device-walks/`）不变：每键一行 `POLYPACK_KEY <json>`（单参数——多参数会被
  RN console 加引号拼接，破坏通道契约，T-M2-05 实测）。tvOS 侧观测：Metro 终端（debug）或
  Console.app / `xcrun simctl spawn <udid> log stream`（release）。
- 激活/遍历语义由共享焦点内核决定（`ui/adapters/native/src/focus.js`，T-M4-01/02/03 已入 main），
  与 adapter-tv（react-dom）同一份遍历 / Back 记忆栈——宿主只负责把遥控器事件译成内核键名。

## 依赖链与版本口径

- `react-native` 以 npm 别名安装：`"react-native": "npm:react-native-tvos@0.87.1-1"`（RN-tvOS
  官方装法，对 Metro/CLI/解析透明——包名仍是 `react-native`）；配套 `react@19.2.3`
  （RN-tvOS 0.87.1-1 的 peer `^19.2.3`）、`@react-native/{babel-preset,metro-config}@0.87.1`、
  社区 CLI `20.2.0`（与 android 宿主同代数——tvOS 线无 RNOH 的 0.84 代数差）。
- `TVEventControl` / `useTVEventHandler` / `BackHandler` 均为别名包的具名导出（0.87.1-1 实测
  口径：`index.js` 的 `get TVEventControl()` / `get useTVEventHandler()` 已验证）。
- tvOS 部署目标 ≥ 15.1（RN-tvOS 0.87 线要求；打包链 pbxproj 口径见 T-M4-07）。

## 10-foot 蒙皮（T-M4-06）

`createAdapter` 按 `target === "tvos"` 把 `platformStyle()` 绑到 10-foot 蒙皮
（`src/tokens.js` 的 `platformStyleTvos`）：基值 × TV_SCALE（字号/间距/圆角 ×2）、安全区
5%（百分比字符串，RN `DimensionValue` 合法形态）、focusRing 4px、card maxWidth 1280、button
常驻 4px 透明边框（聚焦仅换色，零布局抖动）。值与 `ui/adapters/tv/src/tokens.js`（单源）同口径，
漂移由 `test/tvos-host-structure.test.js` 跨包同调用哨兵断言。

## 桥产物挂载点（T-M4-07 口径）

Apple 侧无 android AAR / harmony `.so` 形态的 JS 注册位：核心 staticlib + uniffi Swift 绑定经
Xcode 链编入 App（`poly build --target tvos`：cargo 出 `aarch64-apple-tvos{,-sim}` 静态库 →
xcodebuild 归档 → ipa），不经 `adapter.native.register`。`ios/` 工程位铺 RN-tvOS 的
`*.xcodeproj`（内含 App + App-tvOS target）；未就位时打包链用骨架工程（T-M4-07 同 ios 链）。

## 构建环境前置

- **macOS + Xcode**（tvOS SDK）：打包 / 模拟器真跑均只在 macOS 宿主（
  `crates/poly-bundle` 的宿上门禁：非 macOS 报 `ToolMissing`）。Linux / Windows 经远程农场
  发起（T-M2-13：显式 `--target tvos` 不被发起宿主过滤，等 macOS worker）。
- **Rust 交叉**：`rustup target add aarch64-apple-tvos aarch64-apple-tvos-sim`（打包链前置检查
  会点名缺失项；该两 target 为 Rust tier-3，rustup 是否分发 rust-std 以实际安装为准——
  见 T-M4-07 报告「真实 vs mock 注记」）。
- **JS 侧**：`npm install`（工程根）→ `npx react-native start`（Metro；tvOS 模拟器由 Xcode /
  打包链启动）。

## 阻塞与证据边界

- **本仓宿主为 Linux、无 Xcode / tvOS 模拟器**：未做 `run-tvos`、未做模拟器渲染；渲染与键序列
  证据边界 = `test/tvos-host-structure.test.js`（结构 / 键表 / 10-foot 蒙皮 / 依赖口径哨兵）+
  CI `mobile-contract.yml` 的 macos-14 腿（真跑腿，发起前报 lead）。设备腿（tvOS 模拟器按键
  走查）待 macOS 上机窗口接（键序列与期望值 ≡ `device-walks/expectations.json`，内核键名全同）。
- `iconSource` 待 Pax 派生管线资产（T-M1-14）就位，未就位时省略图标节点。
- `safeArea`：声明（`capabilities().safeArea`）与蒙皮 safeArea 已就位，insets 的宿主消费
  （容器内边距）待上机窗口校准。
