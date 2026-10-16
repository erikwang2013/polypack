// T-M4-03 契约测试：Back 三层语义（层 → 焦点记忆 → 退出应用）、层域隔离、
// 逐层关闭恢复原焦点（验收：子页/弹层逐层 Back 恢复原焦点）、BACK_KEYS 哨兵。
// 零依赖（node:test）；只消费 src/（不 import react 的示例组件面）。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BACK_KEYS,
  createFocusEngine,
  createLayerStack,
  createTvFocus,
  handleKey,
  handleTvKey,
} from "../src/index.js";
import {
  EXAMPLE_FOCUS_NODES,
  TV_MODAL_FOCUS_NODES,
  TV_PLAYER_FOCUS_NODES,
} from "../src/example-nodes.js";

const setup = () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  const layers = createLayerStack();
  return { engine, layers, session: { engine, layers } };
};

test("Back 三层语义（验收）：层打开 → 关层；无层 → 焦点记忆；历史空 → 退出应用", () => {
  const { engine, layers, session } = setup();

  // 第一层：层打开 → 关层，宿主焦点结构性恢复（宿主引擎全程未被触碰）
  layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
  assert.deepEqual(handleTvKey(session, "ArrowRight"), {
    type: "move",
    direction: "right",
    from: "modal-play",
    to: "modal-close",
  });
  assert.equal(engine.current, "counter", "层内导航不触碰宿主域焦点");
  assert.deepEqual(handleTvKey(session, "Back"), {
    type: "back",
    route: "layer",
    layer: "detail",
    from: "modal-close",
    to: "counter",
  });
  assert.equal(layers.depth, 0);

  // 第二层：无层 → 焦点记忆回退（Escape 与 Back 同口径）
  engine.focus("detail");
  assert.deepEqual(handleTvKey(session, "Escape"), {
    type: "back",
    route: "memory",
    from: "detail",
    to: "counter",
  });

  // 第三层：历史空 → 退出应用（宿主裁决；内核焦点不动）
  assert.deepEqual(handleTvKey(session, "Back"), {
    type: "back",
    route: "exit",
    from: "counter",
    to: null,
  });
  assert.equal(engine.current, "counter");
});

test("逐层（嵌套层）：Back 先关内层再关外层，各域焦点原样保留", () => {
  const { engine, layers, session } = setup();
  engine.focus("reset"); // 历史 [counter]
  layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
  layers.open({ id: "player", nodes: TV_PLAYER_FOCUS_NODES, initial: "player-stop" });
  assert.deepEqual(layers.ids, ["detail", "player"], "自底向顶层 id");
  assert.equal(layers.depth, 2);

  assert.deepEqual(handleTvKey(session, "Back"), {
    type: "back",
    route: "layer",
    layer: "player",
    from: "player-stop",
    to: "modal-play",
  });
  assert.deepEqual(handleTvKey(session, "Back"), {
    type: "back",
    route: "layer",
    layer: "detail",
    from: "modal-play",
    to: "reset",
  });
  assert.equal(layers.depth, 0);
  assert.equal(engine.current, "reset", "宿主域焦点原样（逐层恢复原焦点）");

  // 层全部关闭后，宿主域 Back 回到焦点记忆语义
  assert.deepEqual(handleTvKey(session, "Back"), {
    type: "back",
    route: "memory",
    from: "reset",
    to: "counter",
  });
});

test("退化与域隔离：无 layers 仍三层路由（跳过层段）；层内激活走层引擎", () => {
  // 无 layers / shortcuts 配置：Back 走焦点记忆 → 退出两段
  const bare = { engine: createTvFocus({ nodes: EXAMPLE_FOCUS_NODES }) };
  assert.deepEqual(handleTvKey(bare, "DPAD_DOWN"), {
    type: "move",
    direction: "down",
    from: "counter",
    to: "detail",
  });
  assert.deepEqual(handleTvKey(bare, "Back"), {
    type: "back",
    route: "memory",
    from: "detail",
    to: "counter",
  });
  assert.deepEqual(handleTvKey(bare, "Back"), { type: "back", route: "exit", from: "counter", to: null });

  // 层内激活：回调收到层节点 id；宿主焦点不动
  const { engine, layers } = setup();
  layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
  let activated = null;
  assert.deepEqual(
    handleTvKey({ engine, layers }, "DPAD_CENTER", { activate: (id) => (activated = id) }),
    { type: "activate", id: "modal-play" },
  );
  assert.equal(activated, "modal-play");
  assert.equal(engine.current, "counter", "层内激活不触碰宿主域焦点");
});

test("校验：session / 层栈边界快速失败", () => {
  assert.throws(() => handleTvKey(), /session\.engine/);
  assert.throws(() => handleTvKey({}, "Back"), /session\.engine/);

  const layers = createLayerStack();
  assert.equal(layers.current, null);
  assert.equal(layers.close(), null, "空栈关闭返回 null");
  assert.throws(() => layers.open(null), /非空 id/);
  assert.throws(() => layers.open({ nodes: TV_MODAL_FOCUS_NODES }), /非空 id/);
  assert.throws(() => layers.open({ id: "x", nodes: [] }), /非空数组/);
  layers.open({ id: "m", nodes: TV_MODAL_FOCUS_NODES });
  assert.throws(() => layers.open({ id: "m", nodes: TV_MODAL_FOCUS_NODES }), /重复/);
  assert.deepEqual(layers.close(), { id: "m", focus: "modal-play" }, "关闭回报层 id 与层内末焦点");
});

test("BACK_KEYS 哨兵：TV 表与共享内核键口径双向对拍（探测面有限，同 builtin.rs 哨兵款）", () => {
  const model = createFocusEngine({ nodes: [{ id: "a" }, { id: "b" }] });
  const probe = [
    "Back", "Escape", "Enter", " ", "DPAD_CENTER",
    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
    "DPAD_UP", "DPAD_DOWN", "DPAD_LEFT", "DPAD_RIGHT",
    "Home", "Backspace", "GoBack", "ColorF0Red", "MediaPlayPause",
    "0", "1", "9", "VolumeUp",
  ];
  for (const key of probe) {
    const inKernel = handleKey(model, key).type === "back";
    assert.equal(
      BACK_KEYS.includes(key),
      inKernel,
      `TV BACK_KEYS 与内核口径不一致: ${key}（内核判 back=${inKernel}）`,
    );
  }
});
