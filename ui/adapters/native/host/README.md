# Android RN 宿主骨架（T-M2-04）

`@polypack/adapter-native` 的 Android 承载宿主：装配层注册组件、接方向键/Back 事件源、
预留桥产物挂载点。文件取自 RN 官方模板 `@react-native-community/template@0.87.2` 并改名
（`PaxApp` / `com.polypack.host`），差异仅为装配层接线与 AAR 挂载点。

## 装配链

```text
MainActivity(PaxApp) → AppRegistry → host/index.js
  ├─ createAdapter({ target: "android", runtime: RN, host: { pointer } })
  ├─ createExampleApp(RN, adapter) → App / onKey（方向键遍历 + Back 焦点记忆）
  ├─ adapter.mount({ appKey: "PaxApp" }, App)
  └─ adapter.native.register(...)   # T-M2-02 AAR 挂载点（口径见下「桥产物挂载点」）
```

## 未纳入的模板文件（二进制 / 派生资产）

| 缺项 | 来源 |
|---|---|
| `android/gradlew` + `gradle/wrapper/*.jar` | RN CLI 生成（M2 打包链免 Gradle：T-M2-06 AAPT2 直出） |
| `android/app/debug.keystore` | RN CLI / keytool 生成——二进制凭据不入库（secret-scan 约定） |
| `res/mipmap-*/ic_launcher*.png` | Pax 图标派生管线（`tools/build-pax.mjs`，T-M1-14） |
| `node_modules/` | `npm install`（宿主依赖：react-native 0.87.1 + adapter `file:..`） |

## 阻塞与挂载点

- **真机渲染阻塞（宿主/设备）**：本仓库宿主为 Linux，无 Android SDK / 模拟器 / Gradle，
  未做 `run-android`、未做 Gradle 构建。渲染证据由包级测试代理
  （`../test/render.test.js` 静态渲染 + 宿主结构断言），**不伪造成真机渲染证据**。
- **方向键冒烟阻塞（设备）**：`onKey` 需原生按键事件源（外接键盘 / 调试模式），留待
  M4 冒烟（T-M4-05 同形态：无指针完成核心任务）。
- **桥产物挂载点（T-M2-02 口径）**：`poly bridge aar [--abi ...] [--out P]` 产出
  `<工程根>/bridge/android/<核心库名>.aar`（内件 `jni/<abi>/lib<核心库名>.so` + 可选 `classes.jar`；
  非 maven 坐标、不放 `libs/`）。**前置**：本骨架是模板，按工程根布局铺开——`android/` 与
  `polypack.toml`、`bridge/` 同级（CLI 只认 `project.root/bridge/android`）；因此仓库内此目录
  解析不到 `bridge/` 属正常，宿主若挂在工程根子目录下需改 gradle 路径。`app/build.gradle` 已挂
  `fileTree(dir: "${rootProject.projectDir}/../bridge/android", include: ["*.aar"])`，
  缺席时 fileTree 为空、无副作用；`bridge/` 已入 `.gitignore`。
  装配层登记（`index.js` 注释示例，取自真跑夹具 `pax.udl` → `uniffi.pax.PaxKt` + `pax_core`）：
  `artifact` = AAR 路径、`entryClass` = Kotlin 门面类 `uniffi.<ns>.<Ns>Kt`、
  `libraryName` = `.so` 基名（`System.loadLibrary` 入参，可省略）。真加载未验证（同上阻塞）。
- **safeArea**：`capabilities()` 已声明（android = true），insets 注入消费留待 M4 / 穿戴里程碑。
- **iconSource**：示例页 JS 侧图标待 Pax 派生资产就位（`tools/build-pax.mjs`，T-M1-14）；
  未就位时省略图标节点（当前 `index.js` 传 `null`），不阻塞装配与焦点基线。
