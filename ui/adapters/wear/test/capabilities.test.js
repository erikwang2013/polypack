// T-M7-01 能力矩阵对照测试：矩阵形态、宿主提示覆盖（无指针 → focusNav）、降级分支闭合集
// + poly-targets 哨兵。哨兵手法与 adapter-native / adapter-tv 同源：JS 侧不依赖 cargo，
// 正则解析 crates/poly-targets/src/builtin.rs 单源文件；Wear 目标登记（T-M7-02）落位前
// 该哨兵为空集通过，落位后自动收口（id 须与本矩阵一致）。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  CAPABILITY_MATRIX,
  DEGRADATION_IDS,
  resolveCapabilities,
  resolveDegradations,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BUILTIN_RS = path.resolve(HERE, "../../../../crates/poly-targets/src/builtin.rs");

/** 解析内建 TargetSpec 块 → [{ id, ui }]（与 adapter-native 同一解析手法）。 */
function parseBuiltinTargets() {
  const text = fs.readFileSync(BUILTIN_RS, "utf8");
  return text
    .split("TargetSpec {")
    .slice(1)
    .map((block) => ({
      id: block.match(/id: "([a-z0-9-]+)"\.to_string\(\)/)?.[1],
      ui: [...(block.match(/ui: vec!\[([^\]]*)\]/)?.[1] ?? "").matchAll(/"([^"]+)"/g)].map(
        (m) => m[1],
      ),
    }));
}

const builtinTargets = parseBuiltinTargets();

test("poly-targets 哨兵：内建 Wear 目标（id 以 wear 开头）⊆ 本矩阵（T-M7-02 落位后自动收口）", () => {
  assert.ok(builtinTargets.length >= 7, `内建目标解析异常（${builtinTargets.length} 个）`);
  const wearTargets = builtinTargets.filter(
    (t) => typeof t.id === "string" && t.id.startsWith("wear"),
  );
  for (const t of wearTargets) {
    assert.ok(
      Object.hasOwn(CAPABILITY_MATRIX, t.id),
      `内建目标 "${t.id}" 是 Wear 目标，但 adapter-wear 矩阵缺行——请补 capabilities.js`,
    );
  }
});

test("矩阵形态：wearos 基线 = 触摸常态（focusNav false / safeArea true / gestures true）", () => {
  assert.deepEqual(Object.keys(CAPABILITY_MATRIX), ["wearos"]);
  assert.deepEqual(CAPABILITY_MATRIX.wearos, {
    focusNav: false,
    safeArea: true,
    gestures: true,
  });
});

test("capabilities()：宿主提示覆盖（pointer=none → focusNav 置真；显式提示优先）", () => {
  assert.deepEqual(resolveCapabilities("wearos"), {
    focusNav: false,
    safeArea: true,
    gestures: true,
  });
  // ROADMAP §6「无指针 → 焦点环」：无指针设备 / 表冠方向键走查启用焦点环
  assert.equal(resolveCapabilities("wearos", { pointer: "none" }).focusNav, true);
  assert.equal(resolveCapabilities("wearos", { pointer: "touch" }).focusNav, false);
  assert.equal(resolveCapabilities("wearos", { pointer: "mouse" }).focusNav, false);
  assert.deepEqual(resolveCapabilities("wearos", { focusNav: true, gestures: false }), {
    focusNav: true,
    safeArea: true,
    gestures: false,
  });
  // 显式提示可覆盖基线（无指针调试形态下仍可关闭焦点环）
  assert.equal(resolveCapabilities("wearos", { pointer: "none", focusNav: false }).focusNav, false);
  assert.throws(() => resolveCapabilities("web"), /未知目标/);
});

test("能力提示边界校验：非法类型 / 非法 pointer / 未知目标快速失败", () => {
  assert.throws(() => resolveCapabilities("wearos", { focusNav: "yes" }), /布尔值/);
  assert.throws(() => resolveCapabilities("wearos", { pointer: "stylus" }), /pointer/);
  assert.throws(() => resolveCapabilities("nope"), /未知目标/);
});

test("degradations()：分支集合闭合可枚举；缺失能力按契约激活对应分支", () => {
  const ids = (caps) => resolveDegradations(caps).map((branch) => branch.id);
  assert.deepEqual(ids(resolveCapabilities("wearos")), [...DEGRADATION_IDS]);

  const active = (caps) => resolveDegradations(caps).filter((b) => b.active).map((b) => b.id);
  // 触摸基线：只需安全区
  assert.deepEqual(active(resolveCapabilities("wearos")), ["safe-area-padding"]);
  // 无指针 → 焦点环分支（复用共享内核）
  assert.deepEqual(active(resolveCapabilities("wearos", { pointer: "none" })), [
    "focus-ring",
    "safe-area-padding",
  ]);
  // 无返回手势 → 显式返回入口
  assert.deepEqual(active(resolveCapabilities("wearos", { gestures: false })), [
    "safe-area-padding",
    "back-affordance",
  ]);
  // 三项全缺 → 三条分支全开
  assert.deepEqual(
    active(resolveCapabilities("wearos", { pointer: "none", gestures: false })),
    ["focus-ring", "safe-area-padding", "back-affordance"],
  );
  // 安全区调试形态 → 该分支失活
  assert.deepEqual(active(resolveCapabilities("wearos", { safeArea: false })), []);

  for (const branch of resolveDegradations(resolveCapabilities("wearos"))) {
    assert.equal(typeof branch.reason, "string");
    assert.ok(Object.isFrozen(branch));
  }
});
