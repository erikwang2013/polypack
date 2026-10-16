#!/usr/bin/env node
// 生成/刷新 tv-example 档对拍数据（T-M4-13；形态对齐 device-walks/emit-expectations.mjs）：
//   node ui/smoke/tv/emit-expectations.mjs           # 打印到 stdout（供对表/比对）
//   node ui/smoke/tv/emit-expectations.mjs --write   # 覆写同目录 expectations.json
// 期望值全部由 adapter-tv 焦点内核现场推导；改内核或改场景后跑 --write，再跑 node --test 守漂移。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { deriveTvExample } from "./scenarios.mjs";

const json = `${JSON.stringify(deriveTvExample(), null, 2)}\n`;
const file = path.join(path.dirname(fileURLToPath(import.meta.url)), "expectations.json");

if (process.argv.includes("--write")) {
  fs.writeFileSync(file, json);
  console.log(`已写入 ${file}`);
} else {
  process.stdout.write(json);
}
