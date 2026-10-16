// web 系腿驱动（替代腿，非 Tizen/webOS 模拟器本体——证据如实标注）：
// 无头 Chrome + 零依赖 CDP（Node ≥ 22 内建 fetch / WebSocket；缺 WebSocket → SKIP-with-print）。
// 观测三面（与 ui/adapters/tv/README.md 宿主记录口径同形）：
//   1) Runtime.consoleAPICalled 收单参数 `POLYPACK_KEY <json>` 行（描述符通道）；
//   2) Runtime.evaluate 一次调用采齐 DOM 观测量（焦点锚点 / 计数按钮 / 层深 / 退出提示）；
//   3) Input.dispatchKeyEvent 按 keys.mjs 的 DOM 表注键。
// 只观测不定期望：期望由 run.mjs 现场派生传入，对拍在 compare.mjs（web 腿为裁定方）。

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";

import { compareStep, primaryClass } from "../compare.mjs";
import { domKey } from "../keys.mjs";

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const CHROME_CANDIDATES = [
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "google-chrome",
  "google-chrome-stable",
  "chromium",
  "chromium-browser",
];

/** 查找 Chrome/Chromium 可执行文件；缺席返回 null（调用方 SKIP-with-print）。 */
export function findChrome(explicit = null) {
  if (explicit !== null) return existsSync(explicit) ? explicit : null;
  if (process.env.POLY_TV_CHROME && existsSync(process.env.POLY_TV_CHROME)) return process.env.POLY_TV_CHROME;
  const dirs = (process.env.PATH ?? "").split(":").filter((d) => d.length > 0);
  for (const candidate of CHROME_CANDIDATES) {
    if (candidate.startsWith("/")) {
      if (existsSync(candidate)) return candidate;
      continue;
    }
    for (const dir of dirs) {
      const full = join(dir, candidate);
      if (existsSync(full)) return full;
    }
  }
  return null;
}

/** 极简静态服务（dist 直供；web 系目标由同一 UI 产物派生，ENOENT → 404）。 */
function serveDist(dist) {
  const root = resolve(dist);
  const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png", ".ico": "image/x-icon" };
  const server = createServer((req, res) => {
    let path = join(root, decodeURIComponent(new URL(req.url, "http://x").pathname));
    if (!path.startsWith(root + sep) && path !== root) {
      res.writeHead(403).end();
      return;
    }
    if (existsSync(path) && statSync(path).isDirectory()) path = join(path, "index.html");
    if (!existsSync(path)) {
      res.writeHead(404).end("tv-smoke: dist 无此文件 " + path);
      return;
    }
    res.writeHead(200, { "content-type": types[path.slice(path.lastIndexOf("."))] ?? "application/octet-stream" });
    res.end(readFileSync(path));
  });
  return new Promise((ready) => server.listen(0, "127.0.0.1", () => ready(server)));
}

/** CDP 客户端（零依赖）：page 级 WebSocket，id 配对 + 事件分派。 */
class Cdp {
  #pending = new Map();
  #handlers = new Map();
  #seq = 0;

