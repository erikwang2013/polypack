// 安全区盒模型（T-M7-01 验收）：无浏览器 / 无布局引擎的确定性纵流盒模型。
// 与 src/example.js 的页面树同构：frame（安全区内边距）> card > [标题 / 版本 / 2×2 按钮 / 提示]。
// 输入 token 集（form / viewport / safeArea / space / font / radius），输出各元素 px 盒；
// 文案按单行计高（示例页文案在方形参考宽 232px 内可单行容纳）。
// 安全区断言见 test/layout.test.js（逐盒 ⊆ safe）；金样见 golden/wear-tokens.json。

import { insetsFrom, platformStyle, tokens } from "./tokens.js";

const box = (x, y, width, height) => Object.freeze({ x, y, width, height });

/**
 * 排版示例页的同一棵树（两种 Wear 形态同构，仅安全区 / 卡片宽不同）。
 *
 * @param {typeof tokens} [t] token 集（默认圆形）
 * @returns {{ form: string, viewport: object, safe: object,
 *   boxes: Record<"frame"|"card"|"title"|"version"|"counter"|"reset"|"detail"|"exit"|"hint", object> }}
 *   safe = 安全区内接矩形（px）；frame = 整个视口（其内边距 = 安全区）。
 */
export function layoutPage(t = tokens) {
  const styles = platformStyle(t);
  const viewport = t.viewport ?? styles.viewport;
  const insets = insetsFrom(t.safeArea, viewport);
  const safe = box(
    insets.left,
    insets.top,
    viewport.width - insets.left - insets.right,
    viewport.height - insets.top - insets.bottom,
  );

  const cardWidth = styles.card.maxWidth;
  const padding = styles.card.padding;
  const gap = styles.card.gap;
  const contentWidth = cardWidth - padding * 2;
  const rowHeight =
    styles.button.paddingVertical * 2 +
    styles.buttonLabel.lineHeight +
    styles.button.borderWidth * 2;
  const buttonWidth = Math.floor((contentWidth - gap) / 2);
  const contentHeight =
    styles.title.lineHeight +
    styles.muted.lineHeight +
    rowHeight * 2 +
    styles.muted.lineHeight +
    gap * 4;
  const cardHeight = contentHeight + padding * 2;

  const card = box(
    safe.x + Math.floor((safe.width - cardWidth) / 2),
    safe.y + Math.round((safe.height - cardHeight) / 2),
    cardWidth,
    cardHeight,
  );
  const contentX = card.x + padding;
  let y = card.y + padding;

  const title = box(contentX, y, contentWidth, styles.title.lineHeight);
  y += title.height + gap;
  const version = box(contentX, y, contentWidth, styles.muted.lineHeight);
  y += version.height + gap;
  const row1Y = y;
  y += rowHeight + gap;
  const row2Y = y;
  y += rowHeight + gap;
  const hint = box(contentX, y, contentWidth, styles.muted.lineHeight);

  return Object.freeze({
    form: t.form ?? "round",
    viewport,
    safe,
    boxes: Object.freeze({
      frame: box(0, 0, viewport.width, viewport.height),
      card,
      title,
      version,
      counter: box(contentX, row1Y, buttonWidth, rowHeight),
      reset: box(contentX + buttonWidth + gap, row1Y, buttonWidth, rowHeight),
      detail: box(contentX, row2Y, buttonWidth, rowHeight),
      exit: box(contentX + buttonWidth + gap, row2Y, buttonWidth, rowHeight),
      hint,
    }),
  });
}

/**
 * 页面焦点节点（内核形态）：rect 直接取示例按钮盒——焦点遍历图由 adapter-tv 的几何推导
 * （deriveFocusNodes）构造，2×2 网格在两形态下均四向可达（test/focus.test.js 证明）。
 *
 * @param {typeof tokens} [t] token 集（默认圆形）
 * @returns {ReadonlyArray<{ id: string, rect: object }>} counter / reset / detail / exit
 */
export function pageFocusNodes(t = tokens) {
  const { boxes } = layoutPage(t);
  return Object.freeze([
    Object.freeze({ id: "counter", rect: boxes.counter }),
    Object.freeze({ id: "reset", rect: boxes.reset }),
    Object.freeze({ id: "detail", rect: boxes.detail }),
    Object.freeze({ id: "exit", rect: boxes.exit }),
  ]);
}
