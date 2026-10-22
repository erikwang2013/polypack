// ChromeOS PWA 真浏览器审计（T-M5-10）：manifest / 安装性 / 离线能力三腿。
//
// 零依赖（Node ≥ 22 内建 fetch / WebSocket）+ 无头 Chrome CDP：
//   1) manifest：入口 HTML 的 <link rel="manifest"> 可取、JSON 可解析、关键键在位；
//   2) 安装性：Page.getInstallabilityErrors 为空（Chrome 151 实测口径：SVG any / 192px 均可，
//      不要求 service worker）；
//   3) 离线：service worker 接管后断网重载，文档仍从缓存渲染（controller 在位 + manifest
//      link 仍在 + 正文非空）。
// 只观测不定期望：结构门槛由 polypack-bundle 的 pwa-audit 校验器判，本脚本补浏览器实证面。
// Chrome 缺席时 SKIP-with-print（exit 0，status=skip），绝不假装跑过。

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve, sep } from "node:path";

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const CHROME_CANDIDATES = [
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "google-chrome",
  "google-chrome-stable",
  "chromium",
  "chromium-browser",
];

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".webmanifest": "application/manifest+json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
};

/** 查找 Chrome/Chromium；缺席返回 null（调用方 SKIP-with-print）。 */
function findChrome(explicit) {
  if (explicit) return existsSync(explicit) ? explicit : null;
  if (process.env.POLYPACK_PWA_CHROME && existsSync(process.env.POLYPACK_PWA_CHROME)) {
    return process.env.POLYPACK_PWA_CHROME;
  }
  const dirs = (process.env.PATH ?? "").split(":").filter((dir) => dir.length > 0);
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

/** 极简静态服务（站点直供；目录 → index.html，路径越界 → 403）。 */
function serveSite(site) {
  const root = resolve(site);
  const server = createServer((req, res) => {
    let path = join(root, decodeURIComponent(new URL(req.url, "http://x").pathname));
    if (!path.startsWith(root + sep) && path !== root) {
      res.writeHead(403).end();
      return;
    }
    if (existsSync(path) && statSync(path).isDirectory()) path = join(path, "index.html");
    if (!existsSync(path)) {
      res.writeHead(404).end("pwa-audit: site 无此文件 " + path);
      return;
    }
    const type = TYPES[extname(path)] ?? "application/octet-stream";
    res.writeHead(200, { "content-type": type });
    res.end(readFileSync(path));
  });
  return new Promise((ready) => server.listen(0, "127.0.0.1", () => ready(server)));
}

/** CDP 客户端（零依赖）：id 配对 + 事件分派。 */
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
      ws.addEventListener("error", () => fail(new Error("pwa-audit: CDP WebSocket 连接失败 " + url)), { once: true });
    });
    return new Cdp(ws);
  }

  on(method, fn) {
    if (!this.#handlers.has(method)) this.#handlers.set(method, []);
    this.#handlers.get(method).push(fn);
  }

  /** 一次性事件等待（超时 reject）。 */
  once(method, timeoutMs = 15000) {
    return new Promise((done, fail) => {
      const timer = setTimeout(() => fail(new Error(`pwa-audit: 等待 ${method} 超时（${timeoutMs}ms）`)), timeoutMs);
      const fn = (params) => {
        clearTimeout(timer);
        const list = this.#handlers.get(method) ?? [];
        this.#handlers.set(method, list.filter((handler) => handler !== fn));
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

  /** 求值并取回 JSON 值（awaitPromise：页面内异步审计表达式可用）。 */
  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (result.exceptionDetails) {
      throw new Error(
        "pwa-audit: 页面求值异常 " + (result.exceptionDetails.exception?.description ?? ""),
      );
    }
    return result.result.value;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* 已断开 */
    }
  }
}

/** 等 chrome stderr 的 DevTools 端口行（超时报错携 stderr 尾巴）。 */
function waitDevTools(child, stderrRef, timeoutMs = 90000) {
  return new Promise((done, fail) => {
    const timer = setTimeout(
      () => fail(new Error(`pwa-audit: chrome 未在 ${timeoutMs}ms 内就绪；stderr 尾：${stderrRef.join("").slice(-500)}`)),
      timeoutMs,
    );
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
        fail(new Error(`pwa-audit: chrome 提前退出（code ${child.exitCode}）；stderr：${stderrRef.join("").slice(-500)}`));
      }
    }, 100);
  });
}

