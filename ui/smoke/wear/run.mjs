// T-M7-03 Wear 表单冒烟编排器：腿（当前 = android）→ 驱动 → 结构化 result.json
// （schema polypack/wear-smoke/v1）+ 失败定位到具体表单步骤（case/step 期望 vs 实际）。
//
// 用法：
//   node ui/smoke/wear/run.mjs --leg android [--serial <adb serial>]
//        [--apk <Wear 表单 APK>] [--package <应用包名>] [--logs <dir>]
// 缺省：apk = $POLYPACK_WEAR_FORM_APK；package = $POLYPACK_WEAR_FORM_PACKAGE 或
// com.polypack.weatform；logs = $POLYPACK_WEAR_SMOKE_LOGS 或 target/wear-smoke-logs。
// 退出码：任一 case fail → 1；全 pass / SKIP-with-print → 0（SKIP 注记恒打印）。

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { collectBuildSummary } from "./build-summary.mjs";
import { localizeFailure, runAndroidLeg } from "./drivers/android.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const opt = (name) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? null : process.argv[i + 1];
};

const leg = opt("leg") ?? "android";
const logs = resolve(
  opt("logs") ?? process.env.POLYPACK_WEAR_SMOKE_LOGS ?? resolve(HERE, "../../../target/wear-smoke-logs"),
);
const apkArg = opt("apk") ?? process.env.POLYPACK_WEAR_FORM_APK ?? "";
const apk = apkArg === "" ? null : resolve(apkArg);
const packageName = opt("package") ?? process.env.POLYPACK_WEAR_FORM_PACKAGE ?? "com.polypack.weatform";

function main() {
  if (leg !== "android" && leg !== "all") {
    throw new Error(`未知腿 "${leg}"（当前仅 android；all = android）`);
  }
  const startedAt = new Date().toISOString();
  mkdirSync(logs, { recursive: true });
  const record = runAndroidLeg({ logs, serial: opt("serial"), apk, packageName });
  const records = [record];

  const summary = { pass: 0, fail: 0, skip: 0, cases: 0, steps: 0 };
  for (const target of records) {
    summary[target.status] += 1;
    for (const wearCase of target.cases) {
      summary.cases += 1;
      summary.steps += wearCase.steps.length;
    }
  }
  for (const target of records) {
    if (target.failure !== undefined) target.failure.locate = localizeFailure(target.failure);
  }
  const result = {
    schema: "polypack/wear-smoke/v1",
    startedAt,
    finishedAt: new Date().toISOString(),
    node: process.version,
    leg,
    apk,
    build: collectBuildSummary({ apk }),
    package: packageName,
    logs,
    targets: records,
    summary,
  };
  const resultFile = join(logs, "result.json");
  writeFileSync(resultFile, `${JSON.stringify(result, null, 2)}\n`);

  for (const target of records) {
    if (target.status === "skip") {
      console.log(`SKIP ${target.target}（${target.leg}）：${target.skipNote}`);
      continue;
    }
    const passed = target.cases.filter((wearCase) => wearCase.status === "pass").length;
    console.log(
      `${target.status === "pass" ? "PASS" : "FAIL"} ${target.target}（${target.leg}）：${passed}/${target.cases.length} 表单用例`,
    );
    for (const wearCase of target.cases) {
      console.log(`  ${wearCase.status === "pass" ? "PASS" : "FAIL"} ${wearCase.id}：${wearCase.purpose}`);
    }
  }
  console.log(
    `结果：pass ${summary.pass} / fail ${summary.fail} / skip ${summary.skip}（目标）；表单用例 ${summary.cases}，步骤 ${summary.steps}`,
  );
  console.log(`结构化结果：${resultFile}`);
  for (const target of records) {
    if (target.status !== "fail" || target.failure === undefined) continue;
    console.log(`  定位 ${localizeFailure(target.failure)}`);
    if (target.failure.screenshot !== undefined) console.log(`  截图 ${target.failure.screenshot}`);
  }
  process.exit(summary.fail === 0 ? 0 : 1);
}

try {
  main();
} catch (err) {
  console.error(`冒烟套件执行失败：${err.message}`);
  process.exit(1);
}
