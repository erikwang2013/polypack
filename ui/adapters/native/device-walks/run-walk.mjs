// T-M4-05 走查执行器：把 device-walks 的内核派生期望（walks.mjs 现场推导——不读手写值）
// 落到所连真机 / 模拟器上跑。走查全程只发 `adb shell input keyevent`（无任何指针输入）：
// 逐键对拍 ① POLYPACK_KEY 通道（ReactNativeJS tag，单参数 JSON）② 界面「点击计数」文本；
// 失败给出走查 id / 键号 / 键名定位；证据（logcat 帧 + uiautomator dump 文本）归档到 --logs。
//
// 真机形态分派（由内核键表派生，非手写例外）：键 ∈ expectations.keymap → 对拍内核结果行；
// 键 ∉ keymap（如 KeyQ）→ RN 原生事件源不发 onHWKeyEvent（ReactAndroidHWInputDeviceHelper
// 的 KEY_EVENTS_ACTIONS，见 device-walks/README「观测通道」注，源码已核），到不了宿主：
// 真机期望 = 零新增 POLYPACK_KEY 行 + 计数不变（焦点不动）；内核直喂的 {type:"ignored"}
// 一行只在 JS 层出现，由 JS 单测覆盖。
//
// 用法：node device-walks/run-walk.mjs --serial <adb serial> --logs <dir> [--apk <宿主 apk>] [--walk <id>]... [--json <file>]
// 前置：宿主 app 已装（com.polypack.host）；TV 形态须无指针编译（host/app.json polypack.pointer="none"）。
// --json：机器可读走查记录（T-M4-13 冒烟套件汇入 result.json 用；不加时输出与行为不变）。

import { execFileSync } from "node:child_process";
import { isDeepStrictEqual } from "node:util";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { buildExpectations } from "./walks.mjs";

const PKG = "com.polypack.host";
const DUMP = "/sdcard/polypack-walk.xml";
const KEY_LOG_PREFIX = "POLYPACK_KEY ";

const args = process.argv.slice(2);
const opt = (name) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? null : args[i + 1];
};
const only = args.flatMap((a, i) => (a === "--walk" ? [args[i + 1]] : []));
const serial = opt("serial") ?? process.env.ANDROID_SERIAL ?? null;
const logs = opt("logs");
const apk = opt("apk");
const jsonOut = opt("json");

if (serial === null || logs === null || serial.startsWith("--") || logs.startsWith("--")) {
  console.error(
    "用法：node device-walks/run-walk.mjs --serial <adb serial> --logs <dir> [--apk <宿主 apk>] [--walk <id>]... [--json <file>]",
  );
  process.exit(2);
}

const adb = (argv, timeout = 60000) =>
  execFileSync("adb", ["-s", serial, ...argv], { encoding: "utf8", timeout });
const shell = (cmd) => adb(cmd.split(" "));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 当前 logcat 里的 POLYPACK_KEY 行（清缓冲后每键恰一行，多一行即双派发）。 */
const keyLines = () =>
  adb(["logcat", "-d", "-s", "ReactNativeJS"])
    .split("\n")
    .filter((line) => line.includes(KEY_LOG_PREFIX));

/** uiautomator dump（先删旧文件——设备链实测：旧文件在场会读到残影）。
 *  本负载下 uiautomator 偶发瞬时回 "null root node"（实测：键后紧跟 dump 时）→ 有界重试，
 *  重试到见到语义标记或次数用尽（返回最后一次结果/空串，由调用方判定）。 */
const dumpUi = async (tries = 4) => {
  let xml = "";
  for (let attempt = 0; attempt < tries; attempt++) {
    shell(`shell rm -f ${DUMP}`);
    try {
      shell(`shell uiautomator dump ${DUMP}`);
    } catch {
      /* 未就绪：忽略，下面按空界面处理 */
    }
    try {
      xml = adb(["shell", "cat", DUMP]);
    } catch {
      xml = "";
    }
    if (xml.includes("点击计数")) return xml;
    await sleep(400);
  }
  return xml;
};

const clicksOf = (xml) => {
  const hit = xml.match(/点击计数：(\d+)/);
  return hit === null ? null : Number(hit[1]);
};

async function waitForUi() {
  for (let i = 0; i < 60; i++) {
    const xml = await dumpUi(1); // 外层已有循环节奏，内层不再叠加重试
    if (xml.includes("点击计数")) return xml;
    await sleep(500);
  }
  throw new Error(`界面未就绪：等不到「点击计数」节点（${PKG} 是否在前台？）`);
}

