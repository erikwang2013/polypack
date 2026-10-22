// 最小 token 集（T-M2-04）：值与 crates/polypack-cli/templates/app/ui/tokens.css 同源。
// @polypack/ui 包落地（ROADMAP §6）前，polypack init 模板文件是唯一事实源；
// 漂移由 test/tokens.test.js 解析该 CSS 逐一断言（含「模板新增 token 而包内未同步」）。

/** 颜色 / 间距 / 字号 / 圆角。间距与字号为 CSS px 数值，RN 侧即 dp（无单位）。 */
export const tokens = Object.freeze({
  color: Object.freeze({
    brand: "#ea580c",
    brandStrong: "#9a3412",
    accent: "#0e7490",
    surface: "#fff7ed",
    text: "#0f172a",
    muted: "#64748b",
  }),
  space: Object.freeze({ 1: 4, 2: 8, 3: 16, 4: 24 }),
  font: Object.freeze({ body: 16, small: 14 }),
  radius: 12,
});

/** 紧凑间距系数（小屏降级：ROADMAP §6「小屏 → 紧凑间距」）。 */
const COMPACT_SCALE = 0.75;

/** 10-foot 派生倍率与安全区（T-M4-06 RN-tvOS）：口径单源 = ui/adapters/tv/src/tokens.js，
 *  漂移由 test/tvos-host-structure.test.js 跨包同调用哨兵断言（不引跨包运行时依赖）。 */
export const TV_SCALE = Object.freeze({ font: 2, space: 2, radius: 2 });
export const TV_SAFE_AREA = Object.freeze({ top: 5, right: 5, bottom: 5, left: 5 });

const scaled = (obj, factor) =>
  Object.freeze(Object.fromEntries(Object.entries(obj).map(([k, v]) => [k, v * factor])));

/** TV 10-foot token 集（RN 数值即 dp）：基值 × TV_SCALE，颜色直通；TV 无 compact 分支（10-foot 即基线）。 */
export const tvosTokens = Object.freeze({
  color: tokens.color,
  space: scaled(tokens.space, TV_SCALE.space),
  font: scaled(tokens.font, TV_SCALE.font),
  radius: tokens.radius * TV_SCALE.radius,
  safeArea: TV_SAFE_AREA,
});

/**
 * `platformStyle(tokens) -> SkinnedTokens`（ROADMAP §6 契约）。
 * 最小 token 集蒙皮为 RN 样式片段——值为 RN 合法形态（数值即 dp，fontWeight 用字符串）。
 *
 * @param {typeof tokens} [t] token 集（默认内建；@polypack/ui 落地后由其派生）
 * @param {{ compact?: boolean }} [options] compact = true → 间距紧凑化（小屏降级分支）
 */
export function platformStyle(t = tokens, options = {}) {
  const scale = options.compact === true ? COMPACT_SCALE : 1;
  const space = Object.freeze(
    Object.fromEntries(Object.entries(t.space).map(([k, v]) => [k, Math.round(v * scale)])),
  );
  return Object.freeze({
    color: t.color,
    space,
    font: t.font,
    radius: t.radius,
    // 复合样式：示例页面（src/example.js）直接消费
    card: Object.freeze({
      padding: space[4],
      backgroundColor: "#ffffff",
      borderColor: t.color.brandStrong,
      borderWidth: 1,
      borderRadius: t.radius,
      maxWidth: 420,
      // tokens.css 的 .card 是 text-align:center，RN 文本不继承对齐 → 显式两处补齐
      alignItems: "center",
    }),
    title: Object.freeze({
      fontSize: t.font.body * 1.5,
      fontWeight: "600",
      color: t.color.text,
      textAlign: "center",
    }),
    text: Object.freeze({ fontSize: t.font.body, color: t.color.text, textAlign: "center" }),
    muted: Object.freeze({ fontSize: t.font.small, color: t.color.muted, textAlign: "center" }),
    button: Object.freeze({
      marginVertical: space[3],
      paddingVertical: space[2],
      paddingHorizontal: space[3],
      backgroundColor: t.color.brand,
      borderRadius: t.radius,
    }),
    buttonLabel: Object.freeze({ fontSize: t.font.body, color: "#ffffff", textAlign: "center" }),
    // 焦点环（T-M2-05）：无指针环境由焦点内核驱动，见 src/focus.js
    focusRing: Object.freeze({
      borderColor: t.color.accent,
      borderWidth: 2,
      borderRadius: t.radius,
    }),
  });
}

/**
 * `platformStyleTvos(tokens) -> SkinnedTokens`（T-M4-06）：10-foot RN 蒙皮。值与 adapter-tv
 * 的 platformStyle 同口径（字号/间距/圆角 ×2、focusRing 4px、card maxWidth 1280、button 常驻
 * 透明边框零布局抖动），形态取 RN（拆键 vertical/horizontal、safeArea 为百分比字符串——
 * RN 的 DimensionValue 合法形态）。createAdapter 按 target=tvos 绑定：页面 `platformStyle()`
 * 零参直取此份（10-foot 即 TV 默认蒙皮，页面零改动）。
 *
 * @param {typeof tvosTokens} [t] 10-foot token 集（默认内建）
 */
export function platformStyleTvos(t = tvosTokens) {
  const pct = (value) => `${value}%`;
  return Object.freeze({
    color: t.color,
    space: t.space,
    font: t.font,
    radius: t.radius,
    safeArea: Object.freeze({
      top: pct(t.safeArea.top),
      right: pct(t.safeArea.right),
      bottom: pct(t.safeArea.bottom),
      left: pct(t.safeArea.left),
    }),
    // 复合样式：示例页面（src/example.js）直接消费（与 base 蒙皮同键集）
    card: Object.freeze({
      padding: t.space[4],
      backgroundColor: "#ffffff",
      borderColor: t.color.brandStrong,
      borderWidth: 1,
      borderRadius: t.radius,
      // 1920 参考画布约 2/3 宽（10-foot 页卡；与 adapter-tv 同值）
      maxWidth: 1280,
      alignItems: "center",
    }),
    title: Object.freeze({
      fontSize: Math.round(t.font.body * 1.5),
      fontWeight: "600",
      color: t.color.text,
      textAlign: "center",
    }),
    text: Object.freeze({ fontSize: t.font.body, color: t.color.text, textAlign: "center" }),
    muted: Object.freeze({ fontSize: t.font.small, color: t.color.muted, textAlign: "center" }),
    button: Object.freeze({
      marginVertical: t.space[3],
      paddingVertical: t.space[2],
      paddingHorizontal: t.space[3],
      backgroundColor: t.color.brand,
      borderRadius: t.radius,
      // 常驻透明边框 = 焦点环宽度：聚焦仅换色（10-foot 零布局抖动，与 adapter-tv 同值）
      borderWidth: 4,
      borderColor: "transparent",
    }),
    buttonLabel: Object.freeze({ fontSize: t.font.body, color: "#ffffff", textAlign: "center" }),
    // 焦点环（T-M4-02 内核驱动）：10-foot 加粗 4px
    focusRing: Object.freeze({
      borderColor: t.color.accent,
      borderWidth: 4,
      borderRadius: t.radius,
    }),
    // 弹层底幕（T-M4-03 分层 Back 的详情层消费）
    overlay: Object.freeze({
      backgroundColor: "rgba(15, 23, 42, 0.72)",
      paddingVertical: t.space[4],
    }),
  });
}