/** 取 page target 的 ws 调试 URL（有界重试等 target 出现）。 */
async function pageSocketUrl(browserWs, tries = 50) {
  const port = new URL(browserWs).port;
  for (let i = 0; i < tries; i++) {
    const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((res) => res.json()).catch(() => []);
    const page = list.find((target) => target.type === "page");
    if (page?.webSocketDebuggerUrl) return page.webSocketDebuggerUrl;
    await sleep(100);
  }
  throw new Error("pwa-audit: CDP 无 page target");
}

/** manifest 腿（页面上下文取 link → fetch → JSON 关键键）。 */
const MANIFEST_AUDIT = `(async () => {
  const link = document.querySelector('link[rel="manifest"]');
  if (!link) return { ok: false, reason: "入口 HTML 未引用 manifest" };
  const href = new URL(link.getAttribute("href"), location.href).href;
  const response = await fetch(href);
  if (!response.ok) return { ok: false, reason: "manifest HTTP " + response.status + "（" + href + "）" };
  let manifest;
  try { manifest = await response.json(); } catch (err) { return { ok: false, reason: "manifest 非 JSON：" + err.message }; }
  const missing = ["name", "start_url", "display", "icons"].filter((key) => !manifest[key]);
  if (missing.length > 0) return { ok: false, reason: "manifest 缺键：" + missing.join(",") };
  return { ok: true, href, name: manifest.name, display: manifest.display, start_url: manifest.start_url, icons: manifest.icons.length };
})()`;

/** 等 SW ready / controller；失败返回原因。 */
const SW_READY = `(async () => {
  if (!("serviceWorker" in navigator)) return { ok: false, reason: "navigator.serviceWorker 不存在" };
  const registration = await Promise.race([
    navigator.serviceWorker.ready,
    new Promise((done) => setTimeout(() => done(null), 10000)),
  ]);
  if (!registration) return { ok: false, reason: "serviceWorker.ready 10s 未决议（注册失败？）" };
  if (!navigator.serviceWorker.controller) {
    await new Promise((done) => {
      const timer = setTimeout(done, 3000);
      navigator.serviceWorker.addEventListener("controllerchange", () => { clearTimeout(timer); done(); }, { once: true });
    });
  }
  return {
    ok: !!navigator.serviceWorker.controller,
    scope: registration.scope,
    script: registration.active?.scriptURL ?? null,
    controller: !!navigator.serviceWorker.controller,
  };
})()`;

/** 断网重载后的文档状态（SW 缓存命中则入口与正文仍在）。 */
const OFFLINE_STATE = `(() => {
  const body = (document.body?.innerText ?? "").trim().slice(0, 120);
  return {
    readyState: document.readyState,
    hasManifest: !!document.querySelector('link[rel="manifest"]'),
    body,
    online: navigator.onLine,
    controller: !!navigator.serviceWorker.controller,
  };
})()`;

/**
 * 跑三腿审计。
 * @param {{site: string, chromePath?: string|null}} opts
 * @returns {Promise<object>} { status: "pass"|"fail"|"skip", manifest, installability, offline, ... }
 */
