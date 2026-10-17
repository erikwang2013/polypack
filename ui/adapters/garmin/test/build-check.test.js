import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const script = join(ROOT, "scripts", "build-and-check.mjs");

test("构建自检脚本真跑：转译 / 子集 / 结构全 PASS；编译器缺席按 SKIP-with-print", () => {
  const run = spawnSync(process.execPath, [script], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 120_000,
  });
  assert.equal(run.status, 0, `退出码 ${run.status}\n${run.stdout}\n${run.stderr}`);
  assert.match(run.stdout, /\[PASS\] round: 转译 \+ 子集断言 \+ 工程结构自检（6 文件）/);
  assert.match(run.stdout, /\[PASS\] round: 确定性（两跑字节一致，app id [0-9a-f]{32}）/);
  assert.match(run.stdout, /\[PASS\] square: 转译 \+ 子集断言 \+ 工程结构自检（6 文件）/);
  assert.match(run.stdout, /\[PASS\] square: 确定性/);
  assert.doesNotMatch(run.stdout, /\[FAIL\]/);

  // 本仓库 CI / 开发机均无 Connect IQ SDK：编译器腿必须是 SKIP-with-print，而非静默跳过。
  if (/\[PASS\] monkeyc 探测/.test(run.stdout)) {
    assert.match(run.stdout, /\[SKIP\] CIQ_DEVKEY|\[PASS\] round: monkeyc 编译通过/);
  } else {
    assert.match(run.stdout, /\[SKIP\] monkeyc 不在 PATH/);
    assert.match(run.stdout, /\[SKIP\] Connect IQ 编译器 \/ SDK 缺席/);
  }

  assert.equal(existsSync(join(ROOT, "dist", "garmin-round", "manifest.xml")), true);
  assert.equal(existsSync(join(ROOT, "dist", "garmin-round", "source", "PolypackView.mc")), true);
  assert.equal(existsSync(join(ROOT, "dist", "garmin-square", "manifest.xml")), true);
});
