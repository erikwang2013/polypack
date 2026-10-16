# Roku SceneGraph 子集渲染器 · 能力边界（T-M4-11 spike 交付 · T-M4-12 决策输入草稿）

> 状态：**草稿**——由 lead 并入 README / ROADMAP / TargetSpec 与图示（本文件不进 docs/；中英要点见 §6）。
> 依据：`ui/adapters/roku/` spike 产物（T-M4-11 提交组）＋ Roku 官方样例实证（GitHub `rokudev`，developer.roku.com 本网络不可达）＋ 本仓 `@polypack/adapter-tv` 口径。
> 复现：`cd ui/adapters/roku && node --test`（9/9 绿）＋ `node scripts/build-and-check.mjs`（结构自检 19 项 PASS；真机腿默认 SKIP-with-print）。
> 证据级标注：**实证**（本机真跑）/ **官方**（rokudev 样例逐字）/ **文档口径**（未逐字复核）/ **待真机**（无设备可闭环）。

## 0. 结论（继续 / 降级建议）

**建议：继续（受限档）**——按 ROADMAP §10 R2 对策，Roku 维持 `experimental` 并书面标注功能面收窄；打包器按条件任务 T-M4-15 排期。

依据（均可复核）：

1. 示例页面全元素可由子集发射器表达，构建链结构自检全绿（§2 表 A/C/D）；
2. 交互子集（方向键 / OK / Back 焦点记忆）经 adapter-tv 几何推导口径（T-M4-02）＋共享焦点内核的遍历表移植，产物级逐键等价（§2 表 B）；
3. 全部缺口均有替代方案或已明示降级，无「无解」项挂在关键路径上（§3）。

附带条件（T-M4-15 触发前置）：补一次真机/模拟器侧载实证——本机无 Roku SDK / 设备 / 模拟器，这是唯一未闭环环节（§3 G6）。

降级备选（若 T-M4-12 不愿承担）：只保留 L3 目标声明 + `experimental` + 本能力边界（不做打包器）——与 R2 对策同样兼容，代价是放弃「受限但可交付」的 Roku 面。

## 1. 形态：为什么不是「又一个 adapter」

| 维度 | adapter-dom / native / taro / tv | adapter-roku spike |
| --- | --- | --- |
| 渲染时机 | 运行时（react-dom / RN / Taro / react-dom + 焦点系统） | **构建期**：声明式子集 → SceneGraph XML（Roku 无 JS 运行时可托） |
| 逻辑层 | 页面 JS 原样运行 | **原生 BrightScript 重写**（「核心不参与，仅 UI 子集 + 原生逻辑」字面落地） |
| 语义来源 | 共享焦点内核（JS，运行于宿主） | 内核**数据**移植：遍历表由内核快照生成（`source/focusgraph.brs`） |

spike 包：`src/subset.js`（受限词汇 + 示例页面子集；布局手算、焦点图节点 = 布局盒绝对 rect，零手写邻接）· `src/emit.js`（发射器 + XML 良构扫描器）· `src/roku-keys.js`（键映射）· `channel/`（骨架三件：manifest / main.brs / PolypackScene.brs）· `scripts/build-and-check.mjs`（构建 + 结构自检 + 侧载腿）。

示例页口径：取 adapter-tv 示例页 2 按钮核心（counter / reset）；tv 侧在 T-M4-02/03 已演进为 4 节点网格 + 分层 Back / 数字键（均已入 main），spike 按 2 按钮核心收敛——焦点图构建走 tv 同一 `createTvFocus`（`deriveFocusNodes` 几何推导），故「同源可比」落在同一算法与同一内核快照上，而非同一节点集。

## 2. 能力（支持项）

### 表 A · 组件子集清单（polypack 原语 → SceneGraph 节点 → 状态）