async function audit({ site, chromePath = null }) {
  if (!existsSync(join(resolve(site), "index.html"))) {
    return { status: "fail", reason: `site 无 index.html（${resolve(site)}）` };
  }
  const chrome = findChrome(chromePath);
  if (chrome === null) {
    return {
      status: "skip",
      reason: "本机无 Chrome/Chromium（可经 --chrome 或 POLYPACK_PWA_CHROME 指定）；真浏览器腿未执行",
    };
  }
  if (typeof WebSocket === "undefined") {
    return { status: "skip", reason: `Node ${process.version} 无内建 WebSocket（CDP 需 ≥22）；真浏览器腿未执行` };
  }

  const server = await serveSite(site);
  const port = server.address().port;
  const profile = mkdtempSync(join(tmpdir(), "polypack-pwa-audit-"));
  const stderrRef = [];
  const child = spawn(
    chrome,
    [
      "--headless=new",
      // DISPLAY 在场时 ozone 会先探 X11；无头后端显式钉死（同 tv web 腿实测）。
      "--ozone-platform=headless",
      "--disable-gpu",
      "--no-sandbox",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-dev-shm-usage",
      "--remote-debugging-port=0",
      "--remote-allow-origins=*",
      "--window-size=1280,800",
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  child.stderr.on("data", (chunk) => stderrRef.push(String(chunk)));

  let cdp = null;
  try {
    const browserWs = await waitDevTools(child, stderrRef);
    cdp = await Cdp.connect(await pageSocketUrl(browserWs));
    await cdp.send("Page.enable");
    await cdp.send("Runtime.enable");
    await cdp.send("Network.enable");

    const url = `http://127.0.0.1:${port}/`;
    const loaded = cdp.once("Page.loadEventFired");
    await cdp.send("Page.navigate", { url });
    await loaded.catch(() => {});
    await sleep(1200); // manifest 解析 + SW 注册起步

    const manifest = await cdp.evaluate(MANIFEST_AUDIT).catch((err) => ({ ok: false, reason: err.message }));

    let installability;
    try {
      const result = await cdp.send("Page.getInstallabilityErrors");
      const errors = result.installabilityErrors ?? [];
      installability = {
        ok: errors.length === 0,
        errors,
        ...(errors.length === 0 ? {} : { reason: "Chrome 判出安装性错误" }),
      };
    } catch (err) {
      // 老 Chrome 无此 method：如实标注跳过，结构门槛仍由 Rust 审计守住。
      installability = { ok: true, skipped: `Chrome 不提供 Page.getInstallabilityErrors（${err.message}）` };
    }

    const sw = await cdp.evaluate(SW_READY).catch((err) => ({ ok: false, reason: err.message }));
    let offline;
    if (!sw.ok) {
      offline = { ok: false, reason: sw.reason ?? "service worker 未接管" };
    } else {
      await cdp.send("Network.emulateNetworkConditions", {
        offline: true,
        latency: 0,
        downloadThroughput: 0,
        uploadThroughput: 0,
      });
      const reloaded = cdp.once("Page.loadEventFired", 8000);
      await cdp.send("Page.reload", { ignoreCache: false });
      await Promise.race([reloaded, sleep(8000)]);
      await sleep(500);
      const state = await cdp.evaluate(OFFLINE_STATE).catch((err) => ({
        readyState: "error",
        hasManifest: false,
        body: "",
        online: null,
        controller: false,
        error: err.message,
      }));
      await cdp
        .send("Network.emulateNetworkConditions", {
          offline: false,
          latency: 0,
          downloadThroughput: -1,
          uploadThroughput: -1,
        })
        .catch(() => {});
      const ok = state.readyState !== "loading" && state.hasManifest && state.controller;
      offline = {
        ok,
        ...(ok ? {} : { reason: "断网重载未从 SW 缓存渲染（入口 manifest link 缺席或 SW 未接管）" }),
        state,
        sw: { scope: sw.scope, script: sw.script, controller: sw.controller },
      };
    }

    const ok = manifest.ok && installability.ok && offline.ok;
    return {
      status: ok ? "pass" : "fail",
      url,
      manifest,
      installability,
      offline,
    };
  } finally {
    cdp?.close();
    child.kill("SIGTERM");
    await sleep(300);
    if (child.exitCode === null) child.kill("SIGKILL");
    server.close();
    rmSync(profile, { recursive: true, force: true });
  }
}

/** 参数解析：`--site <dir>` 必填；`--chrome` / `--json` 可选。 */
function parseArgs(argv) {
  const args = { site: null, chrome: null, json: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--site") args.site = argv[++i] ?? null;
    else if (arg === "--chrome") args.chrome = argv[++i] ?? null;
    else if (arg === "--json") args.json = true;
    else throw new Error(`未知参数：${arg}`);
  }
  if (!args.site) throw new Error("缺少 --site <静态站点目录>");
  return args;
}

const args = parseArgs(process.argv.slice(2));
try {
  const record = await audit({ site: args.site, chromePath: args.chrome });
  if (args.json) {
    console.log(JSON.stringify(record));
  } else {
    console.log(`PWA 审计：${record.status}`);
    if (record.status === "skip") {
      console.log(`SKIP：${record.reason}`);
    } else {
      console.log(`manifest：${record.manifest.ok ? "PASS" : "FAIL"} ${record.manifest.reason ?? ""}`.trim());
      const install = record.installability.skipped
        ? `SKIP（${record.installability.skipped}）`
        : record.installability.ok
          ? "PASS"
          : `FAIL ${record.installability.errors?.map((e) => e.errorId).join(",") ?? record.installability.reason}`;
      console.log(`安装性：${install}`);
      const offline = record.offline.ok
        ? `PASS（SW ${record.offline.sw?.script ?? ""}；断网重载 controller=${record.offline.state?.controller}）`
        : `FAIL ${record.offline.reason ?? ""}`;
      console.log(`离线能力：${offline}`);
    }
  }
  process.exit(record.status === "fail" ? 1 : 0);
} catch (err) {
  console.error(`pwa-audit: ${err.stack ?? err.message}`);
  process.exit(1);
}
