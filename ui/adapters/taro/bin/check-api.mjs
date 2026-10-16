#!/usr/bin/env node
// 宿主 API 白名单构建期预检（T-M3-05）：扫描页面源码，越界即非零退出。
// poly-bundle 小程序打包器把它排进编译前命令链（构建期拒绝，见 src/apiguard.js 的口径）。
//
// 用法: polypack-taro-api <文件或目录>... [--allow name,name]

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import { checkSource, formatViolations } from "../src/apiguard.js";

const SCANNED = /\.(?:[cm]?[jt]sx?)$/;
const SKIPPED_DIRS = new Set(["node_modules", "dist", ".git", ".polypack"]);

/** 收集扫描目标：文件原样；目录递归（跳 node_modules / dist / .git）。 */
function collect(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) return SCANNED.test(target) ? [target] : [];
  const out = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRS.has(entry.name)) out.push(...collect(path.join(target, entry.name)));
    } else if (SCANNED.test(entry.name)) {
      out.push(path.join(target, entry.name));
    }
  }
  return out;
}

const args = process.argv.slice(2);
const allow = [];
const targets = [];
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === "--allow") {
    allow.push(...(args[(i += 1)] ?? "").split(",").filter(Boolean));
  } else {
    targets.push(args[i]);
  }
}
if (targets.length === 0) {
  console.error("用法: polypack-taro-api <文件或目录>... [--allow name,name]");
  process.exit(2);
}

let violations = 0;
let scanned = 0;
for (const target of targets) {
  for (const file of collect(target)) {
    scanned += 1;
    const report = checkSource(fs.readFileSync(file, "utf8"), { filename: file, allow });
    violations += report.violations.length;
    for (const line of formatViolations(report)) console.error(line);
  }
}
if (violations > 0) {
  console.error(
    `宿主 API 白名单预检失败：${violations} 处越界调用（${scanned} 个文件）；确为示例请加同行标记 poly:allow-api`,
  );
  process.exit(1);
}
console.log(`宿主 API 白名单预检通过：${scanned} 个文件无越界调用`);
