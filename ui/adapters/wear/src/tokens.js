// 紧凑皮肤 tokens（T-M7-01）：基值 = polypack init 模板 tokens.css——与 adapter-native / taro / tv
// 同一事实源（@polypack/ui 包落地前，模板文件是唯一源，ROADMAP §6）。
// 穿戴派生 = 间距 / 圆角 ×0.75（小屏紧凑；字号保持基值——低于 14px 不可读），颜色直通。
// 圆 / 方两种形态的安全区（几何口径见 SAFE_AREA）：圆形 = 内接正方形（Wear 圆形屏的
// Material 安全区口径），方形 = 8% 统一内边距（表圈 / 系统指示）。
// 漂移由 test/tokens.test.js 解析 tokens.css 逐项断言；快照金样见 golden/wear-tokens.json。

/** 基值（= tokens.css 单源；与 native / taro / tv 的 token 集逐一相等）。 */
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

/** 穿戴紧凑倍率（间距 / 圆角 ×0.75；字号直通——可读性底线）。 */
export const WEAR_SCALE = Object.freeze({ font: 1, space: 0.75, radius: 0.75 });

/** 两种 Wear 形态与参考视口（Wear OS 模拟器口径：大圆表 454 / 方表 320，单位 px = dp）。 */
export const WEAR_FORMS = Object.freeze({
  round: Object.freeze({ width: 454, height: 454, shape: "round" }),
  square: Object.freeze({ width: 320, height: 320, shape: "square" }),
});

/** 安全区（视口百分比）。圆形 = 内接正方形：每边内缩 (1 - 1/√2)/2 ≈ 14.6447%，
 *  入库上取整 14.65%（≥ 精确值 → 内容仍在内接正方形内）；方形 = 8% 统一内边距。 */
export const SAFE_AREA = Object.freeze({
  round: Object.freeze({ top: 14.65, right: 14.65, bottom: 14.65, left: 14.65 }),
  square: Object.freeze({ top: 8, right: 8, bottom: 8, left: 8 }),
});

const scaled = (obj, factor) =>
  Object.freeze(
    Object.fromEntries(Object.entries(obj).map(([k, v]) => [k, Math.round(v * factor)])),
  );

const wearTokens = (form) =>
  Object.freeze({
    form,
    viewport: WEAR_FORMS[form],
    color: base.color,
    space: scaled(base.space, WEAR_SCALE.space),
    font: base.font,
    radius: Math.round(base.radius * WEAR_SCALE.radius),
    safeArea: SAFE_AREA[form],
  });

/** 两种形态的 token 集（页面树 / 盒模型 / 蒙皮同源）。 */
export const tokensByForm = Object.freeze({
  round: wearTokens("round"),
  square: wearTokens("square"),
});

/** 默认形态（圆形）token 集。 */
export const tokens = tokensByForm.round;

const num = (value) => (typeof value === "number" ? value : Number.parseFloat(value));

const assertViewport = (viewport) => {
  const { width, height } = viewport ?? {};
  for (const [name, value] of [
    ["width", width],
    ["height", height],
  ]) {
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
      throw new TypeError(`adapter-wear: safeAreaInsets 需视口 ${name}（正数）`);
    }
  }
};

/** 安全区百分比 + 视口 → px 内边距（四舍五入；圆形与方形同一算法，百分比不同）。 */
export function insetsFrom(safeArea, viewport) {
  assertViewport(viewport);
  return Object.freeze({
    top: Math.round((viewport.height * num(safeArea.top)) / 100),
    right: Math.round((viewport.width * num(safeArea.right)) / 100),
    bottom: Math.round((viewport.height * num(safeArea.bottom)) / 100),
    left: Math.round((viewport.width * num(safeArea.left)) / 100),
  });
}

/**
 * 安全区像素内边距：形态（round / square）+ 视口（默认该形态参考视口）→ px。
 * 宿主外壳与冒烟观测用；页面内消费 platformStyle().safeArea（CSS / RN 百分比形态）。
 */
export function safeAreaInsets(form = "round", viewport) {
  if (!Object.hasOwn(WEAR_FORMS, form)) {
    throw new TypeError(`adapter-wear: 未知形态 "${form}"（round / square）`);
  }
  return insetsFrom(SAFE_AREA[form], viewport ?? WEAR_FORMS[form]);
}

const pct = (value) => (typeof value === "string" ? value : `${value}%`);

/**
 * `platformStyle(tokens) -> SkinnedTokens`（ROADMAP §6 契约）。
 * 蒙皮为 RN 合法样式片段（数值即 dp；拆键 paddingVertical/Horizontal——Wear OS UI 运行时
 * 为 RN beta，T-M7-02）：紧凑字号 / 间距 / 圆角 + 安全区百分比 + 焦点环。
 * button 常驻透明边框（宽度 = focusRing.borderWidth），聚焦只换色不换尺寸——零布局抖动。
 *
 * @param {typeof tokens} [t] token 集（默认圆形；宿主可传 tokensByForm[form]）
 */
export function platformStyle(t = tokens) {
  const form = t.form ?? "round";
  const viewport = t.viewport ?? WEAR_FORMS[form] ?? WEAR_FORMS.round;
  const insets = insetsFrom(t.safeArea, viewport);
  const titleSize = Math.round(t.font.body * 1.5);
  return Object.freeze({
    form,
    viewport,
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
      gap: t.space[2],
      backgroundColor: "#ffffff",
      borderColor: t.color.brandStrong,
      borderWidth: 1,
      borderRadius: t.radius,
      // 卡片撑满外框内容盒（= 安全区宽）且不越出安全区
      width: "100%",
      maxWidth: viewport.width - insets.left - insets.right,
    }),
    row: Object.freeze({ flexDirection: "row", gap: t.space[2] }),
    title: Object.freeze({
      fontSize: titleSize,
      lineHeight: Math.round(titleSize * 1.2),
      fontWeight: "600",
      color: t.color.text,
      textAlign: "center",
    }),
    text: Object.freeze({
      fontSize: t.font.body,
      lineHeight: Math.round(t.font.body * 1.2),
      color: t.color.text,
      textAlign: "center",
    }),
    muted: Object.freeze({
      fontSize: t.font.small,
      lineHeight: Math.round(t.font.small * 1.2),
      color: t.color.muted,
      textAlign: "center",
    }),
    button: Object.freeze({
      flex: 1,
      paddingVertical: t.space[2],
      paddingHorizontal: t.space[3],
      backgroundColor: t.color.brand,
      borderRadius: t.radius,
      alignItems: "center",
      // 常驻透明边框 = 焦点环宽度：聚焦仅换色（零布局抖动）
      borderWidth: 2,
      borderColor: "transparent",
    }),
    buttonLabel: Object.freeze({
      fontSize: t.font.body,
      lineHeight: Math.round(t.font.body * 1.2),
      color: "#ffffff",
      textAlign: "center",
    }),
    // 焦点环（无指针降级由共享焦点内核驱动，见 src/focus.js）
    focusRing: Object.freeze({
      borderColor: t.color.accent,
      borderWidth: 2,
      borderRadius: t.radius,
    }),
  });
}
