// Android TV 腿驱动：包装 T-M4-05 走查执行器（run-walk.mjs）——复用硬要求，不另造驱动。
// 逐键对拍与失败定位由 run-walk.mjs 自带（期望现场内核推导；截断在首个不符键）；
// 本驱动只补两件事：① 经 --json 取回逐走查逐键结构化记录（汇入 result.json）；
// ② 前置门禁（adb / 设备 / 宿主包）转 SKIP-with-print（沿用设备侧既有口径：任一缺席不红）。

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { descriptorFailure } from "../compare.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const RUN_WALK = resolve(HERE, "../../../adapters/native/device-walks/run-walk.mjs");
const HOST_APK = resolve(HERE, "../../../adapters/native/host/android/app/build/outputs/apk/release/app-release.apk");

const skip = (skipNote) => ({ target: "android-tv", leg: "device-walk", status: "skip", skipNote, scenes: [] });

/** adb 在场 + 至少一台授权设备（优先显式 serial；口径同 packagers_android_tv.rs 的设备门）。 */
function pickSerial(explicit) {
  if (spawnSync("adb", ["version"], { encoding: "utf8" }).error) return { error: "adb 不在 PATH" };
  const out = spawnSync("adb", ["devices"], { encoding: "utf8" }).stdout ?? "";
  const devices = out
    .split("\n")
    .slice(1)
    .map((line) => line.trim().split(/\s+/))
    .filter(([serial, state]) => serial && state === "device")
    .map(([serial]) => serial);
  if (devices.length === 0) return { error: "无 adb 授权设备（emulator-5554 / 5556 是否已启动并授权？）" };
  if (explicit !== null && devices.includes(explicit)) return { serial: explicit };
  return { serial: devices[0], note: explicit === null ? null : `指定 --serial ${explicit} 不在场，回落 ${devices[0]}` };
}

/**
 * 跑 Android TV 腿。
 *
 * @param {object} opts
 * @param {string} opts.logs 证据目录（run-walk 的 per-walk log 与 --json 都落此）
 * @param {string|null} [opts.serial] adb serial（缺省取首个授权设备）
 * @param {string|null} [opts.apk] 宿主 APK（缺省 POLYPACK_TV_HOST_APK 或 host release 产物）
 */
export function runAndroidLeg({ logs, serial = null, apk = null }) {
  const picked = pickSerial(serial);
  if (picked.error !== undefined) return skip(`SKIP：${picked.error}；Android TV 腿未执行`);
  const apkPath = apk ?? process.env.POLYPACK_TV_HOST_APK ?? (existsSync(HOST_APK) ? HOST_APK : null);
  if (apkPath === null || !existsSync(apkPath)) {
    return skip(`SKIP：宿主 APK 缺席（--apk / POLYPACK_TV_HOST_APK / ${HOST_APK} 皆无）——先 gradle assembleRelease；Android TV 腿未执行`);
  }
  mkdirSync(logs, { recursive: true });
  const jsonOut = join(logs, "android-walk.json");
  const run = spawnSync(process.execPath, [RUN_WALK, "--serial", picked.serial, "--logs", logs, "--apk", apkPath, "--json", jsonOut], { encoding: "utf8" });
  const note = picked.note == null ? "" : `（${picked.note}）`;
  const head = `Android TV 腿（${picked.serial}${note}）：run-walk 退出码 ${run.status}`;
  process.stdout.write(`${head}\n${run.stdout ?? ""}${run.stderr ?? ""}`);

  if (!existsSync(jsonOut)) {
    return { target: "android-tv", leg: "device-walk", status: "fail", scenes: [], failure: { class: "channel", detail: `${head}；未产出结构化记录（run-walk 前置失败——装入告警 / 界面未就绪？现场见 ${logs}）` } };
  }
  const walk = JSON.parse(readFileSync(jsonOut, "utf8"));
  const scenes = walk.walks.map((w) => ({
    id: w.id,
    purpose: w.purpose,
    keys: w.keys,
    status: w.ok ? "pass" : "fail",
    steps: w.steps.map((s) => {
      const failure = s.ok
        ? null
        : {
            // 同判据（compare.mjs）：先描述符再计数；详情文本用 run-walk 的定位串（单一作者）
            class: descriptorFailure(s.key, s.expect.result, s.actual.result)?.class ?? "route",
            detail: s.locate ?? `[${w.id}] 第 ${s.index} 键（${s.key}）不符`,
          };
      return { index: s.index, key: s.key, expect: s.expect, observed: s.actual, ok: s.ok, failures: failure === null ? [] : [failure], evidence: { adb: s.adb, forwarded: s.forwarded } };
    }),
  }));
  const failed = scenes.find((s) => s.status === "fail");
  const step = failed?.steps.find((s) => !s.ok);
  return {
    target: "android-tv",
    leg: "device-walk",
    status: failed ? "fail" : "pass",
    scenes,
    ...(step ? { failure: { class: step.failures[0].class, detail: step.failures[0].detail } } : {}),
    evidence: { logs, json: jsonOut, serial: picked.serial, apk: apkPath, exit: run.status },
  };
}
