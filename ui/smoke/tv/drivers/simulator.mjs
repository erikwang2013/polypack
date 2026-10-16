// Tizen / webOS 模拟器腿：本机（与 CI）若缺席 → SKIP-with-print + 注记（不红）。
// 探测面只读：PATH 上的 tizen（Tizen Studio CLI）/ ares（webOS CLI）与常见安装目录。
// CLI 在场也仍 SKIP：模拟器镜像与遥控键注入通道（luna-send / ares-shell 类）未接线——
// 同 UI 产物与键协议的覆盖由 web 替代腿（无头 Chrome + 同一 dist）承担，证据如实标注「替代腿」。

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

function findOnPath(name) {
  if (name.includes("/")) return existsSync(name) ? name : null;
  for (const dir of (process.env.PATH ?? "").split(":").filter((d) => d.length > 0)) {
    const full = join(dir, name);
    if (existsSync(full)) return full;
  }
  return null;
}

const CANDIDATES = Object.freeze({
  "tizen-tv": [
    ["tizen（Tizen Studio CLI）", "tizen"],
    ["tizen（Tizen Studio 默认安装）", join(homedir(), "tizen-studio/tools/ide/bin/tizen")],
  ],
  "webos-tv": [
    ["ares（webOS CLI）", "ares"],
    ["ares（webOS CLI 默认安装）", join(homedir(), "webOS_TV_SDK/CLI/bin/ares")],
  ],
});

/** 探测两目标分支的模拟器 CLI；返回逐目标的 skip 记录（探测到也注记为未接线）。 */
export function probeSimulators() {
  return Object.entries(CANDIDATES).map(([target, candidates]) => {
    const hit = candidates.map(([label, name]) => [label, findOnPath(name)]).find(([, path]) => path !== null);
    const skipNote =
      hit === undefined
        ? `SKIP：本机无 ${target} 模拟器 CLI（探测：${candidates.map(([label]) => label).join(" / ")}）——${target} 模拟器腿未执行；同一 UI 产物与键协议的覆盖见 web 替代腿（如实标注「替代腿」，非模拟器本体）`
        : `SKIP：检测到 ${hit[0]}（${hit[1]}），但模拟器镜像与遥控键注入通道未接线（超出 T-M4-13 范围）——${target} 模拟器腿未执行；覆盖见 web 替代腿`;
    return { target, leg: "simulator", status: "skip", skipNote, scenes: [] };
  });
}
