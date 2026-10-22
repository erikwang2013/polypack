// T-M4-15：Roku 频道发射入口——polypack-bundle 打包器经
// `node scripts/emit.mjs <staging>` 调用；复用 spike 的
// `emitChannelFiles(createExampleSubset())`（不重写发射器），把两件生成件写进暂存树。
// 退出码：0 = 生成件已落盘；1 = 参数 / 发射失败（stderr 带原因，打包器按 ToolFailed 归因）。

import fs from "node:fs";
import path from "node:path";

import { emitChannelFiles } from "../src/emit.js";
import { createExampleSubset } from "../src/subset.js";

const staging = process.argv[2];
if (!staging) {
  console.error("用法: node scripts/emit.mjs <staging 目录>");
  process.exit(1);
}
try {
  const files = emitChannelFiles(createExampleSubset());
  for (const [rel, contents] of Object.entries(files)) {
    const target = path.join(staging, ...rel.split("/"));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, contents);
    console.log(`[EMIT] ${rel}（${Buffer.byteLength(contents)} 字节）`);
  }
} catch (err) {
  console.error(`[FAIL] 发射失败: ${err.message}`);
  process.exit(1);
}
