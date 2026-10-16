// T-M3-06 宿主工程（host/）结构断言：不装 Taro 工具链也能跑的静态契约。
//
// 真跑链路（Taro 编译 / 产物结构 / 描述符落地）由 crates/poly-bundle/tests/packagers_miniapp.rs
// 与 crates/poly-cli/tests/contract_m3_miniapp.rs 覆盖（工具链缺席即 SKIP-with-print）；
// 本文件钉住「宿主工程长什么样」这类无需工具链的事实——配置注入点、平台映射、描述符清单，
// 任一漂移在 CI 的 node 腿即红，不必等真跑。
//
// 口径（与 poly-bundle 小程序打包器 src/packagers/miniapp.rs 的模块文档同源）：
//  - 宿主候选链：<poly 工程>/ui → <poly 工程> → <poly 工程>/ui/adapters/taro/host（本仓布局）；
//  - 产物落点：Taro `outputRoot` 由 `POLYPACK_OUTPUT_ROOT` 注入（<out>/miniapp/dist）；
//  - devtools 打开点：<out>/miniapp（描述符 `miniprogramRoot = ./dist/`）；
//  - 快应用无 Taro 平台插件：编译工程由打包器组装（.rpk 归 T-M3-12）。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HOST = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../host");

/** 读宿主工程内的文本文件。 */
const read = (rel) => fs.readFileSync(path.join(HOST, rel), "utf8");

/** 读并解析 JSON。 */
const readJson = (rel) => JSON.parse(read(rel));

test("宿主工程清单：三平台插件 + @polypack/adapter-taro 走 file: 依赖", () => {
  const manifest = readJson("package.json");
  assert.equal(manifest.name, "@polypack/host-taro");
  assert.equal(manifest.private, true, "宿主工程不发布");
  assert.equal(
    manifest.dependencies["@polypack/adapter-taro"],
    "file:..",
    "装配层以 file: 依赖消费（本仓布局 ui/adapters/taro/host 的上一级即适配器包）",
  );
  for (const plugin of [
    "@tarojs/plugin-platform-weapp",
    "@tarojs/plugin-platform-alipay",
    "@tarojs/plugin-platform-tt",
    "@tarojs/taro",
    "@tarojs/components",
  ]) {
    assert.ok(manifest.dependencies[plugin], `缺依赖 ${plugin}`);
  }
  assert.ok(manifest.devDependencies["@tarojs/cli"], "编译 CLI 在 devDependencies");
  // react 18 是 @tarojs/react@4.3 的 peer 面
  assert.match(manifest.dependencies.react, /\^18/, "react 须在 18 大版本");
  // 三平台的 npm scripts 是本地手跑入口（poly 经 POLYPACK_OUTPUT_ROOT 注入落点）
  for (const script of ["build:weapp", "build:alipay", "build:tt"]) {
    assert.match(manifest.scripts[script], /^taro build --type (weapp|alipay|tt)$/, script);
  }
});

test("编译配置：outputRoot 走 POLYPACK_OUTPUT_ROOT 注入、持久化缓存开启", () => {
  const config = read("config/index.js");
  assert.match(
    config,
    /outputRoot:\s*process\.env\.POLYPACK_OUTPUT_ROOT\s*\|\|\s*'dist'/,
    "产物目录须由环境变量注入（打包器按目标给目录），本地手跑回落 dist/",
  );
  assert.match(config, /cache:\s*\{\s*enable:\s*true\s*\}/s, "二跑缓存命中：Taro 持久化缓存须开启");
  assert.match(config, /sourceRoot:\s*'src'/);
  assert.match(config, /framework:\s*'react'/);
  assert.match(config, /compiler:\s*'webpack5'/);
});

test("装配接线：pages 声明源 + 平台逆映射 + mount 契约在页面侧", () => {
  const appConfig = read("src/app.config.js");
  assert.match(appConfig, /pages:\s*\['pages\/index\/index'\]/, "pages 清单是路由映射的声明源");

  const adapter = read("src/adapter.js");
  assert.match(adapter, /TARO_PLATFORMS/, "平台 → 目标 id 走适配器的常量表（逆映射）");
  assert.match(adapter, /process\.env\.TARO_ENV/, "平台由 Taro 编译器静态注入");
  for (const method of ["navigateTo", "redirectTo", "switchTab", "reLaunch", "navigateBack"]) {
    assert.ok(adapter.includes(`Taro.${method}(`), `路由方法 ${method} 须经 Taro.* 落地`);
  }

  const app = read("src/app.js");
  assert.match(app, /POLYPACK_TARO target=/, "装配自检标记（产物结构断言的 ASCII 锚点）");

  const page = read("src/pages/index/index.jsx");
  assert.match(
    page,
    /adapter\.mount\(\{\s*pagePath:\s*'pages\/index\/index'\s*\}/s,
    "页面经 mount 契约装配（未声明页在装配点即拒）",
  );
});

test("devtools 描述符三件：目标侧名字 + appid 就位", () => {
  // 微信：devtools 直接读宿主这份
  const wechat = readJson("project.config.json");
  assert.equal(wechat.appid, "touristappid", "游客 appid：本地预览不绑真实账号");
  assert.equal(wechat.miniprogramRoot, "./dist/");
  assert.equal(wechat.compileType, "miniprogram");

  // 支付宝：Alipay IDE 读 mini.project.json（Taro 文档表格里的 project.alipay.json 与 IDE 实际不符）
  const alipay = readJson("mini.project.json");
  assert.equal(alipay.component2, true, "component2 是支付宝 IDE 的编译开关");
  assert.equal(alipay.miniprogramRoot, "./dist/");

  // 抖音：宿主源名 project.tt.json（Taro tt 平台的约定），产物会重写为 project.config.json
  const douyin = readJson("project.tt.json");
  assert.equal(douyin.appid, "tta0000000000000001");
  assert.equal(douyin.miniprogramRoot, "./dist/");
});

test("依赖与产物不入库：node_modules / dist / lockfile 在忽略清单", () => {
  const ignore = read(".gitignore");
  for (const pattern of ["node_modules/", "dist/", "package-lock.json"]) {
    assert.ok(ignore.includes(pattern), `.gitignore 缺 ${pattern}`);
  }
});
