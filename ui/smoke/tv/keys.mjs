// 内核键 → 各通道键映射（单表一处维护）：adb keyevent（Android 腿）与 DOM 键盘
// （web 腿 / 模拟器腿同构）。内核键名口径见 adapter-tv src/focus.js（KEY_MAP / BACK_KEYS）
// 与 shortcuts.js（数字键四族归一）。
//
// adb 侧与 device-walks/walks.mjs 的 KEYMAP 同源对齐（DPAD/CENTER/BACK 常量；数字键
// KEYCODE_1..9）；web 侧：方向键 ArrowUp/Down/Left/Right、OK=Enter（示例页同时收 " "）、
// Back=Escape，数字键按事件 code 保真（Digit3 / Numpad2），主键区数字回落 event.key。

/** 内核键 → adb keyevent 名（Android 腿）。 */
export const ADB_KEYS = Object.freeze({
  DPAD_UP: "KEYCODE_DPAD_UP",
  DPAD_DOWN: "KEYCODE_DPAD_DOWN",
  DPAD_LEFT: "KEYCODE_DPAD_LEFT",
  DPAD_RIGHT: "KEYCODE_DPAD_RIGHT",
  DPAD_CENTER: "KEYCODE_DPAD_CENTER",
  Back: "KEYCODE_BACK",
});

/** 内核键 → DOM 键事件（web 腿；code 用于 Input.dispatchKeyEvent / KeyboardEvent）。 */
export const DOM_KEYS = Object.freeze({
  DPAD_UP: { key: "ArrowUp", code: "ArrowUp" },
  DPAD_DOWN: { key: "ArrowDown", code: "ArrowDown" },
  DPAD_LEFT: { key: "ArrowLeft", code: "ArrowLeft" },
  DPAD_RIGHT: { key: "ArrowRight", code: "ArrowRight" },
  DPAD_CENTER: { key: "Enter", code: "Enter" },
  Back: { key: "Escape", code: "Escape" },
});

/** 数字键四族（"1" / "Digit1" / "Numpad1" / "KEY_1"）→ 单数字。 */
export function digitOf(key) {
  if (/^[0-9]$/.test(key)) return key;
  const hit = /^(?:Digit|Numpad|KEY_)([0-9])$/.exec(key);
  return hit === null ? null : hit[1];
}

/** 内核键 → adb 命令参数（`shell input keyevent <名>` 的 <名> 部分）。 */
export function adbKeyevent(key) {
  const named = ADB_KEYS[key];
  if (named !== undefined) return named;
  const digit = digitOf(key);
  if (digit !== null) return `KEYCODE_${digit}`;
  throw new TypeError(`tv-smoke: 键未入 adb 表：${key}`);
}

/** 内核键 → DOM { key, code }（Input.dispatchKeyEvent 参数）。 */
export function domKey(key) {
  const named = DOM_KEYS[key];
  if (named !== undefined) return named;
  const digit = digitOf(key);
  if (digit !== null) {
    // 保真：Digit3 → code "Digit3"；Numpad2 → code "Numpad2"；"1"/"KEY_1" → 主键区 DigitN
    const code = key.startsWith("Numpad") ? `Numpad${digit}` : `Digit${digit}`;
    return { key: digit, code };
  }
  throw new TypeError(`tv-smoke: 键未入 DOM 表：${key}`);
}

/** DOM 事件 → 内核键（宿主侧归一；页面 onKey 只认内核口径）。 */
export function kernelKeyOfDom(event) {
  const byKey = { ArrowUp: "DPAD_UP", ArrowDown: "DPAD_DOWN", ArrowLeft: "DPAD_LEFT", ArrowRight: "DPAD_RIGHT", Enter: "DPAD_CENTER", " ": "DPAD_CENTER", Escape: "Back" };
  if (byKey[event.key] !== undefined) return byKey[event.key];
  if (/^(Digit|Numpad)[0-9]$/.test(event.code)) return event.code; // 数字键：以 code 保真入内核
  if (/^[0-9]$/.test(event.key)) return event.key;
  return null;
}
