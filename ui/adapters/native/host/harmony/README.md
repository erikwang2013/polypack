# HarmonyOS/RNOH RN 宿主骨架（T-M3-01）

`@polypack/adapter-native` 的 HarmonyOS 承载宿主：装配层注册组件、接硬件键/Back 事件源、
挂载 NAPI（C ABI）桥产物。DevEco 工程文件取自 RNOH 官方脚手架
（`@react-native-oh/react-native-harmony-cli@0.84.3` 内嵌模板，MIT）并改名
（`PaxApp` / `com.polypack.host`），差异仅为装配层接线、Back/键事件转发与桥产物挂载点。

## 铺开布局（重要，与 android 宿主同规矩：模板按工程根铺开）

RNOH 工程结构约定：**DevEco 工程在 JS 工程根下一层**（CMake 的 `NODE_MODULES` =
`<JS 根>/node_modules`、har 的 `file:../node_modules` 依赖路径都以此为准）。因此本目录
（仓库里的模板根 = RNOH 工程根）整体铺开到 polypack **工程根**：

```text
<工程根>/                        ← 本目录内容（index.js / app.json / package.json / metro.config.js …）
├── node_modules/               ← JS 侧 npm 依赖（react-native 0.84.1 + RNOH + RNOH CLI …）
├── index.js                    ← RN JS 入口（装配 adapter + 键事件源）
├── app.json                    ← AppRegistry 键 PaxApp + polypack.pointer
├── harmony/                    ← DevEco 工程（本目录的 harmony/ 子目录）
│   ├── AppScope/ entry/ build-profile.json5 hvigorfile.ts hvigor/ oh-package.json5
│   └── entry/libs/<abi>/       ← NAPI 模块注入点（libpax.so）
├── bridge/harmonyos/<abi>/     ← `poly bridge ohos` 的产物（repository 内本目录解析不到属正常）
└── polypack.toml
```

仓库内（`ui/adapters/native/host/harmony/`）因此是 `index.js … + harmony/` 的形状；
`oh-package.json5` 的 `file:../node_modules` 指向 JS 根的 node_modules。**packager 消费的是
`<工程根>/harmony/`**（T-M3-03/04）。

## 装配链

```text
EntryAbility(RNAbility, PaxApp) → pages/Index(RNApp appKey=PaxApp) → AppRegistry → host/harmony/index.js
  ├─ createAdapter({ target: "harmonyos", runtime: RN, host: { pointer } })  # pointer ← app.json polypack.pointer
  ├─ createExampleApp(RN, adapter) → App / onKey（方向键遍历 + Back 焦点记忆）
  ├─ adapter.mount({ appKey: "PaxApp" }, App)
  ├─ polyKeyEvent / hardwareBackPress → onKey → hilog(ReactNativeJS) `POLYPACK_KEY {…}`（走查通道）
  └─ adapter.native.register(...)   # T-M3-01 NAPI 模块挂载点（口径见下「桥产物挂载点」）
```

## 键事件约定（T-M3-01 定案，rnoh-adapter 的 JS 侧按此接）

- **方向/确认键**：ArkTS 根容器 `.onKeyEvent` → `rnInstance.emitDeviceEvent('polyKeyEvent',
  { keyCode, keyAction })`（`harmony/entry/src/main/ets/pages/Index.ets`；RNInstance 经
  `RNApp.onSetUp` 钩子捕获）。`keyAction` = ArkUI `KeyType`（**Down=0 / Up=1**），
  `keyCode` = OHOS `KeyCode` 原始值。JS 侧映射（`index.js` 的 `KEYCODE_TO_KERNEL`）：
  2012/2013/2014/2015 = DPAD 四向、2016/2054/2050 = DPAD_CENTER，**只消费 Down**。
- **Back 键**（KEYCODE_BACK=2，框架级事件不走 onKeyEvent 分发）：`EntryAbility.onBackPressed`
  （本机 SDK 5.1.1(19) 的 UIAbility 回调名；新 API 线的 `onBackPress` 在此 SDK 不在基类链上，
  编译期实证）→ `AppStorage('RNOHCoreContext').dispatchBackPress()` → `rnInstance.onBackPress`
  → JS 设备事件 `'hardwareBackPress'`（RN 核心 BackHandler 通道，与 android 宿主同语义）；
  JS 未消费时 RNOH 的 `invokeDefaultBackPressHandler` → `RNAbility.defaultBackPressHandler`
  （默认收起/退出，宿主策略留 M4 定）。**Back 不重复走 polyKeyEvent 表**（避免双计）。
