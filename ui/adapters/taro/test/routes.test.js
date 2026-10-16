// T-M3-05 生命周期与路由映射断言：闭合生命周期全集、React 语义映射表、声明式路由校验。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  LIFECYCLE_EVENTS,
  LIFECYCLE_MAP,
  ROUTE_METHODS,
  createPageLifecycle,
  normalizePath,
} from "../src/index.js";

test("生命周期映射表：事件全集闭合，映射值一一对应（无缺行、无孤儿）", () => {
  assert.deepEqual(
    Object.keys(LIFECYCLE_MAP).sort(),
    [...LIFECYCLE_EVENTS].sort(),
    "映射表与事件全集双向一致",
  );
  const values = Object.values(LIFECYCLE_MAP);
  assert.equal(new Set(values).size, values.length, "React 侧语义不得重名");
  // Taro 页面的存在性由编译器保证；此处固化「哪些事件被承认」的闭合面
  for (const event of ["onLoad", "onShow", "onHide", "onUnload"]) {
    assert.ok(LIFECYCLE_EVENTS.includes(event), `缺页面生命周期 ${event}`);
  }
});

test("createPageLifecycle：键集校验（闭集内）与处理器类型校验", () => {
  const seen = [];
  const lifecycle = createPageLifecycle({
    onLoad: (options) => seen.push(["load", options]),
    onShow: () => seen.push(["show"]),
  });
  assert.deepEqual(Object.keys(lifecycle).sort(), ["onLoad", "onShow"]);
  assert.ok(Object.isFrozen(lifecycle));
  lifecycle.onLoad({ id: 1 });
  assert.deepEqual(seen, [["load", { id: 1 }]]);

  assert.throws(() => createPageLifecycle({ onWhatever: () => {} }), /未知生命周期/);
  assert.throws(() => createPageLifecycle({ onShow: "not-a-function" }), /须为函数/);
});

test("路由方法面：与 apiguard 白名单一致（映射方法在允许面内）", () => {
  assert.deepEqual([...ROUTE_METHODS], [
    "navigateTo",
    "redirectTo",
    "switchTab",
    "reLaunch",
    "navigateBack",
  ]);
});

test("normalizePath：前导斜杠 / 查询串 / 哈希归一化，空路径快速失败", () => {
  assert.equal(normalizePath("pages/index/index"), "pages/index/index");
  assert.equal(normalizePath("/pages/index/index"), "pages/index/index");
  assert.equal(normalizePath("/pages/about/index?from=home#top"), "pages/about/index");
  assert.equal(normalizePath("  pages/index/index  "), "pages/index/index");
  assert.throws(() => normalizePath(""), /path/);
  assert.throws(() => normalizePath("/?a=1"), /不含页面路径/);
  assert.throws(() => normalizePath(null), /path/);
});
