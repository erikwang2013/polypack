# 设备走查对拍（T-M2-05 断言侧）

面向 m2-android 的模拟器闭环：把焦点内核的确定性翻成「adb 键序列 → 期望焦点序列」的机器可读数据。
**设备链跑命令、比对期望**；emulator-5554 与实机同用（纯 adb，无 Gradle 依赖）。

| 文件 | 用途 |
|---|---|
| `expectations.json` | 对拍数据（`schema = polypack/adapter-native/device-walks/v1`） |
| `walks.mjs` | 走查定义；焦点序列/键路由**由内核现场推导**（不手写） |
| `emit-expectations.mjs` | `node device-walks/emit-expectations.mjs [--write]` 打印 / 刷新数据 |
| `../test/device-walks.test.js` | 漂移哨兵（JSON ≡ 推导）+ 键表覆盖 + clicks 履约 |

`clicks` 是**期望值**（示例页规则：激活 counter 计数 +1、激活其余清零），不是从实现抄的；
履约断言对着示例页真跑（react 缺席时 skip）。

## adb 键序列 ↔ 内核键

| 内核键 | adb keyevent | Android 码 |
|---|---|---|
| `DPAD_UP` | `KEYCODE_DPAD_UP` | 19 |
| `DPAD_DOWN` | `KEYCODE_DPAD_DOWN` | 20 |
| `DPAD_LEFT` | `KEYCODE_DPAD_LEFT` | 21 |
| `DPAD_RIGHT` | `KEYCODE_DPAD_RIGHT` | 22 |
| `DPAD_CENTER` | `KEYCODE_DPAD_CENTER` | 23 |
| `Back` | `KEYCODE_BACK` | 4 |

## 执行形态

`expectations.json` 的 `walks[].adb` 即逐条命令（同一 walk 按序执行）；**每条走查前回到初始状态**
（`precondition.reset`，app 重启——期望值以初始焦点 `counter`、计数 0 为前置，故走查可任意取子集/顺序）：

```sh
adb -s emulator-5554 shell am force-stop com.polypack.host
adb -s emulator-5554 shell am start -n com.polypack.host/.MainActivity
adb -s emulator-5554 shell input keyevent KEYCODE_DPAD_DOWN
```

一次 `input keyevent` = `ACTION_DOWN(0)` + `ACTION_UP(1)` **两帧**，宿主只消费 DOWN（已接线落实），
否则激活双计、Back 双弹，期望必然对不上。焦点环像素断言另需把 `app.json` 的
`polypack.pointer` 改为 `"none"`（见下「宿主侧接线」第 2 条；键走查不需要改）。

## 观测通道（比对 `expect.focus` / `expect.clicks`）

真机上没有 DOM，焦点 id 不落在文本里。三条通道：

| 通道 | 可观测 | 取法 | 现状 |
|---|---|---|---|
| logcat `POLYPACK_KEY` 行 | 每键 `handleKey` 结果 → 焦点序列 | `adb -s emulator-5554 logcat -d -s ReactNativeJS:V \| grep -o 'POLYPACK_KEY .*' \| sed 's/POLYPACK_KEY //'` | 宿主已接线（见下） |
| uiautomator 文本 | `点击计数：N` 等语义标记 | `adb … shell uiautomator dump /sdcard/d.json && adb … shell cat /sdcard/d.json` | 现成可用（RN `Text` → 节点文本） |
| 截图像素采样 | 焦点环颜色/描边 | dump 取 bounds + 截图裁边框采样 | 需 bounds 锚点（建议 `testID`） |

`ReactNativeJS` 是 RN 把 JS `console.log` 送上 logcat 的 tag（RN 0.87.1 源码
`ReactAndroid/src/main/jni/react/jni/JSLogging.cpp`）；`POLYPACK_KEY` 前缀是宿主约定，行内容 =
`handleKey` 返回描述符的 JSON（`move`/`activate`/`back`/`ignored` 四型，字段见数据）。

注意 `unknown-key-ignored` 走查（`KEYCODE_Q`）：RN 的事件源只对映射表内键发 `onHWKeyEvent`
（`ReactAndroidHWInputDeviceHelper.kt` 的 `KEY_EVENTS_ACTIONS`，源码已核），未映射键**到不了宿主** →
该走查真机期望是 logcat **零新增 `POLYPACK_KEY` 行** + 焦点仍 `counter`；`{type:"ignored"}` 一行
是内核直喂键时会有的结果，由 JS 单测覆盖，真机不经此路。

