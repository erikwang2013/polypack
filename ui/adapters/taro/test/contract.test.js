// T-M3-05 契约测试：RendererAdapter 对象形状、mount 装配（页面路由绑定）、边界校验。
// 零依赖（node:test）；runtime 用桩注入，不要求安装 @tarojs/taro。

import assert from "node:assert/strict";
import { test } from "node:test";

import { CAPABILITY_KEYS, TARO_PLATFORMS, createAdapter } from "../src/index.js";

const PAGES = ["pages/index/index", "pages/about/index"];

const stubRuntime = (calls = []) => ({
  calls,
  config: () => ({ pages: PAGES }),
  navigateTo(options) {
    calls.push({ method: "navigateTo", options });
    return Promise.resolve();
  },
  redirectTo(options) {
    calls.push({ method: "redirectTo", options });
    return Promise.resolve();
  },
  reLaunch(options) {
    calls.push({ method: "reLaunch", options });
    return Promise.resolve();
  },
  switchTab(options) {
    calls.push({ method: "switchTab", options });
    return Promise.resolve();
  },
  navigateBack(options) {
    calls.push({ method: "navigateBack", options });
    return Promise.resolve();
  },
});

const makeAdapter = (options = {}) =>
  createAdapter({ target: "wechat", runtime: stubRuntime(), ...options });

test("契约对象形状：mount / platformStyle / capabilities 三件套齐备且不可变", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.target, "wechat");
  for (const name of ["mount", "platformStyle", "capabilities", "degradations", "router"]) {
    assert.equal(typeof adapter[name], "function", `${name} 应为函数`);
  }
  assert.ok(Object.isFrozen(adapter), "契约对象应冻结（宿主不得改写装配层）");
});

test("target → Taro 编译平台映射：四目标齐备（打包器 --type 值由此派生）", () => {
  for (const [target, platform] of Object.entries(TARO_PLATFORMS)) {
    assert.equal(makeAdapter({ target }).platform, platform, `${target} → ${platform}`);
  }
  assert.deepEqual(
    Object.keys(TARO_PLATFORMS).sort(),
    ["alipay", "douyin", "quickapp", "wechat"],
    "四目标集合为 T-M3-06 打包器的目标面",
  );
});

test("capabilities()：恰好契约三键，值为布尔", () => {
  const caps = makeAdapter().capabilities();
  assert.deepEqual(Object.keys(caps), [...CAPABILITY_KEYS]);
  for (const key of CAPABILITY_KEYS) {
    assert.equal(typeof caps[key], "boolean", `${key} 应为布尔`);
  }
});

test("platformStyle()：默认与传参两条路径都返回蒙皮样式", () => {
  const adapter = makeAdapter();
  const skinned = adapter.platformStyle();
  assert.equal(typeof skinned.card.padding, "number");
  assert.equal(typeof skinned.focusRing.borderColor, "string");
  const custom = adapter.platformStyle(
    { ...skinned, color: { ...skinned.color, brand: "#123456" } },
    { compact: true },
  );
  assert.equal(custom.button.backgroundColor, "#123456", "platformStyle(tokens) 应消费传入 token 集");
  assert.equal(custom.space[4], 18, "compact 间距紧凑化应生效");
});

test("mount(root, app)：页面绑定到声明的路由；mounted 可观测", () => {
  const adapter = makeAdapter();
  const Page = () => null;

  const mounted = adapter.mount({ pagePath: "pages/index/index" }, Page);
  assert.equal(mounted.pagePath, "pages/index/index");
  assert.equal(mounted.app, Page);
  assert.equal(mounted.platform, "weapp");
  assert.equal(adapter.mounted, mounted);
  assert.ok(Object.isFrozen(mounted));

  // 前导斜杠归一化（小程序 url 形态 `/pages/x/index` 与 pages 数组写法同指一页）
  assert.equal(adapter.mount({ pagePath: "/pages/about/index" }, Page).pagePath, "pages/about/index");
});

test("装配边界校验：未知目标 / 缺 runtime / 非法 root / 未声明页 / 非法 app 快速失败", () => {
  assert.throws(() => createAdapter({ target: "symbian", runtime: stubRuntime() }), /未知目标/);
  assert.throws(() => createAdapter({ target: "wechat" }), /runtime/);
  const adapter = makeAdapter();

  assert.throws(() => adapter.mount({}, () => null), /path/);
  assert.throws(() => adapter.mount({ pagePath: "" }, () => null), /path/);
  assert.throws(() => adapter.mount({ pagePath: "pages/index/index" }, null), /app 组件/);
  // 路由映射前置：未在 app.config 声明的页面不得装配（构建期 / 启动期可测）
  assert.throws(
    () => adapter.mount({ pagePath: "pages/ghost/index" }, () => null),
    /未在 app.config 的 pages 中声明/,
  );
});

test("router()：声明式路由经 runtime 落地；未声明目标页与未知方法在调用点拒绝", async () => {
  const calls = [];
  const adapter = makeAdapter({ runtime: stubRuntime(calls) });
  const router = adapter.router();

  await router.navigateTo("pages/about/index?from=home");
  assert.deepEqual(calls.at(-1), {
    method: "navigateTo",
    options: { url: "/pages/about/index?from=home" },
  });
  await router.navigateBack({ delta: 2 });
  assert.deepEqual(calls.at(-1), { method: "navigateBack", options: { delta: 2 } });

  assert.throws(() => router.navigateTo("pages/ghost/index"), /未在 app.config 的 pages 中声明/);
  assert.throws(() => router.navigateBack({ delta: 0 }), /delta/);
  assert.throws(() => router.somehow("pages/index/index"), /is not a function/);
});

test("router()：runtime 缺 config() 或 pages 为空时快速失败（路由映射无声明源）", () => {
  const adapter = makeAdapter({ runtime: { navigateTo() {} } });
  assert.throws(() => adapter.router(), /config\(\)/);
  const empty = makeAdapter({ runtime: { config: () => ({ pages: [] }), navigateTo() {} } });
  assert.throws(() => empty.router(), /pages/);
});
