// Roku 键映射与共享焦点内核的接口核对：字面表 + 行为断言（handleKey 结果类型）。
// 行为断言 = 真正的口径核验（内核经 adapter-tv 再导出，已在 main）；字面表防手滑改名。

import assert from "node:assert/strict";
import { test } from "node:test";

import { EXAMPLE_FOCUS_NODES } from "../../tv/src/example-nodes.js";
import { createFocusEngine, handleKey } from "../../tv/src/focus.js";
import { ROKU_KEY_MAP, mapRokuKey } from "../src/roku-keys.js";

const engine = () => createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });

test("字面映射：方向 / OK(select) / Back；未列键放行", () => {
  assert.equal(mapRokuKey("up"), "DPAD_UP");
  assert.equal(mapRokuKey("down"), "DPAD_DOWN");
  assert.equal(mapRokuKey("left"), "DPAD_LEFT");
  assert.equal(mapRokuKey("right"), "DPAD_RIGHT");
  assert.equal(mapRokuKey("OK"), "DPAD_CENTER");
  assert.equal(mapRokuKey("select"), "DPAD_CENTER");
  assert.equal(mapRokuKey("back"), "Back");
  assert.equal(mapRokuKey("info"), null);
  assert.equal(mapRokuKey("1"), null);
  assert.equal(ROKU_KEY_MAP.select, ROKU_KEY_MAP.OK);
});

test("行为核验：映射后的内核键名经 handleKey 路由为 move / activate / back", () => {
  const e = engine();
  assert.deepEqual(handleKey(e, mapRokuKey("down")), { type: "move", direction: "down", from: "counter", to: "reset" });
  assert.deepEqual(handleKey(e, mapRokuKey("OK")), { type: "activate", id: "reset" });
  const back = handleKey(e, mapRokuKey("back"));
  assert.equal(back.type, "back");
  assert.equal(back.to, "counter"); // 焦点记忆：down 前在 counter
  assert.deepEqual(handleKey(e, mapRokuKey("up")), { type: "move", direction: "up", from: "counter", to: "counter" }); // 边界自环
});
