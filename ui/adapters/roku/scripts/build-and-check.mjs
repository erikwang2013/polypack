// T-M4-11：Roku 频道构建 + 自检（spike）。
//   1) 发射生成件（components/PolypackScene.xml + source/focusgraph.brs）并入 staging
//   2) 组 zip（布局对齐 rokudev/hello-world 官方 makefile：zip 根 = manifest + source/ + components/）
//   3) 结构自检（zip 布局 / manifest 必填字段 / XML 良构 / BRS 标记）——无真机时的替代验证
//   4) 侧载自检：POLY_ROKU_REAL=1 + ROKU_DEV_TARGET 时真跑（ECP 探测 + plugin_install）；
//      否则 SKIP-with-print 打印官方式手册命令（依据 rokudev/hello-world makefile/app.mk）。
//
// 用法: node scripts/build-and-check.mjs [--src <频道骨架目录>] [--out <产物目录>]
// 依赖: 系统 zip / unzip（Linux/mac/WSL；spike 边界，Windows 原生缺 zip 会明报错）。
// 退出码: 0 = 结构自检过（含 SKIP）；1 = 任一检查失败或真跑侧载失败（绝不假装绿）。

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { emitChannelFiles, xmlWellFormed } from "../src/emit.js";
import { createExampleSubset } from "../src/subset.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PKG = path.resolve(HERE, "..");

const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i === -1 ? fallback : process.argv[i + 1];
};
const SRC = path.resolve(arg("--src", path.join(PKG, "channel")));
const OUT = path.resolve(arg("--out", path.join(PKG, "dist")));

const failures = [];
const pass = (msg) => console.log(`[PASS] ${msg}`);
const fail = (msg) => {
  failures.push(msg);
  console.log(`[FAIL] ${msg}`);
};
const skip = (msg) => console.log(`[SKIP] ${msg}`);

const run = (cmd, args, opts = {}) => {
  const r = spawnSync(cmd, args, { encoding: "utf8", ...opts });
  if (r.error) throw new Error(`${cmd} 不可用: ${r.error.message}`);
  return r;
};

// ---- 1) staging + 生成件 ----
const staging = path.join(OUT, "build", "channel");
fs.rmSync(path.join(OUT, "build"), { recursive: true, force: true });
fs.mkdirSync(staging, { recursive: true });
fs.cpSync(SRC, staging, { recursive: true });

const generated = emitChannelFiles(createExampleSubset());
for (const [rel, content] of Object.entries(generated)) {
  const dest = path.join(staging, rel);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(dest, content);
}
pass(`生成件入 staging（${Object.keys(generated).join(" + ")}）`);

// ---- 2) zip ----
const zipPath = path.join(OUT, "polypack-spike-channel.zip");
fs.mkdirSync(OUT, { recursive: true });
fs.rmSync(zipPath, { force: true });
const zr = run("zip", ["-X", "-r", "-q", zipPath, "manifest", "source", "components"], { cwd: staging });
if (zr.status !== 0) fail(`zip 失败: ${zr.stderr?.trim()}`);
else pass(`zip 生成: ${path.relative(process.cwd(), zipPath)}（${fs.statSync(zipPath).size} B）`);

// ---- 3) 结构自检 ----
const lr = run("unzip", ["-Z1", zipPath]);
if (lr.status !== 0) {
  fail(`unzip 列目录失败: ${lr.stderr?.trim()}`);
} else {
  const entries = lr.stdout.split("\n").map((s) => s.trim()).filter(Boolean);
  const required = [
    "manifest",
    "source/main.brs",
    "source/focusgraph.brs",
    "components/PolypackScene.xml",
    "components/PolypackScene.brs",
  ];
  for (const rel of required) {
    entries.includes(rel) ? pass(`zip 条目: ${rel}`) : fail(`zip 缺条目: ${rel}`);
  }
  const badPath = entries.find((e) => e.startsWith("/") || e.includes("..") || e.includes("\\"));
  badPath ? fail(`zip 条目路径异常: ${badPath}`) : pass("zip 条目路径形态（无绝对/回溯/反斜杠）");
  const roots = new Set(entries.map((e) => e.split("/")[0]));
  const allowed = new Set(["manifest", "source", "components", "images", "fonts", "locale"]);
  const stray = [...roots].filter((r) => !allowed.has(r));
  stray.length === 0 ? pass("zip 根布局（manifest + source/ + components/）") : fail(`zip 根多出未知条目: ${stray.join(", ")}`);
}

const read = (rel) => fs.readFileSync(path.join(staging, rel), "utf8");

