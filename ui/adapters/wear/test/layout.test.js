// T-M7-01 安全区验收：同一页面盒模型（与 src/example.js 的页面树同构）在圆 / 方两种
// Wear 尺寸下均不越出安全区。盒模型 = frame > card > [标题 / 版本 / 2×2 按钮 / 提示]。

import assert from "node:assert/strict";
import { test } from "node:test";

import { WEAR_FORMS, layoutPage, pageFocusNodes, tokensByForm } from "../src/index.js";

const FORMS = Object.freeze(["round", "square"]);

/** 盒包含判定（边界相接算在内——卡片宽恰好等于安全区宽）。 */
const inside = (outer, inner) =>
  inner.x >= outer.x &&
  inner.y >= outer.y &&
  inner.x + inner.width <= outer.x + outer.width &&
  inner.y + inner.height <= outer.y + outer.height;

test("两种形态：页面树全部盒（卡片 / 文本 / 按钮 / 提示）都落在安全区内", () => {
  for (const form of FORMS) {
    const layout = layoutPage(tokensByForm[form]);
    assert.equal(layout.form, form);
    assert.deepEqual(layout.viewport, WEAR_FORMS[form]);
    for (const [id, box] of Object.entries(layout.boxes)) {
      if (id === "frame") continue;
      assert.ok(
        inside(layout.safe, box),
        `${form}: ${id} 越出安全区（box=${JSON.stringify(box)} safe=${JSON.stringify(layout.safe)}）`,
      );
    }
    // 外框即整视口；其内边距由安全区百分比派生（platformStyle().safeArea）
    assert.deepEqual(layout.boxes.frame, {
      x: 0,
      y: 0,
      width: WEAR_FORMS[form].width,
      height: WEAR_FORMS[form].height,
    });
  }
});

test("安全区口径：圆形 = 内接正方形，方形 = 8% 内边距（两种形态均居中）", () => {
  const round = layoutPage(tokensByForm.round);
  assert.equal(round.safe.width, round.safe.height);
  assert.ok(
    round.safe.width <= WEAR_FORMS.round.width / Math.SQRT2,
    "圆形内容区不得超出内接正方形",
  );
  assert.ok(round.safe.width > WEAR_FORMS.round.width / 2, "内接正方形口径不应过度收缩");

  const square = layoutPage(tokensByForm.square);
  assert.equal(square.safe.width, 320 - 2 * 26);
  assert.equal(square.safe.width, square.safe.height);

  for (const layout of [round, square]) {
    assert.equal(layout.safe.x, (layout.viewport.width - layout.safe.width) / 2);
    assert.equal(layout.safe.y, (layout.viewport.height - layout.safe.height) / 2);
  }
});

test("卡片：宽 = 安全区宽、水平贴安全区、纵向居中（±1px 取整容差）", () => {
  for (const form of FORMS) {
    const { safe, boxes } = layoutPage(tokensByForm[form]);
    assert.equal(boxes.card.width, safe.width);
    assert.equal(boxes.card.x, safe.x);
    const top = boxes.card.y - safe.y;
    const bottom = safe.y + safe.height - (boxes.card.y + boxes.card.height);
    assert.ok(Math.abs(top - bottom) <= 1, `${form}: 卡片纵向未居中（上 ${top} / 下 ${bottom}）`);
    assert.ok(boxes.card.height > 0 && boxes.card.height < safe.height, `${form}: 卡片高度异常`);
  }
});

test("聚焦节点 = 页面按钮盒（同树派生；两种形态均 2×2 网格且上下行不重叠）", () => {
  for (const form of FORMS) {
    const layout = layoutPage(tokensByForm[form]);
    const nodes = pageFocusNodes(tokensByForm[form]);
    assert.deepEqual(nodes.map((node) => node.id), ["counter", "reset", "detail", "exit"]);
    for (const node of nodes) {
      assert.deepEqual(node.rect, layout.boxes[node.id], `${form}: ${node.id} 焦点盒漂移`);
    }
    assert.equal(layout.boxes.counter.y, layout.boxes.reset.y);
    assert.equal(layout.boxes.detail.y, layout.boxes.exit.y);
    assert.ok(layout.boxes.exit.y >= layout.boxes.counter.y + layout.boxes.counter.height);
  }
});
