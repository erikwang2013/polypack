// T-M2-05 设备走查数据（emulator / 实机同用）：把焦点内核的确定性翻成
// 「adb 键序列 → 期望焦点序列」的对拍数据，供 m2-android 的设备链执行（它跑 adb、比对期望）。
//
// 焦点序列与键路由结果由内核现场推导（不手写）；`clicks` 是期望值（示例页 press 规则：
// 激活 counter 计数 +1、激活其余清零），由 test/device-walks.test.js 对着示例页履约。

import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { createFocusEngine, handleKey } from "../src/focus.js";

/** 内核键 → adb keyevent（code 为 Android KeyEvent 常量，keyevent 也接数值）。 */
export const KEYMAP = Object.freeze([
  { kernel: "DPAD_UP", adb: "KEYCODE_DPAD_UP", code: 19 },
  { kernel: "DPAD_DOWN", adb: "KEYCODE_DPAD_DOWN", code: 20 },
  { kernel: "DPAD_LEFT", adb: "KEYCODE_DPAD_LEFT", code: 21 },
  { kernel: "DPAD_RIGHT", adb: "KEYCODE_DPAD_RIGHT", code: 22 },
  { kernel: "DPAD_CENTER", adb: "KEYCODE_DPAD_CENTER", code: 23 },
  { kernel: "Back", adb: "KEYCODE_BACK", code: 4 },
]);

const adbOf = (key) => {
  const hit = KEYMAP.find((m) => m.kernel === key);
  if (hit === undefined) throw new TypeError(`device-walks: 键未入表 ${key}`);
  return `shell input keyevent ${hit.adb}`;
};

/** 每条走查的前置：重启宿主 app 回到初始焦点（包名 = android/app/build.gradle 的 applicationId）。 */
const FRESH_APP = Object.freeze([
  "shell am force-stop com.polypack.host",
  "shell am start -n com.polypack.host/.MainActivity",
]);

/** 走查序列；keys 为内核键名，clicks 为各键后 `点击计数：N` 的期望值。 */
export const WALKS = Object.freeze([
  {
    id: "traverse-cycle",
    purpose: "方向键遍历：DOWN / UP 在环内往返",
    keys: ["DPAD_DOWN", "DPAD_UP"],
    clicks: [0, 0],
  },
  {
    id: "boundary-hold",
    purpose: "边界自环：无邻接方向原地不动（无死路，焦点不丢）",
    keys: ["DPAD_UP", "DPAD_DOWN", "DPAD_DOWN"],
    clicks: [0, 0, 0],
  },
  {
    id: "lateral-hold",
    purpose: "未定义方向（左右）不外溢，焦点原地",
    keys: ["DPAD_LEFT", "DPAD_RIGHT"],
    clicks: [0, 0],
  },
  {
    id: "activate-counter",
    purpose: "中心键激活当前焦点（计数 +1）",
    keys: ["DPAD_CENTER"],
    clicks: [1],
  },
  {
    id: "reset-then-back",
    purpose: "移焦到重置 → 激活清零 → Back 回上一焦点 → 再激活（计数 +1）",
    keys: ["DPAD_DOWN", "DPAD_CENTER", "Back", "DPAD_CENTER"],
    clicks: [0, 0, 0, 1],
  },
  {
    id: "unknown-key-ignored",
    purpose: "未映射键被忽略（焦点不动、不崩）",
    keys: ["KeyQ"],
    clicks: [0],
    adb: ["shell input keyevent KEYCODE_Q"], // 未入内核键表，命令单列
  },
]);

/**
 * 生成对拍数据（确定性；`node device-walks/emit-expectations.mjs --write` 刷新）。
 * 每条走查独立起引擎：期望值以「初始状态」为前置（设备链每条前重启 app，见 README），
 * 因此走查可任意取子集、任意顺序执行。
 */
export function buildExpectations() {
  const fresh = () => createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  return {
    schema: "polypack/adapter-native/device-walks/v1",
    target: "android",
    derivedFrom: { kernel: "src/focus.js", nodes: "src/example-nodes.js" },
    precondition: { focus: fresh().current, clicks: 0, reset: FRESH_APP },
    keymap: KEYMAP,
    walks: WALKS.map((walk) => {
      const engine = fresh();
      const results = [];
      const focus = [];
      for (const key of walk.keys) {
        results.push(handleKey(engine, key));
        focus.push(engine.current);
      }
      return {
        id: walk.id,
        purpose: walk.purpose,
        keys: walk.keys,
        adb: walk.adb ?? walk.keys.map(adbOf),
        expect: { results, focus, clicks: walk.clicks },
      };
    }),
  };
}