## 渲染冒烟：代理断言 → 真机可上移子集

对照 `../test/render.test.js` 的逐条断言：

| 代理断言 | 真机 | 通道 / 说明 |
|---|---|---|
| 语义标记（`PolyPack App` / 版本行 / `重置` / 方向键提示） | ✅ 可上移 | uiautomator 文本 |
| 初始 `点击计数：0` | ✅ 可上移 | 文本 |
| 激活路径：中心键 → `点击计数：1`；移焦重置激活 → `0` | ✅（待键接线） | 文本 + 键序列 |
| Back 后激活对象回到上一焦点 | ✅（待键接线） | `POLYPACK_KEY` 序列（`back.to`） |
| 焦点环 `#0e7490` / `border-width: 2` / 恰好一处 | ⚠️ 需像素采样 | 代理渲染只证 style **计算**；真机还须证 RN 样式**应用**，故降一档 |
| 两形态（pointer none/touch）strip 相等 | ✗ 代理专属 | DOM 字符串比对无真机对应；真机等价物 = 两形态各跑一遍截图比对 |
| 图标节点（`accessibilityLabel="Pax"`） | 暂不适用 | 宿主当前 `iconSource: null`（待 T-M1-14 资产） |
| 模板页 marker 对照 | ✗ 源码级 | CI 已覆盖，无需真机 |

### 焦点环像素采样（配方；仅 `polypack.pointer: "none"` 形态有环）

差分采样（环色与按钮底色相隔极远，见色即判，无容差歧义）：

1. `adb exec-out screencap -p > shot.png`（`exec-out` 避免 CRLF 损坏 PNG）+ `uiautomator dump` 取
   `resource-id="counter"` / `"reset"` 的 `bounds`（testID → resource-id 由 RN 无障碍层提供）；
2. 采两按钮**上边框内侧**像素：`x = bounds 中心`、`y = bounds.top + 1px`——2dp 描边 = 2px@mdpi(160dpi)
   / 5px@420dpi，`top+1` 在两种密度下都落环带内（设备链 mdpi AVD 实测：`top+2` 恰好错到按钮底色）；
3. 期望（色值出处 `../src/tokens.js`）：

| 状态 | counter 上边 | reset 上边 |
|---|---|---|
| 初始（焦点 counter） | `#0e7490`（accent = 环） | `#ea580c`（brand = 按钮底色，无环） |
| `DPAD_DOWN` 后（焦点 reset） | `#ea580c` | `#0e7490` |

两钮同色（同有环或同无环）即失败——环「恰好一处且随焦点移动」由这组对照证。

## 宿主侧接线（已完成；`host/index.js`）

三条已按 lead 裁定落进宿主（纯 JS，无 Kotlin、无新依赖）：

1. **键事件源**：`ReactRootView.dispatchKeyEvent` **无条件**派发 `onHWKeyEvent`
   （`ReactAndroidHWInputDeviceHelper.kt`：`DPAD_UP/DOWN/LEFT/RIGHT → up/down/left/right`，
   `DPAD_CENTER/ENTER/SPACE → select`；载荷 `{eventType, eventKeyAction, tag?}`；
   JS 侧 `DeviceEventEmitter` 由 `index.js:254` 导出），宿主订阅后每键经 `onKey` 入内核并打
   `POLYPACK_KEY {…}`。`Back` 不在该表内，走核心 `BackHandler`（`index.js:236`）——两者互补即覆盖
   本目录全部键。**DOWN-only**：只消费 `eventKeyAction === 0`（见「执行形态」的坑）。
   备选通路（视图 `onKeyDown` prop）由 feature flag `enableKeyEvents` 门控、0.87.1 默认 **false**
   （`ReactNativeFeatureFlagsDefaults.kt:92`），M4 再评估。