async function main() {
  if (apk !== null) {
    try {
      adb(["install", "-r", apk], 300000); // 大 APK + 高负载模拟器：install 可远超 60s
    } catch {
      // 设备上残留旧签名安装（debug.keystore 每次现生成）→ -r 会 INSTALL_FAILED_UPDATE_INCOMPATIBLE，
      // 清装重来；走查本就按「冷启动 + 前置重置」设计，无状态损失。
      shell(`uninstall ${PKG}`);
      adb(["install", apk], 300000);
    }
    console.log(`已安装宿主包：${apk}`);
  }
  mkdirSync(logs, { recursive: true });

  const expectations = buildExpectations();
  const walks = expectations.walks.filter((w) => only.length === 0 || only.includes(w.id));
  if (walks.length === 0) throw new Error(`--walk 无命中：${only.join(", ")}`);
  // 内核键表（keymap）即设备可达集合：表内键可由 adb 送达宿主并逐键产生 POLYPACK_KEY 行，
  // 表外键到不了宿主（见文件头注），其真机形态按「零新增行 + 计数不变」对拍。
  const reachable = new Set(expectations.keymap.map((m) => m.kernel));

  const failures = [];
  const records = [];
  for (const walk of walks) {
    const frames = [];
    const steps = [];
    let locate = null;
    // 前置重置：重启宿主回初始焦点与初始计数（每走查独立，前步状态不渗漏）
    for (const cmd of expectations.precondition.reset) shell(cmd);
    await waitForUi();
    adb(["logcat", "-c"]);

    // 已映射键的行序 = 到该键为止的已映射键计数（未映射键到不了宿主，不占行）
    let lineIdx = 0;
    for (let i = 0; i < walk.keys.length; i++) {
      const forwarded = reachable.has(walk.keys[i]);
      shell(walk.adb[i]);
      let lines = [];
      if (forwarded) {
        lineIdx += 1;
        for (let t = 0; t < 40; t++) {
          lines = keyLines();
          if (lines.length >= lineIdx) break;
          await sleep(200);
        }
      } else {
        // 未映射键：给足沉降窗后应「零新增行」（见文件头注）
        await sleep(1000);
        lines = keyLines();
      }
      const expected = walk.expect.results[i];
      let got = null;
      let parseNote = "";
      if (!forwarded) {
        parseNote = "（未映射键：真机形态=零新增行+焦点不动；内核直喂期望另见 walks.mjs）";
      } else if (lines.length === lineIdx) {
        try {
          got = JSON.parse(
            lines[lineIdx - 1].slice(lines[lineIdx - 1].indexOf(KEY_LOG_PREFIX) + KEY_LOG_PREFIX.length),
          );
        } catch (err) {
          parseNote = `（JSON 解析失败：${err.message}）`;
        }
      } else {
        parseNote = `（POLYPACK_KEY 行数 ${lines.length} ≠ ${lineIdx}）`;
      }
      const xml = await dumpUi();
      const gotClicks = clicksOf(xml);
      const ok = forwarded
        ? isDeepStrictEqual(got, expected) && gotClicks === walk.expect.clicks[i]
        : lines.length === 0 && gotClicks === walk.expect.clicks[i];
      frames.push(
        [
          `--- 键 ${i + 1}/${walk.keys.length}：${walk.keys[i]}（adb ${walk.adb[i]}）`,
          `期望结果：${
            forwarded
              ? JSON.stringify(expected)
              : `真机形态：零新增 POLYPACK_KEY 行 + 计数不变（内核直喂 ${JSON.stringify(expected)}，不经设备）`
          }${parseNote}`,
          `实际结果：${forwarded ? (got === null ? "（无）" : JSON.stringify(got)) : `POLYPACK_KEY 行数 ${lines.length}（期望 0）`}`,
          `期望计数：${walk.expect.clicks[i]} / 实际计数：${gotClicks}`,
          `logcat 帧：${lines.length === 0 ? "（空）" : lines.map((l) => l.trim()).join(" | ")}`,
          `界面证据：${xml.split("\n").filter((l) => l.includes("点击计数")).join(" ").slice(0, 400)}`,
        ].join("\n"),
      );
      const stepLocate = ok
        ? null
        : forwarded
          ? `[${walk.id}] 第 ${i + 1} 键（${walk.keys[i]}）不符：期望 ${JSON.stringify(expected)} / 计数 ${walk.expect.clicks[i]}，实际 ${JSON.stringify(got)} / 计数 ${gotClicks}${parseNote}`
          : `[${walk.id}] 第 ${i + 1} 键（${walk.keys[i]}）不符：未映射键真机应零新增行，实际 ${lines.length} 行 / 计数 ${gotClicks}`;
      if (stepLocate !== null && locate === null) locate = stepLocate;
      steps.push({
        index: i + 1,
        key: walk.keys[i],
        adb: walk.adb[i],
        forwarded,
        expect: { result: expected, clicks: walk.expect.clicks[i] },
        actual: { result: got, clicks: gotClicks, lineCount: lines.length },
        ok,
        locate: stepLocate,
      });
    }

    const pass = locate === null;
    records.push({ id: walk.id, purpose: walk.purpose, keys: walk.keys, ok: pass, locate, steps });
    if (!pass) failures.push({ id: walk.id, locate });
    writeFileSync(
      path.join(logs, `${walk.id}.log`),
      [
        `走查 ${walk.id}：${pass ? "PASS" : "FAIL"}（${walk.purpose}）`,
        `键序列：${walk.keys.join(" → ")}`,
        ...frames,
        "",
      ].join("\n"),
    );
    console.log(`走查 ${pass ? "PASS" : "FAIL"} ${walk.id}（${walk.purpose}）`);
  }

  console.log(`日志归档：${logs}`);
  if (jsonOut !== null && !jsonOut.startsWith("--")) {
    writeFileSync(
      jsonOut,
      `${JSON.stringify({ schema: "polypack/tv-smoke/android-walk/v1", serial, apk, logs, ok: failures.length === 0, walks: records }, null, 2)}\n`,
    );
  }
  if (failures.length === 0) {
    console.log(
      `无指针完成核心任务：PASS（${walks.length}/${walks.length} 走查仅由 D-pad / OK / Back 键事件驱动，逐键对拍内核期望）`,
    );
    process.exit(0);
  }
  console.log(`无指针完成核心任务：FAIL（${failures.length}/${walks.length} 走查未过）`);
  for (const f of failures) console.log(`  定位 ${f.locate}`);
  for (const f of failures) console.log(`  现场 ${path.join(logs, `${f.id}.log`)}`);
  process.exit(1);
}

main().catch((err) => {
  console.error(`走查执行失败：${err.message}`);
  process.exit(1);
});
