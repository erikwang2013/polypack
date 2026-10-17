// T-M7-04 设备冒烟流水线结构测试：经 ANDROID_HOME 注入假 adb，真跑 run.mjs 全链路
// （install → launch → 表单用例 → uninstall + 截图 / logcat 归档），并验证 SKIP-with-print
// 与「失败定位到具体表单步骤」语义。真机 / 模拟器缺席时由驱动自身 SKIP（README 复跑命令）。

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { apkFingerprint } from "../build-summary.mjs";

const RUN = fileURLToPath(new URL("../run.mjs", import.meta.url));

// 假 adb：满足设备门（online + watch）、安装 / 启动 / 输入 / dump / 截图 / logcat / 卸载；
// FAKE_ADB_OMIT_NOTE=1 时缺 "step: note" 锚点，用于失败定位。调用流水写 FAKE_ADB_LOG。
const FAKE_ADB = `#!/bin/sh
printf '%s\\n' "$*" >> "$FAKE_ADB_LOG"
case "$*" in
  devices*) printf 'List of devices attached\\nemulator-5554\\tdevice\\n' ;;
  *"getprop ro.build.characteristics"*) printf 'watch\\r\\n' ;;
  *"wm size"*) printf 'Physical size: 320x320\\n' ;;
  *"exec-out cat"*)
    if [ "\${FAKE_ADB_OMIT_NOTE:-0}" = "1" ]; then
      printf '<hierarchy><node text="Wear Form"/><node text="step: amount"/><node text="amount: 12"/></hierarchy>'
    else
      printf '<hierarchy><node text="Wear Form"/><node text="step: amount"/><node text="amount: 12"/><node text="step: note"/></hierarchy>'
    fi ;;
  *"logcat -d"*) printf '10-18 00:00:00.000 I/ReactNativeJS: wear smoke\\n' ;;
  *"screencap"*) printf 'PNGDATA' ;;
  *monkey*) printf 'Events injected: 1\\n' ;;
esac
exit 0
`;

function fakeAdbEnv(t) {
  const root = mkdtempSync(join(tmpdir(), "wear-fake-adb-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const adb = join(root, "platform-tools", "adb");
  mkdirSync(dirname(adb), { recursive: true });
  writeFileSync(adb, FAKE_ADB);
  chmodSync(adb, 0o755);
  const log = join(root, "adb.log");
  const env = { ...process.env, ANDROID_HOME: root, FAKE_ADB_LOG: log };
  delete env.ANDROID_SDK_ROOT;
  delete env.POLYPACK_WEAR_FORM_APK;
  return { root, log, env };
}

function runCli(env, args) {
  return spawnSync(process.execPath, [RUN, "--leg", "android", ...args], {
    encoding: "utf8",
    env,
    timeout: 120_000,
  });
}

test("生命周期与归档：install → launch → 三用例 → uninstall + 截图/logcat，result.json 带时间戳与构建摘要", (t) => {
  const { root, log, env } = fakeAdbEnv(t);
  const apk = join(root, "wear-form.apk");
  writeFileSync(apk, "fake-apk-bytes");
  const logs = join(root, "logs");
  const out = runCli(env, ["--serial", "emulator-5554", "--apk", apk, "--logs", logs]);
  assert.equal(out.status, 0, out.stderr);

  const result = JSON.parse(readFileSync(join(logs, "result.json"), "utf8"));
  assert.equal(result.schema, "polypack/wear-smoke/v1");
  assert.ok(Date.parse(result.startedAt) > 0, "缺 startedAt 时间戳");
  assert.ok(Date.parse(result.finishedAt) >= Date.parse(result.startedAt));
  assert.deepEqual(result.summary, { pass: 1, fail: 0, skip: 0, cases: 3, steps: 9 });
  assert.equal(result.build.apk.sha256, apkFingerprint(apk).sha256);
  assert.match(result.build.commit, /^[0-9a-f]{40}$/);
  assert.equal(typeof result.build.dirty, "boolean");

  const target = result.targets[0];
  assert.equal(target.status, "pass");
  assert.deepEqual(
    {
      install: target.lifecycle.install.ok,
      launch: target.lifecycle.launch.ok,
      uninstall: target.lifecycle.uninstall.ok,
    },
    { install: true, launch: true, uninstall: true },
  );
  for (const wearCase of target.cases) {
    assert.equal(wearCase.status, "pass");
    assert.equal(readFileSync(wearCase.screenshot, "utf8"), "PNGDATA", `${wearCase.id} 缺终态截图`);
    for (const step of wearCase.steps) assert.ok(existsSync(step.dump), `${wearCase.id}/${step.id} 缺 dump`);
  }
  assert.match(readFileSync(target.lifecycle.logcat, "utf8"), /wear smoke/);

  const adbLog = readFileSync(log, "utf8");
  const at = (needle) => {
    const index = adbLog.indexOf(needle);
    assert.ok(index >= 0, `adb 未执行：${needle}`);
    return index;
  };
  const install = at("install -r");
  const launch = at("monkey");
  const uninstall = at("uninstall com.polypack.weatform");
  assert.ok(install < launch && launch < uninstall, "生命周期顺序应为 install → launch → uninstall");
  at("input text 12");
  at("input swipe");
  at("input keyevent 4");
  at("logcat -c");
  at("logcat -d");
});

test("SKIP 语义：无 APK → 全 SKIP、退出 0、result.json 照写（不静默降级）", (t) => {
  const { root, env } = fakeAdbEnv(t);
  const logs = join(root, "logs");
  const out = runCli(env, ["--serial", "emulator-5554", "--logs", logs]);
  assert.equal(out.status, 0, out.stderr);
  assert.match(out.stdout, /SKIP wearos（android）：.*APK/);

  const result = JSON.parse(readFileSync(join(logs, "result.json"), "utf8"));
  assert.deepEqual(result.summary, { pass: 0, fail: 0, skip: 1, cases: 0, steps: 0 });
  assert.equal(result.targets[0].status, "skip");
  assert.match(result.targets[0].skipNote, /APK/);
  assert.equal(result.build.apk, null);
});

test("失败定位：缺表单锚点 → [swipe/swipe-to-note]、截图/dump 归档、退出 1", (t) => {
  const { root, env } = fakeAdbEnv(t);
  const apk = join(root, "wear-form.apk");
  writeFileSync(apk, "fake-apk-bytes");
  const logs = join(root, "logs");
  const out = runCli({ ...env, FAKE_ADB_OMIT_NOTE: "1" }, [
    "--serial",
    "emulator-5554",
    "--apk",
    apk,
    "--logs",
    logs,
  ]);
  assert.equal(out.status, 1);
  assert.match(out.stdout, /\[swipe\/swipe-to-note\]/);

  const result = JSON.parse(readFileSync(join(logs, "result.json"), "utf8"));
  assert.equal(result.summary.fail, 1);
  const failure = result.targets[0].failure;
  assert.equal(failure.case, "swipe");
  assert.equal(failure.step, "swipe-to-note");
  assert.match(failure.locate, /^\[swipe\/swipe-to-note\] 期望 step: note/);
  assert.ok(existsSync(failure.screenshot), "失败截图未归档");
  assert.ok(existsSync(failure.dump), "失败 dump 未归档");
});
