# TV 遥控器冒烟套件（T-M4-13）

仅用方向键 + OK + Back + 数字键驱动 TV 示例应用完成核心任务（浏览 → 详情 → 播放占位 →
返回），覆盖 Android TV 模拟器与 web 系（Tizen / webOS）目标分支；输出结构化 `result.json`
与失败定位（任一目标焦点死路即失败并指到 目标 / 场景 / 键号 / 键名 / 期望 vs 实际）。

**复用而非重写**（T-M4-05 已交付的真跑基建）：

| 复用件 | 位置 | 本套件用法 |
|---|---|---|
| 走查执行器（adb 键注入 + POLYPACK_KEY 对拍 + uiautomator 有界重试 + 清装重来） | `ui/adapters/native/device-walks/run-walk.mjs` | Android 腿直接驱动（node 子进程），不另造驱动 |
| 内核派生期望（不读手写值） | `ui/adapters/native/device-walks/walks.mjs` | Android 腿（native-host 档）期望来源 |
| TV 焦点内核与全导航图 | `ui/adapters/tv/src/`（T-M4-02/03） | web 腿（tv-example 档）期望现场推导 |
| 宿主记录口径（跨端同形） | `ui/adapters/tv/README.md`「宿主记录口径」 | 两腿通道契约（单参数 `POLYPACK_KEY <json>`） |

## 两条应用档（app profile）

| 档 | 落地的 TV 应用 | 核心任务 | 期望推导源 |
|---|---|---|---|
| `native-host` | RN 宿主（Android TV APK，`com.polypack.host`，pointer=none） | 启动 → 方向键遍历 → 激活 → Back 返回（M2 两节点页） | `walks.mjs buildExpectations()` 现场推导 |
| `tv-example` | `@polypack/adapter-tv` 示例页（web 系；四节点网格 + 详情弹层 + 播放器子层） | 浏览 → 详情 → 播放占位 → 返回 + 数字键直达 | `createExamplePage().onKey` 重放现场推导 |

> Android TV 的落地宿主当前挂载 M2 两节点示例页（adapter-native；M2 走查期望被其冻结），
> 故 Android 腿跑 `native-host` 档；`tv-example` 档在 web 系腿真跑全导航图。
> 框架为档无关：宿主升级为全导航图后同一 `tv-example` 档键序列可直接复用。

## 观测通道

- **web 腿**：无头 Chrome（CDP）——`Input.dispatchKeyEvent` 注入；`Runtime.consoleAPICalled`
  收 `POLYPACK_KEY <json>`（单参数契约）；`Runtime.evaluate` 读 DOM 锚点 `[data-focus="true"]`
  与壳锚点（tv-frame / tv-card / tv-overlay / tv-modal）+ 提示行文案。**替代腿**（非模拟器本体）：
  同一 UI 产物 + 键盘协议一致，证据如实标注。
- **Android 腿**：`run-walk.mjs` 的 logcat `ReactNativeJS` + uiautomator（dump 前先删旧）。
- **Tizen / webOS 模拟器腿**：`tizen` / `ares` CLI 探测，缺席 SKIP-with-print + 注记（不红）。

## 结构化结果与失败定位

`result.json`（schema `polypack/tv-smoke/v1`）：per-target / per-scene / per-step 的
`expect` / `observed` / `channel` / 产物指针（logcat 帧、dump 文本、截图路径）。

失败诊断三分类：

| 类 | 判据 |
|---|---|
| `focus-dead-end` | 期望焦点移动而实际未动（焦点死路/焦点丢失） |
| `route` | 层关闭 / 焦点记忆 / 退出裁决路由不符 |
| `shortcut` | 数字键直达不符或层内拦截违例 |
| `channel` | 观测通道失效（POLYPACK_KEY 行数不符 / DOM 锚点缺失） |

## 用法

```sh
# mock 腿（每提交 / PR 可跑，零 npm install）：漂移哨兵 + 死路覆盖 + 履约哨兵（react 在场才跑）
node --test "ui/smoke/tv/test/*.test.js"    # 或 cd ui/smoke/tv && npm test

# web 系腿（替代腿；缺 Chrome / Node<22 自动 SKIP-with-print）
npm --prefix ui/smoke/tv install && npm --prefix ui/smoke/tv run build      # vite build → host/dist
node ui/smoke/tv/run.mjs --leg web [--targets tizen-tv,webos-tv] [--dist <dir>] [--logs <dir>]

# Android TV 腿（宿主 APK 在场 + adb 设备在场才真跑，任一缺席 SKIP-with-print）
node ui/smoke/tv/run.mjs --leg android [--serial emulator-5556] [--apk <宿主 apk>] [--logs <dir>]

# 全腿（android + web + tizen/webOS 模拟器探测）
node ui/smoke/tv/run.mjs --leg all --logs <dir>
```

缺省 `--dist host/dist`、`--logs $POLY_TV_SMOKE_LOGS`（否则 `target/tv-smoke-logs`，与
`packagers_android_tv.rs` 的设备腿归档同址）。退出码：任一目标 fail → 1；SKIP 不红。
期望漂移后用 `node ui/smoke/tv/emit-expectations.mjs --write` 刷新金样。
