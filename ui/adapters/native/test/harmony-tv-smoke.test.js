// T-M4-14 鸿蒙 TV 冒烟（本地真跑层）：
//   ① 宿主 TV 结构快照（≡ M3 宿主，差异仅 TV 行：deviceTypes tv / TV 能力提示 / 安全区外框）；
//   ② 键序内核推导对拍（键序列与期望值由 adapter-tv 共享内核现场推导，≡ device-walks 对拍数据）
//      + T-M4-02 连通性检查器作用于宿主图与 TV 参考网格（0 孤立 / 0 陷阱 / 全可达）；
//   ③ 行为对拍（react 在场时）：走查键序在 RNOH TV 形态页逐键履约；同键名序列在 RNOH 与
//      adapter-tv 参考页各自图上 ≡ 内核推导落点（规则同源、示例图各自），焦点环恰一处随焦点；
//   ④ 目标分辨率（1080p/4K）下 TV 安全区无遮挡核对（adapter-tv safeAreaInsets 单一源换算）。
// 设备腿（HAP 安装启动 + 遥控器键走查）本机阻塞：无 HarmonyOS 设备/模拟器（hdc 无目标）——
// SKIP-with-print 附执行清单（末项，键码现场取自宿主键表），走查手册见 ../device-walks/README.md「鸿蒙 TV」段。
// react/react-dom 未安装（CI 无 npm install）时行为层 skip 不 fail（与 render / structure 同纪律）。

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { createAdapter as createNativeAdapter } from "../src/index.js";
import * as nativeKernel from "../src/focus.js";
import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { KEYMAP, WALKS } from "../device-walks/walks.mjs";
import {
  CAPABILITY_MATRIX,
  SAFE_AREA,
  createAdapter as createTvAdapter,
  platformStyle as tvPlatformStyle,
  safeAreaInsets,
} from "../../tv/src/index.js";
import * as tvKernel from "../../tv/src/focus.js";
// tv 参考页宿主域 2×2 网格（原样别名，避免与 adapter-native 的 EXAMPLE_FOCUS_NODES 同名混淆）
import { EXAMPLE_FOCUS_NODES as TV_GRID_NODES } from "../../tv/src/example-nodes.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOST = path.resolve(HERE, "../host/harmony");
const read = (rel) => fs.readFileSync(path.join(HOST, rel), "utf8");
const INDEX = read("index.js");
const ACCENT = "#0e7490"; // 焦点环色（两 adapter 同源 token：tokens.color.accent）
const EXPECTATIONS = JSON.parse(fs.readFileSync(path.join(HERE, "../device-walks/expectations.json"), "utf8"));

const stubRuntime = { AppRegistry: { registerComponent() {} } };
/** 宿主键表（index.js 单一源）→ 内核键名 → KEYCODE 列表。 */
const keyTable = () => {
  const block = INDEX.match(/const KEYCODE_TO_KERNEL = \{([\s\S]*?)\};/)[1];
  const map = new Map();
  for (const m of block.matchAll(/(\d+):\s*"([A-Z_]+)"/g)) {
    map.set(m[2], [...(map.get(m[2]) ?? []), Number(m[1])]);
  }
  return map;
};

// 只吞「react/react-dom 未安装」一类失败；本包加载错误照常炸出（同 render.test.js 防假绿约束）。
// 非 react 依赖的包（src/index.js / focus.js / example-nodes.js / adapter-tv 全系）载入在守卫外。
let mods = null;
try {
  mods = {
    react: await import("react"),
    server: await import("react-dom/server"),
    nativeDom: await import("./helpers/dom-primitives.js"), // 零导入（React 注入），此处取用而已
    tvDom: await import("../../tv/test/helpers/dom-primitives.js"),
    nativeExample: await import("../example.js"), // 依赖 react
    tvExample: await import("../../tv/example.js"), // 依赖 react
  };
} catch (error) {
  const reactMissing =
    error?.code === "ERR_MODULE_NOT_FOUND" && /Cannot find package 'react(-dom)?'/.test(error.message);
  if (!reactMissing) throw error;
}
// tv 侧独立 module 图（自 tv/package.json 解析）：两包各自的 react/react-dom 成对使用，
// 不做跨包混用——否则 react 双副本（各包 node_modules 各自安装）下 hooks 派发器为 null。
// 提升（单副本）布局下两侧解析到同一实例，等价成立。
const tvRequire = createRequire(path.join(HERE, "../../tv/package.json"));
const tvReactMods = (() => {
  try {
    return { react: tvRequire("react"), server: tvRequire("react-dom/server") };
  } catch {
    return null;
  }
})();
const skip =
  mods === null
    ? "react/react-dom 未安装（npm install 后启用行为对拍断言）"
    : tvReactMods === null
      ? "tv 包缺 react/react-dom（cd ui/adapters/tv && npm install 后启用行为对拍断言）"
      : false;

