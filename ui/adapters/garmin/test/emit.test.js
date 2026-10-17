import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_PRODUCT,
  GARMIN_FORMS,
  appIdFor,
  createGarminPage,
  describePage,
  transpileGarmin,
} from "../src/index.js";

const page = (form) => createGarminPage(form);
const emitted = (form) => transpileGarmin(page(form));

test("圆表页模型复用 adapter-wear 的 tokens / 安全区 / 焦点序", () => {
  const round = page("round");
  assert.equal(round.product, "venu2");
  assert.deepEqual(round.reference, { width: 454, height: 454 });
  assert.deepEqual(round.safe, { x: 67, y: 67, width: 320, height: 320 });
  assert.deepEqual(round.boxes.counter, { x: 85, y: 207, width: 139, height: 35 });
  assert.deepEqual(round.boxes.exit, { x: 230, y: 248, width: 139, height: 35 });
  assert.deepEqual([...round.focusOrder], ["counter", "reset", "detail", "exit"]);
  assert.equal(round.initialFocus, "counter");
  assert.equal(round.capabilities.focusNav, true);
  // 遍历快照来自共享内核：4 个节点、双向可达（无死胡同由内核 traversalSnapshot 给出）
  assert.deepEqual(Object.keys(round.traversal).sort(), ["counter", "detail", "exit", "reset"]);
  assert.equal(round.traversal.counter.down, "detail");
  assert.equal(round.traversal.exit.left, "detail");
  assert.match(describePage(round).focusOrder, /counter → reset → detail → exit/);
});

test("方表页模型使用方表 token 集与安全区", () => {
  const square = page("square");
  assert.equal(square.product, "venusq");
  assert.deepEqual(square.reference, { width: 320, height: 320 });
  assert.deepEqual(square.safe, { x: 26, y: 26, width: 268, height: 268 });
  assert.deepEqual(square.boxes.counter, { x: 44, y: 140, width: 113, height: 35 });
  assert.deepEqual([...square.focusOrder], ["counter", "reset", "detail", "exit"]);
});

test("未知形态直接拒绝", () => {
  assert.throws(() => createGarminPage("pebble"), /未知形态/);
});

test("转译产物 = Connect IQ 工程六件套，字段可被结构自检消费", () => {
  const { files, meta } = emitted("round");
  assert.deepEqual(Object.keys(files), [
    "manifest.xml",
    "monkey.jungle",
    "resources/strings/strings.xml",
    "source/PolypackApp.mc",
    "source/PolypackView.mc",
    "source/PolypackDelegate.mc",
  ]);
  assert.equal(meta.appId, appIdFor(DEFAULT_PRODUCT.identifier));
  assert.match(meta.appId, /^[0-9a-f]{32}$/);

  const manifest = files["manifest.xml"];
  assert.match(manifest, /<iq:manifest xmlns:iq="http:\/\/www\.garmin\.com\/xml\/connectiq" version="3">/);
  assert.match(manifest, /id="[0-9a-f]{32}" type="watch-app" entry="PolypackApp"/);
  assert.match(manifest, /name="@Strings\.AppName" version="1\.2\.3" minSdkVersion="3\.1\.0"/);
  assert.match(manifest, /<iq:product id="venu2"\/>/);
  assert.doesNotMatch(manifest, /venusq/);

  assert.match(files["monkey.jungle"], /^project\.manifest = manifest\.xml$/m);
  assert.match(files["monkey.jungle"], /^base\.sourcePath = source$/m);
  assert.match(files["monkey.jungle"], /^base\.resourcePath = resources$/m);

  assert.match(files["source/PolypackApp.mc"], /class PolypackApp extends App\.AppBase/);
  assert.match(files["source/PolypackApp.mc"], /function getInitialView\(\)/);
  assert.match(files["source/PolypackApp.mc"], /return \[view, new PolypackDelegate\(view\)\];/);
  assert.match(files["source/PolypackDelegate.mc"], /class PolypackDelegate extends Ui\.BehaviorDelegate/);
  assert.match(files["source/PolypackDelegate.mc"], /function onBack\(\)/);
});

test("布局坐标与焦点序来自 wear 产物，视图不含手写邻接", () => {
  const view = emitted("round").files["source/PolypackView.mc"];
  for (const fragment of [
    "scaleX(85, width), scaleY(207, height), scaleX(139, width), scaleY(35, height)",
    "scaleX(230, width), scaleY(207, height), scaleX(139, width), scaleY(35, height)",
    "scaleX(85, width), scaleY(248, height), scaleX(139, width), scaleY(35, height)",
    "scaleX(230, width), scaleY(248, height), scaleX(139, width), scaleY(35, height)",
    'var mOrder = ["counter","reset","detail","exit"];',
    'var mIndex = { "counter" => 0, "reset" => 1, "detail" => 2, "exit" => 3 };',
    'var mFocus = "counter";',
  ]) {
    assert.ok(view.includes(fragment), `缺少布局/焦点片段：${fragment}`);
  }
  assert.doesNotMatch(view, /up.*down.*left.*right/s, "视图不得含手写四向邻接");
  assert.match(view, /static const REF_WIDTH = 454;/);
  assert.match(view, /static const REF_HEIGHT = 454;/);
});

test("颜色取自 wear tokens / platformStyle", () => {
  const view = emitted("round").files["source/PolypackView.mc"];
  for (const hex of ["0xFFF7ED", "0xFFFFFF", "0x0F172A", "0x64748B", "0xEA580C", "0x0E7490"]) {
    assert.ok(view.includes(hex), `缺少 token 颜色 ${hex}`);
  }
});

test("两种形态均确定性：同输入两跑字节一致", () => {
  for (const form of ["round", "square"]) {
    const a = emitted(form).files;
    const b = emitted(form).files;
    assert.deepEqual(a, b);
  }
});

test("方表 manifest 指向 venusq，缩放参考为 320", () => {
  const { files } = emitted("square");
  assert.match(files["manifest.xml"], /<iq:product id="venusq"\/>/);
  assert.doesNotMatch(files["manifest.xml"], /venu2/);
  assert.match(files["source/PolypackView.mc"], /static const REF_WIDTH = 320;/);
});

test("产品元数据：名称 XML 转义进 strings.xml，非法值拒绝", () => {
  const product = { name: 'A&B <C> "D"', version: "0.9.0", identifier: "com.example.safe-app" };
  const { files, meta } = transpileGarmin(page("round"), product);
  assert.match(files["resources/strings/strings.xml"], /A&amp;B &lt;C&gt; &quot;D&quot;/);
  assert.equal(meta.version, "0.9.0");
  assert.notEqual(meta.appId, appIdFor(DEFAULT_PRODUCT.identifier));

  assert.throws(() => transpileGarmin(page("round"), { ...DEFAULT_PRODUCT, version: "1.2" }), /version/);
  assert.throws(() => transpileGarmin(page("round"), { ...DEFAULT_PRODUCT, identifier: "Com.Example" }), /identifier/);
  assert.throws(() => transpileGarmin(page("round"), { ...DEFAULT_PRODUCT, name: "a\nb" }), /控制字符/);
  assert.throws(() => transpileGarmin(page("round"), { ...DEFAULT_PRODUCT, name: "" }), /name/);
});

test("形态表与 BOM 产品表保持单一来源", () => {
  assert.deepEqual(Object.keys(GARMIN_FORMS), ["round", "square"]);
  assert.equal(GARMIN_FORMS.round.product, "venu2");
  assert.equal(GARMIN_FORMS.square.product, "venusq");
});
