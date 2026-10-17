import assert from "node:assert/strict";
import test from "node:test";

import { assertSubset, checkMonkeyC, createGarminPage, sanitize, transpileGarmin } from "../src/index.js";

const cleanFiles = (form) => transpileGarmin(createGarminPage(form)).files;

const violationsOf = (source) => checkMonkeyC("probe.mc", source);

test("转译产物两形态均通过子集 / API 白名单断言", () => {
  for (const form of ["round", "square"]) {
    assert.equal(assertSubset(cleanFiles(form)), true);
  }
});

test("sanitize 去注释与字符串，保留行号", () => {
  const source = '// try\nvar a = "throw";\n/* switch */\nvar b = 1;\n';
  const clean = sanitize(source);
  assert.doesNotMatch(clean, /try|throw|switch/);
  assert.equal(clean.split("\n").length, source.split("\n").length);
  assert.match(clean, /var a = "";/);
});

test("注释与字符串里的禁用词不误报", () => {
  const source = [
    "using Toybox.WatchUi as Ui;",
    '// try { switch } 这是注释',
    'class PolypackView extends Ui.View {',
    "    function initialize() {",
    "        View.initialize();",
    '        var note = "dc.setAntiAlias(true) 只是文本";',
    "    }",
    "}",
  ].join("\n");
  assert.deepEqual(violationsOf(source), []);
});

test("解释器类构造被拒绝：try/catch/throw、switch、for/while、module", () => {
  const source = [
    "using Toybox.WatchUi as Ui;",
    "module Foo {",
    "class PolypackView extends Ui.View {",
    "    function initialize() {",
    "        try { throw 1; } catch (e) { }",
    "        for (var i = 0; i < 3; i++) { }",
    "        switch (1) { case 1: break; default: }",
    "    }",
    "}",
    "}",
  ].join("\n");
  const found = violationsOf(source);
  for (const keyword of ["try", "catch", "throw", "for", "switch", "case", "default", "break", "module"]) {
    assert.ok(found.some((v) => v.includes(`禁止关键字："${keyword}"`)), `未拦截 ${keyword}`);
  }
});

test("using 只允许三个模块且别名固定", () => {
  assert.ok(violationsOf("using Toybox.System as Sys;").some((v) => v.includes("模块不在白名单")));
  assert.ok(violationsOf("using Toybox.Graphics as Graphics;").some((v) => v.includes("别名不匹配")));
  assert.ok(violationsOf("using Toybox.Graphics;").some((v) => v.includes("using 形态")));
});

test("Dc / Graphics / WatchUi / Application 白名单外成员被拒绝", () => {
  const bad = [
    "using Toybox.WatchUi as Ui;",
    "class PolypackView extends Ui.View {",
    "    function initialize() {",
    "        dc.setAntiAlias(true);",
    "        dc.drawLine(1, 2, 3, 4);",
    '        var f = Gfx.FONT_GIANT;',
    "        Ui.pushView(self);",
    "        App.getApp();",
    "    }",
    "}",
  ].join("\n");
  const found = violationsOf(bad);
  for (const fragment of ["dc.setAntiAlias", "dc.drawLine", "Gfx.FONT_GIANT", "Ui.pushView", "App.getApp"]) {
    assert.ok(found.some((v) => v.includes(fragment)), `未拦截 ${fragment}`);
  }
});

test("Toybox 直接引用、System、未知成员、未知构造器与未知基类被拒绝", () => {
  const bad = [
    "using Toybox.WatchUi as Ui;",
    "class PolypackView extends Object {",
    "    function initialize() {",
    "        var x = Toybox.WatchUi;",
    '        Sys.println("x");',
    "        mClicks.toFloat();",
    "        var y = new Timer();",
    "    }",
    "}",
  ].join("\n");
  const found = violationsOf(bad);
  assert.ok(found.some((v) => v.includes("直接引用 Toybox")));
  assert.ok(found.some((v) => v.includes("System API")));
  assert.ok(found.some((v) => v.includes("未知成员调用")));
  assert.ok(found.some((v) => v.includes("构造器不在白名单")));
  assert.ok(found.some((v) => v.includes("基类不在白名单")));
});

test("定界符不平衡被拦截", () => {
  assert.ok(violationsOf("class A extends Ui.View { function initialize() {").some((v) => v.includes("未闭合")));
  assert.ok(violationsOf('class A extends Ui.View { var x = "}"; }').length === 0, "字符串内的括号不算");
});

test("violations 带文件名与行号", () => {
  const found = violationsOf("using Toybox.WatchUi as Ui;\n\nclass A extends Ui.View { function initialize() { dc.drawLine(1,2,3,4); } }");
  assert.ok(found.some((v) => v.startsWith("probe.mc:3: ")), found.join("\n"));
});
