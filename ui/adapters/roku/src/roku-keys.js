// Roku 遥控器键字面（onKeyEvent 的 key 形参）→ 共享焦点内核键名（ui/adapters/tv/src/focus.js
// 再导出的 @polypack/adapter-native 内核，KEY_MAP / 激活键 / Back 键三族口径）。
// 与 adapter-tv 的差异仅在「宿主事件源」这一层——RN 收 DPAD_*，Roku 收小写名与 "OK"。
// 交叉核验：test/keys.test.js 以 handleKey 行为断言（move/activate/back），不比对字面表。

/** Roku onKeyEvent key → 内核键名。select 为防御性兼容（部分文档形态把 OK 报作 select）。 */
export const ROKU_KEY_MAP = Object.freeze({
  up: "DPAD_UP",
  down: "DPAD_DOWN",
  left: "DPAD_LEFT",
  right: "DPAD_RIGHT",
  OK: "DPAD_CENTER",
  select: "DPAD_CENTER",
  back: "Back",
});

/** 未映射键（数字键 / info 等）→ null：宿主放行，不消费。 */
export function mapRokuKey(key) {
  return ROKU_KEY_MAP[key] ?? null;
}
