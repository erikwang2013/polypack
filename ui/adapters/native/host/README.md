# Android RN 宿主骨架（T-M2-04）

`@polypack/adapter-native` 的 Android 承载宿主：装配层注册组件、接方向键/Back 事件源、
预留桥产物挂载点。文件取自 RN 官方模板 `@react-native-community/template@0.87.2` 并改名
（`PaxApp` / `com.polypack.host`），差异仅为装配层接线与 AAR 挂载点。

## 装配链

```text
MainActivity(PaxApp) → AppRegistry → host/index.js
  ├─ createAdapter({ target: "android", runtime: RN, host: { pointer } })  # pointer ← app.json polypack.pointer
  ├─ createExampleApp(RN, adapter) → App / onKey（方向键遍历 + Back 焦点记忆）
  ├─ adapter.mount({ appKey: "PaxApp" }, App)
  ├─ onHWKeyEvent / BackHandler → onKey → logcat `POLYPACK_KEY {…}`（T-M2-05 走查通道）
  └─ adapter.native.register(...)   # T-M2-02 AAR 挂载点（口径见下「桥产物挂载点」）
```

## 宿主集成依赖（T-M2-02 上机实证）

桥产物接进真 app 时，除 AAR 本身外还需（口径来自 m2-core 的 T-M2-02 上机三层实证）：

- `net.java.dev.jna:jna:5.14.0`（uniffi 的 Kotlin 绑定经 JNA 走 `dlsym`；含各 ABI
  `libjnidispatch.so`）+ `kotlin-stdlib`
- `.so` 进 APK 的 `lib/<abi>/` 后由 `System.loadLibrary("<核心库名>")` 装载：AAR 内件
  `jni/<abi>/lib<核心库名>.so` 由 AGP 合入，或按 jniLibs 直投（本骨架 `.gitignore` 已忽略该目录）
- 库名两端对齐：绑定侧别名用 `-Duniffi.component.<UDL namespace>.libraryOverride` 覆盖，
  与装配层 `native-registry` 的 `libraryName` 字段互为两端

本骨架尚未接线这两条依赖（`index.js` 的 `register` 仍是注释示例）：真 app 装配时随 AAR
一并补 `build.gradle` 依赖。实证边界（如实，非本仓库复现）：emulator x86_64 API 36 上
loadLibrary 真载 + `dlsym` 真调用（`ADD(2,3)=5`）+ Kotlin→JNA→Rust（`GREET=hello, polypack`）；
arm64 仅到 ELF 符号层，未上机。

## 未纳入的模板文件（二进制 / 派生资产）

| 缺项 | 来源 |
|---|---|
| `android/gradlew` + `gradle/wrapper/*.jar` | RN CLI 生成（M2 打包链免 Gradle：T-M2-06 AAPT2 直出） |
| `android/app/debug.keystore` | keytool 生成（命令见上「构建环境前置」）——二进制凭据不入库（secret-scan 约定） |
| `res/mipmap-*/ic_launcher*.png` | Pax 图标派生管线（`tools/build-pax.mjs`，T-M1-14） |
| `node_modules/` | `npm install`（宿主依赖：react-native 0.87.1 + adapter `file:..`） |

## 构建环境前置（m2-android 设备链实测）

- **`npx` 须可用**：RN 0.87 `settings.gradle` 的 autolink 会调用它；无独立 `npx` 可执行时用 shim 顶。
- **JDK 17**：Gradle 工具链检查在 JDK 18（其构建机默认）不过，指定
  `JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64` 才过。
- **`debug.keystore` 须先生成**：骨架不含（二进制凭据不入库——本仓密钥零入库是 secret-scan 红线，
  不给 debug 开例外；`.gitignore` 的 `*.keystore` 兜底），缺它 `:app:validateSigningDebug` 直接失败
  （设备链实测：构建末段才炸）。日后若要模板级 UX，正确形态是 gradle 任务缺时自动 keytool 生成
  （构建脚本生成而非入库文件）。同名同参生成到 `android/app/`：

  ```sh
  keytool -genkeypair -v -storetype PKCS12 -keystore android/app/debug.keystore \
    -alias androiddebugkey -storepass android -keypass android \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US"
  ```

- **Metro 链接包的 watch 开销**：`@polypack/adapter-native` 经 `file:..` 以符号链接入
  `node_modules`，其真实路径在项目根之外——`metro.config.js` 的 `watchFolders` 是**解析必需**；
  符号链接形态下 Metro 爬 watch 树的耗时受 I/O 压力放大（设备链实测：数十分钟 D 状态 vs
  真实目录拷贝 40 秒起服务）。I/O 紧张时用真实目录拷贝替换链接（拷贝包内容到
  `node_modules/@polypack/adapter-native`），`watchFolders` 配置不变。

## 阻塞与挂载点

- **真机渲染阻塞（宿主/设备）**：本仓库宿主为 Linux，无 Android SDK / 模拟器 / Gradle，
  未做 `run-android`、未做 Gradle 构建。渲染证据由包级测试代理
  （`../test/render.test.js` 静态渲染 + 宿主结构断言），**不伪造成真机渲染证据**。
- **方向键冒烟（宿主已接线；真机仍阻塞）**：`onKey` 事件源 = RN 核心的 `onHWKeyEvent`
  （`ReactRootView.dispatchKeyEvent` 无条件派发，d-pad 四向 → up/down/left/right，中心 → select）
  + Back 走核心 `BackHandler`；每键结果打 logcat `POLYPACK_KEY {…}`（`ReactNativeJS` tag）。
  **只消费 `ACTION_DOWN`**：一次 `adb shell input keyevent` 派发 DOWN(0)+UP(1) 两帧，全收则中心键
  激活双计、Back 双弹（走查期望必对不上）。**真机冒烟仍阻塞（设备）**：需模拟器/实机；
  对拍数据、adb 命令与观测通道见 `../device-walks/README.md`。
  冒烟形态同 T-M4-05：无指针完成核心任务（方向键遍历 → 中心键激活 → Back 回上一焦点）。
- **pointer 开关（走查会话用）**：`app.json` 的 `polypack.pointer`，默认 `"touch"`；设备走查改
  `"none"` 即无指针形态（焦点环 + `focusNav`）。用 app.json 而非 `process.env`：RN 运行时
  `process.env` 只保证 `NODE_ENV`（`Libraries/Core/setUpGlobals.js`），自定义 env 不在打包内联
  且无插件支持（`@react-native/babel-preset` 无 inline-env），写 env 会**静默失效**。
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
