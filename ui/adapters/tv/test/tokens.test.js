// T-M4-01 tokens 对照 + 金样测试：
// ① 基值 = poly init 模板 tokens.css（唯一事实源）逐项相等（键集合双向，含「模板新增而包内未同步」）；
// ② TV 值 = 基值 × TV_SCALE（10-foot 派生）；③ 蒙皮为 react-dom 合法形态；
// ④ 快照金样（golden/tv-tokens.json，再生成：node golden/emit-tokens.mjs）。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  SAFE_AREA,
  TV_SCALE,
  platformStyle,
  safeAreaInsets,
  tokens,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TOKENS_CSS = path.resolve(HERE, "../../../../crates/poly-cli/templates/app/ui/tokens.css");
const GOLDEN = path.resolve(HERE, "../golden/tv-tokens.json");

const cssVars = Object.fromEntries(
  [...fs.readFileSync(TOKENS_CSS, "utf8").matchAll(/--poly-([\w-]+):\s*([^;]+);/g)].map((m) => [
    m[1],
    m[2].trim(),
  ]),
);

test("token 基值：与模板 tokens.css 逐一相等（CSS 为唯一事实源），键集合双向齐全", () => {
  // 基值 = TV 值 ÷ TV_SCALE（倍数派生可逆：颜色直通）
  const expected = {
    "color-brand": tokens.color.brand,
    "color-brand-strong": tokens.color.brandStrong,
    "color-accent": tokens.color.accent,
    "color-surface": tokens.color.surface,
    "color-text": tokens.color.text,
    "color-muted": tokens.color.muted,
    "space-1": `${tokens.space[1] / TV_SCALE.space}px`,
    "space-2": `${tokens.space[2] / TV_SCALE.space}px`,
    "space-3": `${tokens.space[3] / TV_SCALE.space}px`,
    "space-4": `${tokens.space[4] / TV_SCALE.space}px`,
    "font-body": `${tokens.font.body / TV_SCALE.font}px`,
    "font-small": `${tokens.font.small / TV_SCALE.font}px`,
    radius: `${tokens.radius / TV_SCALE.radius}px`,
  };
  assert.deepEqual(
    Object.keys(cssVars).sort(),
    Object.keys(expected).sort(),
    "token 键集合漂移：模板与包内 token 集须同步",
  );
  for (const [key, value] of Object.entries(expected)) {
    assert.equal(cssVars[key], value, `token --poly-${key} 漂移`);
  }
});

test("10-foot 派生：字号 / 间距 / 圆角 = 基值 × TV_SCALE（远距字号；颜色直通）", () => {
  assert.deepEqual(tokens, {
    color: {
      brand: "#ea580c",
      brandStrong: "#9a3412",
      accent: "#0e7490",
      surface: "#fff7ed",
      text: "#0f172a",
      muted: "#64748b",
    },
    space: { 1: 8, 2: 16, 3: 32, 4: 48 },
    font: { body: 32, small: 28 },
    radius: 24,
    safeArea: { top: 5, right: 5, bottom: 5, left: 5 },
  });
  assert.deepEqual(TV_SCALE, { font: 2, space: 2, radius: 2 });
});

test("安全区：tokens 为百分比、蒙皮为 CSS 形态、insets 按视口换算（默认 1080p）", () => {
  assert.deepEqual(SAFE_AREA, { top: 5, right: 5, bottom: 5, left: 5 });
  assert.deepEqual(platformStyle().safeArea, { top: "5%", right: "5%", bottom: "5%", left: "5%" });
  assert.deepEqual(safeAreaInsets(), { top: 54, right: 96, bottom: 54, left: 96 });
  assert.deepEqual(safeAreaInsets({ width: 3840, height: 2160 }), {
    top: 108,
    right: 192,
    bottom: 108,
    left: 192,
  });
  assert.throws(() => safeAreaInsets({ width: -1, height: 1080 }), /width/);
  assert.throws(() => safeAreaInsets(null), /width/);
});

test("platformStyle()：react-dom 合法形态（数值即 px；无 RN 拆键；10-foot 环零布局抖动）", () => {
  const styles = platformStyle();
  // RN 专有拆键（marginVertical 等）不入 react-dom 蒙皮
  assert.ok(!JSON.stringify(styles).includes("Vertical"), "react-dom 蒙皮不得含 RN 拆键");
  assert.ok(!JSON.stringify(styles).includes("Horizontal"), "react-dom 蒙皮不得含 RN 拆键");
  assert.equal(styles.card.padding, tokens.space[4]);
  assert.equal(styles.title.fontSize, tokens.font.body * 1.5, "标题 = 字号 ×1.5（远距字号统一派生）");
  assert.equal(styles.button.backgroundColor, tokens.color.brand);
  assert.equal(styles.focusRing.borderColor, tokens.color.accent);
  assert.equal(styles.focusRing.borderWidth, 4, "10-foot 焦点环加粗");
  // 聚焦零布局抖动：button 常驻透明边框，宽度与环一致（聚焦仅换色）
  assert.equal(styles.button.borderWidth, styles.focusRing.borderWidth);
  assert.equal(styles.button.borderColor, "transparent");
  assert.equal(styles.overlay.backgroundColor.startsWith("rgba"), true);
});

test("platformStyle()：确定性（两次求值深相等）且冻结", () => {
  assert.deepEqual(platformStyle(), platformStyle());
  assert.ok(Object.isFrozen(platformStyle()));
  assert.ok(Object.isFrozen(platformStyle().focusRing));
});

test("tokens 快照金样比对（golden/tv-tokens.json；再生成 node golden/emit-tokens.mjs）", () => {
  const golden = JSON.parse(fs.readFileSync(GOLDEN, "utf8"));
  assert.deepEqual(
    golden,
    { tokens, TV_SCALE, SAFE_AREA, platformStyle: platformStyle() },
    "金样漂移：token 有意变更时重跑 node golden/emit-tokens.mjs 并评审 diff",
  );
});
