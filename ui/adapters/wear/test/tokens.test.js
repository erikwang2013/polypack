// T-M7-01 tokens 对照 + 金样测试：
// ① 基值 = poly init 模板 tokens.css（唯一事实源）逐项相等（键集合双向，含「模板新增而包内未同步」）；
// ② 穿戴值 = 基值 × WEAR_SCALE（间距 / 圆角紧凑，字号直通）；③ 圆 / 方安全区口径与像素换算；
// ④ 蒙皮为 RN 合法形态（数值即 dp、拆键）；⑤ 快照金样（再生成：node golden/emit-tokens.mjs）。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  SAFE_AREA,
  WEAR_FORMS,
  WEAR_SCALE,
  layoutPage,
  platformStyle,
  safeAreaInsets,
  tokens,
  tokensByForm,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TOKENS_CSS = path.resolve(HERE, "../../../../crates/poly-cli/templates/app/ui/tokens.css");
const GOLDEN = path.resolve(HERE, "../golden/wear-tokens.json");

const cssVars = Object.fromEntries(
  [...fs.readFileSync(TOKENS_CSS, "utf8").matchAll(/--poly-([\w-]+):\s*([^;]+);/g)].map((m) => [
    m[1],
    m[2].trim(),
  ]),
);

test("token 基值：与模板 tokens.css 逐一相等（CSS 为唯一事实源），键集合双向齐全", () => {
  // 基值 = 穿戴值 ÷ WEAR_SCALE（倍数派生可逆：字号倍率 1，颜色直通）
  const expected = {
    "color-brand": tokens.color.brand,
    "color-brand-strong": tokens.color.brandStrong,
    "color-accent": tokens.color.accent,
    "color-surface": tokens.color.surface,
    "color-text": tokens.color.text,
    "color-muted": tokens.color.muted,
    "space-1": `${tokens.space[1] / WEAR_SCALE.space}px`,
    "space-2": `${tokens.space[2] / WEAR_SCALE.space}px`,
    "space-3": `${tokens.space[3] / WEAR_SCALE.space}px`,
    "space-4": `${tokens.space[4] / WEAR_SCALE.space}px`,
    "font-body": `${tokens.font.body / WEAR_SCALE.font}px`,
    "font-small": `${tokens.font.small / WEAR_SCALE.font}px`,
    radius: `${tokens.radius / WEAR_SCALE.radius}px`,
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

test("穿戴派生：间距 / 圆角 ×0.75，字号直通；圆 / 方两形态同值域", () => {
  assert.deepEqual(WEAR_SCALE, { font: 1, space: 0.75, radius: 0.75 });
  assert.deepEqual(WEAR_FORMS, {
    round: { width: 454, height: 454, shape: "round" },
    square: { width: 320, height: 320, shape: "square" },
  });
  for (const form of ["round", "square"]) {
    assert.deepEqual(tokensByForm[form].space, { 1: 3, 2: 6, 3: 12, 4: 18 }, form);
    assert.deepEqual(tokensByForm[form].font, { body: 16, small: 14 }, form);
    assert.equal(tokensByForm[form].radius, 9, form);
    assert.equal(tokensByForm[form].form, form);
    assert.deepEqual(tokensByForm[form].viewport, WEAR_FORMS[form]);
  }
  assert.equal(tokens, tokensByForm.round, "默认形态 = 圆形");
});

test("安全区：圆形 = 内接正方形（14.65% ≥ 精确内缩），方形 = 8%；像素换算按形态视口", () => {
  assert.deepEqual(SAFE_AREA.round, { top: 14.65, right: 14.65, bottom: 14.65, left: 14.65 });
  assert.deepEqual(SAFE_AREA.square, { top: 8, right: 8, bottom: 8, left: 8 });
  assert.deepEqual(safeAreaInsets("round"), { top: 67, right: 67, bottom: 67, left: 67 });
  assert.deepEqual(safeAreaInsets("square"), { top: 26, right: 26, bottom: 26, left: 26 });
  assert.deepEqual(safeAreaInsets("round", { width: 908, height: 908 }), {
    top: 133,
    right: 133,
    bottom: 133,
    left: 133,
  });
  // 圆形内容区宽 ≤ 内接正方形边长（454 / √2 ≈ 321.03）
  const round = safeAreaInsets("round");
  assert.ok(
    WEAR_FORMS.round.width - round.left - round.right <= WEAR_FORMS.round.width / Math.SQRT2,
    "圆形安全区必须落在内接正方形内",
  );
  assert.throws(() => safeAreaInsets("oval"), /形态/);
  assert.throws(() => safeAreaInsets("round", { width: 0, height: 454 }), /width/);
  assert.throws(() => safeAreaInsets("square", { width: 320, height: "320" }), /height/);
});

test("platformStyle()：RN 合法形态、紧凑值、焦点环零布局抖动、自定义 token 直通", () => {
  const round = platformStyle();
  assert.equal(round.form, "round");
  assert.deepEqual(round.safeArea, {
    top: "14.65%",
    right: "14.65%",
    bottom: "14.65%",
    left: "14.65%",
  });
  assert.equal(round.card.maxWidth, 320, "卡片宽 = 视口减左右安全区");
  assert.equal(round.card.padding, 18);
  assert.equal(round.card.gap, 6);
  assert.equal(round.title.fontSize, 24);
  assert.equal(round.title.lineHeight, 29);
  assert.equal(round.button.paddingVertical, 6);
  assert.equal(round.buttonLabel.lineHeight, 19);
  // 聚焦零布局抖动：button 常驻透明边框，宽度与环一致（聚焦仅换色）
  assert.equal(round.button.borderWidth, round.focusRing.borderWidth);
  assert.equal(round.button.borderColor, "transparent");
  assert.equal(round.focusRing.borderColor, "#0e7490");
  assert.equal(round.focusRing.borderWidth, 2);

  const square = platformStyle(tokensByForm.square);
  assert.equal(square.form, "square");
  assert.equal(square.card.maxWidth, 268);
  assert.deepEqual(square.safeArea, { top: "8%", right: "8%", bottom: "8%", left: "8%" });

  // 蒙皮回灌（自定义 token）：颜色直通、安全区不双写百分号、卡片宽仍按形态视口
  const custom = platformStyle({ ...round, color: { ...round.color, brand: "#123456" } });
  assert.equal(custom.button.backgroundColor, "#123456");
  assert.equal(custom.safeArea.top, "14.65%");
  assert.equal(custom.card.maxWidth, 320);

  // RN 拆键合法；不得混入 react-dom 拆键
  assert.ok(JSON.stringify(round).includes("paddingVertical"));
  assert.ok(!JSON.stringify(round).includes("paddingTop"));
  assert.deepEqual(platformStyle(), platformStyle());
  assert.ok(Object.isFrozen(platformStyle()));
  assert.ok(Object.isFrozen(platformStyle().button));
});

test("金样比对：tokens / 蒙皮 / 圆方安全区盒模型（golden/wear-tokens.json）", () => {
  const golden = JSON.parse(fs.readFileSync(GOLDEN, "utf8"));
  assert.deepEqual(
    golden,
    {
      WEAR_SCALE,
      WEAR_FORMS,
      SAFE_AREA,
      tokens: tokensByForm,
      platformStyle: {
        round: platformStyle(tokensByForm.round),
        square: platformStyle(tokensByForm.square),
      },
      layouts: {
        round: layoutPage(tokensByForm.round),
        square: layoutPage(tokensByForm.square),
      },
    },
    "金样漂移：token / 安全区有意变更时重跑 node golden/emit-tokens.mjs 并评审 diff",
  );
});
