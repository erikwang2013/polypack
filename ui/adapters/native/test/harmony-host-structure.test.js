// T-M3-02 鸿蒙（RNOH）宿主结构断言：真机渲染阻塞（无 HarmonyOS 设备/模拟器，hdc 无目标）下，
// 以骨架完整性、装配链一致性、键事件通道覆盖与 SDK 声明漂移哨兵替代——证据边界见 README「阻塞」。

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { CAPABILITY_MATRIX, createAdapter, createFocusEngine, handleKey } from "../src/index.js";
import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { KEYMAP } from "../device-walks/walks.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOST = path.resolve(HERE, "../host/harmony");
const read = (rel) => fs.readFileSync(path.join(HOST, rel), "utf8");
const exists = (rel) => fs.existsSync(path.join(HOST, rel));

const ETS = "harmony/entry/src/main/ets";
const INDEX = read("index.js");
/** 注释示例去注释（仅取装配样例——register 示例按 android 宿主同规矩保持注释态）。 */
const INDEX_UNCOMMENTED = INDEX.replace(/^\/\/ ?/gm, "");

const stubRuntime = { AppRegistry: { registerComponent() {} } };

test("宿主骨架文件齐备（RNOH 官方模板 0.84.3 形态 + packager 消费的 harmony/ 工程）", () => {
  const files = [
    "package.json",
    "app.json",
    "index.js",
    "babel.config.js",
    "metro.config.js",
    "README.md",
    "harmony/build-profile.json5",
    "harmony/oh-package.json5",
    "harmony/hvigorfile.ts",
    "harmony/hvigor/hvigor-config.json5",
    "harmony/entry/build-profile.json5",
    "harmony/entry/hvigorfile.ts",
    "harmony/entry/oh-package.json5",
    "harmony/entry/src/main/module.json5",
    "harmony/entry/src/main/cpp/CMakeLists.txt",
    `${ETS}/entryability/EntryAbility.ets`,
    `${ETS}/pages/Index.ets`,
    `${ETS}/PackageProvider.ets`,
    "harmony/entry/libs/arm64-v8a/.gitkeep",
    "harmony/entry/libs/x86_64/.gitkeep",
  ];
  for (const file of files) {
    assert.ok(exists(file), `缺宿主文件: ${file}`);
  }
});