// manifest 必填字段（rokudev/samples 官方样例字段集的最小子集）
const manifest = read("manifest");
const manifestFields = Object.fromEntries(
  manifest.split("\n").map((l) => l.trim()).filter((l) => l !== "" && !l.startsWith("#")).map((l) => {
    const i = l.indexOf("=");
    return [l.slice(0, i), l.slice(i + 1)];
  }),
);
for (const key of ["title", "major_version", "minor_version", "build_version"]) {
  manifestFields[key] ? pass(`manifest.${key} = ${manifestFields[key]}`) : fail(`manifest 缺必填字段: ${key}`);
}
for (const key of ["major_version", "minor_version", "build_version"]) {
  if (manifestFields[key] && !/^\d+$/.test(manifestFields[key])) fail(`manifest.${key} 非数字: ${manifestFields[key]}`);
}

// XML 良构 + 根组件形态
const xml = read("components/PolypackScene.xml");
const wf = xmlWellFormed(xml);
wf.ok ? pass("SceneGraph XML 良构（自检扫描器）") : fail(`XML 不良构: ${wf.error}`);
/<component\s+name="PolypackScene"\s+extends="Scene"/.test(xml)
  ? pass('根组件 <component name="PolypackScene" extends="Scene">')
  : fail("根组件形态不符");
/initialFocus="node_[a-z]+"/.test(xml) ? pass("initialFocus 已声明") : fail("initialFocus 缺失");

// 焦点表可解析（深等价由 test/emit.test.js 钉住）
const brs = read("source/focusgraph.brs");
const tableLine = brs.split("\n").find((l) => l.trim().startsWith("return "));
try {
  const table = JSON.parse(tableLine.trim().slice("return ".length));
  table.order.length > 0 && Object.keys(table.edges).length === table.order.length
    ? pass(`焦点表可解析（${table.order.length} 节点，initial=${table.initial}）`)
    : fail("焦点表形态不符");
} catch (e) {
  fail(`焦点表解析失败: ${e.message}`);
}

// BRS 标记（形态检查——BrightScript 无本机执行器，行为验证属真机侧载）
const mainBrs = read("source/main.brs");
/roSGScreen/.test(mainBrs) && /CreateScene\("PolypackScene"\)/.test(mainBrs)
  ? pass("main.brs 入口形态（roSGScreen + CreateScene）")
  : fail("main.brs 入口形态不符");
const sceneBrs = read("components/PolypackScene.brs");
/function onKeyEvent\(/.test(sceneBrs) && /\.setFocus\(/.test(sceneBrs)
  ? pass("PolypackScene.brs 交互逻辑形态（onKeyEvent + setFocus）")
  : fail("PolypackScene.brs 交互逻辑形态不符");

// ---- 4) 侧载自检 ----
const target = process.env.ROKU_DEV_TARGET;
if (process.env.POLY_ROKU_REAL !== "1") {
  skip("无真机侧载（结构自检替代）。真跑: POLY_ROKU_REAL=1 + ROKU_DEV_TARGET=<ip>（+ ROKU_DEV_PASSWORD）");
  console.log("      手册（依据 rokudev/hello-world makefile/app.mk）：");
  console.log("      1) Roku 设备开开发者模式（设置 → 系统 → 开发者模式），记 IP 与开发者密码");
  console.log("      2) export ROKU_DEV_TARGET=<ip> ROKU_DEV_PASSWORD=<pwd> POLY_ROKU_REAL=1");
  console.log(`      3) 重跑本脚本；或手动: curl --user rokudev:<pwd> --digest -F "mysubmit=Install" -F "archive=@${zipPath}" http://<ip>/plugin_install`);
  console.log("      4) 结果页 Success 即侧载成功；另: ECP 设备描述 http://<ip>:8060/");
} else if (!target) {
  fail("POLY_ROKU_REAL=1 但未设 ROKU_DEV_TARGET");
} else {
  const ecp = run("curl", ["--connect-timeout", "2", "-s", `http://${target}:8060/`]);
  const name = /<friendlyName>([^<]*)<\/friendlyName>/.exec(ecp.stdout ?? "");
  if (ecp.status !== 0 || !name) {
    fail(`ECP 探测失败（设备不可达或非 Roku）: ${target}`);
  } else {
    pass(`ECP 设备: ${name[1]}（${target}）`);
    const pwd = process.env.ROKU_DEV_PASSWORD ?? "rokudev";
    const inst = run("curl", [
      "--user", `rokudev:${pwd}`, "--digest", "-s", "--show-error",
      "-F", "mysubmit=Install", "-F", `archive=@${zipPath}`,
      `http://${target}/plugin_install`,
    ]);
    const out = `${inst.stdout ?? ""}\n${inst.stderr ?? ""}`;
    /Success/i.test(out) ? pass("plugin_install 侧载成功") : fail(`侧载失败（响应片段）: ${out.slice(0, 300)}`);
  }
}

// ---- 汇总 ----
if (failures.length > 0) {
  console.log(`结构自检 FAIL（${failures.length} 项）`);
  process.exit(1);
}
console.log("结构自检 PASS（全部项通过；侧载真腿按 env 开关）");