- 走查通道契约（与 `device-walks/`）不变：每键一行 `POLYPACK_KEY <json>` 到 hilog
  （tag `ReactNativeJS`，`hdc shell hilog` 观测）。

## 依赖链（两层包管理，RNOH 特有）

- **JS 侧（npm，JS 根）**：`react-native@0.84.1` + `react@19.2.3`（社区配套；RNOH 0.84
  Release 同步升级到 19.2.3，react-native 0.84.1 的 peer 便是 `^19.2.3`）+
  `@react-native-oh/react-native-harmony@0.84.3`（含 `react_native_openharmony.har` +
  `harmony/` ets 源）+ devDep `@react-native-oh/react-native-harmony-cli@0.84.3`
  （**hvigor 插件随它分发**：`harmony/rnoh-hvigor-plugin-0.84.3.tgz`，`hvigor-config.json5`
  按其相对路径引用，缺它 hvigor 直接起不来）+ devDep `@react-native-community/cli@20.1.0`
  （RN 0.84 起社区 CLI 不再随 react-native 捆绑，`react-native` 可执行文件只做转发；
  hvigor 的 rnohModulePlugin 要跑 `react-native codegen-harmony`，缺它直接
  `react-native depends on @react-native-community/cli` 报错——RN 官方模板同为 20.1.0）。
- **ohpm 侧（DevEco 工程）**：`harmony/oh-package.json5` 以
  `file:../node_modules/@react-native-oh/react-native-harmony/react_native_openharmony.har`
  双声明 dependencies + overrides（官方模板同款）；`ohpm install` 在 `harmony/` 内执行。
- **版本口径**：RNOH 0.84.3 ↔ 社区 RN 0.84.x（无 0.87 线；与 android 宿主的 RN 0.87.1
  存在代数差，属上游节奏，能力矩阵见 `ui/adapters/native`）。

## RNOH native 构建（无预编译 .so，har 源码在 app 构建期真编译）

- `@rnoh/react-native-openharmony` 的 har **不含预编译库**：native 由
  `entry/src/main/cpp/CMakeLists.txt` 在构建期 `add_subdirectory` har 的 `src/main/cpp`
  真编译（RNOH C++ + hermes/boost/folly 三方件），产出 `librnoh_core.so` 等。
- `entry/src/main/cpp/PackageProvider.cpp`、`entry/src/main/ets/{PackageProvider,RNOHWorker}.ets`
  、`workers/RNOHWorker.ets` 为 RNOH 官方模板件（autolinking / codegen 生成物在构建期落盘，
  已在 `.gitignore`）。**改 PackageProvider 是接自定义 native 组件的入口**（T-M3 后续）。

## 桥产物挂载点（T-M3-01 口径）

- `poly bridge generate --lang napi` 产出 `<工程根>/bridge/napi/<UDL namespace>.c` 胶水
  （装载入口 `napi_register_module_v1` + OHOS 构造注册 `napi_module_register`）；
  `poly bridge ohos [--abi arm64-v8a|x86_64|armeabi-v7a]` 交叉编译出
  `<工程根>/bridge/harmonyos/<abi>/lib<namespace>.so`（**单文件自包含**：核心 staticlib
  已静态入，NEEDED 里不出现第二个核心 .so）。缺 OHOS SDK 时 `poly::E_BRIDGE_0003` +
  环境变量修复提示（`$HARMONYOS_TOOLS` / `$DEVECO_SDK_HOME` / `$POLYPACK_OHOS_SDK`）。
- **注入**：`bridge/harmonyos/<abi>/lib<ns>.so` → `harmony/entry/libs/<abi>/`（目录已备
  `.gitkeep`；`.so` 入库禁止，见 `.gitignore`），随 HAP 进 `libs/<abi>/`；ArkTS 侧
  `import { greet } from 'lib<ns>.so'`（**模块名 = UDL namespace**，示例 `pax`）。
