// tokens 快照金样再生成（T-M4-01）：node golden/emit-tokens.mjs
// 金样 = { tokens, TV_SCALE, SAFE_AREA, platformStyle } 的 JSON——token 有意变更时重跑并评审
// 本文件的 diff（test/tokens.test.js 逐字段比对，漂移即红）。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { SAFE_AREA, TV_SCALE, platformStyle, tokens } from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const target = path.join(HERE, "tv-tokens.json");
const snapshot = { tokens, TV_SCALE, SAFE_AREA, platformStyle: platformStyle() };
fs.writeFileSync(target, `${JSON.stringify(snapshot, null, 2)}\n`);
console.log(`tokens 金样已写入 ${path.relative(process.cwd(), target)}`);
