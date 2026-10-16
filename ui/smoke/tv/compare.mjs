// T-M4-13 期望对拍与失败定位（四类：focus-dead-end / route / shortcut / channel）。
// web 腿逐键调用本函数决定 pass/fail；Android 腿的 ok 由 run-walk.mjs 裁定（单一真源），
// 本函数只按其结构化输出给失败打同类标签（标签不参与真值）。
//
// 输入形同：expect = { result: 描述符, focus, clicks, depth, exitRequested }（现场派生），
// observed = { result, focus, focusCount, clicks, depth, exitHint }（通道观测，见 drivers/*）。

/** 键序稳定的 JSON 化（跨通道对拍不依赖键序——浏览器序列化与内核对象序不保证一致）。 */
export function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const body = Object.keys(value)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`)
      .join(",");
    return `{${body}}`;
  }
  return JSON.stringify(value);
}

/**
 * 描述符判据（两腿共用）：actual 为 null/undefined = 通道失效；内容不等 = 路由/快捷违例。
 * 返回 failure 对象或 null。
 */
export function descriptorFailure(key, expected, actual) {
  const digit = /^(?:Digit|Numpad|KEY_)?[0-9]$/.test(key);
  if (actual === null || actual === undefined) {
    return { class: "channel", detail: `未收到本键 POLYPACK_KEY 行（观测通道失效）；期望 ${stableStringify(expected)}` };
  }
  if (stableStringify(actual) !== stableStringify(expected)) {
    return {
      class: expected?.type === "back" ? "route" : digit ? "shortcut" : "route",
      detail: `描述符不符：期望 ${stableStringify(expected)} / 实际 ${stableStringify(actual)}`,
    };
  }
  return null;
}

/**
 * 单键对拍：期望 vs 观测 → { ok, failures: [{ class, detail }] }。
 * prevFocus：上一键观测焦点（区分「期望移动而实际未动」与「移错位置」）。
 */
export function compareStep({ key, expect, observed, prevFocus = null }) {
  const failures = [];
  const digit = /^(?:Digit|Numpad|KEY_)?[0-9]$/.test(key);

  // 通道层：描述符行缺失 / 焦点锚点异常（先判——通道坏了别的观测都不可信）
  const descriptor = descriptorFailure(key, expect.result, observed.result);
  if (descriptor !== null) failures.push(descriptor);
  if (observed.focusCount > 1) {
    failures.push({ class: "focus-dead-end", detail: `焦点锚点 ${observed.focusCount} 个（唯一性违背：${JSON.stringify(observed.focus)}）` });
  }

  // 焦点层：期望移动而实际未动 → 焦点死路；移错方向/位置 → 路由
  if (observed.focus !== expect.focus) {
    if (observed.focus === null) {
      failures.push({ class: "focus-dead-end", detail: `焦点丢失：期望 ${expect.focus}，实际无 [data-focus="true"] 锚点` });
    } else if (observed.focus === prevFocus && expect.focus !== prevFocus) {
      failures.push({ class: "focus-dead-end", detail: `焦点死路：期望移到 ${expect.focus}，实际原地（${prevFocus}）` });
    } else {
      const cls = digit ? "shortcut" : "route";
      failures.push({ class: cls, detail: `焦点不符：期望 ${expect.focus}，实际 ${observed.focus}` });
    }
  }

  // 状态层：层深 / 退出裁决 / 激活计数（layer 路由与宿主视图同步）
  if (observed.depth !== expect.depth) {
    failures.push({ class: "route", detail: `层深不符：期望 ${expect.depth}，实际 ${observed.depth}` });
  }
  if (observed.exitHint !== expect.exitRequested) {
    failures.push({ class: "route", detail: `退出裁决不符：期望 exitRequested=${expect.exitRequested}，实际提示行 exit=${observed.exitHint}（${JSON.stringify(observed.hint ?? null)}）` });
  }
  if (observed.clicks !== expect.clicks) {
    failures.push({ class: "route", detail: `激活计数不符：期望 ${expect.clicks}，实际 ${observed.clicks}` });
  }
  return failures.length === 0
    ? { ok: true, failures }
    : { ok: false, failures };
}

/** 首类（failures[0]）作为定位主类；无失败为 null。 */
export function primaryClass(result) {
  return result.ok ? null : result.failures[0].class;
}
