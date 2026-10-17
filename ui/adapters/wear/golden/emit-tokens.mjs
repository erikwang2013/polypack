// 紧凑 tokens / 蒙皮 / 安全区盒模型快照金样再生成（T-M7-01）：node golden/emit-tokens.mjs
// 金样 = { WEAR_SCALE, WEAR_FORMS, SAFE_AREA, tokens, platformStyle, layouts } 的 JSON——
// token / 安全区有意变更时重跑并评审本文件 diff（test/tokens.test.js 逐字段比对，漂移即红）。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  SAFE_AREA,
  WEAR_FORMS,
  WEAR_SCALE,
  layoutPage,
  platformStyle,
  tokensByForm,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const target = path.join(HERE, "wear-tokens.json");
const snapshot = {
  WEAR_SCALE,
  WEAR_FORMS,
  SAFE_AREA,
  tokens: tokensByForm,
  platformStyle: {
    round: platformStyle(tokensByForm.round),
    square: platformStyle(tokensByForm.square),
  },
  layouts: {
    round: layoutPage(tokensByForm.round),
    square: layoutPage(tokensByForm.square),
  },
};
fs.writeFileSync(target, `${JSON.stringify(snapshot, null, 2)}\n`);
console.log(`wear 金样已写入 ${path.relative(process.cwd(), target)}`);
