// T-M3-05 能力矩阵对照测试：adapter-taro 的 capabilities 与 poly-targets 内建声明
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
  TARO_PLATFORMS,
  createAdapter,
  resolveCapabilities,
  resolveDegradations,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BUILTIN_RS = path.resolve(HERE, "../../../../crates/poly-targets/src/builtin.rs");

/** 解析内建 TargetSpec 块 → [{ id, ui, format }]（正则够用：单源文件结构由 Rust 侧测试保证）。 */
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
      format: block.match(/format: "([a-z0-9-]+)"\.to_string\(\)/)?.[1],
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

test("Taro 承载目标（ui 含 taro）全部在矩阵内且平台映射齐备", () => {
  const taroTargets = builtinTargets.filter((t) => t.ui.includes("taro"));
  assert.ok(taroTargets.length >= 4, `内建声明应有 Taro 四目标（当前 ${taroTargets.length}）`);
  for (const t of taroTargets) {
    assert.ok(
      Object.hasOwn(CAPABILITY_MATRIX, t.id),
      `内建目标 "${t.id}" 声明了 Taro 承载，但 adapter-taro 矩阵缺行——请补 capabilities.js`,
    );
    assert.ok(
      Object.hasOwn(TARO_PLATFORMS, t.id),
      `内建目标 "${t.id}" 缺 Taro 编译平台映射（taro build --type 值）`,
    );
  }
});

test("capabilities()：小程序族基线 + 宿主提示覆盖（无指针 → focusNav）", () => {
  for (const id of Object.keys(CAPABILITY_MATRIX)) {
    assert.deepEqual(
      resolveCapabilities(id),
      { focusNav: false, safeArea: true, gestures: true },
      `${id} 基线：触屏 / 有安全区 / 有系统返回手势`,
    );
  }
  // RN / react-dom 族是别的适配器的职责：经本包询问即快速失败
  assert.throws(() => resolveCapabilities("android"), /未知目标/);
  assert.throws(() => resolveCapabilities("web"), /未知目标/);
  // 无指针容器（车机 / 带遥控器的宿主）→ 焦点导航必需，其余键保持基线
  assert.deepEqual(resolveCapabilities("wechat", { pointer: "none" }), {
    focusNav: true,
    safeArea: true,
    gestures: true,
  });
  assert.equal(resolveCapabilities("wechat", { pointer: "touch" }).focusNav, false);
});

test("能力提示边界校验：非法类型 / 非法 pointer 快速失败", () => {
  assert.throws(() => resolveCapabilities("wechat", { focusNav: "yes" }), /布尔值/);
  assert.throws(() => resolveCapabilities("wechat", { pointer: "stylus" }), /pointer/);
  assert.throws(() => resolveCapabilities("nope"), /未知目标/);
});

test("degradations()：分支集合闭合可枚举；沙箱恒真两条 + 宿主驱动两条", () => {
  const touch = resolveDegradations(resolveCapabilities("wechat"));
  assert.deepEqual(
    touch.map((b) => b.id),
    [...DEGRADATION_IDS],
    "分支 id 集即闭合全集",
  );
  assert.deepEqual(
    touch.filter((b) => b.active).map((b) => b.id),
    ["no-direct-dom", "lifecycle-mapping"],
    "触屏微信基线：无 DOM 直连与生命周期映射恒真；有系统手势 + 非小屏 → 无其余分支",
  );

  const compactNoGesture = resolveDegradations(
    resolveCapabilities("alipay", { gestures: false }),
    { compact: true },
  );
  assert.deepEqual(
    compactNoGesture.filter((b) => b.active).map((b) => b.id),
    ["no-direct-dom", "lifecycle-mapping", "compact-spacing", "back-affordance"],
    "小屏 + 无系统返回手势：四条分支全开",
  );
  for (const branch of touch) {
    assert.equal(typeof branch.reason, "string");
    assert.ok(Object.isFrozen(branch));
  }
});

test("确定性：同一输入两次求值结果相等（快照稳定）", () => {
  const adapter = createAdapter({
    target: "douyin",
    runtime: { config: () => ({ pages: ["pages/index/index"] }) },
    host: { pointer: "none" },
  });
  assert.deepEqual(adapter.capabilities(), adapter.capabilities());
  assert.deepEqual(adapter.degradations(), adapter.degradations());
});
