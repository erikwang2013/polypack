// T-M2-04 宿主工程结构断言：真机渲染阻塞（无 Android SDK / 模拟器）下，
// 以骨架完整性与装配链一致性替代——文件齐备、AppRegistry 键三处一致、
// 桥产物挂载点就位、二进制与凭据资产不入库。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOST = path.resolve(HERE, "../host");
const read = (rel) => fs.readFileSync(path.join(HOST, rel), "utf8");
const exists = (rel) => fs.existsSync(path.join(HOST, rel));

const KOTLIN_DIR = "android/app/src/main/java/com/polypack/host";

test("宿主骨架文件齐备（RN 模板 0.87.2 形态）", () => {
  const files = [
    "package.json",
    "app.json",
    "index.js",
    "babel.config.js",
    "metro.config.js",
    "README.md",
    "android/settings.gradle",
    "android/build.gradle",
    "android/gradle.properties",
    "android/app/build.gradle",
    "android/app/proguard-rules.pro",
    "android/app/src/main/AndroidManifest.xml",
    "android/app/src/main/res/values/strings.xml",
    "android/app/src/main/res/values/styles.xml",
    "android/app/src/main/res/drawable/rn_edit_text_material.xml",
    `${KOTLIN_DIR}/MainActivity.kt`,
    `${KOTLIN_DIR}/MainApplication.kt`,
  ];
  for (const file of files) {
    assert.ok(exists(file), `缺宿主文件: ${file}`);
  }
});

test("二进制与凭据资产不入库（keystore / gradle wrapper / mipmap）", () => {
  for (const file of [
    "android/app/debug.keystore",
    "android/gradlew",
    "android/gradle/wrapper/gradle-wrapper.jar",
  ]) {
    assert.ok(!exists(file), `${file} 不应入库（见 host/README.md「未纳入的模板文件」）`);
  }
  assert.ok(!exists("android/app/src/main/res/mipmap-hdpi"), "mipmap 由 Pax 图标管线派生，不入库");
});

test("装配链闭合：AppRegistry 键三处一致（app.json / MainActivity / index.js）", () => {
  assert.equal(JSON.parse(read("app.json")).name, "PaxApp");
  assert.match(
    read(`${KOTLIN_DIR}/MainActivity.kt`),
    /getMainComponentName\(\): String = "PaxApp"/,
    "MainActivity 组件名须与 app.json name 一致",
  );
  const index = read("index.js");
  assert.match(index, /createAdapter\(\{ target: "android", runtime: RN/);
  assert.match(index, /adapter\.mount\(\{ appKey: "PaxApp" \}, App\)/);
  assert.match(index, /from "@polypack\/adapter-native"/);
  assert.match(index, /from "@polypack\/adapter-native\/example"/);
});

test("清单与主题：MainApplication / MainActivity 注册完整", () => {
  const manifest = read("android/app/src/main/AndroidManifest.xml");
  assert.match(manifest, /android:name="\.MainApplication"/);
  assert.match(manifest, /android:name="\.MainActivity"/);
  assert.match(manifest, /android:theme="@style\/AppTheme"/);
  assert.match(manifest, /android.intent.category.LAUNCHER/);
});

test("桥产物挂载点就位：gradle fileTree 指向 T-M2-02 口径路径 + 装配层注册位 + 产物不入库", () => {
  assert.match(
    read("android/app/build.gradle"),
    /fileTree\(dir: "\$\{rootProject\.projectDir\}\/\.\.\/bridge\/android", include: \["\*\.aar"\]\)/,
    "app/build.gradle 须挂 <工程根>/bridge/android（`polypack bridge aar` 的输出目录，非 libs/）",
  );
  assert.ok(!exists("android/app/libs"), "libs/ 挂载点已按 T-M2-02 口径移除");
  assert.match(read("index.js"), /adapter\.native\.register/, "装配层须保留桥产物注册位");
  assert.match(read(".gitignore"), /^bridge\/$/m, "桥产物目录不入库");
});

test("键事件源接线（T-M2-05 走查前提）：onHWKeyEvent + BackHandler + DOWN-only + pointer 开关", () => {
  const index = read("index.js");
  assert.match(index, /DeviceEventEmitter\.addListener\("onHWKeyEvent"/, "须订阅 RN 核心 d-pad 设备事件");
  assert.match(index, /event\.eventKeyAction !== 0/, "须只消费 ACTION_DOWN（一次 keyevent = 两帧）");
  assert.match(index, /BackHandler\.addEventListener\("hardwareBackPress"/, "Back 走核心 BackHandler");
  // 单参数拼接：多参数形态被 RN console 加引号拼成 `'POLYPACK_KEY', '{…}'`，
  // 破坏 device-walks/README「POLYPACK_KEY <json>」通道契约（设备链实测，2026-10-16）
  assert.match(
    index,
    /console\.log\("POLYPACK_KEY " \+ JSON\.stringify/,
    "须保留 logcat 观测通道（单参数，走查比对用）",
  );
  assert.match(index, /appConfig\.polypack\?\.pointer/, "pointer 由 app.json 驱动（RN 无自定义 env 内联）");
  assert.equal(JSON.parse(read("app.json")).polypack.pointer, "touch", "默认触屏形态；走查会话改 none");
});

test("宿主依赖与阻塞说明：adapter 本地接线、react-native 引入、阻塞项成文", () => {
  const pkg = JSON.parse(read("package.json"));
  assert.ok(pkg.private, "宿主工程不发布");
  assert.equal(pkg.dependencies["@polypack/adapter-native"], "file:..");
  assert.ok(pkg.dependencies["react-native"], "宿主须声明 react-native");
  const readme = read("README.md");
  for (const marker of ["阻塞", "T-M2-02", "无指针", "keytool"]) {
    assert.ok(readme.includes(marker), `宿主 README 缺说明: ${marker}`);
  }
  // file:.. 链接包的真实路径在项目根之外：缺 watchFolders 解析必失败（设备链实测 UnableToResolveError）
  assert.match(
    read("metro.config.js"),
    /watchFolders: \[path\.resolve\(__dirname, '\.\.'\)\]/,
    "metro.config.js 须把 adapter 包纳入 watch 树",
  );
});
