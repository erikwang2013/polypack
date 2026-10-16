// T-M3-05 tokens 对照测试：包内最小 token 集与 poly init 模板 tokens.css 同源。
// 双重哨兵：值逐一相等 + 键集合双向相等（模板新增 token 而包内未同步即失败）。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { platformStyle, tokens } from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TOKENS_CSS = path.resolve(HERE, "../../../../crates/poly-cli/templates/app/ui/tokens.css");

const cssVars = Object.fromEntries(
  [...fs.readFileSync(TOKENS_CSS, "utf8").matchAll(/--poly-([\w-]+):\s*([^;]+);/g)].map((m) => [
    m[1],
    m[2].trim(),
  ]),
);

test("token 值与 poly init 模板 tokens.css 逐一相等（CSS 为唯一事实源）", () => {
  const expected = {
    "color-brand": tokens.color.brand,
    "color-brand-strong": tokens.color.brandStrong,
    "color-accent": tokens.color.accent,
    "color-surface": tokens.color.surface,
    "color-text": tokens.color.text,
    "color-muted": tokens.color.muted,
    "space-1": `${tokens.space[1]}px`,
    "space-2": `${tokens.space[2]}px`,
    "space-3": `${tokens.space[3]}px`,
    "space-4": `${tokens.space[4]}px`,
    "font-body": `${tokens.font.body}px`,
    "font-small": `${tokens.font.small}px`,
    radius: `${tokens.radius}px`,
  };
  assert.deepEqual(
    Object.keys(cssVars).sort(),
    Object.keys(expected).sort(),
    "token 键集合漂移：模板与包内最小 token 集须同步",
  );
  for (const [key, value] of Object.entries(expected)) {
    assert.equal(cssVars[key], value, `token --poly-${key} 漂移`);
  }
});

test("platformStyle()：Taro 合法形态（数值即设计稿 px，无 px 字符串；fontWeight 用字符串）", () => {
  const styles = platformStyle();
  assert.ok(!JSON.stringify(styles).includes("px"), "样式值不得含 px 单位（Taro 编译期换算 rpx）");
  assert.equal(styles.card.padding, tokens.space[4]);
  assert.equal(styles.card.borderRadius, tokens.radius);
  assert.equal(styles.title.fontSize, tokens.font.body * 1.5);
  assert.equal(styles.title.fontWeight, "600");
  assert.equal(styles.button.backgroundColor, tokens.color.brand);
  assert.equal(styles.muted.color, tokens.color.muted);
});

test("platformStyle()：文本对齐与 tokens.css 的 .card（text-align:center）同形", () => {
  assert.match(fs.readFileSync(TOKENS_CSS, "utf8"), /\.card\s*\{[^}]*text-align:\s*center/);
  const styles = platformStyle();
  assert.equal(styles.card.alignItems, "center", "小程序组件不继承对齐：卡片需显式居中（图标）");
  for (const key of ["title", "text", "muted", "buttonLabel"]) {
    assert.equal(styles[key].textAlign, "center", `${key} 须与模板页居中对齐一致`);
  }
});

test("platformStyle()：焦点环由 token 派生，compact 只紧凑间距", () => {
  const normal = platformStyle();
  assert.equal(normal.focusRing.borderColor, tokens.color.accent);
  assert.equal(normal.focusRing.borderWidth, 2);
  assert.equal(normal.focusRing.borderRadius, tokens.radius);

  const compact = platformStyle(tokens, { compact: true });
  assert.equal(compact.space[3], 12, "16px × 0.75");
  assert.equal(compact.space[4], 18, "24px × 0.75");
  assert.equal(compact.card.padding, 18, "复合样式跟随紧凑间距");
  assert.equal(compact.font.body, tokens.font.body, "字号不随 compact 变化");
  assert.equal(compact.focusRing.borderWidth, 2);
});

test("platformStyle()：确定性（两次求值深相等）且冻结", () => {
  assert.deepEqual(platformStyle(), platformStyle());
  assert.ok(Object.isFrozen(platformStyle()));
});
