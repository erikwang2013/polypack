# @polypack/adapter-tv v0

ROADMAP §6 RendererAdapter 契约的电视（10-foot）装配层（T-M4-01）：react-dom 挂载 +
10-foot 皮肤 tokens（远距字号 / 间距 / 安全区）+ 焦点导航内核（遍历图 / 焦点环 /
分层 Back / 遥控器数字键，T-M4-02/03）。

```text
ui/adapters/tv/
├── src/
│   ├── index.js          # createAdapter：mount（react-dom）/ platformStyle / capabilities / degradations / safeArea
│   ├── capabilities.js   # TV 能力矩阵（focusNav 基线 true）+ 降级分支（对照 poly-targets 内建声明）
│   ├── tokens.js         # 10-foot tokens：基值同源 tokens.css，TV 派生 ×TV_SCALE + 安全区
│   ├── focus.js          # TV 焦点内核入口：复用 adapter-native 共享内核（T-M2-05），跨包相对导入
│   ├── geometry.js       # 几何推导（rect → 方向邻居 / focusOrder 覆盖）+ 连通性检查器（T-M4-02）
│   ├── example-nodes.js  # 示例页面焦点节点（2×2 网格 rect；无 react 依赖，供快照 / 遍历断言）
│   └── example.js        # 示例页面组件树（原语注入；react 依赖；data-focus 观测锚点）
├── golden/               # tokens 快照金样 + 再生成脚本（emit-tokens.mjs）
├── example.js            # 子路径入口：@polypack/adapter-tv/example
└── test/                 # node:test 测试（37 项；无 node_modules 时 5 项代理渲染跳过、不失败）
```

## RendererAdapter 契约（ROADMAP §6）

```js
const adapter = createAdapter({
  target: "tizen-tv",             // 电视目标面（L3 六枚）：android-tv / fire-tv / tizen-tv / webos-tv / vidaa-tv / smartcast-tv
  runtime: { React, ReactDOM },   // 宿主注入 react + react-dom/client（装配层不臆测宿主）
});
adapter.mount(document.getElementById("root"), App);  // react-dom 挂载 = createRoot(root).render(<App/>)
adapter.platformStyle();          // → 10-foot 蒙皮（远距字号 / 间距 / 安全区 / 焦点环 / 弹层底幕）
adapter.capabilities();           // → { focusNav: true, safeArea: true, gestures: false }（恰好契约三键）
adapter.safeArea();               // → { top: 54, right: 96, bottom: 54, left: 96 }（1080p 安全区像素）
```

- **与 adapter-native 的能力口径差异**（有意为之）：遥控器恒有方向键 → `focusNav` 基线真、
  宿主 `pointer` 提示不翻转它（Magic Remote 的指针不取消焦点环）；`gestures` 假 →
  `back-affordance` 降级由遥控器 Back 键承接（分层 Back 语义）。
- **目标 id** 以 ROADMAP §3.4 的 `tizen-tv` 命名口径为准；poly-targets 登记
  （T-M4-04/08/09/10）落位后由 `capabilities.test.js` 哨兵收口（内建 `*-tv` ⊆ 本矩阵）。
- **降级分支闭合集**：`focus-ring` / `safe-area-padding` / `back-affordance`。

## 焦点导航遍历图（T-M4-02）

```js
const engine = createTvFocus({ nodes, initial });   // rect → 邻居；等价于 createFocusEngine(deriveFocusNodes(...))
engine.move("down");  engine.back();  engine.current;
handleKey(engine, "DPAD_CENTER", { activate });     // {type:"move"|"activate"|"back"|"ignored", ...} 描述符
checkConnectivity(engine.graph);                    // { from, total, reachable, isolated, traps, connected }
```

- **几何推导**（`deriveFocusNodes`）：候选 = 中心严格位于该方向侧 且 正交轴投影相交（> 0）；
  裁定 = 主轴中心距 → 正交中心距 → 文档序。无候选 = 无邻接（内核语义边界自环 → 无死路）。
