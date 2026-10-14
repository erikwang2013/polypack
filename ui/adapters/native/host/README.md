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
  └─ adapter.native.register(...)   # T-M2-02 AAR 挂载点，见 android/app/libs/README.md
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
- **AAR 挂载点**：`android/app/libs/polypack-core.aar`（T-M2-02 产物），`app/build.gradle`
  已挂 `fileTree`；真加载未验证（同上阻塞）。
- **safeArea**：`capabilities()` 已声明（android = true），insets 注入消费留待 M4 / 穿戴里程碑。
- **iconSource**：示例页 JS 侧图标待 Pax 派生资产就位（`tools/build-pax.mjs`，T-M1-14）；
  未就位时省略图标节点（当前 `index.js` 传 `null`），不阻塞装配与焦点基线。