| 原语 | SceneGraph 表达 | 状态 | 依据 |
| --- | --- | --- | --- |
| View（容器 / 安全区框 / 卡片） | Group + translation；安全区 = 坐标内缩 5% → 96/54 px @1920×1080 | 实证 | 发射件 + `safeAreaInsets` 同源（adapter-tv）；官方 Action Safe 90% 数字相符（见表 C） |
| Text | Label（font role / color / horizAlign / vertAlign） | 实证 | 官方 hello-world `helloworld.xml`（Label + horizAlign/vertAlign 逐字） |
| Image | Poster（`uri=pkg:/…`） | 映射就绪，demo 未含（图标资产缺位，G9） | 官方 hero-grid `HeroScreen.xml`/`customItem.xml`（Poster + pkg:/ uri） |
| Pressable | button 复合糖：Group(focusable) + 焦点环 Rectangle + 底色 Rectangle + Label | 实证 | 发射件；官方 hero-grid `customItem`（可聚焦 Group 形态） |
| 描边（卡片 1px 边） | 双层 Rectangle（SceneGraph Rectangle 无 border 字段） | 实证 | 发射件 `card_border`/`card_fill` |
| 焦点环 | Rectangle + BRS 切 `visible` | 实证 | 本包 `PolypackScene.brs` `applyFocus()` |
| 圆角（包 radius=24） | **降级：方角**（圆角需 9 切片贴图资产） | 明示降级 | §3 G4 |

### 表 B · 交互子集清单（无指针核心任务）

| 交互 | 语义（共享内核口径） | 实现点 | 状态 |
| --- | --- | --- | --- |
| 方向键 | 遍历图 = 布局盒几何推导（T-M4-02 `deriveFocusNodes` 口径，邻居零手写）；边界自环（无死路） | Scene 层 `onKeyEvent` 消费 + 遍历表查找 | 数据级等价**实证**；行为级**待真机** |
| OK | 激活当前节点（计数递增 / 归零，与 adapter-tv 示例页同语义） | `activate()` | 同上 |
| Back | 焦点记忆回退（空历史 = 顶层，退出语义留 G7） | 历史栈 push/pop | 同上 |
| 键映射 | Roku 字面（up/down/left/right/OK/back）→ 内核键名（DPAD_* / DPAD_CENTER / Back） | `src/roku-keys.js`；`keys.test.js` 行为断言（move/activate/back 路由） | 实证 |
| 数字键 | 未实现（口径已就绪：T-M4-03 `shortcuts.js` 四族归一，已入 main；spike demo 未接线） | — | 缺口 G8 |

「数据级等价」= 打进 zip 的焦点表与内核 `traversalSnapshot` 逐键相等（`test/emit.test.js` 钉住；焦点图经 tv 同一 `createTvFocus` 构建，故两侧对拍的是同一推导实现的产物）；「行为级待真机」= BrightScript 无本机执行器（G6）。

### 表 C · tokens / 布局口径

- 配色 / 间距 / 圆角基值 = adapter-tv tokens **同源**（本包直 import）；`#rrggbb` → `0xRRGGBBAA` 转换实证。
- TV_SCALE ×2 字号 → Roku 内置字体 role 近似（title→`font:Large` 等）；精确 px 通道见 G3。
- 画布 1920×1080（fhd）自有；布局手算（无 flex，G2），卡高自堆叠推导（发射件自洽）。
- 安全区：本仓 5% 四边内缩（FHD 96/54）与官方 Action Safe 90%（FHD 1726×970 @ 96,53）方向一致，1px 差为取整方式差异（本仓偏保守）；官方 Title Safe 80% 更保守，未采用。

### 表 D · 打包 / 侧载（官方 makefile 逐字实证）

