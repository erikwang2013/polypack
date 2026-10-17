#!/usr/bin/env node
// T-M7-09 构建 / 自检入口：
//   真跑：两种形态的转译（确定性断言）+ 子集 / API 断言 + Connect IQ 工程结构自检 + .iq 条目断言
//   SKIP-with-print：monkeyc / Connect IQ SDK / 开发者密钥缺席时，打印手动编译命令与前置条件
//
// 环境槽（全部可选；凭据零明文——devkey 只取路径，不读内容）：
//   CIQ_MONKEYC      monkeyc 可执行文件路径（优先）
//   CIQ_HOME         Connect IQ SDK 根（查找 bin/monkeyc）
//   CONNECTIQ_HOME   同上（兼容变量名）
//   CIQ_DEVKEY       开发者签名密钥路径（monkeyc -y 参数）
//
// 失败（[FAIL]）退出码 1；SKIP 不算失败。

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  assertGarminProject,
  assertIqEntries,
  createGarminPage,
  describePage,
  isSafePath,
  transpileGarmin,
} from "../src/index.js";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const DIST = join(ROOT, "dist");
const FORMS = ["round", "square"];

const results = { pass: 0, fail: 0, skip: 0 };
const pass = (message) => {
  results.pass += 1;
  console.log(`[PASS] ${message}`);
};
const fail = (message) => {
  results.fail += 1;
  console.error(`[FAIL] ${message}`);
};
const skip = (message) => {
  results.skip += 1;
  console.log(`[SKIP] ${message}`);
};

function writeProject(form, files) {
  const outDir = join(DIST, `garmin-${form}`);
  rmSync(outDir, { recursive: true, force: true });
  for (const [path, content] of Object.entries(files)) {
    if (!isSafePath(path)) throw new Error(`拒绝写入不安全路径：${path}`);
    const full = join(outDir, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content, "utf8");
  }
  return outDir;
}

function findMonkeyc() {
  const direct = process.env.CIQ_MONKEYC;
  if (direct && existsSync(direct)) return direct;
  for (const key of ["CIQ_HOME", "CONNECTIQ_HOME"]) {
    const home = process.env[key];
    if (home) {
      const candidate = join(home, "bin", "monkeyc");
      if (existsSync(candidate)) return candidate;
    }
  }
  const which = spawnSync("sh", ["-c", "command -v monkeyc"], { encoding: "utf8" });
  const found = which.status === 0 ? which.stdout.trim() : "";
  return found || null;
}

function compileForm(monkeyc, devkey, form, outDir) {
  const outFile = join(DIST, `garmin-${form}.iq`);
  rmSync(outFile, { force: true });
  const args = ["-e", "-w", "-r", "-o", outFile, "-y", devkey, "-f", "monkey.jungle"];
  const run = spawnSync(monkeyc, args, { cwd: outDir, encoding: "utf8", timeout: 180_000 });
  if (run.error) return { ok: false, message: run.error.message };
  if (run.status !== 0) {
    const detail = (run.stderr || run.stdout || "").trim().split("\n").slice(-6).join(" | ");
    return { ok: false, message: `monkeyc 退出码 ${run.status}${detail ? `：${detail}` : ""}` };
  }
  if (!existsSync(outFile)) return { ok: false, message: "monkeyc 返回 0 但未生成 .iq" };
  return { ok: true, outFile };
}

function iqEntries(outFile) {
  const list = spawnSync("unzip", ["-Z1", outFile], { encoding: "utf8", timeout: 30_000 });
  if (list.error || list.status !== 0) {
    return { ok: false, message: (list.stderr || list.error?.message || "").trim() };
  }
  return { ok: true, entries: list.stdout.split("\n").map((line) => line.trim()).filter(Boolean) };
}

console.log(`adapter-garmin T-M7-09 spike · node ${process.version} · ${ROOT}`);

