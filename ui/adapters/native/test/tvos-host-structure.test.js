// T-M4-06 RN-tvOS 宿主结构断言：本仓无 macOS/Xcode（tvOS 模拟器腿属 CI macos-14 / 上机窗口）下，
// 以骨架完整性、装配链一致性、遥控器事件通道覆盖、10-foot 蒙皮跨包同调用对照与依赖口径哨兵
// 替代——证据边界见 host/tvos/README「阻塞与证据边界」。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import * as tvAdapter from "../../tv/src/index.js";
import {
  CAPABILITY_MATRIX,
  HOST_PROP_OMISSIONS,
  TV_SAFE_AREA,
  TV_SCALE,
  createAdapter,
  createFocusEngine,
  handleKey,
  mapProps,
  platformStyle,
  platformStyleTvos,
  tokens,
  tvosTokens,
} from "../src/index.js";
import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { KEYMAP } from "../device-walks/walks.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOST = path.resolve(HERE, "../host/tvos");
const read = (rel) => fs.readFileSync(path.join(HOST, rel), "utf8");
const exists = (rel) => fs.existsSync(path.join(HOST, rel));

const INDEX = read("index.js");

const stubRuntime = { AppRegistry: { registerComponent() {} } };
const tvosAdapter = () => createAdapter({ target: "tvos", runtime: stubRuntime });

test("宿主骨架文件齐备（RN-tvOS 社区 RN 工程形态 + 打包链消费的 ios/ 工程位）", () => {
  const files = [
    "package.json",
    "app.json",
    "index.js",
    "babel.config.js",
    "metro.config.js",
    "README.md",
    ".gitignore",
    "ios/.gitkeep",
  ];
  for (const file of files) {
    assert.ok(exists(file), `缺宿主文件: ${file}`);
  }
});

