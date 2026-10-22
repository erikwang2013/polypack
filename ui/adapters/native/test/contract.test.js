// T-M2-04 契约测试：RendererAdapter 对象形状、mount 装配、原生模块注册位。
// 零依赖（node:test）；runtime 用桩注入，不要求安装 react-native。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { CAPABILITY_KEYS, createAdapter } from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));

const stubRuntime = (registered = []) => ({
  registered,
  AppRegistry: {
    registerComponent(appKey, factory) {
      registered.push({ appKey, factory });
    },
  },
});

const makeAdapter = (options = {}) =>
  createAdapter({ target: "android", runtime: stubRuntime(), ...options });

test("契约对象形状：mount / platformStyle / capabilities 三件套齐备且不可变", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.target, "android");
  for (const name of ["mount", "platformStyle", "capabilities", "degradations"]) {
    assert.equal(typeof adapter[name], "function", `${name} 应为函数`);
  }
  assert.equal(typeof adapter.native.register, "function");
  assert.ok(Object.isFrozen(adapter), "契约对象应冻结（宿主不得改写装配层）");
});

test("capabilities()：恰好契约三键，值为布尔", () => {
  const caps = makeAdapter().capabilities();
  assert.deepEqual(Object.keys(caps), [...CAPABILITY_KEYS]);
  for (const key of CAPABILITY_KEYS) {
    assert.equal(typeof caps[key], "boolean", `${key} 应为布尔`);
  }
});

test("platformStyle()：默认与传参两条路径都返回蒙皮样式", () => {
  const adapter = makeAdapter();
  const skinned = adapter.platformStyle();
  assert.equal(typeof skinned.card.padding, "number");
  assert.equal(typeof skinned.focusRing.borderColor, "string");
  const custom = adapter.platformStyle(
    { ...skinned, color: { ...skinned.color, brand: "#123456" } },
    { compact: true },
  );
  assert.equal(custom.button.backgroundColor, "#123456", "platformStyle(tokens) 应消费传入 token 集");
  assert.equal(custom.space[4], 18, "compact 间距紧凑化应生效");
});

test("mount(root, app)：AppRegistry 注册工厂返回同一组件；mounted 可观测", () => {
  const registered = [];
  const adapter = makeAdapter({ runtime: stubRuntime(registered) });
  const App = () => null;

  const mounted = adapter.mount({ appKey: "PaxApp" }, App);
  assert.equal(registered.length, 1);
  assert.equal(registered[0].appKey, "PaxApp");
  assert.equal(registered[0].factory(), App, "注册工厂应返回装配的组件");
  assert.equal(mounted.appKey, "PaxApp");
  assert.equal(adapter.mounted, mounted);
  assert.ok(Object.isFrozen(mounted));
});

test("装配边界校验：未知目标 / 缺 runtime / 非法 root / 非法 app 快速失败", () => {
  assert.throws(() => createAdapter({ target: "symbian", runtime: stubRuntime() }), /未知目标/);
  assert.throws(() => createAdapter({ target: "android" }), /runtime/);
  assert.throws(() => createAdapter({ target: "android", runtime: {} }), /AppRegistry/);
  const adapter = makeAdapter();
  assert.throws(() => adapter.mount({}, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "" }, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "PaxApp" }, null), /app 组件/);
});

test("原生模块注册位：默认空，登记描述符后校验生效（T-M2-02 挂载点）", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.native.has(), false);
  assert.equal(adapter.native.get(), null);
  assert.throws(() => adapter.native.register({ artifact: "", entryClass: "a.B" }), /artifact/);
  assert.throws(() => adapter.native.register({ artifact: "bridge/android/x.aar" }), /entryClass/);
  assert.throws(
    () => adapter.native.register({ artifact: "a.aar", entryClass: "a.B", libraryName: "" }),
    /libraryName/,
  );

  // 口径样例（T-M2-02）：AAR 路径 + Kotlin 门面类 + .so 基名
  const descriptor = adapter.native.register({
    artifact: "bridge/android/pax_core.aar",
    entryClass: "uniffi.pax.PaxKt",
    libraryName: "pax_core",
  });
  assert.deepEqual(descriptor, {
    artifact: "bridge/android/pax_core.aar",
    entryClass: "uniffi.pax.PaxKt",
    libraryName: "pax_core",
  });
  assert.equal(adapter.native.has(), true);
  assert.ok(Object.isFrozen(descriptor), "描述符应冻结（登记后不可被宿主改写）");

  // 最小描述符仍可用：libraryName 省略 → 归一到 null
  assert.deepEqual(adapter.native.register({ artifact: "a.aar", entryClass: "a.B" }), {
    artifact: "a.aar",
    entryClass: "a.B",
    libraryName: null,
  });
});

test("T-M3-02 鸿蒙 NAPI 注册位：口径按 target 分族（moduleName = UDL namespace），跨宿主混用即拒", () => {
  const adapter = createAdapter({ target: "harmonyos", runtime: stubRuntime() });
  assert.equal(adapter.native.has(), false);

  // 口径样例（T-M3-01）：.so 路径 + NAPI 模块名（<工程根>/bridge/harmonyos/<abi>/lib<ns>.so）
  const descriptor = adapter.native.register({
    artifact: "bridge/harmonyos/arm64-v8a/libpax.so",
    moduleName: "pax",
  });
  assert.deepEqual(descriptor, { artifact: "bridge/harmonyos/arm64-v8a/libpax.so", moduleName: "pax" });
  assert.equal(adapter.native.has(), true);
  assert.ok(Object.isFrozen(descriptor), "描述符应冻结");

  assert.throws(
    () => adapter.native.register({ artifact: "bridge/harmonyos/arm64-v8a/libpax.so" }),
    /moduleName/,
  );
  assert.throws(() => adapter.native.register({ artifact: "a.so", moduleName: "pax", entryClass: "uniffi.pax.PaxKt" }), /不含 entryClass/);
  assert.throws(() => adapter.native.register({ artifact: "a.so", moduleName: "pax", libraryName: "pax_core" }), /不含 entryClass/);
  assert.throws(() => adapter.native.register({ artifact: "", moduleName: "pax" }), /artifact/);
  assert.throws(() => adapter.native.register({ artifact: "a.so", moduleName: "" }), /moduleName/);
});

test("T-M2-02 口径哨兵：host 示例描述符与真跑夹具一致（库名 / UDL namespace）", (t) => {
  const fixture = path.resolve(HERE, "../../../../crates/polypack-core/tests/fixtures/bridge_core");
  const cargo = path.join(fixture, "Cargo.toml");
  const udl = path.join(fixture, "src/pax.udl");
  if (!fs.existsSync(cargo) || !fs.existsSync(udl)) {
    t.skip("真跑夹具未入库（m2-core 在途）");
    return;
  }
  const libraryName = fs.readFileSync(cargo, "utf8").match(/^name = "([^"]+)"/m)[1];
  const ns = fs.readFileSync(udl, "utf8").match(/^namespace (\w+)/m)[1];
  const entryClass = `uniffi.${ns}.${ns[0].toUpperCase()}${ns.slice(1)}Kt`;
  const index = fs.readFileSync(path.resolve(HERE, "../host/index.js"), "utf8");
  assert.ok(index.includes(`bridge/android/${libraryName}.aar`), `host 示例 AAR 路径须为 bridge/android/${libraryName}.aar`);
  assert.ok(index.includes(entryClass), `host 示例入口类须为 ${entryClass}`);
  assert.ok(index.includes(`"${libraryName}"`), `host 示例 libraryName 须为 ${libraryName}`);
});
