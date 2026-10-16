// 遥控器数字键快捷映射（T-M4-03）：数字 → 宿主域焦点目标 id。
// 键名四族归一：键盘 "3" / "Digit3" / "Numpad3" 与 web 电视外壳 "KEY_3"（宿主事件源
// 只做平台键码 → 逻辑键名的映射，归一在本模块）。
// 口径：数字键只在宿主域路由（层打开时被 handleTvKey 拦截，不穿透弹层）。

/** 数字键归一：键名 → "1".."9"；非数字键 / 畸形键 → null（0 与多位数不映射）。 */
export function normalizeShortcutKey(key) {
  if (typeof key !== "string") return null;
  const match = /^(?:Digit|Numpad|KEY_)?([1-9])$/.exec(key);
  return match === null ? null : match[1];
}

/**
 * 构造快捷映射：{ 数字键: 宿主域节点 id }。构造期校验（键须为数字键、值须为非空 id、
 * 归一后不得重复）；目标节点是否在宿主域由 handleTvKey 在宿主引擎上校验（层内节点
 * 会被拒——域隔离）。
 *
 * @param {Record<string, string>} entries 如 `{ 1: "counter", Digit3: "detail" }`
 * @returns {Readonly<Record<string, string>>} 归一后的数字 → 节点 id（冻结）
 */
export function createShortcutMap(entries) {
  if (entries === null || typeof entries !== "object") {
    throw new TypeError("shortcuts: 映射须为对象（数字键 → 节点 id）");
  }
  const map = {};
  for (const [key, id] of Object.entries(entries)) {
    const digit = normalizeShortcutKey(key);
    if (digit === null) {
      throw new TypeError(`shortcuts: 键须为数字键（1-9；亦收 Digit3 / Numpad3 / KEY_3 形态）: ${key}`);
    }
    if (typeof id !== "string" || id === "") {
      throw new TypeError(`shortcuts: 数字 ${key} 须映射非空节点 id`);
    }
    if (Object.hasOwn(map, digit)) {
      throw new TypeError(`shortcuts: 数字键归一后重复: ${key}`);
    }
    map[digit] = id;
  }
  return Object.freeze(map);
}
