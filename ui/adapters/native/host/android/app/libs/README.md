# 桥产物挂载点（T-M2-02）

`poly build --target android` / T-M2-02 产出的 AAR（JNI `.so` + Kotlin 绑定）落在本目录：

```text
android/app/libs/polypack-core.aar   # T-M2-02 产物
```

`app/build.gradle` 已挂 `fileTree(dir: "libs", include: ["*.aar"])`——AAR 就位即随包编译，
缺席时无副作用（空目录 fileTree 为空）。登记到装配层：

```js
// host/index.js
adapter.native.register({ artifact: "libs/polypack-core.aar", entryClass: "org.polypack.core.CoreBridge" });
```

描述符字段以 T-M2-02 报告为准（当前最小集：artifact / entryClass）。
AAR 真加载与模块解析未验证：仓库宿主无 Android SDK / 模拟器（阻塞，见 ../README.md）。
