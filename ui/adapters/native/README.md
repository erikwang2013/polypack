# @polypack/adapter-native v0

ROADMAP §6 RendererAdapter 契约的 React Native 宿主装配层（T-M2-04）＋ 共享焦点内核（T-M2-05）。

```text
ui/adapters/native/
├── src/
│   ├── index.js           # createAdapter：mount / platformStyle / capabilities / degradations / native
│   ├── capabilities.js    # 能力矩阵 + 降级分支（对照 poly-targets 内建声明）
│   ├── tokens.js          # 最小 token 集 + platformStyle 蒙皮（同源 poly init 模板 tokens.css）
│   ├── focus.js           # 共享焦点内核：遍历图 / Back 焦点记忆 / 键路由（M4 TV 复用）
│   ├── example-nodes.js   # 示例页面最小遍历图（无 react 依赖，供快照断言）
│   ├── example.js         # 示例页面组件树（原语注入；react 依赖）
│   └── native-registry.js # 原生模块注册位（T-M2-02 AAR/JNI 挂载点）
├── host/                  # Android RN 宿主骨架（RN 模板 0.87.2 形态；见 host/README.md）
├── device-walks/          # 设备走查对拍数据（adb 键序列 → 期望焦点序列；见其 README）
└── test/                  # node:test 测试（42 项；无 node_modules 时 6 项跳过、不失败）
```

## RendererAdapter 契约（ROADMAP §6）

```js
const adapter = createAdapter({ target: "android", runtime: RN, host: { pointer: "touch" } });
adapter.mount({ appKey: "PaxApp" }, App);        // RN 宿主装配 = AppRegistry 注册（RN 无 DOM 挂载点）
adapter.platformStyle(tokens, { compact });      // → 蒙皮为 RN 样式片段（数值即 dp）
adapter.capabilities();                          // → { focusNav, safeArea, gestures }（恰好契约三键）
adapter.degradations({ pointer: "none" }, { compact: true });  // → 降级分支清单（闭合可枚举）
adapter.native.register({ artifact, entryClass, libraryName }); // 桥产物挂载点（T-M2-02；libraryName 可省）
```

- **runtime 显式注入**：装配层不臆测宿主（Metro/CI 无需真装 react-native），测试注入桩即可覆盖契约。
- **capabilities 基线**来自 `capabilities.js` 矩阵，与 `crates/poly-targets/src/builtin.rs` 对照；
  宿主运行时提示（`pointer` / `safeArea` / `gestures`）覆盖基线——`pointer: "none"`（TV / 车机 /
  无指针调试）→ `focusNav: true`，即「无指针 → 焦点环」降级驱动。
- **降级分支闭合集**：`focus-ring`（无指针）、`compact-spacing`（小屏）、`back-affordance`
  （无系统返回手势）。完整导航图 / 手势栈留待 M4。

## 共享焦点内核（T-M2-05，M4 TV 复用）

`src/focus.js` 宿主无关、零依赖：显式邻接遍历图（确定性由构造保证，无几何猜测）、
方向键遍历（边界原地，**无死路**）、Back 焦点记忆栈、键盘/d-pad 键路由。
M4 复用方式：新增遥控器事件源 + 几何自动布局（显式邻接 → 自动推导），不改内核接口。

```js
const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
handleKey(engine, "DPAD_DOWN");  // → { type: "move", direction: "down", from, to }
handleKey(engine, "Back");       // → { type: "back", from, to }（栈空 to = null，退出交给宿主）
traversalSnapshot(engine.graph); // 快照断言用；reachableFrom / hasNoDeadEnds 为无死路证明手段
```

## 测试（`npm test` / `node --test`）

| 测试 | 断言 |
|---|---|
| `contract.test.js` | 契约对象形状、mount 装配、边界校验、原生注册位 + T-M2-02 口径哨兵（对照真跑夹具） |
| `capabilities.test.js` | 矩阵 ↔ builtin.rs 漂移哨兵（含 react-native 承载目标）、降级分支闭合集、确定性 |
| `tokens.test.js` | token 值与模板 tokens.css 双向齐全、RN 合法形态、compact 降级 |
| `focus.test.js` | 遍历图快照、焦点顺序稳定、Back 记忆、无死路 + 全图可达、构造边界 |
| `host-structure.test.js` | 宿主骨架齐备、AppRegistry 键三处一致、挂载点就位、键事件源接线（DOWN-only）、二进制/凭据不入库 |
| `render.test.js` | 代理渲染（react-dom/server）：语义标记与模板页一致、键节点锚点、焦点环由 capabilities 驱动 |
| `device-walks.test.js` | 走查对拍数据 ≡ 内核推导（漂移哨兵）、键表覆盖内核路由、clicks 期望由示例页履约 |

零运行时依赖：`node --test` 即可跑；`render.test.js` / 走查履约断言需 `npm install`
（react/react-dom 为 devDeps），未安装时 skip 不 fail（CI 三宿主一致性；只吞「react 未安装」
一类失败，本包加载错误照常炸出）。

**CI**：`.github/workflows/rust-ci.yml` 三宿主跑 `working-directory: ui/adapters/native` + `node --test`。
CI 不装依赖（不引 npm registry 网络依赖）：42 项中 36 项常驻守 `src/` 回归（无外部依赖；
含 T-M2-02 口径哨兵——真跑夹具 `crates/poly-core/tests/fixtures/bridge_core` 已随 `9e5aec2` 入库），
5 项代理渲染 + 1 项走查履约缺 react 时 skip 不 fail；本机 `npm install` 后 42 项全真跑。

## 阻塞清单（宿主/设备，不伪造证据）

| 项 | 状态 | 替代证据 |
|---|---|---|
| Android RN 宿主真机渲染 | **阻塞**：无 Android SDK / 模拟器 / Gradle | `render.test.js` 代理渲染 + `host-structure.test.js` 结构断言 |
| 方向键完成核心任务（真机冒烟） | **阻塞**：需模拟器/实机（宿主键事件源已接线：`onHWKeyEvent` + `BackHandler`，DOWN-only） | `focus.test.js` 键路由断言 + `device-walks/` 对拍数据（键序列 → 期望焦点序列，见其 README） |
| AAR/JNI 真加载 | **阻塞**：同上；口径已定（T-M2-02：`bridge/android/<核心库名>.aar` + `uniffi.<ns>.<Ns>Kt` + `libraryName`） | 注册位契约测试 + 口径哨兵 + gradle `fileTree` 挂载点结构断言 |

## 落位说明

- 包落 `ui/adapters/native/`：README「仓库结构」已规划 `ui/` = `polypack-ui/` + `adapters/`
  （M1 规划的目标形态），`app/` 保留给 poly-gui 控制台（Tauri + React）。
- 宿主骨架落包内 `host/`（同根下的 host 模板），不占 `app/`；如需迁移（如 M4 特权 helper
  规划的 `ui/android/**/jniLibs`），骨架整体平移即可，路径引用集中在测试与 gradle 挂载点。