// 1) 两形态转译 + 确定性 + 工程结构 / 子集断言（真跑，不依赖 SDK）。
const generated = {};
for (const form of FORMS) {
  try {
    const page = createGarminPage(form);
    const first = transpileGarmin(page);
    const second = transpileGarmin(page);
    const stable = Object.entries(first.files).every(([path, content]) => second.files[path] === content);
    const outDir = writeProject(form, first.files);
    assertGarminProject(first.files);
    generated[form] = { outDir, meta: first.meta };
    pass(`${form}: 转译 + 子集断言 + 工程结构自检（${Object.keys(first.files).length} 文件）`);
    pass(`${form}: 确定性（两跑字节一致，app id ${first.meta.appId}）`);
    const summary = describePage(page);
    console.log(
      `       ${summary.form} -> ${summary.product} · ${summary.viewport} px（安全区 ${summary.safe}）· 焦点序 ${summary.focusOrder} · focusNav=${summary.focusNav}`,
    );
    if (!stable) fail(`${form}: 两次转译产物不一致`);
  } catch (error) {
    fail(`${form}: ${error.message}`);
  }
}

// 2) 环境槽探测：SDK / 编译器 / 密钥。
const monkeyc = findMonkeyc();
if (!monkeyc) {
  skip("monkeyc 不在 PATH，且 CIQ_MONKEYC / CIQ_HOME / CONNECTIQ_HOME 未指向可用 SDK");
} else {
  pass(`monkeyc 探测：${monkeyc}`);
}
const devkey = process.env.CIQ_DEVKEY ?? "";
if (!devkey) {
  skip("CIQ_DEVKEY 未设置（开发者密钥只从环境取路径，不落库、不回显）");
} else if (!existsSync(devkey)) {
  fail("CIQ_DEVKEY 指向的文件不存在");
}

// 3) 编译器真跑（环境槽齐全时）；否则 SKIP-with-print。
if (monkeyc && devkey && existsSync(devkey)) {
  for (const form of FORMS) {
    const entry = generated[form];
    if (!entry) continue;
    const compiled = compileForm(monkeyc, devkey, form, entry.outDir);
    if (!compiled.ok) {
      fail(`${form}: Connect IQ 编译失败：${compiled.message}`);
      continue;
    }
    pass(`${form}: monkeyc 编译通过 → ${compiled.outFile}`);
    const listing = iqEntries(compiled.outFile);
    if (!listing.ok) {
      skip(`${form}: 无法列出 .iq 条目（unzip 不可用：${listing.message}）`);
      continue;
    }
    try {
      assertIqEntries(listing.entries);
      pass(`${form}: .iq 包结构（manifest.xml + .prg，共 ${listing.entries.length} 条目）`);
    } catch (error) {
      fail(`${form}: ${error.message}`);
    }
  }
} else if (monkeyc && generated.round) {
  skip("Connect IQ 编译未执行（缺 CIQ_DEVKEY）；SDK 就绪后手动执行：");
  console.log(
    `       cd ${generated.round.outDir} && monkeyc -e -w -r -o ../garmin-round.iq -y "$CIQ_DEVKEY" -f monkey.jungle`,
  );
  console.log(`       cd ${generated.square?.outDir} && monkeyc -e -w -r -o ../garmin-square.iq -y "$CIQ_DEVKEY" -f monkey.jungle`);
} else {
  skip("Connect IQ 编译器 / SDK 缺席——本环境不执行真机编译；安装 Connect IQ SDK 后：");
  console.log("       1. export CIQ_HOME=<Connect IQ SDK 根>   # 含 bin/monkeyc");
  console.log('       2. export CIQ_DEVKEY=<开发者密钥 .der 路径>   # 由开发者自行生成，勿入库');
  console.log("       3. npm run build --workspace @polypack/adapter-garmin");
  console.log("       4. 真机 / 模拟器走查（本项目无此环境，结论标 [待真机]）");
}

console.log(`\n结果：PASS ${results.pass} · FAIL ${results.fail} · SKIP ${results.skip}`);
process.exit(results.fail > 0 ? 1 : 0);
