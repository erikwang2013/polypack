// T-M4-13 编排器：腿选择 → 驱动 → 结构化 result.json（schema polypack/tv-smoke/v1）+ 失败定位。
// 期望全部现场派生（不读手写值）：tv-example 档 = adapter-tv 焦点内核重放（scenarios.mjs）；
// native-host 档 = walks.mjs buildExpectations（经 run-walk.mjs 使用，另在结果里逐键自洽核对）。
//
// 用法：
//   node run.mjs --leg android [--serial <adb serial>] [--apk <宿主 apk>] [--logs <dir>]
//   node run.mjs --leg web [--targets tizen-tv,webos-tv] [--dist <ui dist>] [--chrome <路径>] [--logs <dir>]
//   node run.mjs --leg all          # 缺省：android + web + tizen/webOS 模拟器探测（缺席 SKIP）
// 缺省：dist = host/dist（vite build 产物）；logs = POLY_TV_SMOKE_LOGS 或 target/tv-smoke-logs。
// 退出码：任一目标 fail → 1；全 pass / skip（SKIP-with-print 注记）→ 0。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { stableStringify } from "./compare.mjs";
import { runAndroidLeg } from "./drivers/android.mjs";
import { probeSimulators } from "./drivers/simulator.mjs";
import { runWebTargets } from "./drivers/web.mjs";
import { deriveNativeHost, deriveTvExample } from "./scenarios.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const opt = (name) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? null : process.argv[i + 1];
};

const leg = opt("leg") ?? "all";
const logs = resolve(opt("logs") ?? process.env.POLY_TV_SMOKE_LOGS ?? resolve(HERE, "../../../target/tv-smoke-logs"));
const dist = resolve(opt("dist") ?? resolve(HERE, "host/dist"));
const webTargets = (opt("targets") ?? "tizen-tv,webos-tv").split(",").map((t) => t.trim()).filter((t) => t.length > 0);

/** 派生场景 → 驱动输入（expect 逐键对齐，键序即场景键序）。 */
function toSteps(scene) {
  return scene.keys.map((key, i) => ({
    key,
    expect: { result: scene.results[i], focus: scene.focus[i], clicks: scene.state[i].clicks, depth: scene.state[i].depth, exitRequested: scene.state[i].exitRequested },
  }));
}

/** Android 腿自洽核对：run-walk --json 的逐键期望 ≡ scenarios 现场派生（同一 walks.mjs 源）。 */
function crossCheck(record, derived) {
  if (record.status === "skip" || record.scenes.length === 0) return null;
  for (const scene of record.scenes) {
    const expectScene = derived.scenes.find((s) => s.id === scene.id);
    if (expectScene === undefined) return `[${scene.id}] 不在现场派生场景集`;
    if (scene.keys.join(",") !== expectScene.keys.join(",")) return `[${scene.id}] 键序列与现场派生不符`;
    for (const step of scene.steps) {
      const i = step.index - 1;
      if (stableStringify(step.expect.result) !== stableStringify(expectScene.results[i]) || step.expect.clicks !== expectScene.clicks?.[i]) {
        return `[${scene.id}] 第 ${step.index} 键（${step.key}）期望与现场派生不符（--json 记录漂移）`;
      }
    }
  }
  return null;
}

async function main() {
  const startedAt = new Date().toISOString();
  mkdirSync(logs, { recursive: true });
  console.log("期望现场派生：tv-example ← adapter-tv 焦点内核重放；native-host ← walks.mjs（不读手写值）");
  const records = [];

  if (leg === "android" || leg === "all") {
    const record = runAndroidLeg({ logs, serial: opt("serial"), apk: opt("apk") });
    const drift = crossCheck(record, await deriveNativeHost());
    if (drift !== null) {
      record.status = "fail";
      record.failure = { class: "channel", detail: drift };
    }
    records.push(record);
  }
  if (leg === "web" || leg === "all") {
    const derived = deriveTvExample();
    const scenes = derived.scenes.map((scene) => ({ id: scene.id, purpose: scene.purpose, initial: scene.initial, steps: toSteps(scene) }));
    records.push(...(await runWebTargets({ targets: webTargets, dist, logs, scenes, chromePath: opt("chrome") ?? null })));
  }
  if (leg === "all") records.push(...probeSimulators());

  const summary = { pass: 0, fail: 0, skip: 0, scenes: 0, steps: 0 };
  for (const record of records) {
    summary[record.status] += 1;
    for (const scene of record.scenes) {
      summary.scenes += 1;
      summary.steps += scene.steps.length;
    }
  }
  const result = { schema: "polypack/tv-smoke/v1", startedAt, finishedAt: new Date().toISOString(), node: process.version, leg, dist, logs, targets: records, summary };
  const resultFile = join(logs, "result.json");
  writeFileSync(resultFile, `${JSON.stringify(result, null, 2)}\n`);

  for (const record of records) {
    if (record.status === "skip") {
      console.log(`SKIP ${record.target}（${record.leg}）：${record.skipNote}`);
      continue;
    }
    const passed = record.scenes.filter((s) => s.status === "pass").length;
    console.log(`${record.status === "pass" ? "PASS" : "FAIL"} ${record.target}（${record.leg}）：${passed}/${record.scenes.length} 场景`);
  }
  console.log(`结果：pass ${summary.pass} / fail ${summary.fail} / skip ${summary.skip}（目标）；场景 ${summary.scenes}，键步 ${summary.steps}`);
  console.log(`结构化结果：${resultFile}`);
  for (const record of records) {
    if (record.status !== "fail") continue;
    console.log(`  定位 [${record.target}] ${record.failure.class}：${record.failure.detail}`);
    if (record.evidence?.log !== undefined) console.log(`  现场 ${record.evidence.log}`);
    else if (record.evidence?.logs !== undefined) console.log(`  现场 ${record.evidence.logs}`);
  }
  process.exit(summary.fail === 0 ? 0 : 1);
}

main().catch((err) => {
  console.error(`冒烟套件执行失败：${err.message}`);
  process.exit(1);
});