- **覆盖优先级**：节点 `neighbors` 逐向显式（最高）> `focusOrder` 全量线性覆盖（逐对沿主轴
  带符号连链；已知边界：锯齿序下两邻居同向会互相覆盖，后写胜）> 几何推导。
- **连通性静态检查器**：`isolated` = 自 `from` 不可达；`traps` = 四向全无出边（n > 1 才判）。
  示例应用验收：4 节点、0 孤立、0 陷阱；已知边界：不做强连通判定。
- **口径对齐**：`createTvFocus` 与 adapter-native 共享内核（T-M2-05）同一构造——遍历 /
  焦点记忆 / 键路由行为对拍见 `test/focus.test.js`；下游（RN-tvOS / 鸿蒙 TV / Roku spike /
  冒烟套件）按同一接口消费。

## 10-foot tokens（tokens 由单一源派生）

基值 = poly init 模板 `crates/poly-cli/templates/app/ui/tokens.css`（与 adapter-native /
adapter-taro 同一事实源；`@polypack/ui` 包落地前模板文件是唯一源，ROADMAP §6）。
TV 值 = 基值 × `TV_SCALE`（字号 / 间距 / 圆角各 ×2，远距可读），颜色直通；
安全区 = 视口百分比（四边 5%，1080p 即 96/54px），蒙皮给 CSS 形态、`safeAreaInsets` 给像素。
漂移由 `test/tokens.test.js` 解析 tokens.css 逐项断言；快照金样 `golden/tv-tokens.json`
（再生成：`node golden/emit-tokens.mjs`）。

## 测试（`npm test` / `node --test`）

| 测试 | 断言 |
|---|---|
| `contract.test.js` | 契约对象形状、react-dom 挂载装配（桩 runtime）、边界校验、安全区换算 |
| `capabilities.test.js` | 矩阵形态、宿主提示覆盖、降级分支闭合集、poly-targets 哨兵（`*-tv` ⊆ 矩阵） |
| `tokens.test.js` | 基值 ↔ tokens.css 双向齐全、10-foot 派生、react-dom 合法形态（环零布局抖动）、金样比对 |
| `geometry.test.js` | 2×2 网格四向邻接 / 遍历快照、几何裁定三重平手、focusOrder 覆盖、连通性检查器（0 孤立验收）、推导边界校验 |
| `focus.test.js` | 与共享内核口径对拍（遍历快照恒等）、键路由四类描述符、Back 焦点记忆、入口边界 |
| `render.test.js` | 代理渲染（react-dom/server）：模板页语义标记一致、四向移动焦点环随焦点（冒烟）、data-focus 锚点、安全区外框 |

零运行时依赖：`node --test` 即可跑；`render.test.js` 需 `npm install`（react/react-dom 为
devDeps），未安装时 skip 不 fail（CI 三宿主一致性；只吞「react 未安装」一类失败）。

**跨包导入**：`src/focus.js` 相对导入 `ui/adapters/native/src/focus.js`（共享焦点内核，
T-M2-05）——本仓布局同树、CI node 腿零 npm install 可加载；M4 各 TV 目标（Android TV /
RN-tvOS / 鸿蒙 TV / Roku spike / 冒烟套件）复用同一内核（RN 侧经 adapter-native 再挂遥控器
事件源，web 侧经本包 `>= T-M4-02` 的 `createTvFocus` 几何推导）。

## 阻塞清单（设备链，不伪造证据）

| 项 | 状态 | 替代证据 |
|---|---|---|
| TV 真机 / 模拟器渲染（Tizen / webOS / VIDAA / SmartCast） | **阻塞**：本机无 TV 设备 / 模拟器（Android TV 侧由 android-tv 探 AVD） | `render.test.js` 代理渲染 + 结构断言；设备腿 SKIP-with-print 注记 |
| `@polypack/adapter-dom` 同树渲染对照 | **阻塞**：该包尚未落位（ROADMAP §6 计划包；T-M4-01 验收的 adapter-dom 腿） | 代理证据：同一页面组件树经 react-dom/server 渲染成功 + 语义标记与模板页逐一一致（本仓 native / taro 同款代理模式） |
