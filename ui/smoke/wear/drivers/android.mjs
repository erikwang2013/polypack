// T-M7-03 Wear 表单 Android 腿：adb 设备门 → 安装 → 逐 case/step 注入 + uiautomator 观测。
// 任一环境槽缺席（adb / Wear 设备 / APK）→ status=skip + skipNote，打印后通过，绝不伪造真跑。
// 失败定位到 case/step：记录期望锚点、实际锚点与现场 dump（截图另存）。

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, statSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";

import { WEAR_FORM_CASES } from "../forms.mjs";

const DEFAULT_PACKAGE = "com.polypack.weatform";
const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

/** PATH / SDK 里找 adb（平台腿只跑 Linux / macOS runner，Windows 仅兜底）。 */
export function findAdb(env = process.env) {
  const name = process.platform === "win32" ? "adb.exe" : "adb";
  for (const root of [env.ANDROID_HOME, env.ANDROID_SDK_ROOT]) {
    if (!root) continue;
    const candidate = join(root, "platform-tools", name);
    if (existsSync(candidate)) return candidate;
  }
  for (const dir of (env.PATH ?? "").split(delimiter)) {
    if (dir.length === 0) continue;
    const candidate = join(dir, name);
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

/** `wm size` 输出 → 屏幕像素（模拟器 / 真机同源；解析失败返回 null）。 */
export function parseWmSize(output) {
  const match = /(\d+)x(\d+)/.exec(output ?? "");
  return match === null ? null : { width: Number(match[1]), height: Number(match[2]) };
}

/**
 * 表单动作 → adb 参数列表（`adb -s <serial>` 之后的部分）。
 * 滑动坐标按屏幕比例派生（圆 / 方屏同一口径），不写死设备像素。
 */
export function buildActionCommands(action, geometry) {
  switch (action.type) {
    case "launch":
      return [];
    case "text":
      return [["shell", "input", "text", String(action.value)]];
    case "swipe": {
      const { width, height } = geometry;
      const y = Math.round(height / 2);
      const x1 = Math.round(width * 0.75);
      const x2 = Math.round(width * 0.25);
      const [from, to] = action.direction === "left" ? [x1, x2] : [x2, x1];
      return [["shell", "input", "swipe", `${from}`, `${y}`, `${to}`, `${y}`, "200"]];
    }
    case "back":
      return [["shell", "input", "keyevent", "4"]];
    default:
      throw new Error(`未知表单动作：${action.type}`);
  }
}

/** uiautomator dump 的可见文本（text + content-desc；RN Text 与无障碍锚点都在此）。 */
export function extractTexts(xml) {
  return [...(xml ?? "").matchAll(/(?:text|content-desc)="([^"]*)"/g)]
    .map((match) => match[1])
    .filter((value) => value.length > 0);
}

/** 期望锚点缺失清单（空数组 = 该步通过）。 */
export function missingExpectations(texts, expect) {
  const haystack = texts.join("\n");
  return expect.filter((anchor) => !haystack.includes(anchor));
}

/** 失败定位文案：case/step + 期望 + 实际 + dump。 */
export function localizeFailure(failure) {
  const observed = failure.observed.length === 0 ? "（空）" : failure.observed.join(" | ");
  const dump = failure.dump === undefined ? "" : `（dump: ${failure.dump}）`;
  return `[${failure.case}/${failure.step}] 期望 ${failure.expect.join(" / ")}；实际 ${observed}${dump}`;
}

function adbRun(adb, serial, args, timeout = 60_000) {
  return spawnSync(adb, ["-s", serial, ...args], { encoding: "utf8", timeout });
}

function onlineDevices(adb) {
  const out = spawnSync(adb, ["devices"], { encoding: "utf8", timeout: 30_000 });
  return (out.stdout ?? "")
    .split("\n")
    .slice(1)
    .map((line) => line.trim().split(/\s+/))
    .filter(([serial, state]) => serial !== undefined && state === "device")
    .map(([serial]) => serial);
}

function isWear(adb, serial) {
  const out = adbRun(adb, serial, ["shell", "getprop", "ro.build.characteristics"]);
  return (out.stdout ?? "").includes("watch");
}

function dumpUi(adb, serial) {
  adbRun(adb, serial, ["shell", "rm", "-f", "/sdcard/window_dump.xml"]);
  adbRun(adb, serial, ["shell", "uiautomator", "dump", "/sdcard/window_dump.xml"]);
  const out = adbRun(adb, serial, ["exec-out", "cat", "/sdcard/window_dump.xml"]);
  return out.status === 0 ? out.stdout : "";
}

/** 观测（有界重试）：滑动动画 / RN 渲染延迟都算在等待窗口内。 */
function observe(adb, serial, expect, attempts = 6) {
  let xml = "";
  let texts = [];
  let missing = [...expect];
  for (let i = 0; i < attempts; i += 1) {
    xml = dumpUi(adb, serial);
    texts = extractTexts(xml);
    missing = missingExpectations(texts, expect);
    if (missing.length === 0) break;
    sleep(500);
  }
  return { xml, texts, missing };
}

function launch(adb, serial, packageName) {
  adbRun(adb, serial, ["shell", "am", "force-stop", packageName]);
  // 只认 monkey 退出码；应用是否真的起来由后续 launch 步的锚点观测裁决（便于定位）。
  const out = adbRun(adb, serial, [
    "shell",
    "monkey",
    "-p",
    packageName,
    "-c",
    "android.intent.category.LAUNCHER",
    "1",
  ]);
  return out.status === 0 && !(out.stdout ?? "").includes("aborted");
}

function screenshot(adb, serial, path) {
  const out = spawnSync(adb, ["-s", serial, "exec-out", "screencap", "-p"], {
    timeout: 30_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (out.status !== 0 || out.stdout === undefined) return;
  writeFileSync(path, out.stdout);
}

/**
 * Android 腿。返回 record：{ target, leg, status, skipNote?, device, apk, package, cases, failure? }。
 * 环境门顺序：adb → 在线 Wear 设备 → 预构建 APK；任一缺席 SKIP-with-print。
 */
export function runAndroidLeg({ logs, serial, apk, packageName = DEFAULT_PACKAGE } = {}) {
  const record = {
    target: "wearos",
    leg: "android",
    status: "skip",
    device: null,
    apk: apk ?? null,
    package: packageName,
    cases: [],
    skipNote: null,
  };
  const adb = findAdb();
  if (adb === null) {
    record.skipNote = "本机无 adb（ANDROID_HOME/ANDROID_SDK_ROOT/platform-tools 与 PATH 均未命中）";
    return record;
  }
  const online = onlineDevices(adb);
  if (serial !== undefined && serial !== null && !online.includes(serial)) {
    record.skipNote = `指定 serial ${serial} 不在线（adb devices: ${online.join(", ") || "无"}）`;
    return record;
  }
  const device = serial ?? online[0];
  if (device === undefined) {
    record.skipNote = "adb 无在线设备——Wear 模拟器 / 真机未连接";
    return record;
  }
  record.device = device;
  if (!isWear(adb, device)) {
    record.skipNote = `设备 ${device} 的 ro.build.characteristics 不含 watch——非 Wear 设备，不冒充`;
    return record;
  }
  if (apk === undefined || apk === null || !existsSync(apk) || !statSync(apk).isFile()) {
    record.skipNote =
      "无预构建 Wear 表单 APK（--apk / vars.POLYPACK_WEAR_FORM_APK 未设或缺文件）；本机构建与 AVD 命令见 ui/smoke/wear/README.md";
    return record;
  }
  mkdirSync(logs, { recursive: true });

  const install = adbRun(adb, device, ["install", "-r", apk], 180_000);
  if (install.status !== 0) {
    record.status = "fail";
    record.failure = {
      case: "install",
      step: "adb-install",
      expect: [`install exit 0（${apk}）`],
      observed: [(install.stderr ?? "").trim() || `exit ${install.status}`],
      dump: null,
    };
    return record;
  }

  const sizeOut = adbRun(adb, device, ["shell", "wm", "size"]).stdout;
  const geometry = parseWmSize(sizeOut) ?? { width: 320, height: 320 };
  record.status = "pass";
  for (const wearCase of WEAR_FORM_CASES) {
    const caseRecord = {
      id: wearCase.id,
      purpose: wearCase.purpose,
      status: "pass",
      steps: [],
    };
    for (const formStep of wearCase.steps) {
      const stepRecord = {
        id: formStep.id,
        action: formStep.action.type,
        expect: [...formStep.expect],
        status: "pass",
        observed: [],
        dump: join(logs, `${wearCase.id}-${formStep.id}.xml`),
      };
      let actionError = null;
      if (formStep.action.type === "launch") {
        if (!launch(adb, device, packageName)) actionError = "monkey 冷启动未注入事件";
      } else {
        for (const args of buildActionCommands(formStep.action, geometry)) {
          const out = adbRun(adb, device, args);
          if (out.status !== 0) actionError = (out.stderr ?? "").trim() || `adb exit ${out.status}`;
        }
      }
      const seen = observe(adb, device, actionError === null ? formStep.expect : []);
      writeFileSync(stepRecord.dump, seen.xml);
      stepRecord.observed = seen.texts;
      if (actionError !== null || seen.missing.length > 0) {
        stepRecord.status = "fail";
        caseRecord.status = "fail";
        record.status = "fail";
        caseRecord.failure = {
          case: wearCase.id,
          step: formStep.id,
          expect: [...formStep.expect],
          observed:
            actionError === null
              ? seen.texts.filter((text) => formStep.expect.some((anchor) => text.includes(anchor)))
              : [actionError],
          dump: stepRecord.dump,
        };
        const shot = join(logs, `${wearCase.id}-${formStep.id}.png`);
        screenshot(adb, device, shot);
        caseRecord.failure.screenshot = shot;
        record.failure ??= caseRecord.failure;
      }
      caseRecord.steps.push(stepRecord);
      if (stepRecord.status === "fail") break;
    }
    record.cases.push(caseRecord);
  }
  return record;
}
