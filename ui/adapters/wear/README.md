# @polypack/adapter-wear v0

ROADMAP §6 RendererAdapter 契约的穿戴装配层（T-M7-01）：紧凑 tokens（圆 / 方两种安全区）+
能力降级（`capabilities()` 缺失能力时交互按契约降级；无指针 → 复用 adapter-tv /
adapter-native 的共享焦点内核，不新写导航系统）。零 npm 依赖，纯 `node:test`。

```text
ui/adapters/wear/
├── src/
│   ├── index.js          # createAdapter：mount（RN AppRegistry）/ platformStyle / capabilities / degradations / safeArea
│   ├── tokens.js         # 紧凑 tokens：间距 / 圆角 ×0.75 + 圆 / 方安全区 + platformStyle(tokens)
│   ├── capabilities.js   # 能力矩阵（wearos 触摸基线；pointer=none → focusNav）+ 降级分支闭合集
│   ├── focus.js          # 焦点内核入口：re-export adapter-tv / native 共享内核（零新增导航代码）
│   ├── layout.js         # 页面同构盒模型（安全区断言与金样）+ 聚焦节点（按钮盒）
│   └── example.js        # 示例页面描述树（宿主无关；零 react）
├── golden/               # tokens / 蒙皮 / 圆方盒模型金样 + 再生成脚本（emit-tokens.mjs）
├── example.js            # 子路径入口：@polypack/adapter-wear/example
└── test/                 # node:test（31 项，零 npm install）
```

## RendererAdapter 契约（ROADMAP §6）

```js
const adapter = createAdapter({
  target: "wearos",                 // T-M7-02 TargetSpec 口径（内建 id 以 wear 开头）
  runtime: { AppRegistry },         // 宿主注入 RN runtime（Wear OS UI 运行时为 RN beta）
  form: "round",                    // round / square（决定 token 集与安全区）
});
adapter.mount({ appKey: "wear" }, App);  // AppRegistry.registerComponent
adapter.platformStyle();          // 紧凑蒙皮（当前形态）
adapter.capabilities();           // { focusNav: false, safeArea: true, gestures: true }
adapter.degradations();           // focus-ring / safe-area-padding / back-affordance
adapter.safeArea();               // 圆表 454：{ top: 67, right: 67, bottom: 67, left: 67 }
```

- **紧凑 tokens**：基值 = polypack init 模板 `tokens.css`（与 native / taro / tv 同一事实源）；
  穿戴派生 = 间距 / 圆角 ×0.75，字号直通（可读性底线 14px），颜色直通。
- **两种形态**：`WEAR_FORMS` = 圆形 454×454 / 方形 320×320（Wear OS 模拟器参考视口）。
- **安全区**：圆形 = 内接正方形（每边内缩 `(1 - 1/√2)/2 ≈ 14.6447%`，入库上取整 14.65%）；
  方形 = 8% 统一内边距。`safeAreaInsets(form, viewport)` 给像素值。
- **蒙皮形态**：RN 合法片段（数值即 dp；`paddingVertical` / `paddingHorizontal` 拆键），
  卡片宽 = 安全区宽；button 常驻透明边框（宽度 = `focusRing.borderWidth`）→ 聚焦零布局抖动。

## 安全区验收（T-M7-01）

`layoutPage(tokens)` 是与 `src/example.js` 页面树同构的确定性盒模型（frame 安全区内边距 >
card > 标题 / 版本 / 2×2 按钮 / 提示）。`test/layout.test.js` 断言圆 / 方两种形态下所有盒
都落在安全区内；`golden/wear-tokens.json` 同时钉住 tokens / 蒙皮 / 两种形态的盒模型
（有意变更后重生成：`node golden/emit-tokens.mjs`，并在 diff 中评审）。

## 焦点环复用（无指针降级）

`src/focus.js` 直接 re-export `ui/adapters/tv/src/focus.js`（其内核 = `ui/adapters/native/src/focus.js`
共享内核 T-M2-05 + 几何推导 T-M4-02）：`createWearFocus` 即 `createTvFocus` 别名。
`capabilities()` 的 `pointer: "none"` → `focusNav: true`，页面渲染焦点环并由方向键 / OK 键
遍历（Back 走焦点记忆，历史空交宿主裁决）；触摸基线不渲染焦点环。本包零导航代码。

## 测试与金样

```bash
cd ui/adapters/wear && node --test   # 31 项，无 npm install 也绿
node golden/emit-tokens.mjs          # token / 安全区 / 盒模型有意变更后重生成
```