  constructor(ws) {
    this.ws = ws;
    ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id !== undefined) {
        const slot = this.#pending.get(msg.id);
        this.#pending.delete(msg.id);
        if (slot) (msg.error ? slot.reject(new Error(`CDP ${msg.error.message}`)) : slot.resolve(msg.result));
        return;
      }
      for (const fn of this.#handlers.get(msg.method) ?? []) fn(msg.params);
    });
  }

  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((done, fail) => {
      ws.addEventListener("open", done, { once: true });
      ws.addEventListener("error", () => fail(new Error("web 腿：CDP WebSocket 连接失败 " + url)), { once: true });
    });
    return new Cdp(ws);
  }

  on(method, fn) {
    if (!this.#handlers.has(method)) this.#handlers.set(method, []);
    this.#handlers.get(method).push(fn);
  }

  /** 一次性事件等待（超时报错）——场景间重导航用它锚定「新文档已 load」。 */
  once(method, timeoutMs = 15000) {
    return new Promise((done, fail) => {
      const timer = setTimeout(() => fail(new Error(`web 腿：等待 ${method} 超时（${timeoutMs}ms）`)), timeoutMs);
      const fn = (params) => {
        clearTimeout(timer);
        const list = this.#handlers.get(method) ?? [];
        this.#handlers.set(method, list.filter((f) => f !== fn));
        done(params);
      };
      this.on(method, fn);
    });
  }

  send(method, params = {}) {
    const id = ++this.#seq;
    return new Promise((resolve_, reject) => {
      this.#pending.set(id, { resolve: resolve_, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** 求值并取回 JSON 值（观测面：一次调用采齐 DOM 状态）。 */
  async evaluate(expression) {
    const r = await this.send("Runtime.evaluate", { expression, returnByValue: true });
    if (r.exceptionDetails) throw new Error("web 腿：页面求值异常 " + (r.exceptionDetails.exception?.description ?? ""));
    return r.result.value;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* 已断开 */
    }
  }
}

/** 观测表达式：一次调用采齐（same-call 采集；解析面 = 宿主记录的 DOM 形态）。 */
const OBSERVE = `(() => {
  const anchors = document.querySelectorAll('[data-focus="true"]');
  const counter = document.querySelector('[data-testid="counter"]');
  const overlay = document.querySelector('[data-testid="tv-overlay"]');
  const modalTitle = overlay ? (overlay.querySelector('[data-testid="tv-modal"] span')?.textContent ?? null) : null;
  const hints = [...document.querySelectorAll('[data-testid="tv-card"] span')].map((s) => s.textContent);
  const hint = hints.find((t) => t !== null && (t.startsWith("已请求退出") || t.startsWith("返回键") || t.startsWith("遥控器") || t.startsWith("点击按钮"))) ?? null;
  return {
    focus: anchors[0]?.getAttribute("data-testid") ?? null,
    focusCount: anchors.length,
    clicks: counter ? Number(/(\\d+)/.exec(counter.textContent)?.[1] ?? NaN) : null,
    depth: overlay ? (modalTitle === "播放器" ? 2 : 1) : 0,
    modalTitle,
    exitHint: hint !== null && hint.startsWith("已请求退出"),
    hint,
  };
})()`;

/** 等待 chrome stderr 的 DevTools 端口行（超时报错携 stderr 尾巴）。 */
function waitDevTools(child, stderrRef, timeoutMs = 20000) {
  return new Promise((done, fail) => {
    const timer = setTimeout(() => fail(new Error(`web 腿：chrome 未在 ${timeoutMs}ms 内就绪；stderr 尾：${stderrRef.join("").slice(-500)}`)), timeoutMs);
    const probe = setInterval(() => {
      const hit = /DevTools listening on (ws:\/\/\S+)/.exec(stderrRef.join(""));
      if (hit !== null) {
        clearTimeout(timer);
        clearInterval(probe);
        done(hit[1]);
      }
      if (child.exitCode !== null) {
        clearTimeout(timer);
        clearInterval(probe);
        fail(new Error(`web 腿：chrome 提前退出（code ${child.exitCode}）；stderr：${stderrRef.join("").slice(-500)}`));
      }
    }, 100);
  });
}

/** 取 page target 的 ws 调试 URL（browser ws 端口派生；有界重试等 target 出现）。 */
async function pageSocketUrl(browserWs, tries = 40) {
  const port = new URL(browserWs).port;
  for (let i = 0; i < tries; i++) {
    const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json()).catch(() => []);
    const page = list.find((t) => t.type === "page");
    if (page?.webSocketDebuggerUrl) return page.webSocketDebuggerUrl;
    await sleep(100);
  }
  throw new Error("web 腿：CDP 无 page target");
}

/**
 * 跑全部 web 系目标（同一 chrome / 同一 dist / 每目标一次导航）。
 *
 * @param {object} opts
 * @param {string[]} opts.targets 目标 id 列表（tizen-tv / webos-tv 等）
 * @param {string} opts.dist UI 产物目录（vite build 输出；含 index.html）
 * @param {string} opts.logs 证据目录
 * @param {Array} opts.scenes 派生场景 [{ id, purpose, initial, steps: [{ key, expect }] }]
 * @param {string|null} [opts.chromePath] 显式 chrome 路径
 * @returns {Promise<Array>} 每目标记录（status: pass/fail/skip；skip 带 skipNote）
 */
export async function runWebTargets({ targets, dist, logs, scenes, chromePath = null }) {
  const chrome = findChrome(chromePath);
  if (chrome === null) {
    const skipNote = "SKIP：本机无 Chrome/Chromium（可经 --chrome 或 POLY_TV_CHROME 指定）；web 系腿未执行";
    return targets.map((target) => ({ target, leg: "web-surrogate", status: "skip", skipNote, scenes: [] }));
  }
  if (typeof WebSocket === "undefined") {
    const skipNote = `SKIP：Node ${process.version} 无内建 WebSocket（CDP 需 ≥ 22）；web 系腿未执行`;
    return targets.map((target) => ({ target, leg: "web-surrogate", status: "skip", skipNote, scenes: [] }));
  }
  if (!existsSync(join(resolve(dist), "index.html"))) {
    throw new Error(`web 腿：dist 无 index.html（${resolve(dist)}）——先跑 vite build（npm run build）`);
  }

  mkdirSync(logs, { recursive: true });
  const profile = mkdtempSync(join(tmpdir(), "poly-tv-smoke-"));
  const stderrRef = [];
  const child = spawn(chrome, [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-dev-shm-usage",
    "--remote-debugging-port=0",
    "--remote-allow-origins=*",
    "--window-size=1920,1080",
    `--user-data-dir=${profile}`,
    "about:blank",
  ], { stdio: ["ignore", "ignore", "pipe"] });
  child.stderr.on("data", (chunk) => stderrRef.push(String(chunk)));

  const server = await serveDist(dist);
  const httpPort = server.address().port;
  const records = [];
  let cdp = null;
  try {
    const browserWs = await waitDevTools(child, stderrRef);
    cdp = await Cdp.connect(await pageSocketUrl(browserWs));
    const lines = []; // 单参数 POLYPACK_KEY 行（描述符通道）
    const errors = []; // 页面异常（通道证据）
    cdp.on("Runtime.consoleAPICalled", (params) => {
      const text = params.args?.[0]?.value;
      if (typeof text === "string" && text.startsWith("POLYPACK_KEY ")) lines.push(text);
    });
    cdp.on("Runtime.exceptionThrown", (params) => errors.push(params.exceptionDetails?.exception?.description ?? "unknown"));
    await cdp.send("Runtime.enable");
    await cdp.send("Page.enable");

    for (const target of targets) {
      const url = `http://127.0.0.1:${httpPort}/?target=${encodeURIComponent(target)}`;
      const base = lines.length;
      const sceneRecords = [];
      // 场景间重导航 = web 腿的前置重置（对齐 run-walk.mjs 的 per-walk 重启：每场景回
      // 初始焦点 / 计数与空层栈，前场景状态不渗漏）。
      const reload = async () => {
        const loaded = cdp.once("Page.loadEventFired");
        await cdp.send("Page.navigate", { url });
        await loaded.catch(() => {}); // 已 load 完成的竞态（事件先于监听）由下面锚点轮询兜底
        let ok = false;
        for (let i = 0; i < 60 && !ok; i++) {
          // 双条件：新文档锚点在场 + 目标名对得上（防读旧文档假就绪）
          ok = await cdp
            .evaluate(
              `Boolean(document.querySelector('[data-testid="tv-frame"]')) && window.__polypack?.target === ${JSON.stringify(target)}`,
            )
            .catch(() => false);
          if (!ok) await sleep(250);
        }
        return ok;
      };
      if (!(await reload())) {
        records.push({ target, leg: "web-surrogate", status: "fail", scenes: [], failure: { class: "channel", detail: `页面未就绪（tv-frame 锚点缺席或目标不符）：${url}；异常：${errors.slice(-3).join(" | ") || "无"}` } });
        continue;
      }

      for (const scene of scenes) {
        if (!(await reload())) {
          sceneRecords.push({ id: scene.id, purpose: scene.purpose, status: "fail", initial: null, steps: [], failures: [{ class: "channel", detail: `场景重导航未就绪：${url}` }] });
          continue;
        }
        const initial = await cdp.evaluate(OBSERVE);
        let prevFocus = initial.focus;
        const steps = [];
        for (const [index, step] of scene.steps.entries()) {
          const before = lines.length;
          const dom = domKey(step.key);
          await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: dom.key, code: dom.code });
          await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: dom.key, code: dom.code });
          for (let i = 0; i < 25 && lines.length <= before; i++) await sleep(120); // 有界等待本键描述符行
          const line = lines.length > before ? lines[before] : null;
          const observed = { ...(await cdp.evaluate(OBSERVE)), result: line === null ? null : JSON.parse(line.slice("POLYPACK_KEY ".length)) };
          const verdict = compareStep({ key: step.key, expect: step.expect, observed, prevFocus });
          prevFocus = observed.focus;
          const evidence = { descriptorLine: line, observed };
          // 截图证据：每场景首末键 + 任一不符键（失败定位现场）
          if (index === 0 || index === scene.steps.length - 1 || !verdict.ok) {
            const shot = await cdp.send("Page.captureScreenshot", { format: "png" });
            const file = join(logs, `web-${target}-${scene.id}-step${index + 1}.png`);
            writeFileSync(file, Buffer.from(shot.data, "base64"));
            evidence.screenshot = file;
          }
          steps.push({ index: index + 1, key: step.key, expect: step.expect, observed, ok: verdict.ok, failures: verdict.failures, evidence });
        }
        const ok = steps.every((s) => s.ok);
        sceneRecords.push({ id: scene.id, purpose: scene.purpose, status: ok ? "pass" : "fail", initial, steps });
      }
      const logFile = join(logs, `web-${target}.json`);
      writeFileSync(logFile, JSON.stringify({ target, url, scenes: sceneRecords, consoleLines: lines.length - base, pageErrors: errors }, null, 2));
      const failed = sceneRecords.find((s) => s.status === "fail");
      const step = failed?.steps.find((s) => !s.ok);
      const failure = !failed
        ? null
        : step
          ? { class: primaryClass(step), detail: `[${failed.id}] 第 ${step.index} 键（${step.key}）：${step.failures.map((f) => f.detail).join("；")}` }
          : { class: failed.failures[0].class, detail: `[${failed.id}] ${failed.failures[0].detail}` };
      records.push({
        target,
        leg: "web-surrogate",
        status: failed ? "fail" : "pass",
        scenes: sceneRecords,
        ...(failure === null ? {} : { failure }),
        evidence: { log: logFile, pageErrors: errors.length },
      });
    }
  } finally {
    cdp?.close();
    child.kill("SIGTERM");
    await sleep(300);
    if (child.exitCode === null) child.kill("SIGKILL");
    server.close();
    rmSync(profile, { recursive: true, force: true });
  }
  return records;
}
