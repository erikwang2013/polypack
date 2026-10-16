// T-M2-04 能力矩阵对照测试：adapter-native 的 capabilities 与 poly-targets 内建声明
// （crates/poly-targets/src/builtin.rs，单一事实源）不得漂移。
// JS 侧不依赖 cargo（CI 三宿主的 JS 步骤不构建 Rust），故以正则解析该 Rust 单源文件做哨兵。

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
const BUILTIN_RS = path.resolve(HERE, "../../../../crates/poly-targets/src/builtin.rs");

/** 解析内建 TargetSpec 块 → [{ id, ui }]（正则够用：单源文件结构由 Rust 侧测试保证）。 */
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

test("矩阵与内建目标声明对照：矩阵每行都有对应 TargetSpec", () => {
  const ids = builtinTargets.map((t) => t.id);
  assert.ok(ids.length >= 7, `内建目标解析异常（${ids.length} 个）`);
  for (const id of Object.keys(CAPABILITY_MATRIX)) {
    assert.ok(ids.includes(id), `矩阵目标 "${id}" 不在 poly-targets 内建声明中`);
  }
});

test("RN 承载目标（ui 以 react-native 开头，含 RNOH）全部在矩阵内", () => {
  const rnTargets = builtinTargets.filter((t) => t.ui.some((name) => name.startsWith("react-native")));
  assert.ok(rnTargets.length >= 1, "内建声明应至少有一个 react-native 承载目标");
  for (const t of rnTargets) {
    assert.ok(
      Object.hasOwn(CAPABILITY_MATRIX, t.id),
      `内建目标 "${t.id}" 声明了 react-native 承载，但 adapter-native 矩阵缺行——请补 capabilities.js`,
    );
  }
});

test("capabilities()：目标基线 + 宿主提示覆盖（无指针 → focusNav）", () => {
  assert.deepEqual(resolveCapabilities("android"), {
    focusNav: false,
    safeArea: true,
    gestures: true,
  });
  assert.deepEqual(resolveCapabilities("harmonyos"), {
    focusNav: false,
    safeArea: true,
    gestures: true,
  });
  // react-dom 族（web / 桌面）是 adapter-dom 的职责：经本包询问即快速失败
  assert.throws(() => resolveCapabilities("web"), /未知目标/);
  // 无指针（TV / 车机 / 无指针调试）→ 焦点导航必需，其余键保持目标基线
  assert.deepEqual(resolveCapabilities("android", { pointer: "none" }), {
    focusNav: true,
    safeArea: true,
    gestures: true,
  });
  // 显式提示优先于 pointer 推导
  assert.deepEqual(resolveCapabilities("android", { pointer: "none", focusNav: false }), {
    focusNav: false,
    safeArea: true,
    gestures: true,
  });
  // 触屏指针不翻转基线
  assert.equal(resolveCapabilities("android", { pointer: "touch" }).focusNav, false);
});

test("T-M3-02 鸿蒙承载：宿主提示覆盖与降级驱动与 Android 同构（同一页面树两宿主同语义）", () => {
  // 外接键盘/无指针调试形态（RNOH 走查形态）：pointer none → focusNav，其余键保持基线
  assert.deepEqual(resolveCapabilities("harmonyos", { pointer: "none" }), {
    focusNav: true,
    safeArea: true,
    gestures: true,
  });
  assert.deepEqual(
    resolveCapabilities("harmonyos", { pointer: "none" }),
    resolveCapabilities("android", { pointer: "none" }),
    "两 RN 承载目标对同一宿主提示应给出同构能力（页面降级分支不分叉）",
  );
  const branches = resolveDegradations(resolveCapabilities("harmonyos", { pointer: "none" }));
  assert.deepEqual(branches.filter((b) => b.active).map((b) => b.id), ["focus-ring"]);
  // 触屏基线不翻转；显式提示仍优先于 pointer 推导
  assert.equal(resolveCapabilities("harmonyos", { pointer: "touch" }).focusNav, false);
  assert.equal(resolveCapabilities("harmonyos", { pointer: "none", focusNav: false }).focusNav, false);
});

test("能力提示边界校验：非法类型 / 非法 pointer 快速失败", () => {
  assert.throws(() => resolveCapabilities("android", { focusNav: "yes" }), /布尔值/);
  assert.throws(() => resolveCapabilities("android", { pointer: "stylus" }), /pointer/);
  assert.throws(() => resolveCapabilities("nope"), /未知目标/);
});

test("degradations()：分支集合闭合可枚举，生效项由 capabilities 驱动", () => {
  const touch = resolveDegradations(resolveCapabilities("android"));
  assert.deepEqual(
    touch.map((b) => b.id),
    [...DEGRADATION_IDS],
    "分支 id 集即闭合全集",
  );
  assert.deepEqual(
    touch.filter((b) => b.active).map((b) => b.id),
    [],
    "触屏 Android：有指针、有系统返回手势、非小屏 → 无降级分支生效",
  );

  const tvLike = resolveDegradations(resolveCapabilities("android", { pointer: "none" }), {
    compact: true,
  });
  assert.deepEqual(
    tvLike.filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "compact-spacing"],
    "无指针 + 小屏（仍有系统手势）：焦点环与紧凑间距生效",
  );

  const tvRemote = resolveDegradations(
    resolveCapabilities("android", { pointer: "none", gestures: false }),
  );
  assert.deepEqual(
    tvRemote.filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "back-affordance"],
    "无指针 + 无系统手势（遥控器形态）：焦点环与显式返回入口生效",
  );
  for (const branch of tvLike) {
    assert.equal(typeof branch.reason, "string");
    assert.ok(Object.isFrozen(branch));
  }
});

test("确定性：同一输入两次求值结果相等（快照稳定）", () => {
  const adapter = createAdapter({
    target: "android",
    runtime: { AppRegistry: { registerComponent() {} } },
    host: { pointer: "none" },
  });
  assert.deepEqual(adapter.capabilities(), adapter.capabilities());
  assert.deepEqual(adapter.degradations(), adapter.degradations());
});
