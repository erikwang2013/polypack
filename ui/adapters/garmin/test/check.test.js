import assert from "node:assert/strict";
import test from "node:test";

import {
  assertGarminProject,
  assertIqEntries,
  checkGarminProject,
  checkIqEntries,
  createGarminPage,
  isSafePath,
  parseXml,
  transpileGarmin,
} from "../src/index.js";

const filesFor = (form = "round") => transpileGarmin(createGarminPage(form)).files;
const withFile = (path, content, form = "round") => ({ ...filesFor(form), [path]: content });

test("两种形态的工程文件树均通过结构自检", () => {
  for (const form of ["round", "square"]) {
    assert.deepEqual(checkGarminProject(filesFor(form)), []);
    assert.equal(assertGarminProject(filesFor(form)), true);
  }
});

test("路径安全：拒绝绝对路径 / 上跳 / 反斜杠 / 空段", () => {
  assert.equal(isSafePath("source/PolypackView.mc"), true);
  assert.equal(isSafePath("../evil.mc"), false);
  assert.equal(isSafePath("/etc/passwd"), false);
  assert.equal(isSafePath("source\\PolypackView.mc"), false);
  assert.equal(isSafePath("source//x.mc"), false);
  assert.equal(isSafePath(""), false);
  assert.equal(isSafePath(42), false);
  const found = checkGarminProject(withFile("../evil.mc", "x"));
  assert.ok(found.some((v) => v.includes("路径不安全")));
});

test("必需文件缺失被拦截", () => {
  const files = filesFor();
  delete files["monkey.jungle"];
  const found = checkGarminProject(files);
  assert.deepEqual(found, ['缺少必需文件："monkey.jungle"']);
});

test("manifest 字段断言：id / type / entry / version / minSdkVersion / 产品", () => {
  const mutate = (fn) => {
    const files = filesFor();
    files["manifest.xml"] = fn(files["manifest.xml"]);
    return checkGarminProject(files);
  };
  assert.ok(mutate((m) => m.replace(/id="[0-9a-f]{32}"/, 'id="ABC"')).some((v) => v.includes("32 位小写 hex")));
  assert.ok(mutate((m) => m.replace(/type="watch-app"/, 'type="widget"')).some((v) => v.includes("watch-app")));
  assert.ok(mutate((m) => m.replace(/entry="PolypackApp"/, 'entry="Nope"')).some((v) => v.includes("入口类 Nope 未在")));
  assert.ok(mutate((m) => m.replace(/version="1\.2\.3"/, 'version="1.2"')).some((v) => v.includes("version 须为数字三段")));
  assert.ok(mutate((m) => m.replace(/minSdkVersion="3\.1\.0"/, 'minSdkVersion="3"')).some((v) => v.includes("minSdkVersion")));
  assert.ok(mutate((m) => m.replace(/\s*<iq:product id="venu2"\/>/, "")).some((v) => v.includes("至少登记 1 个")));
  assert.ok(mutate((m) => m.replace(/<iq:product id="venu2"\/>/, '<iq:product id="Venu 2"/>')).some((v) => v.includes("iq:product id 非法")));
  assert.ok(mutate((m) => m.replace("http://www.garmin.com/xml/connectiq", "http://example.com")).some((v) => v.includes("xmlns:iq")));
});

test("XML 扫描：良构性、DOCTYPE、重复属性、根外文本", () => {
  assert.throws(() => parseXml("<a><b></a>"), /闭合不匹配/);
  assert.throws(() => parseXml("<!DOCTYPE x><a/>"), /DOCTYPE/);
  assert.throws(() => parseXml('<a x="1" x="2"/>'), /属性重复/);
  assert.throws(() => parseXml("text<a/>"), /根元素外/);
  assert.throws(() => parseXml('<a x=1/>'), /属性非法/);
  assert.throws(() => parseXml("<a/>tail"), /根元素外/);
  const tags = parseXml('<?xml version="1.0"?>\n<a x="1"><b/><!-- c --></a>\n');
  assert.deepEqual(tags.map((tag) => tag.name), ["a", "b"]);
  assert.equal(tags[0].attrs.x, "1");
  assert.equal(tags[0].selfClosing, false);
  assert.equal(tags[1].selfClosing, true);
});

test("monkey.jungle：缺键 / 值不符 / 无法解析", () => {
  const missing = withFile("monkey.jungle", "project.manifest = manifest.xml\nbase.sourcePath = source\n");
  assert.ok(checkGarminProject(missing).some((v) => v.includes("base.resourcePath")));
  const wrong = withFile("monkey.jungle", "project.manifest = manifest.xml\nbase.sourcePath = src\nbase.resourcePath = resources\n");
  assert.ok(checkGarminProject(wrong).some((v) => v.includes("base.sourcePath = source")));
  const garbage = withFile("monkey.jungle", "project.manifest = manifest.xml\noops\nbase.sourcePath = source\nbase.resourcePath = resources\n");
  assert.ok(checkGarminProject(garbage).some((v) => v.includes("无法解析")));
});

test("strings.xml 与入口类声明", () => {
  const noName = withFile("resources/strings/strings.xml", "<strings></strings>");
  assert.ok(checkGarminProject(noName).some((v) => v.includes("AppName")));
  const badXml = withFile("resources/strings/strings.xml", "<strings>");
  assert.ok(checkGarminProject(badXml).some((v) => v.includes("strings.xml 不是良构")));
  const noClass = withFile("source/PolypackApp.mc", "// empty\n");
  assert.ok(checkGarminProject(noClass).some((v) => v.includes("未在 source/PolypackApp.mc 声明")));
});

test("工程自检把子集违规冒泡为结构失败", () => {
  const files = filesFor();
  files["source/PolypackView.mc"] += "\nclass Evil extends Ui.View { function initialize() { dc.setAntiAlias(true); } }\n";
  const found = checkGarminProject(files);
  assert.ok(found.some((v) => v.includes("dc.setAntiAlias")), found.join("\n"));
  assert.throws(() => assertGarminProject(files), /结构自检失败/);
});

test(".iq 条目断言：manifest.xml + ≥1 个 .prg，路径安全且不重复", () => {
  assert.deepEqual(checkIqEntries(["manifest.xml", "venu2/", "venu2/PolypackApp.prg", "resources/strings/strings.xml"]), []);
  assert.equal(assertIqEntries(["manifest.xml", "venu2/PolypackApp.prg"]), true);

  assert.deepEqual(checkIqEntries([]), ["`.iq` 条目清单为空"]);
  assert.ok(checkIqEntries(["venu2/PolypackApp.prg"]).some((v) => v.includes("根缺少 manifest.xml")));
  assert.ok(checkIqEntries(["manifest.xml", "resources/"]).some((v) => v.includes("缺少按设备编译的 .prg")));
  assert.ok(checkIqEntries(["manifest.xml", "../evil.prg"]).some((v) => v.includes("路径不安全")));
  assert.ok(checkIqEntries(["manifest.xml", "a.prg", "a.prg"]).some((v) => v.includes("条目重复")));
  assert.throws(() => assertIqEntries(["manifest.xml"]), /包结构断言失败/);
});