- 装载语义实证（T-M3-01 真跑，`crates/poly-core/tests/napi_real.rs`，
  `POLY_CONTRACT_REAL=1`）：同域 NAPI ABI 下 node 真装载真调用 → `greet('world')="hello, world"`、
  `add(2,3)=5`，与 Rust 单元测试同字面量对拍；OHOS aarch64 侧到 ELF 符号契约
  （`napi_register_module_v1` 导出 / `napi_*` 未定义由宿主运行时提供 / 自包含）。

## 构建环境前置

- **HarmonyOS command-line-tools**（`ohpm` + `hvigorw` + SDK 5.1.1(19)）：
  `$HARMONYOS_TOOLS`（本机 `/home/component/command-line-tools`）。
  `poly doctor` 探测该项，缺时给修法。
- **Node.js ≥ 22.11**：RN 0.84 的 CLI 依赖 `node:util.styleText`（Node 20.12+/22 才有）；
  command-line-tools 自带 Node 18 会在 rnohModulePlugin 处报
  `TypeError: styleText is not a function`（T-M3-01 实测）。跑 hvigorw 时用
  `DEVECO_NODE_HOME=<含 bin/node 的目录>` 显式指到系统 Node 22。
- **sdkmgr/SDK**：`compatibleSdkVersion` 依本机 SDK 钉 `5.1.1(19)`；官方模板的
  `"nativeCompiler": "BiSheng"` 已省略——BiSheng 只在 DevEco IDE 内分发，
  command-line-tools 的 SDK 不含（用内置 clang）。
- **JS 侧**：`npm install`（JS 根）→ `ohpm install`（`harmony/` 内）→
  `hvigorw assembleHap`（调 `$HARMONYOS_TOOLS/bin/hvigorw`，见上 Node 口径）。
- **签名**：`build-profile.json5` 的 `signingConfigs: []`（与官方模板一致，debug 不签；
  真机安装需 DevEco 自动签名或手工签名配置——上机走查时补，T-M3-09 口径）。

## 阻塞与挂载点

- **模板构建实测（T-M3-01 真跑）**：`npm install` → `ohpm install` → `hvigorw assembleHap
  --no-daemon` 真跑到 ArkTS 编译（codegen 产物 + CMake configure 均已产出），**止步于
  CompileArkTS 的 2 处错误（都在 RNOH 0.84.3 har 自身的 TS）**：`WebSocketTurboModule.ts`
  使用 `@ohos.net.webSocket` 的 `pingInterval/pongTimeout`，d.ts 标注 `@since 21`，
  本机 SDK 5.1.1(19) 无此字段 → 需 SDK API ≥ 21（HarmonyOS 6.0.1+，DevEco 6.x）；
  低 SDK 机器可考虑 RNOH 降 0.84.2（心跳支持引入于 0.84.3，未验证）。
  宿主自身四处问题已在真跑中修掉：Node 18 缺 `styleText`（→ Node 22 + DEVECO_NODE_HOME）、
  缺 `@react-native-community/cli`（→ 20.1.0）、react 19.1.0 不满足 RN 0.84.1 peer（→ 19.2.3）、
  `onBackPress` 不在 API 19 基类链（→ `onBackPressed`）。
- **真机渲染阻塞（宿主/设备）**：本仓库宿主为 Linux、无 HarmonyOS 设备/模拟器（hdc 无目标），
  未做 `run-harmony`、未做真机安装；渲染与键序列证据由设备走查里程碑接。
- **NAPI 装载未上真机**：模块 `.so` 的架构/符号/自包含与 node 装载调用对拍已实证
  （见上）；「RNOH 宿主内 `import 'libpax.so'`」待设备腿（T-M3-09 / T-M3-04 打包闭环）补。
- **JS bundle**：release 形态读 `entry/src/main/resources/rawfile/hermes_bundle.hbc`
  （debug 走 Metro）。bundle 注入属打包链（T-M3-03/04）。
- **iconSource**：示例页 JS 侧图标待 Pax 派生资产就位（T-M1-14）；未就位时省略图标节点。
- **safeArea**：`capabilities()` 声明与 insets 注入消费留待 M4 / 穿戴里程碑。
