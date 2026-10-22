# @polypack/adapter-taro v0

ROADMAP §6 RendererAdapter 契约的小程序族装配层（T-M3-05）：Taro 渲染器（React 语法）承载
微信 / 支付宝 / 抖音 / 快应用四目标。

```text
ui/adapters/taro/
├── src/
│   ├── index.js         # createAdapter：mount / platformStyle / capabilities / degradations / router
│   ├── capabilities.js  # 目标 → Taro 编译平台映射 + 能力矩阵 + 降级分支（对照 polypack-targets 内建声明）
│   ├── tokens.js        # 最小 token 集 + platformStyle 蒙皮（同源 polypack init 模板 tokens.css）
│   ├── routes.js        # 生命周期映射表（onLoad/onShow/… → React 语义）+ 声明式路由（页面清单校验）
│   ├── apiguard.js      # 宿主 API 白名单：构建期扫描（DOM 全局 / 白名单外 Taro API 即拒）
│   ├── example.js       # 示例页面组件树（原语注入；react 依赖）
│   └── …                # 装配层零 react 依赖：无 node_modules 也能导入 src/index.js
├── bin/check-api.mjs    # 白名单预检 CLI（polypack-bundle 小程序打包器的编译前命令）
└── test/                # node:test 测试（36 项；无 react 时 4 项代理渲染跳过、不失败）
```

## RendererAdapter 契约（ROADMAP §6）

```js
const adapter = createAdapter({ target: "wechat", runtime: taroRuntime, host: { pointer: "touch" } });
adapter.mount({ pagePath: "pages/index/index" }, Page);  // 页面绑定到 app.config 声明的路由
adapter.platformStyle(tokens, { compact });              // → 蒙皮为 Taro 样式片段（数值即设计稿 px）
adapter.capabilities();                                  // → { focusNav, safeArea, gestures }（恰好契约三键）
adapter.degradations({ gestures: false }, { compact: true }); // → 降级分支清单（闭合可枚举）
adapter.router().navigateTo("pages/about/index?a=1");    // 声明式路由：未声明页在调用点拒绝
```

- **mount 契约的小程序语义**：Taro 无 DOM 挂载点也无 AppRegistry——装配 = 页面组件绑定到
  声明的路由（`root.pagePath` 须在 `runtime.config().pages` 内），页面模块把返回值作 default
  export，Taro 编译器按 pages 清单接线。
- **runtime 显式注入**：装配层不臆测宿主（devtools / CI 无需真装 `@tarojs/taro`），
  生产注入 `@tarojs/taro` + `config()`（读 `app.config.js`），测试注入桩。
- **能力矩阵**：四目标基线均为触屏 / 有安全区 / 有系统返回手势，与
  `crates/polypack-targets/src/builtin.rs`（L2 小程序族，`ui = "taro"`）逐一对照。
- **降级分支闭合集**：`no-direct-dom`（沙箱恒真：视图经 Taro 组件树，DOM 直连构建期拒绝）、
  `lifecycle-mapping`（沙箱恒真：生命周期经 routes.js 映射表接线）、`compact-spacing`（小屏）、
  `back-affordance`（无系统返回手势的容器形态）。

## 宿主 API 白名单（构建期拒绝，T-M3-05 验收项）

`src/apiguard.js` 两个方向，`bin/check-api.mjs` 汇总为构建期预检（越界即非零退出）：

| 方向 | 规则 | 例 |
|---|---|---|
| `forbidden-global` | DOM / 浏览器全局 | `document` / `window` / `localStorage` / `fetch` … |
| `taro-api` | 白名单外的 `Taro.<name>`（含裸下标形态） | `Taro.request` / `Taro['createSelectorQuery']` |

构造样本 / 文档示例走同行标记 `polypack:allow-api` 放行（同 polypack-core scan 的行内标记口径）。
v0 口径：行级词边界匹配（无 parser 依赖）；`import { x } from "@tarojs/taro"` 的裸解构不在扫描面。

## 测试（`npm test` / `node --test`）

| 测试 | 断言 |
|---|---|
| `contract.test.js` | 契约对象形状、四目标平台映射、mount 路由绑定与边界校验、router 落地 |
| `capabilities.test.js` | 矩阵 ↔ builtin.rs 漂移哨兵（含 Taro 承载目标）、降级分支闭合集、确定性 |
| `tokens.test.js` | token 值与模板 tokens.css 双向齐全、Taro 合法形态、compact 降级 |
| `routes.test.js` | 生命周期映射表闭合、键集 / 类型校验、路径归一化、方法面 |
| `apiguard.test.js` | 白名单自洽、正负例、放行标记、CLI 非零退出（构建期拒绝的证据链） |
| `render.test.js` | 代理渲染（react-dom/server）：语义标记与模板页一致、降级驱动、四目标同构 |

零运行时依赖：`node --test` 即可跑；`render.test.js` 需 `npm install`（react / react-dom 为
devDeps），未安装时 skip 不 fail（只吞「react 未安装」一类失败，本包加载错误照常炸出）。

## 阻塞清单（宿主 / 工具链，不伪造证据）

| 项 | 状态 | 替代证据 |
|---|---|---|
| 微信 / 支付宝 / 抖音开发者工具预览 | **阻塞**：本机无 devtools（Linux 宿主无官方工具链） | `render.test.js` 代理渲染 + 编译产物结构断言（T-M3-06 打包器）+ `host-structure.test.js` |
| devtools CLI 校验（T-M3-11） | 同上 | 打包器产物齐备 + 金样比对；CLI 腿按 SKIP-with-print 注记 |
| 快应用真机 / 联盟商店提审 | **阻塞**：无商店凭据（R4：自动化止步提审） | hap-toolkit 构建 + lint（T-M3-12）+ 提审脚本 dry-run |

## 落位说明

- 包落 `ui/adapters/taro/`，与 `ui/adapters/native/` 同构（ROADMAP §6 的适配器矩阵）。
- Taro 宿主工程（编译 + devtools 打开预览的对象）落包内 `host/`，见其 README（T-M3-06）。