| 项 | 形态 | 依据 |
| --- | --- | --- |
| zip 布局 | 根 = `manifest` + `source/` + `components/`（`images/` 等可选） | `rokudev/hello-world` `makefile/app.mk`（zip 两段压缩：png 免压 / 其余 -9）+ `rokudev/samples` 目录形态 |
| manifest 必填 | title / major_version / minor_version / build_version（数字） | 官方样例 manifest 字段集；本包自检逐项断言 |
| 侧载协议 | 设备开发者模式 + `POST /plugin_install`（digest 认证 `rokudev:<密码>`；`mysubmit=Install` + `archive=@zip`；结果页 Success/Failed） | `app.mk` install target 逐字 |
| 设备发现 | ECP `http://<ip>:8060/`（friendlyName） | `app.mk` CHECK_ROKU_DEV_TARGET |
| 出包 `.pkg`（发布线） | 需 dev key + 密码（`plugin_package` 或桌面 app-package 工具） | `app.mk` pkg/app-pkg target；发布签名归 T-M4-15 |

## 3. 缺口（影响 + 替代方案 / 明示无替代）

| # | 缺口 | 影响 | 替代方案 | 证据级 |
| --- | --- | --- | --- | --- |
| G1 | **无 JS 运行时**：React 组件函数体（任意逻辑）不可转换 | 页面逻辑无法复用；`RendererAdapter` 契约无法按 runtime 形态实现 | 渲染器只承载**声明式子集**，逻辑按原生 BRS 重写（本 spike 即此形态）；适配器接口降阶为「子集发射器 + 逻辑移植手册」——R2「功能面收窄」的实体 | 官方（SceneGraph/BrightScript 无 JS 引擎）＋本 spike 形态 |
| G2 | 无 CSS/flex：布局全手算坐标 | 版式需设计侧约束；动态测量（文本自适应）不可行 | 发射器吸收布局计算（本 spike 手算堆叠/居中已证）；动态文本 → 固定框 + 截断策略（待真机验证） | 实证 |
| G3 | 精确字号需 Font 节点 / BRS 赋值；内置 role 尺寸随分辨率 | 10-foot tokens 对拍存在近似误差 | BRS 赋值通道（官方 `helloworld.xml` 实证 `font.size=92`）；后续随资产管线带字体文件 | 官方 |
| G4 | Rectangle 无圆角 / 描边字段 | 包 radius=24 的视觉降级 | 描边 → 双层矩形（已落地）；圆角 → 9 切片贴图（T-M4-15 资产管线）或方角（本 spike 取方角） | 实证 |
| G5 | SceneGraph 内建焦点机制 ≠ 内核口径（无边界自环落点解析、无焦点记忆栈；内建 roving 行为不可编程） | 直接放养会破坏「无死路 / 焦点记忆」语义 | Scene 层消费方向键 + 遍历表驱动（本 spike 机制；`onKeyEvent` 返回 true 即接管）；遍历图与 adapter-tv 同为几何推导口径（T-M4-02 `deriveFocusNodes`，已入 main），数据级等价已证，行为级随 G6 | 实证 |
| G6 | **本机无 Roku SDK / 设备 / 模拟器**；开发文档域网络受限（GitHub 绕行取官方样例） | 侧载与渲染无法本机复现 | 结构自检 19 项 + SKIP-with-print 手册腿（已交付；`POLY_ROKU_REAL=1` 真跑形态就绪）；真机实证列 T-M4-15 触发前置 | 本机探测实证 |
| G7 | 单层 demo：Back 空历史无动作；退出频道 / 分层 Back 未做 | 完整返回链未覆盖 | 分层 Back 口径已就绪（T-M4-03 `layers.js` / `handleTvKey` 三层裁决，已入 main）——spike demo 未接线，列为 T-M4-15 承载面扩展；退出 = main.brs 消息循环收尾（形态已在） | 口径已就绪（未接线） |
| G8 | 浏览类组件（RowList / MarkupGrid / Video）未入 demo；数字键未实现 | 「浏览→详情→播放」核心任务未覆盖（属 M4-13 冒烟范围） | 官方有对应组件（hero-grid 实证 RowList 用法）；列为 T-M4-15 承载面扩展；数字键口径已就绪（T-M4-03 `shortcuts.js`，已入 main，spike 未接线） | 官方（hero-grid） |
| G9 | 资产管线缺位（图标 / 启动图 / 圆角贴图） | manifest 图标 / splash 字段省略（dev 侧载可运行形态） | T-M4-15 接 poly 资产管线；manifest 字段随资产补齐 | 实证 |
| G10 | 构建依赖系统 zip / unzip | Windows 原生缺 zip | 现行走 WSL/Git-Bash；未来打包器复用仓内既有 Rust zip 基建（zip 9.0.3：`poly-bundle::zipwrite` / `poly-publish::package`） | 本仓 crate 实证 |

