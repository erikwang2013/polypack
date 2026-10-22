// 10-foot 皮肤 tokens（T-M4-01）：基值 = polypack init 模板 tokens.css 的值域——与 adapter-native /
// adapter-taro 同一事实源（@polypack/ui 包落地前，模板文件是唯一源，ROADMAP §6）。
// TV 值 = 基值 × TV_SCALE（远距可读：字号 / 间距 / 圆角各 ×2），颜色直通；
// 安全区按视口百分比（5% = 1080p 的 96/54px 边距，防过扫描遮挡）。
// 漂移由 test/tokens.test.js 解析 tokens.css 逐项断言；快照金样见 golden/tv-tokens.json。

/** 基值（= tokens.css 单源；与 native / taro 的 token 集逐一相等）。 */
const base = Object.freeze({
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

/** 10-foot 派生倍率（远距字号 / 间距；TV 无 compact 分支——10-foot 即基线）。 */
export const TV_SCALE = Object.freeze({ font: 2, space: 2, radius: 2 });

/** 电视安全区（视口百分比；标题/动作安全区口径，四边均一，平台可经宿主另调）。 */
export const SAFE_AREA = Object.freeze({ top: 5, right: 5, bottom: 5, left: 5 });

const scaled = (obj, factor) =>
  Object.freeze(Object.fromEntries(Object.entries(obj).map(([k, v]) => [k, v * factor])));

/** TV 10-foot token 集（react-dom 数值即 px；space/font/radius 为派生值）。 */
export const tokens = Object.freeze({
  color: base.color,
  space: scaled(base.space, TV_SCALE.space),
  font: scaled(base.font, TV_SCALE.font),
  radius: base.radius * TV_SCALE.radius,
  safeArea: SAFE_AREA,
});

const pct = (value) => `${value}%`;

/**
 * 安全区像素内边距：视口（默认 1080p）→ { top, right, bottom, left } px（四舍五入）。
 * 宿主外壳与冒烟观测用；页面内消费 platformStyle().safeArea（CSS 百分比形态）。
 */
export function safeAreaInsets(viewport = { width: 1920, height: 1080 }) {
  const { width, height } = viewport ?? {};
  for (const [name, value] of [
    ["width", width],
    ["height", height],
  ]) {
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
      throw new TypeError(`adapter-tv: safeAreaInsets 需视口 ${name}（正数）`);
    }
  }
  return Object.freeze({
    top: Math.round((height * SAFE_AREA.top) / 100),
    right: Math.round((width * SAFE_AREA.right) / 100),
    bottom: Math.round((height * SAFE_AREA.bottom) / 100),
    left: Math.round((width * SAFE_AREA.left) / 100),
  });
}

/**
 * `platformStyle(tokens) -> SkinnedTokens`（ROADMAP §6 契约）。
 * 蒙皮为 react-dom 样式片段——键集与 adapter-native / adapter-taro 同形，TV 增量：
 * safeArea（CSS 百分比）+ overlay（弹层底幕）。10-foot 细节：button 常驻 transparent
 * 边框（宽度 = focusRing.borderWidth），聚焦只换色不换尺寸——焦点环零布局抖动。
 *
 * @param {typeof tokens} [t] token 集（默认内建；@polypack/ui 落地后由其派生）
 */
export function platformStyle(t = tokens) {
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
    // 复合样式：示例页面（src/example.js）直接消费
    card: Object.freeze({
      padding: t.space[4],
      backgroundColor: "#ffffff",
      borderColor: t.color.brandStrong,
      borderWidth: 1,
      borderRadius: t.radius,
      // 1920 参考画布约 2/3 宽（10-foot 页卡）
      maxWidth: 1280,
      textAlign: "center",
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
      marginTop: t.space[3],
      paddingTop: t.space[2],
      paddingBottom: t.space[2],
      paddingLeft: t.space[3],
      paddingRight: t.space[3],
      backgroundColor: t.color.brand,
      borderRadius: t.radius,
      // 常驻透明边框 = 焦点环宽度：聚焦仅换色（零布局抖动）
      borderWidth: 4,
      borderColor: "transparent",
    }),
    buttonLabel: Object.freeze({ fontSize: t.font.body, color: "#ffffff", textAlign: "center" }),
    // 焦点环（T-M4-02 由焦点内核驱动）：10-foot 加粗（4px）
    focusRing: Object.freeze({
      borderColor: t.color.accent,
      borderWidth: 4,
      borderRadius: t.radius,
    }),
    // 弹层底幕（T-M4-03 分层 Back 的详情层消费）
    overlay: Object.freeze({
      backgroundColor: "rgba(15, 23, 42, 0.72)",
      paddingTop: t.space[4],
      paddingBottom: t.space[4],
    }),
  });
}