test("装配链闭合：AppRegistry 键三处一致（app.json / mount / ios 工程位约定）+ 组件映射已接", () => {
  assert.equal(JSON.parse(read("app.json")).name, "PaxApp");
  assert.match(INDEX, /adapter\.mount\(\{ appKey: "PaxApp" \}, TvKeyBridge\)/, "mount 键须与 app.json name 一致（TvKeyBridge = 事件源桥包 App）");
  assert.match(INDEX, /from "@polypack\/adapter-native"/);
  assert.match(INDEX, /from "@polypack\/adapter-native\/example"/);
  assert.match(INDEX, /bindPrimitives\(/, "宿主须经 bindPrimitives 应用组件映射");
  assert.match(INDEX, /createElement/, "bindPrimitives 需注入 createElement");
  // 事件源桥须包住 App：useTVEventHandler 是 hook，不能像 android/harmony 宿主那样模块顶层订阅
  assert.match(INDEX, /function TvKeyBridge\(\)/);
  assert.match(INDEX, /return createElement\(App, null\)/);
});

test("target id 哨兵：createAdapter 的 target 是 capabilities 矩阵内的目标（'tvos'，TV 基线）", () => {
  const target = INDEX.match(/createAdapter\(\{ target: "([^"]+)"/)?.[1];
  assert.equal(target, "tvos", "target = TargetSpec id（crates/polypack-targets/src/builtin.rs）");
  assert.ok(Object.hasOwn(CAPABILITY_MATRIX, target));
  assert.deepEqual(CAPABILITY_MATRIX.tvos, { focusNav: true, safeArea: true, gestures: false });
  // TV 基线经宿主真路径成立（未知目标会在此抛错）；pointer none 不改变 TV 基线
  assert.deepEqual(tvosAdapter().capabilities(), { focusNav: true, safeArea: true, gestures: false });
  assert.deepEqual(
    tvosAdapter().capabilities(),
    createAdapter({ target: "tvos", runtime: stubRuntime, host: { pointer: "none" } }).capabilities(),
  );
  assert.equal(JSON.parse(read("app.json")).polypack.pointer, "none", "TV 恒无指针：默认即 none");
  // 降级分支：焦点环（无指针）与显式返回入口（无系统手势）生效；TV 无 compact 分支
  assert.deepEqual(
    tvosAdapter().degradations().filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "back-affordance"],
  );
});

test("组件映射：tvos 行 = []（hasTVPreferredFocus 原生支持，焦点锚点保留）+ 能力行齐备", () => {
  assert.deepEqual(HOST_PROP_OMISSIONS.tvos, []);
  assert.deepEqual(
    mapProps("tvos", { focusable: true, hasTVPreferredFocus: true }),
    { focusable: true, hasTVPreferredFocus: true },
    "示例页的 TV 焦点锚点对 tvos 零改动透传",
  );
  // 页面组件树零改动：图例见 src/example.js 的 hasTVPreferredFocus 焦点锚点
  assert.match(fs.readFileSync(path.resolve(HERE, "../src/example.js"), "utf8"), /hasTVPreferredFocus/);
});

test("键事件源接线：useTVEventHandler + Down-only + BackHandler（menu 不入键表）+ 单参数 POLYPACK_KEY", () => {
  assert.match(INDEX, /RN\.useTVEventHandler\(\(event\) => \{/, "须经 RN-tvOS 的 useTVEventHandler 订阅遥控器");
  assert.match(INDEX, /event\.eventKeyAction !== 0/, "须只消费 Down（eventKeyAction=0）：一次按键派发 Down+Up 两帧");
  assert.match(INDEX, /RN\.BackHandler\.addEventListener\("hardwareBackPress"/, "Back 走核心 BackHandler 通道");
  assert.match(INDEX, /RN\.TVEventControl\.enableTVMenuKey\(\)/, "menu（Back）须开闸（Apple 指南默认吞 menu 键）");
  assert.match(INDEX, /RN\.TVEventControl\.disableTVMenuKey\(\)/, "开闸须对偶关闭（effect 清理）");
  assert.match(
    INDEX,
    /console\.log\("POLYPACK_KEY " \+ JSON\.stringify/,
    "须保留单参数观测通道（走查比对用；android 设备链同款约束）",
  );
});

test("键表覆盖：EVENTTYPE_TO_KERNEL ≡ 走查键（Back 除外，走 BackHandler）且全部可被内核路由", () => {
  const block = INDEX.match(/const EVENTTYPE_TO_KERNEL = \{([\s\S]*?)\};/)?.[1];
  assert.ok(block, "index.js 应有 EVENTTYPE_TO_KERNEL 表");
  const table = [...block.matchAll(/(\w+):\s*"([A-Z_]+)"/g)].map((m) => [m[1], m[2]]);
  assert.ok(table.length >= 5, `键表解析异常（${table.length} 项）`);

  const walkKeys = KEYMAP.map((m) => m.kernel).filter((k) => k !== "Back");
  const mapped = new Set(table.map(([, key]) => key));
  for (const key of walkKeys) {
    assert.ok(mapped.has(key), `走查键 ${key} 未入宿主键表（tvOS 走查无法按对拍数据执行）`);
  }
  // menu 不入表（两路都接会双计）：Back 只经 BackHandler
  assert.ok(!table.some(([eventType, key]) => eventType === "menu" || key === "Back"), "menu/Back 不得入键表");
  // 每个映射值都是内核能路由的键（无死表项）
  const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  for (const key of mapped) {
    assert.notEqual(handleKey(engine, key).type, "ignored", `内核应路由 ${key}`);
  }
});

test("10-foot 蒙皮（T-M4-06）：token 值与蒙皮数值 ≡ adapter-tv 单源（跨包同调用对照，不引运行时依赖）", () => {
  // ① token 集：与 adapter-tv（10-foot 单源）逐值相等（含 safeArea 百分比基值）
  assert.deepEqual(tvosTokens, tvAdapter.tokens, "10-foot token 集漂移：与 ui/adapters/tv 单源须同步");
  assert.deepEqual(TV_SCALE, tvAdapter.TV_SCALE);
  assert.deepEqual(TV_SAFE_AREA, tvAdapter.SAFE_AREA);
  // 基值口径（native）不变：10-foot 是派生而非改写
  assert.deepEqual(tokens.space, { 1: 4, 2: 8, 3: 16, 4: 24 });

  const rn = platformStyleTvos();
  const dom = tvAdapter.platformStyle();
  // ② 蒙皮数值同口径（同一组探针两侧各取一次）
  assert.deepEqual(rn.safeArea, dom.safeArea, "安全区形态（百分比字符串）须两宿主一致");
  assert.deepEqual(rn.space, dom.space);
  assert.deepEqual(rn.font, dom.font);
  assert.equal(rn.radius, dom.radius);
  assert.equal(rn.card.padding, dom.card.padding);
  assert.equal(rn.card.maxWidth, dom.card.maxWidth, "10-foot 页卡宽 1280");
  assert.equal(rn.card.maxWidth, 1280);
  assert.equal(rn.card.borderRadius, dom.card.borderRadius);
  assert.equal(rn.title.fontSize, dom.title.fontSize);
  assert.equal(rn.title.fontWeight, dom.title.fontWeight);
  assert.equal(rn.text.fontSize, dom.text.fontSize);
  assert.equal(rn.muted.fontSize, dom.muted.fontSize);
  assert.equal(rn.button.backgroundColor, dom.button.backgroundColor);
  assert.equal(rn.button.borderRadius, dom.button.borderRadius);
  assert.equal(rn.button.paddingVertical, dom.button.paddingTop, "RN 拆键 vertical ↔ react-dom paddingTop");
  assert.equal(rn.focusRing.borderColor, dom.focusRing.borderColor);
  assert.equal(rn.focusRing.borderWidth, dom.focusRing.borderWidth, "10-foot 焦点环 4px");
  assert.equal(rn.focusRing.borderWidth, 4);
  assert.equal(rn.button.borderWidth, rn.focusRing.borderWidth, "聚焦零布局抖动：常驻透明边框宽度 = 环宽");
  assert.equal(rn.button.borderColor, "transparent");
  assert.equal(rn.overlay.backgroundColor, dom.overlay.backgroundColor);
  // ③ 形态差异是刻意的（RN 合法性 + RN 拆键；react-dom 侧不得含 RN 拆键）
  assert.ok(!JSON.stringify(rn).includes("px"), "RN 样式不得含 px 单位");
  assert.ok(JSON.stringify(rn).includes("Vertical"), "RN 蒙皮用拆键（vertical/horizontal）");
  assert.ok(!JSON.stringify(dom).includes("Vertical"), "react-dom 蒙皮不得含 RN 拆键");
  assert.equal(rn.card.alignItems, "center", "RN 文本不继承对齐：卡片显式居中");
  assert.equal(dom.card.textAlign, "center");
});

test("10-foot 默认蒙皮绑定：createAdapter(target=tvos) 零参直取 10-foot；其余目标不漂移", () => {
  const bound = tvosAdapter().platformStyle();
  assert.deepEqual(bound, platformStyleTvos(), "页面零参 platformStyle() 即 10-foot 蒙皮");
  assert.deepEqual(platformStyleTvos(), platformStyleTvos(), "确定性（两次求值深相等）");
  assert.ok(Object.isFrozen(platformStyleTvos()) && Object.isFrozen(platformStyleTvos().focusRing));
  // 与 base 蒙皮的判别位（防止回落到手机皮肤）
  assert.equal(bound.focusRing.borderWidth, 4);
  assert.equal(bound.card.maxWidth, 1280);
  assert.notDeepEqual(bound, platformStyle());
  // 非 tvos 目标不受影响（android 仍为基座蒙皮）
  assert.deepEqual(createAdapter({ target: "android", runtime: stubRuntime }).platformStyle(), platformStyle());
  // tvos 不接 compact 分支（10-foot 即基线）
  assert.equal(tvosAdapter().platformStyle(tvosTokens, { compact: true }).space[3], tvosTokens.space[3]);
});

test("事件名 / 导出漂移哨兵：EVENTTYPE_TO_KERNEL ≡ 已装 RN-tvOS 原生常量（node_modules 缺席时 skip）", (t) => {
  const pkgDir = path.join(HOST, "node_modules/react-native");
  if (!fs.existsSync(pkgDir)) {
    t.skip(
      "node_modules 未安装（npm install 后此哨兵生效）——现口径 = RN-tvOS 0.87.1-1 上游常量" +
        "（RCTTVRemoteHandlerConstants.mm：up/down/left/right/select/menu/playPause…）+ index.js 具名导出" +
        "（get TVEventControl / get useTVEventHandler，已按 tag 核对）",
    );
    return;
  }
  // 别名安装哨兵：包名 react-native 实为 react-native-tvos（package.json 的 npm: 别名口径）
  const alias = JSON.parse(read("package.json")).dependencies["react-native"];
  const aliasMatch = /^npm:react-native-tvos@(.+)$/.exec(alias);
  assert.ok(aliasMatch, `react-native 须为 RN-tvOS 别名安装，实际 ${alias}`);
  const installed = JSON.parse(fs.readFileSync(path.join(pkgDir, "package.json"), "utf8"));
  assert.equal(installed.name, "react-native-tvos");
  assert.equal(installed.version, aliasMatch[1], "安装版本须与 package.json 别名钉的版本一致");
  // 具名导出（index.js getter）+ 原生事件常量
  const rnIndex = fs.readFileSync(path.join(pkgDir, "index.js"), "utf8");
  for (const name of ["useTVEventHandler", "TVEventControl", "BackHandler"]) {
    assert.match(rnIndex, new RegExp(`get ${name}\\b`), `RN-tvOS 缺具名导出 ${name}`);
  }
  const constantsPath = [
    "React/Base/RCTTVRemoteHandlerConstants.mm",
    "ReactApple/Libraries/RCTTVRemoteHandlerConstants.mm",
  ]
    .map((rel) => path.join(pkgDir, rel))
    .find((p) => fs.existsSync(p));
  if (!constantsPath) {
    t.skip("原生常量文件未随包分发（路径漂移）——事件名口径以本文件 skip 注记的上游 tag 为准");
    return;
  }
  const constants = fs.readFileSync(constantsPath, "utf8");
  const block = INDEX.match(/const EVENTTYPE_TO_KERNEL = \{([\s\S]*?)\};/)?.[1] ?? "";
  for (const [, eventType] of [...block.matchAll(/(\w+):\s*"([A-Z_]+)"/g)]) {
    assert.ok(
      new RegExp(`@"${eventType}"`).test(constants),
      `键表事件名 ${eventType} 不在 RN-tvOS 原生常量内（${path.relative(pkgDir, constantsPath)}）`,
    );
  }
});

test("二进制与凭据资产不入库：无 .so/.har/.hbc/.xcframework/证书，ios/ 仅 .gitkeep，忽略规则兜底", () => {
  const forbidden = /\.(so|har|hbc|p12|cer|jks|keystore|ipa)$/;
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        assert.ok(!entry.name.endsWith(".xcframework"), `${path.relative(HOST, full)} 不应入库`);
        walk(full);
      } else {
        assert.ok(!forbidden.test(entry.name), `${path.relative(HOST, full)} 不应入库`);
      }
    }
  };
  walk(HOST);
  assert.deepEqual(
    fs.readdirSync(path.join(HOST, "ios")),
    [".gitkeep"],
    "ios/ 只留工程位占位（Xcode 工程 / 打包链产物不入库）",
  );
  assert.match(read(".gitignore"), /^node_modules\/$/m);
  assert.match(read(".gitignore"), /^ios\/build\/$/m);
  assert.match(read(".gitignore"), /^bridge\/$/m);
});

test("宿主依赖与阻塞说明：adapter 本地接线、RN-tvOS 别名钉版、watch 树、阻塞成文", () => {
  const pkg = JSON.parse(read("package.json"));
  assert.ok(pkg.private, "宿主工程不发布");
  assert.equal(pkg.dependencies["@polypack/adapter-native"], "file:../..");
  assert.equal(pkg.dependencies["react-native"], "npm:react-native-tvos@0.87.1-1", "RN-tvOS 官方别名装法");
  assert.equal(pkg.dependencies["react"], "19.2.3");
  assert.equal(pkg.devDependencies["@react-native/metro-config"], "0.87.1");
  // file:../.. 链接包的真实路径在工程根之外：缺 watchFolders 解析必失败（android/harmony 宿主同因）
  assert.match(
    read("metro.config.js"),
    /watchFolders: \[path\.resolve\(__dirname, '\.\.\/\.\.'\)\]/,
    "metro.config.js 须把 adapter 包纳入 watch 树",
  );
  const readme = read("README.md");
  for (const marker of ["阻塞", "useTVEventHandler", "TVEventControl", "POLYPACK_KEY", "polypack build --target tvos"]) {
    assert.ok(readme.includes(marker), `宿主 README 缺说明: ${marker}`);
  }
});

test("设备腿（tvOS 模拟器渲染 + 键序列走查）：SKIP-with-print——本机 Linux 无 Xcode/tvOS SDK", (t) => {
  const hasXcode = fs.existsSync("/Applications/Xcode.app");
  t.skip(
    `本机 ${process.platform} ${hasXcode ? "（有 Xcode.app 但不在 CI 腿契约内）" : "无 Xcode / tvOS SDK"}——` +
      "tvOS 模拟器渲染与键序列走查阻塞，不假装已跑。证据边界：本包测试为 JS 层替代证据（结构快照 + " +
      "事件通道断言 + 10-foot 跨包对照），模拟器证据由 CI macos-14 腿（mobile-contract.yml，发起前报 lead）" +
      "与上机窗口补。就绪后按 device-walks 同款对拍执行（键序列与期望值 ≡ device-walks/expectations.json，内核键名全同）：\n" +
      "  前置： xcrun simctl boot <tvOS udid>；Xcode 模拟器 Siri Remote 面板（或 XCUITest）驱动按键\n" +
      "  按键： 五向 = 遥控面板上/下/左/右/中心；Back = menu 键（须 TvKeyBridge 已 enableTVMenuKey）\n" +
      "  观测： xcrun simctl spawn <udid> log stream --predicate 'eventMessage CONTAINS \"POLYPACK_KEY\"'" +
      "（debug 态直接看 Metro 终端）\n" +
      "  比对： 每键结果的 expect.focus / expect.clicks 与 device-walks/expectations.json 逐条对拍" +
      "（一次物理按键派发 Down+Up 两帧，宿主只消费 Down）",
  );
});