test("装配链闭合：AppRegistry 键三处一致（app.json / Index.ets RNApp / index.js mount）+ 组件映射已接", () => {
  assert.equal(JSON.parse(read("app.json")).name, "PaxApp");
  const indexEts = read(`${ETS}/pages/Index.ets`);
  assert.match(indexEts, /appKey: "PaxApp"/, "RNApp 组件名须与 app.json name 一致");
  assert.match(indexEts, /name: "PaxApp"/, "rnInstanceConfig.name 同键");
  assert.match(INDEX, /adapter\.mount\(\{ appKey: "PaxApp" \}, App\)/);
  assert.match(INDEX, /from "@polypack\/adapter-native"/);
  assert.match(INDEX, /from "@polypack\/adapter-native\/example"/);
  // 组件映射（T-M3-02）：页面前零改动，宿主专属 prop 差异经 bindPrimitives 落在装配边界
  assert.match(INDEX, /bindPrimitives\(/, "宿主须经 bindPrimitives 应用组件映射");
  assert.match(INDEX, /createElement/, "bindPrimitives 需注入 createElement");
});

test("target id 哨兵：createAdapter 的 target 是 capabilities 矩阵内的目标（'harmonyos'）", () => {
  const target = INDEX.match(/createAdapter\(\{\s*target: "([^"]+)"/)?.[1];
  assert.equal(target, "harmonyos", "target = TargetSpec id（与 <工程根>/harmony/ 目录名无关）");
  assert.ok(Object.hasOwn(CAPABILITY_MATRIX, target));
  const adapter = createAdapter({ target, runtime: stubRuntime }); // 未知目标会在此抛错（宿主装配真路径）
  assert.deepEqual(adapter.capabilities(), { focusNav: false, safeArea: true, gestures: true });
});

test("键事件源接线：polyKeyEvent + DOWN-only + BackHandler + 单参数 POLYPACK_KEY 观测通道", () => {
  assert.match(INDEX, /DeviceEventEmitter\.addListener\("polyKeyEvent"/, "须订阅 ArkTS 转发通道");
  assert.match(INDEX, /event\.keyAction !== 0/, "须只消费 Down（KeyType=0）：一次物理按键 Down+Up 两帧");
  assert.match(INDEX, /BackHandler\.addEventListener\("hardwareBackPress"/, "Back 走核心 BackHandler 通道");
  assert.match(
    INDEX,
    /console\.log\("POLYPACK_KEY " \+ JSON\.stringify/,
    "须保留 hilog 观测通道（单参数，走查比对用；android 设备链同款约束）",
  );
  // ArkTS 侧转发契约（T-M3-01）：onKeyEvent → emitDeviceEvent('polyKeyEvent', {keyCode, keyAction})
  const indexEts = read(`${ETS}/pages/Index.ets`);
  assert.match(indexEts, /emitDeviceEvent\('polyKeyEvent',\s*\{/, "ArkTS 根容器须转发 polyKeyEvent");
  assert.match(indexEts, /keyCode:\s*event\.keyCode/);
  assert.match(indexEts, /keyAction:\s*event\.type/);
  // Back 单独通道（SDK 5.1.1(19) 回调名 onBackPressed）：EntryAbility → dispatchBackPress → 'hardwareBackPress'
  assert.match(read(`${ETS}/entryability/EntryAbility.ets`), /onBackPressed\(\)/);
  assert.match(INDEX, /appConfig\.polypack\?\.pointer/, "pointer 由 app.json 驱动");
  assert.equal(JSON.parse(read("app.json")).polypack.pointer, "touch", "默认触屏形态；走查会话改 none");
});

test("键表覆盖：KEYCODE_TO_KERNEL ⊇ 走查键（Back 除外）且全部可被共享焦点内核路由", () => {
  const block = INDEX.match(/const KEYCODE_TO_KERNEL = \{([\s\S]*?)\};/)?.[1];
  assert.ok(block, "index.js 应有 KEYCODE_TO_KERNEL 表");
  const table = [...block.matchAll(/(\d+):\s*"([A-Z_]+)"/g)].map((m) => [Number(m[1]), m[2]]);
  assert.ok(table.length >= 5, `键表解析异常（${table.length} 项）`);
  for (const [code, key] of table) assert.ok(Number.isInteger(code) && code > 0);

  // 走查键全表可达（device-walks 的键序列在 RNOH 下同序可驱；Back 走 BackHandler 不在本表）
  const walkKeys = KEYMAP.map((m) => m.kernel).filter((k) => k !== "Back");
  const mapped = new Set(table.map(([, key]) => key));
  for (const key of walkKeys) {
    assert.ok(mapped.has(key), `走查键 ${key} 未入宿主键表（RNOH 走查无法按对拍数据执行）`);
  }
  // 每个映射值都是内核能路由的键（无死表项）
  const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  for (const key of mapped) {
    assert.notEqual(handleKey(engine, key).type, "ignored", `内核应路由 ${key}`);
  }
});

test("SDK 漂移哨兵：键表数值 ≡ OHOS SDK @ohos.multimodalInput.keyCode.d.ts 声明（HARMONYOS_TOOLS 缺席时 skip）", (t) => {
  const tools = process.env.HARMONYOS_TOOLS;
  if (!tools) {
    t.skip("HARMONYOS_TOOLS 未设置（装 HarmonyOS command-line-tools 后此哨兵生效）");
    return;
  }
  const dts = path.join(tools, "sdk/default/openharmony/ets/api/@ohos.multimodalInput.keyCode.d.ts");
  if (!fs.existsSync(dts)) {
    t.skip(`SDK d.ts 未找到: ${dts}`);
    return;
  }
  const declared = new Map(
    [...fs.readFileSync(dts, "utf8").matchAll(/^\s*(KEYCODE_[A-Z0-9_]+) = (\d+)/gm)].map((m) => [
      m[1],
      Number(m[2]),
    ]),
  );
  assert.ok(declared.size > 10, `d.ts 解析异常（${declared.size} 项）`);
  const block = INDEX.match(/const KEYCODE_TO_KERNEL = \{([\s\S]*?)\};/)?.[1] ?? "";
  const table = [...block.matchAll(/(\d+):\s*"([A-Z_]+)"/g)].map((m) => [Number(m[1]), m[2]]);
  const SDK_NAMES = {
    DPAD_UP: ["KEYCODE_DPAD_UP"],
    DPAD_DOWN: ["KEYCODE_DPAD_DOWN"],
    DPAD_LEFT: ["KEYCODE_DPAD_LEFT"],
    DPAD_RIGHT: ["KEYCODE_DPAD_RIGHT"],
    DPAD_CENTER: ["KEYCODE_DPAD_CENTER", "KEYCODE_ENTER", "KEYCODE_SPACE"],
  };
  for (const [code, key] of table) {
    assert.ok(
      SDK_NAMES[key].some((name) => declared.get(name) === code),
      `键表 ${code}→${key} 与 SDK 声明不符（${SDK_NAMES[key].map((n) => `${n}=${declared.get(n)}`).join(" / ")}）`,
    );
  }
  // 反向覆盖：确认键键位声明均在表内（漏一个即 RNOH 走查缺口）
  const codes = new Set(table.map(([code]) => code));
  for (const names of Object.values(SDK_NAMES)) {
    for (const name of names) {
      assert.ok(codes.has(declared.get(name)), `${name}=${declared.get(name)} 应入宿主键表`);
    }
  }
});

test("NAPI 桥产物口径哨兵：宿主示例描述符与真跑夹具一致（模块名 / .so 基名）+ 注册位接受", (t) => {
  const sample = INDEX_UNCOMMENTED.match(
    /adapter\.native\.register\(\{\s*artifact: "([^"]+)",\s*moduleName: "([^"]+)",?\s*\}\)/,
  );
  assert.ok(sample, "index.js 须保留 register 示例（口径随真跑产物）");
  const [, artifact, moduleName] = sample;

  const fixture = path.resolve(HERE, "../../../../crates/poly-core/tests/fixtures/bridge_core");
  const udl = path.join(fixture, "src/pax.udl");
  if (!fs.existsSync(udl)) {
    t.skip("真跑夹具未入库（T-M3-01 在途）");
    return;
  }
  const ns = fs.readFileSync(udl, "utf8").match(/^namespace (\w+)/m)[1];
  assert.equal(moduleName, ns, `NAPI 模块名须为 UDL namespace（${ns}）`);
  assert.match(artifact, new RegExp(`^bridge/harmonyos/(arm64-v8a|x86_64)/lib${ns}\\.so$`));
  // 注册位接受该口径（target=harmonyos 分族校验；见 contract.test.js）
  const descriptor = createAdapter({ target: "harmonyos", runtime: stubRuntime }).native.register({
    artifact,
    moduleName,
  });
  assert.deepEqual(descriptor, { artifact, moduleName });
});

test("二进制与凭据资产不入库：无 .so/.har/.hbc/证书，libs/ 仅 .gitkeep，忽略规则兜底", () => {
  const forbidden = /\.(so|har|hbc|p12|cer|jks|keystore)$/;
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else assert.ok(!forbidden.test(entry.name), `${path.relative(HOST, full)} 不应入库`);
    }
  };
  walk(HOST);
  for (const abi of ["arm64-v8a", "x86_64"]) {
    assert.deepEqual(
      fs.readdirSync(path.join(HOST, "harmony/entry/libs", abi)),
      [".gitkeep"],
      `libs/${abi}/ 只留占位（.so 由 poly bridge ohos 注入）`,
    );
  }
  assert.match(read(".gitignore"), /^node_modules\/$/m);
  assert.match(read("harmony/.gitignore"), /bridge\/|oh_modules/);
});

test("宿主依赖与阻塞说明：adapter 本地接线、RNOH 依赖配对、watch 树、阻塞成文", () => {
  const pkg = JSON.parse(read("package.json"));
  assert.ok(pkg.private, "宿主工程不发布");
  assert.equal(pkg.dependencies["@polypack/adapter-native"], "file:../..");
  assert.equal(pkg.dependencies["react-native"], "0.84.1");
  assert.equal(pkg.dependencies["react"], "19.2.3");
  assert.equal(pkg.dependencies["@react-native-oh/react-native-harmony"], "0.84.3");
  // 链接包（adapter-native file:../..、adapter-tv file:../../../tv——T-M4-14 TV 安全区单一源）
  // 的真实路径在工程根之外：缺 watchFolders 解析必失败（android 设备链同因）
  assert.match(
    read("metro.config.js"),
    /watchFolders: \[path\.resolve\(__dirname, '\.\.\/\.\.'\), path\.resolve\(__dirname, '\.\.\/\.\.\/\.\.\/tv'\)\]/,
    "metro.config.js 须把 adapter-native 与 adapter-tv 两个链接包纳入 watch 树",
  );
  const readme = read("README.md");
  for (const marker of ["阻塞", "hdc", "polyKeyEvent"]) {
    assert.ok(readme.includes(marker), `宿主 README 缺说明: ${marker}`);
  }
});

test("设备腿（RNOH 真机渲染 + 键序列走查）：SKIP-with-print——无 HarmonyOS 设备/模拟器", (t) => {
  const tools = process.env.HARMONYOS_TOOLS;
  const hdc = tools ? path.join(tools, "sdk/default/openharmony/toolchains/hdc") : null;
  if (!hdc || !fs.existsSync(hdc)) {
    t.skip(`hdc 未找到（HARMONYOS_TOOLS=${tools ?? "未设置"}）——RNOH 设备腿阻塞，不假装已跑`);
    return;
  }
  let targets;
  try {
    targets = execFileSync(hdc, ["list", "targets"], { encoding: "utf8", timeout: 10_000 }).trim();
  } catch (error) {
    t.skip(`hdc list targets 调用失败：${String(error.message).split("\n")[0]}——设备腿阻塞，不假装已跑`);
    return;
  }
  // 无论探测结果如何，本测试都不跑真机（渲染/走查属设备链上机窗口）：只如实注记并打印执行清单
  t.skip(
    `hdc list targets = ${targets === "" ? "空" : targets}（2026-10-16 实测）；本机无 DevEco/模拟器，` +
      "RNOH 真机渲染与键序列走查阻塞——证据边界：本包测试为 JS 层替代证据（结构快照 + 宿主结构断言），" +
      "真机证据由设备链里程碑（T-M3-04/09 上机窗口）补。设备就绪后按 device-walks 同款对拍执行" +
      "（键序列与期望值 ≡ device-walks/expectations.json，内核键名全同）：\n" +
      "  前置： hdc shell aa force-stop com.polypack.host && hdc shell aa start -b com.polypack.host -a EntryAbility\n" +
      "  按键： hdc shell uitest uiInput keyEvent 2012|2013|2014|2015|2016（DPAD 四向/中心）；hdc shell uitest uiInput keyEvent Back\n" +
      "  观测： hdc shell hilog -x | grep -o 'POLYPACK_KEY .*'（tag ReactNativeJS，每键一行；单参数通道契约同 android）\n" +
      "  比对： 每键结果的 expect.focus / expect.clicks 与 device-walks/expectations.json 逐条对拍" +
      "（一次命令 = Down+Up 两帧，宿主只消费 Down）",
  );
});