无「无替代」硬项。G1 / G6 为影响最大两项：G1 收窄功能面（设计使然），G6 是唯一未闭环验证（前置条件已写明）。

## 4. TargetSpec 文案草案（experimental / notes，供 T-M4-12 采用）

- `experimental`: **true**（必须——R2 对策）。
- `notes`（中）：`实验：SceneGraph 子集受限渲染——仅 UI 子集 + 原生逻辑（BrightScript 重写，无 JS 运行时）；侧载结构自检可用，真机实证待补`
- `notes`（en）：`Experimental: SceneGraph subset rendering only — UI subset + native logic (BrightScript; no JS runtime); structural sideload self-check available, on-device validation pending`
- 其余字段草案：`runtime=none`、`bridge=none`、`ui=["scene-graph-subset"]`、`package.format=zip`、`package.builder=roku-channel`（命名待定）、`verify=["侧载结构自检"]`、`signing=dev/平台证书`（出包线）、`publish=["Roku 渠道商店（未接）"]`。

## 5. 证据索引

官方（GitHub `rokudev`，2026-10-17 抓取）：

- `hello-world`：`makefile/app.mk`（zip 两段压缩 / `plugin_install` digest / ECP / `plugin_package` 逐字）、`source/source/Main.brs`、`components/helloworld.xml`（Label + inline BRS `font.size`）。
- `samples`：`advertising/CSASAdSample/manifest`（字段集）、`advertising/RAF4RSG-sample-master/components/Player.brs`（`onKeyEvent(key as String, press as Boolean)` 形态 + `back` 字面）。
- `hero-grid-channel`：`components/HeroScreen/HeroScreen.xml`（`initialFocus` / `translation="[x,y]"` / `<script uri>`）、`components/Item/customItem.xml`（可聚焦 Group / Poster）。
- `tv-safe-zone-channel`：README（Title Safe 80% / Action Safe 90% 数字）。

本仓：`ui/adapters/tv/src/{focus.js, geometry.js, capabilities.js, tokens.js, example-nodes.js}`（口径同源；T-M4-01/02/03 均已入 main）；本包 `node --test` 9/9、`scripts/build-and-check.mjs` 结构自检 19 项（复现命令见文件头）。

诚实注记（未复核项）：OK 键字面「OK」未在已抓样例逐字命中（BRS 防御性兼容 `select`）；`source/` 下 .brs 全局函数为文档口径（`app.mk` imports→`source/common` 机制旁证）；BrightScript 未在本机执行（无解释器，行为级一律「待真机」）。

## 6. English key points（README / ROADMAP 双语同步用）

- Roku stays **experimental** (risk R2): no JS runtime — a **build-time subset emitter** (declarative subset → SceneGraph XML) replaces the runtime renderer; component logic must be **rewritten in BrightScript**.
- Supported UI subset: Group / Rectangle / Label / Poster + focusable button composite; focus ring via Rectangle visibility; 1px border via double Rectangle; **no corner radius** (9-slice asset pending).
- Interaction subset ported from adapter-tv's geometry-derived traversal graph + the shared focus kernel (boundary self-loop = no dead ends, Back focus memory, OK activate) through a **generated traversal table** (data-level equivalence tested).
- Packaging / sideload: channel zip (`manifest` + `source/` + `components/`), dev-mode `plugin_install` sideload, ECP discovery — protocol per Roku's official makefile.
- Verification: structural self-check + SKIP-with-print manual sideload; **on-device validation pending** (no SDK/device locally) — required before T-M4-15 triggers.