2. **`pointer` 开关** = `app.json` 的 `polypack.pointer`（默认 `"touch"`；走查会话改 `"none"`）。
   **不用 env**：RN 运行时 `process.env` 只保证 `NODE_ENV`（`setUpGlobals.js`），
   `@react-native/babel-preset` 无 inline-env 插件 → 自定义 env 静默失效。改后重启 app
   （Metro 按模块变更重打包）。**键走查在两种形态下都成立**（`onKey` 与 capabilities 无关）；
   只有焦点环像素断言需要 `"none"`。
3. **`testID`**：示例页键节点 `testID="counter"` / `"reset"`（= 焦点节点 id）→ 真机
   uiautomator `resource-id`、代理渲染 `data-testid`（同名同源，`test/render.test.js` 断言其存在）。

## 鸿蒙 TV（T-M4-14，hdc 等价）

同一份对拍数据在 HarmonyOS 宿主的等价执行：键名与期望值全同，`hdc` 替代 `adb`，观测走 hilog。
TV HAP 前置 `app.json` 的 `polypack.tv=true`（宿主 TV 形态：pointer none + gestures false；
`module.json5` 的 `deviceTypes` 已含 `tv`），打包/签名复用 M3 链（`polypack build --target harmonyos`）。

```sh
hdc shell aa force-stop com.polypack.host
hdc shell aa start -b com.polypack.host -a EntryAbility
hdc shell uitest uiInput keyEvent 2013          # DPAD_DOWN，见下表
hdc shell hilog -x | grep -o 'POLYPACK_KEY .*'
```

| 内核键 | hdc keyEvent | OHOS KeyCode |
|---|---|---|
| `DPAD_UP` | `2012` | KEYCODE_DPAD_UP |
| `DPAD_DOWN` | `2013` | KEYCODE_DPAD_DOWN |
| `DPAD_LEFT` | `2014` | KEYCODE_DPAD_LEFT |
| `DPAD_RIGHT` | `2015` | KEYCODE_DPAD_RIGHT |
| `DPAD_CENTER` | `2016` / `2054` / `2050` | CENTER / ENTER / SPACE |
| `Back` | `uitest uiInput keyEvent Back` | KEYCODE_BACK=2（不经键表；EntryAbility.onBackPressed → `hardwareBackPress`，避免双计） |

- 一次 `keyEvent` = Down+Up 两帧，宿主只消费 Down（同 android 口径）；焦点环差分采样同 android
  配方（真机取帧 + `uitest dumpLayout` 取 bounds，环宽同为 2dp；具体取帧命令随设备腿补全）。
- 观测通道：hilog tag `ReactNativeJS`，每键一行 `POLYPACK_KEY <json>`（单参数 = `onKey` 描述符
  JSON；与 android logcat / adapter-tv「宿主记录口径」同形，见 `ui/adapters/tv/README.md`）。
- 比对：逐键 `expect.focus` / `expect.clicks` ≡ `expectations.json`；TV 形态页面另包
  `tv-frame` 百分比安全区容器（adapter-tv `platformStyle().safeArea` 单一源；1080p = 四边
  54/96px），窗口尺寸可用 `hdc shell hidumper -s WindowManagerService` 核对。
- **本机阻塞**：无 HarmonyOS 设备/模拟器（`hdc list targets` = `[Empty]`），本节命令未真跑；
  本地替代证据见 `../test/harmony-tv-smoke.test.js`（结构快照 + 键序内核推导对拍 + 安全区换算，
  设备腿 SKIP-with-print 打印本清单）。

## 边界与已知限制（如实）

- **已在模拟器真跑**（2026-10-16，AVD polypack-test / emulator-5554，320x640@mdpi）：六走查
  touch 形态 6/6、none 形态 6/6；环像素差分采样双态 4 点实证（`top+1` 配方）。构建环境适配
  （JDK 17 / npx shim / 镜像与资源注入）见 `../host/README.md`；宿主侧修因（首键吞噬、CENTER 双计）
  见 `../src/example.js` 焦点锚点注释。
- **栈空 Back**：内核返回 `to: null`，退出语义交宿主（RN 默认可能退到桌面）；走查不含该步。
- **图为示例页最小图**（`counter ⇄ reset` 两节点）；M4 TV 完整导航图另出，键表与通道口径可复用。
- 文本通道证明页面与交互、像素采样证明样式应用（环恰好一处且随焦点移动）——两层证据齐；
  触屏（pointer）手势与安全区断言仍归 M4 导航系统。
