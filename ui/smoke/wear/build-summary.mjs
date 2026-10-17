// T-M7-04 构建摘要：归档可追溯（时间戳由 result.json 的 startedAt/finishedAt 承载；
// 此处钉住源码提交与 APK 指纹，脏工作树显式标注，不冒充干净构建）。

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";

function git(cwd, args) {
  const out = spawnSync("git", args, { cwd, encoding: "utf8", timeout: 10_000 });
  return out.status === 0 ? out.stdout.trim() : null;
}

/** APK 指纹（路径 / 字节数 / SHA-256）；缺文件时字段为 null，不伪造。 */
export function apkFingerprint(apk, readFile = readFileSync) {
  if (apk === null) return null;
  try {
    return {
      path: apk,
      bytes: statSync(apk).size,
      sha256: createHash("sha256").update(readFile(apk)).digest("hex"),
    };
  } catch {
    return { path: apk, bytes: null, sha256: null };
  }
}

export function collectBuildSummary({ apk = null, cwd = process.cwd() } = {}) {
  const commit = git(cwd, ["rev-parse", "HEAD"]);
  return {
    commit,
    dirty: commit === null ? null : git(cwd, ["status", "--porcelain"]) !== "",
    node: process.version,
    platform: `${process.platform}-${process.arch}`,
    apk: apkFingerprint(apk),
  };
}
