// T-M4-13 TV 冒烟场景层：两档应用 profile 的「键序列 → 期望」现场推导（不手写期望值）。
//
// - native-host 档（Android TV 落地宿主）：期望直接消费 device-walks/walks.mjs 的
//   buildExpectations()（T-M4-05 真跑同伴——同一函数，不读手写值）。
// - tv-example 档（@polypack/adapter-tv 示例页，web 系全导航图）：期望由 TV 焦点内核
//   现场推导——createTvFocus + handleTvKey + 层栈/数字键（react-free 路径）+ 示例页
//   press 语义镜像；镜像与真页等价由 react 在场时的履约哨兵（test/）逐键证明。
//
// 键名统一用内核口径（DPAD_UP/DOWN/LEFT/RIGHT、DPAD_CENTER、Back、数字 "1".."3"）；
// 各驱动把内核键翻成自身通道（adb keyevent / DOM KeyboardEvent），翻表在 drivers/。

import { createTvFocus, handleTvKey } from "../../adapters/tv/src/focus.js";
import { createLayerStack } from "../../adapters/tv/src/layers.js";
import { createShortcutMap } from "../../adapters/tv/src/shortcuts.js";
import {
  EXAMPLE_FOCUS_NODES,
  TV_MODAL_FOCUS_NODES,
  TV_PLAYER_FOCUS_NODES,
} from "../../adapters/tv/src/example-nodes.js";

/** tv-example 档：示例页 press 语义镜像（对照 src/example.js 的 press；履约哨兵守漂移）。 */
export function createTvExampleSession() {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  const layers = createLayerStack();
  const shortcuts = createShortcutMap({ 1: "counter", 2: "reset", 3: "detail" });
  const state = { clicks: 0, exitRequested: false };
  const press = (id) => {
    if (id === "counter") state.clicks += 1;
    else if (id === "reset") state.clicks = 0;
    else if (id === "detail") {
      layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
    } else if (id === "modal-play") {
      layers.open({ id: "player", nodes: TV_PLAYER_FOCUS_NODES, initial: "player-stop" });
    } else if (id === "modal-close" || id === "player-stop") {
      layers.close();
    }
    return { type: "press", id };
  };
  const onKey = (key) => {
    const result = handleTvKey({ engine, layers, shortcuts }, key, { activate: press });
    if (result.type === "back" && result.route === "exit") state.exitRequested = true;
    return result;
  };
  return {
    onKey,
    getState: () => ({
      focus: (layers.current ?? engine).current,
      clicks: state.clicks,
      depth: layers.depth,
      exitRequested: state.exitRequested,
    }),
  };
}

/** tv-example 档场景：核心任务全导航图（浏览 → 详情 → 播放占位 → 返回）+ 数字键 + 负例。 */
export const TV_EXAMPLE_SCENES = Object.freeze([
  {
    id: "core-task",
    purpose: "核心任务：浏览 → 详情 → 播放占位 → 返回（逐层 Back + 焦点记忆）",
    keys: [
      "DPAD_RIGHT", // 浏览：counter → reset
      "DPAD_LEFT", // reset → counter
      "DPAD_DOWN", // counter → detail
      "DPAD_RIGHT", // detail → exit（浏览覆盖退出节点）
      "DPAD_LEFT", // exit → detail
      "DPAD_CENTER", // OK：详情弹层打开（焦点 modal-play）
      "DPAD_CENTER", // OK：播放器子层打开（焦点 player-stop，播放占位）
      "Back", // 关播放器子层 → modal-play
      "Back", // 关详情弹层 → detail
      "Back", // 焦点记忆 → counter
    ],
  },
  {
    id: "digit-shortcuts",
    purpose: "数字键直达（宿主域）与层内拦截（blocked，不穿透）",
    keys: ["Digit3", "DPAD_CENTER", "Digit1", "Back", "Numpad2", "1", "DPAD_CENTER"],
    // Digit3 → detail（宿主域直达）；OK 开详情层；Digit1 层内 blocked（不穿透，焦点不动）；
    // Back 关层回 detail；Numpad2 → reset（小键盘族）；"1" → counter（主键区族）；
    // OK 激活计数（clicks=1——宿主视图同步的正信号）。
  },
  {
    id: "edge-holds",
    purpose: "边界自环与单节点域自环（无死路：期望移动而原地即移动）",
    keys: ["DPAD_UP", "DPAD_LEFT", "DPAD_DOWN", "DPAD_CENTER", "DPAD_UP", "DPAD_LEFT", "DPAD_RIGHT", "DPAD_LEFT"],
    // counter 上/左自环；DOWN 到 detail；OK 开层；modal-play 上/左自环；
    // RIGHT/LEFT：层内两节点往返（modal-play ↔ modal-close，覆盖层内横向导航）。
  },
  {
    id: "exit-route",
    purpose: "空历史 Back 的退出裁决（route=exit + 退出锚点文案）",
    keys: ["Back", "Back", "Back"],
    // 首两 Back 无层无记忆 → exit；（幂等重复仍 exit，不退焦点）
  },
]);

/** 现场推导：重放场景键序列，得逐键描述符 / 焦点 / 状态（期望值唯一来源）。 */
export function deriveScene(scene, factory = createTvExampleSession) {
  const session = factory();
  const initial = session.getState(); // 注键前基线（对拍「期望移动而实际未动」需要）
  const results = [];
  const focus = [];
  const state = [];
  for (const key of scene.keys) {
    results.push(session.onKey(key));
    const snap = session.getState();
    focus.push(snap.focus);
    state.push(snap);
  }
  return { ...scene, initial, results, focus, state };
}

/** tv-example 档全量推导（emit-expectations / 驱动对拍共用）。 */
export function deriveTvExample() {
  return {
    schema: "polypack/tv-smoke/v1",
    profile: "tv-example",
    derivedFrom: {
      kernel: "ui/adapters/tv/src/focus.js",
      layers: "ui/adapters/tv/src/layers.js",
      shortcuts: "ui/adapters/tv/src/shortcuts.js",
      nodes: "ui/adapters/tv/src/example-nodes.js",
    },
    scenes: TV_EXAMPLE_SCENES.map((scene) => deriveScene(scene)),
  };
}

/** native-host 档：期望消费 device-walks 的现场推导（Android 腿）。 */
export async function deriveNativeHost() {
  const { buildExpectations } = await import(
    "../../adapters/native/device-walks/walks.mjs"
  );
  const expectations = buildExpectations();
  return {
    schema: "polypack/tv-smoke/v1",
    profile: "native-host",
    derivedFrom: expectations.derivedFrom,
    precondition: expectations.precondition,
    keymap: expectations.keymap,
    scenes: expectations.walks.map((walk) => ({
      id: walk.id,
      purpose: walk.purpose,
      keys: walk.keys,
      adb: walk.adb,
      results: walk.expect.results,
      focus: walk.expect.focus,
      clicks: walk.expect.clicks,
    })),
  };
}
