// T-M4-01 能力矩阵对照测试：矩阵形态、宿主提示覆盖、降级分支闭合集 + polypack-targets 哨兵。
// 哨兵手法与 adapter-native 同源：JS 侧不依赖 cargo，正则解析 crates/polypack-targets/src/builtin.rs
// 单源文件；TV 目标登记（T-M4-04/08/09/10）落位前该哨兵为空集通过，落位后自动收口。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  CAPABILITY_MATRIX,
  DEGRADATION_IDS,
  createAdapter,
  resolveCapabilities,
  resolveDegradations,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BUILTIN_RS = path.resolve(HERE, "../../../../crates/polypack-targets/src/builtin.rs");

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

test("polypack-targets 哨兵：内建 L3 电视目标（*-tv）⊆ 本矩阵（登记落位后自动收口）", () => {
  assert.ok(builtinTargets.length >= 7, `内建目标解析异常（${builtinTargets.length} 个）`);
  // L3 七枚口径（lead 定案）：android-tv / fire-tv / harmonyos-tv / tizen-tv / webos-tv /
  // vidaa-tv / smartcast-tv——凡内建目标 id 以 -tv 结尾者必在本矩阵（tvos 不带后缀，不在本哨兵面）。
  const tvTargets = builtinTargets.filter(
    (t) => typeof t.id === "string" && t.id.endsWith("-tv"),
  );
  for (const t of tvTargets) {
    assert.ok(
      Object.hasOwn(CAPABILITY_MATRIX, t.id),
      `内建目标 "${t.id}" 是 L3 电视目标，但 adapter-tv 矩阵缺行——请补 capabilities.js`,
    );
  }
});

test("矩阵形态：电视基线逐目标一致（focusNav / safeArea 恒真、gestures 恒假）", () => {
  for (const [id, row] of Object.entries(CAPABILITY_MATRIX)) {
    assert.deepEqual(row, { focusNav: true, safeArea: true, gestures: false }, id);
  }
});

test("capabilities()：宿主提示覆盖（pointer 不翻转 TV 焦点环；显式提示优先）", () => {
  assert.deepEqual(resolveCapabilities("tizen-tv", { pointer: "none" }), {
    focusNav: true,
    safeArea: true,
    gestures: false,
  });
  // Magic Remote 等指针型遥控器：不取消焦点环（与 RN 侧推导方向相反，见模块头注）
  assert.equal(resolveCapabilities("tizen-tv", { pointer: "mouse" }).focusNav, true);
  assert.equal(resolveCapabilities("tizen-tv", { pointer: "touch" }).focusNav, true);
  // 显式提示仍可覆盖（如无电视安全区的调试形态）
  assert.deepEqual(resolveCapabilities("tizen-tv", { focusNav: false, safeArea: false }), {
    focusNav: false,
    safeArea: false,
    gestures: false,
  });
  // react-dom 族非电视目标是 adapter-dom 的职责：经本包询问即快速失败
  assert.throws(() => resolveCapabilities("web"), /未知目标/);
});

test("能力提示边界校验：非法类型 / 非法 pointer / 未知目标快速失败", () => {
  assert.throws(() => resolveCapabilities("tizen-tv", { focusNav: "yes" }), /布尔值/);
  assert.throws(() => resolveCapabilities("tizen-tv", { pointer: "stylus" }), /pointer/);
  assert.throws(() => resolveCapabilities("nope"), /未知目标/);
});

test("degradations()：分支集合闭合可枚举，三条分支在 TV 基线下全生效", () => {
  const branches = resolveDegradations(resolveCapabilities("tizen-tv"));
  assert.deepEqual(
    branches.map((b) => b.id),
    [...DEGRADATION_IDS],
    "分支 id 集即闭合全集",
  );
  assert.deepEqual(
    branches.filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "safe-area-padding", "back-affordance"],
    "TV 基线：焦点环 + 安全区 + 遥控器 Back 入口全部生效",
  );
  for (const branch of branches) {
    assert.equal(typeof branch.reason, "string");
    assert.ok(Object.isFrozen(branch));
  }
  // 显式关闭安全区（调试形态）→ 该分支失活
  const noSafeArea = resolveDegradations(resolveCapabilities("tizen-tv", { safeArea: false }));
  assert.deepEqual(
    noSafeArea.filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "back-affordance"],
  );
});

test("确定性：同一输入两次求值结果相等（快照稳定）", () => {
  const adapter = createAdapter({
    target: "webos-tv",
    runtime: { React: { createElement: () => null }, ReactDOM: { createRoot: () => ({ render() {} }) } },
  });
  assert.deepEqual(adapter.capabilities(), adapter.capabilities());
  assert.deepEqual(adapter.degradations(), adapter.degradations());
});
