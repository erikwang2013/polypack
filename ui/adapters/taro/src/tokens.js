// 最小 token 集（T-M3-05）：值与 crates/polypack-cli/templates/app/ui/tokens.css 同源
// （与 adapter-native 同一事实源；@polypack/ui 包落地前，polypack init 模板文件是唯一源）。
// 漂移由 test/tokens.test.js 解析该 CSS 逐一断言（含「模板新增 token 而包内未同步」）。

/** 颜色 / 间距 / 字号 / 圆角。间距与字号为设计稿 px 数值（Taro 编译期按 designWidth 换算 rpx）。 */
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
 * 蒙皮为 Taro 样式片段——键集与 adapter-native 同形（同一页面组件树跨适配器蒙皮），
 * 值为 Taro 合法形态（数值即设计稿 px，Taro 编译期换算 rpx；fontWeight 用字符串）。
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
      // tokens.css 的 .card 是 text-align:center；小程序组件不继承对齐 → 显式补齐
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
      marginTop: space[3],
      paddingTop: space[2],
      paddingBottom: space[2],
      paddingLeft: space[3],
      paddingRight: space[3],
      backgroundColor: t.color.brand,
      borderRadius: t.radius,
    }),
    buttonLabel: Object.freeze({ fontSize: t.font.body, color: "#ffffff", textAlign: "center" }),
    // 无指针降级环（与 adapter-native 同形）：小程序族默认不渲染（capabilities.focusNav = false），
    // 仅无指针容器（宿主提示 pointer: "none"）下由示例页渲染
    focusRing: Object.freeze({
      borderColor: t.color.accent,
      borderWidth: 2,
      borderRadius: t.radius,
    }),
  });
}
