// 最小 token 集（T-M2-04）：值与 crates/poly-cli/templates/app/ui/tokens.css 同源。
// @polypack/ui 包落地（ROADMAP §6）前，poly init 模板文件是唯一事实源；
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
