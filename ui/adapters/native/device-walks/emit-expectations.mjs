#!/usr/bin/env node
// 生成/刷新设备走查对拍数据（T-M2-05）：
//   node device-walks/emit-expectations.mjs           # 打印到 stdout（供对表/比对）
//   node device-walks/emit-expectations.mjs --write   # 覆写同目录 expectations.json
// 期望值全部由焦点内核现场推导；改内核或改走查后跑 --write，再跑 node --test 守漂移。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { buildExpectations } from "./walks.mjs";

const json = `${JSON.stringify(buildExpectations(), null, 2)}\n`;
const file = path.join(path.dirname(fileURLToPath(import.meta.url)), "expectations.json");

if (process.argv.includes("--write")) {
  fs.writeFileSync(file, json);
  console.log(`已写入 ${file}`);
} else {
  process.stdout.write(json);
}