/** 代理渲染 HTML 中 testID 节点的按钮标签段是否带焦点环色。 */
function ringOn(html, id) {
  const segs = html.split(/<button/).slice(1);
  const seg = segs.find((s) => s.includes(`data-testid="${id}"`));
  assert.ok(seg, `代理渲染缺节点 ${id}`);
  return seg.slice(0, seg.indexOf(">")).includes(ACCENT);
}

test("宿主 TV 结构快照：deviceTypes 含 tv（逐项 ≡ SDK 19 枚举）+ TV 开关/能力提示/安全区外框三处就位，M3 通道零改动", () => {
  // ① 清单：deviceTypes = 手机族 default + TV，逐项在 SDK 19 modulecheck 枚举内（TV 分发/安装要求）
  const moduleJson5 = read("harmony/entry/src/main/module.json5");
  const declared = moduleJson5
    .match(/"deviceTypes":\s*\[([^\]]*)\]/)[1]
    .split(",")
    .map((s) => s.trim().replace(/"/g, ""))
    .filter(Boolean);
  assert.deepEqual(declared, ["default", "tv"], "deviceTypes = 手机族 default + TV");
  const schema = process.env.HARMONYOS_TOOLS
    ? path.join(process.env.HARMONYOS_TOOLS, "sdk/default/openharmony/toolchains/modulecheck/module.json")
    : null;
  if (schema && fs.existsSync(schema)) {
    const enumValues = JSON.parse(fs.readFileSync(schema, "utf8")).properties.module.properties
      .deviceTypes.items.enum;
    for (const device of declared) assert.ok(enumValues.includes(device), `${device} 不在 SDK 枚举内`);
    assert.ok(enumValues.includes("tv"), "SDK 枚举须含 tv");
  }

  // ② TV 形态开关与能力提示：app.json 默认手机（tv=false）；打开后 pointer none + gestures false
  const app = JSON.parse(read("app.json"));
  assert.equal(app.polypack.tv, false, "默认手机形态；TV HAP 构建前置 polypack.tv=true");
  assert.equal(app.polypack.pointer, "touch", "M3 走查口径不动");
  assert.match(INDEX, /const tvForm = appConfig\.polypack\?\.tv === true/);
  assert.match(INDEX, /host: tvForm \? \{ pointer, gestures: false \} : \{ pointer \}/, "TV = 无指针 + 无手势");

  // ③ 安全区外框：四处内边距取自 adapter-tv platformStyle().safeArea（单一源，非复制的字面量）
  assert.match(INDEX, /import \{ platformStyle as tvPlatformStyle \} from "@polypack\/adapter-tv"/);
  for (const side of ["top", "right", "bottom", "left"]) {
    assert.match(INDEX, new RegExp(`padding${side[0].toUpperCase()}${side.slice(1)}: tvSafeArea\\.${side}`));
  }
  assert.match(INDEX, /testID: "tv-frame"/);
  assert.doesNotMatch(INDEX, /"5%"/, "安全区百分比不得手抄（单一源经 adapter-tv）");
  assert.equal(JSON.parse(read("package.json")).dependencies["@polypack/adapter-tv"], "file:../../../tv");

  // ④ M3 通道零改动（键源 / DOWN-only / Back / 观测通道 / 装配链）
  assert.match(INDEX, /adapter\.mount\(\{ appKey: "PaxApp" \}, App\)/, "装配链三处一致不动");
  assert.match(INDEX, /DeviceEventEmitter\.addListener\("polyKeyEvent"/);
  assert.match(INDEX, /event\.keyAction !== 0/);
  assert.match(INDEX, /BackHandler\.addEventListener\("hardwareBackPress"/);
  assert.match(INDEX, /console\.log\("POLYPACK_KEY " \+ JSON\.stringify/);
});

test("键序内核推导对拍：adapter-tv 内核 ≡ RNOH 宿主内核（同一函数对象）+ 同键序现场推导 ≡ 走查对拍数据", () => {
  // 构造性基础（T-M4-01 口径）：不是「两处实现碰巧一致」，是同一实现的两个入口（跨包相对导入）
  for (const name of [
    "DIRECTIONS",
    "KEY_MAP",
    "createFocusEngine",
    "handleKey",
    "hasNoDeadEnds",
    "reachableFrom",
    "traversalSnapshot",
  ]) {
    assert.equal(tvKernel[name], nativeKernel[name], `${name} 须为同一函数/对象（不复制实现）`);
  }

  // TV 能力面：RNOH 宿主 TV 形态（pointer none + gestures false）× adapter-tv TV 基线 —— 三键相等
  const adapter = createNativeAdapter({
    target: "harmonyos",
    runtime: stubRuntime,
    host: { pointer: "none", gestures: false },
  });
  assert.deepEqual(adapter.capabilities(), { focusNav: true, safeArea: true, gestures: false });
  assert.deepEqual(adapter.capabilities(), { ...CAPABILITY_MATRIX["android-tv"] }, "≡ TV 基线三键");

  // 走查键序：内核现场推导（同图同键 ⇒ 同描述符 / 同焦点序列），与入库对拍数据逐条相等
  const fresh = () => tvKernel.createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  for (const walk of WALKS) {
    const engine = fresh();
    const results = [];
    const focus = [];
    for (const key of walk.keys) {
      results.push(tvKernel.handleKey(engine, key));
      focus.push(engine.current);
    }
    const golden = EXPECTATIONS.walks.find((w) => w.id === walk.id);
    assert.deepEqual(results, golden.expect.results, `${walk.id}: 描述符漂移`);
    assert.deepEqual(focus, golden.expect.focus, `${walk.id}: 焦点序列漂移`);
    assert.deepEqual(walk.clicks, golden.expect.clicks, `${walk.id}: clicks 期望漂移`);
  }

  // 走查键全表可达（Back 走 BackHandler 不在键表）：RNOH TV 走查可按同一键序执行
  const table = keyTable();
  for (const { kernel } of KEYMAP) {
    if (kernel !== "Back") assert.ok(table.has(kernel), `宿主键表缺 ${kernel}`);
  }

  // 连通性（M4 出口条件）：遍历无死路 + 初始焦点可达全图
  const engine = fresh();
  assert.ok(tvKernel.hasNoDeadEnds(engine.graph), "遍历图无死路");
  assert.deepEqual(tvKernel.reachableFrom(engine.graph, "counter"), [...engine.order], "初始焦点可达全图");

  // T-M4-02 连通性检查器（adapter-tv 焦点系统 API）作用于两图：宿主图（RNOH TV 形态实际走的图）
  // 与 TV 参考网格（createTvFocus 几何推导）——均须 0 孤立 / 0 陷阱 / 全可达
  assert.deepEqual(tvKernel.checkConnectivity(engine.graph, { from: "counter" }), {
    from: "counter",
    total: 2,
    reachable: ["counter", "reset"],
    isolated: [],
    traps: [],
    connected: true,
  });
  const tvRef = tvKernel.createTvFocus({ nodes: TV_GRID_NODES, initial: "counter" });
  assert.deepEqual(tvKernel.checkConnectivity(tvRef.graph), {
    from: "counter",
    total: 4,
    reachable: ["counter", "reset", "detail", "exit"],
    isolated: [],
    traps: [],
    connected: true,
  });
});

test("TV 分辨率与安全区核对：1080p/4K 换算 ≡ 四边 5%（自 SAFE_AREA 派生复核）且内容盒容下页卡", () => {
  assert.deepEqual(SAFE_AREA, { top: 5, right: 5, bottom: 5, left: 5 }, "安全区 = 四边 5% 视口");
  assert.deepEqual(tvPlatformStyle().safeArea, { top: "5%", right: "5%", bottom: "5%", left: "5%" });
  // 1080p（鸿蒙 TV 基准）与 4K 两档：像素换算金样 + 自 token 派生复核（金样钉值与单源同检）
  for (const [viewport, golden] of [
    [{ width: 1920, height: 1080 }, { top: 54, right: 96, bottom: 54, left: 96 }],
    [{ width: 3840, height: 2160 }, { top: 108, right: 192, bottom: 108, left: 192 }],
  ]) {
    const insets = safeAreaInsets(viewport);
    assert.deepEqual(insets, golden, `${viewport.width}×${viewport.height} 换算`);
    for (const [side, span] of [
      ["top", viewport.height],
      ["right", viewport.width],
      ["bottom", viewport.height],
      ["left", viewport.width],
    ]) {
      assert.equal(insets[side], Math.round((span * SAFE_AREA[side]) / 100), `${side} 须自 SAFE_AREA 派生`);
    }
    // 内容盒为正且容下两页卡宽（RNOH 页卡 maxWidth 420 / TV 10-foot 页卡 maxWidth 1280）⇒ 无遮挡
    const content = {
      width: viewport.width - insets.left - insets.right,
      height: viewport.height - insets.top - insets.bottom,
    };
    assert.ok(content.width > 0 && content.height > 0, "内容盒为正（无过扫描吞没）");
    assert.ok(420 <= content.width && 1280 <= content.width, `内容盒 ${content.width} 容不下页卡`);
  }
  assert.throws(() => safeAreaInsets({ width: 0, height: 1080 }), /需视口 width/, "非法视口快速失败");
});

test("行为对拍（react 在场）：走查键序在 RNOH TV 形态页逐键履约；同键名序列在 RNOH / adapter-tv 参考页各自图上 ≡ 内核推导落点，焦点环恰一处随焦点", { skip }, async () => {
  const rnAdapter = createNativeAdapter({
    target: "harmonyos",
    runtime: stubRuntime,
    host: { pointer: "none", gestures: false },
  });
  const tvAdapter = createTvAdapter({
    target: "android-tv",
    runtime: {
      React: tvReactMods.react,
      ReactDOM: { createRoot: () => ({ render() {} }) },
    },
  });
  const makeRn = () => mods.nativeExample.createExampleApp(mods.nativeDom.createDomPrimitives(mods.react), rnAdapter);
  const makeTv = () => mods.tvExample.createExamplePage(mods.tvDom.createDomPrimitives(tvReactMods.react), tvAdapter);

  // ① RNOH TV 形态：走查键序逐键履约 device-walks 对拍数据（焦点 / 计数）——页状态层独立验证
  for (const walk of WALKS) {
    const golden = EXPECTATIONS.walks.find((w) => w.id === walk.id);
    const page = makeRn();
    walk.keys.forEach((key, index) => {
      page.onKey(key);
      assert.equal(page.getState().focus, golden.expect.focus[index], `${walk.id}[${index}]: ${key} 后焦点漂移`);
      assert.equal(page.getState().clicks, walk.clicks[index], `${walk.id}[${index}]: clicks 履约`);
    });
  }

  // ② 双宿主对拍（T-M3-02 做法）：同键名序列在各自图上由内核现场推导落点（RNOH 2 节点环 /
  //    adapter-tv 参考页 2×2 网格），页状态须 ≡ 推导（行为同规则，图各自）；回绝「两页同态」式
  //    对拍——两宿主示例图已分道（T-M4-02 起 TV 侧为 4 节点网格），同一的是键名 / 规则 / 内核。
  const rn = makeRn();
  const tv = makeTv();
  const rnEngine = nativeKernel.createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  const tvEngine = tvKernel.createTvFocus({ nodes: TV_GRID_NODES, initial: "counter" });
  const SEQ = ["DPAD_CENTER", "DPAD_DOWN", "DPAD_RIGHT", "DPAD_UP", "DPAD_LEFT", "DPAD_DOWN", "DPAD_DOWN"];
  for (const key of SEQ) {
    nativeKernel.handleKey(rnEngine, key, { activate: () => {} });
    tvKernel.handleKey(tvEngine, key, { activate: () => {} });
    rn.onKey(key);
    tv.onKey(key);
    assert.equal(rn.getState().focus, rnEngine.current, `${key}: RNOH 页焦点 ≠ 内核推导落点`);
    assert.equal(tv.getState().focus, tvEngine.current, `${key}: adapter-tv 页焦点 ≠ 内核推导落点`);
  }
  assert.equal(rn.getState().clicks, 1, "中心键在 counter 上激活一次（RNOH）");
  assert.equal(tv.getState().clicks, 1, "中心键在 counter 上激活一次（adapter-tv）");

  // 焦点环行为：恰一处、随焦点移动（当前焦点各为 RNOH reset / TV detail，取自推导落点）。
  // RN 代理原语不透传 data-focus（M3 口径）→ RN 侧以环色判定；adapter-tv 代理原语透传 data-focus
  // → 帧内恰一（T-M4-13 观测锚点通道）。各侧渲染用各侧成对的 react + react-dom（跨包混用会在
  // 双副本布局下触发 hooks 派发器为 null）。
  const renderRn = () => mods.server.renderToStaticMarkup(mods.react.createElement(rn.App));
  const renderTv = () => tvReactMods.server.renderToStaticMarkup(tvReactMods.react.createElement(tv.Page));
  const countRing = (html) => (html.match(new RegExp(ACCENT, "gi")) ?? []).length;
  const rnFocused = rn.getState().focus;
  const tvFocused = tv.getState().focus;
  assert.notEqual(rnFocused, "counter", "序列后焦点已离开初始节点（否则环断言退化）");
  assert.equal(countRing(renderRn()), 1, "RNOH：环恰一处");
  assert.equal(countRing(renderTv()), 1, "adapter-tv：环恰一处");
  assert.ok(ringOn(renderRn(), rnFocused), `RNOH：环随焦点在 ${rnFocused}`);
  assert.ok(ringOn(renderTv(), tvFocused), `adapter-tv：环随焦点在 ${tvFocused}`);
  assert.ok(!ringOn(renderRn(), "counter") && !ringOn(renderTv(), "counter"), "原焦点无环");
  assert.equal((renderTv().match(/data-focus="true"/g) ?? []).length, 1, "adapter-tv：data-focus 每帧恰一");

  // 触屏形态对照：RN 宿主默认（pointer touch）不渲染环（M3 口径不动）
  const touch = createNativeAdapter({ target: "harmonyos", runtime: stubRuntime });
  const touchApp = mods.nativeExample.createExampleApp(mods.nativeDom.createDomPrimitives(mods.react), touch);
  assert.equal(countRing(mods.server.renderToStaticMarkup(mods.react.createElement(touchApp.App))), 0);
});

test("设备腿（HAP 安装启动 + 遥控器键走查）：SKIP-with-print——无 HarmonyOS 设备/模拟器", (t) => {
  const tools = process.env.HARMONYOS_TOOLS;
  const hdc = tools ? path.join(tools, "sdk/default/openharmony/toolchains/hdc") : null;
  if (!hdc || !fs.existsSync(hdc)) {
    t.skip(`hdc 未找到（HARMONYOS_TOOLS=${tools ?? "未设置"}）——RNOH TV 设备腿阻塞，不假装已跑`);
    return;
  }
  let targets;
  try {
    targets = execFileSync(hdc, ["list", "targets"], { encoding: "utf8", timeout: 10_000 }).trim();
  } catch (error) {
    t.skip(`hdc list targets 调用失败：${String(error.message).split("\n")[0]}——设备腿阻塞，不假装已跑`);
    return;
  }
  // 无论探测结果如何，本测试都不跑真机（安装/启动/键走查属设备上机窗口）：只如实注记并打印执行清单。
  // 键码取自宿主键表（单一源），避免手册与实现漂移；结果比对口径 ≡ device-walks/expectations.json。
  const table = keyTable();
  const keyEvents = KEYMAP.filter((m) => m.kernel !== "Back")
    .map((m) => `${m.kernel}=${(table.get(m.kernel) ?? []).join("|")}`)
    .join("  ");
  t.skip(
    `hdc list targets = ${targets === "" ? "空" : targets}（本次运行实测）；本机无 HarmonyOS 设备/模拟器，` +
      "HAP 安装启动与遥控器键走查阻塞——证据边界：本文件为 JS 层替代证据（结构快照 + 键序内核推导对拍 + " +
      "安全区换算），真机证据由设备上机窗口补。设备就绪后按下列清单执行（TV HAP 前置 app.json " +
      "polypack.tv=true 再出包）：\n" +
      "  出包： polypack build --target harmonyos（复用 M3 链：hvigorw assembleHap + hap-sign-tool 签名）\n" +
      "  安装： hdc install <签名 HAP>（bundleName com.polypack.host）\n" +
      "  启动： hdc shell aa force-stop com.polypack.host && hdc shell aa start -b com.polypack.host -a EntryAbility\n" +
      `  按键： hdc shell uitest uiInput keyEvent <code>（内核键 ↔ KEYCODE：${keyEvents}）；Back 键单独 uitest uiInput keyEvent Back\n` +
      "  观测： hdc shell hilog -x | grep -o 'POLYPACK_KEY .*'（tag ReactNativeJS，每键一行；单参数通道契约同 android）\n" +
      "  比对： 每键结果的 expect.focus / expect.clicks 与 device-walks/expectations.json 逐条对拍" +
      "（一次键命令 = Down+Up 两帧，宿主只消费 Down）；安全区可经 hdc shell hidumper -s WindowManagerService " +
      "取窗口尺寸核对四边 5% 边距",
  );
});
