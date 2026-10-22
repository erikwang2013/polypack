# Garmin Monkey C 子集转译 spike · 能力边界（T-M7-09 交付 · T-M7-10 决策输入草稿）

> 状态：**草稿**——由 lead 并入 README / ROADMAP / TargetSpec 与图示（本文件不进 docs/；中英要点见 §6）。
> 依据：`ui/adapters/garmin/` spike 产物（T-M7-09）＋ Connect IQ 官方文档（Monkey C Reference / Jungle Reference / API Docs，2026-10-18 抓取）＋ 本仓 `@polypack/adapter-wear` 口径（T-M7-01 已入 main）。
> 复现：`cd ui/adapters/garmin && npm test`（29/29 绿）＋ `npm run build`（结构/子集自检 PASS；编译器腿 SKIP-with-print）。
> 证据级标注：**实证**（本机真跑）/ **官方**（Garmin 官方文档或样例逐字）/ **文档口径**（未逐字复核）/ **待真机**（无 SDK / 设备可闭环）。

## 0. 结论（继续 / 降级建议）

**建议：继续（受限档）**——Garmin 维持 `experimental` 并书面标注功能面收窄；是否接 Rust 打包器列为条件任务（T-M7-10 决策）。

依据（均可复核）：

1. 示例页全元素可由 Monkey C 子集发射器表达，两种形态（圆 / 方）的转译、子集断言、Connect IQ 工程结构自检全部真跑通过（§2 表 A/C/D）；
2. 交互子集经 adapter-wear 共享焦点内核的 `engine.order` 线性化，Back 历史语义同形；数据级等价由测试钉住（§2 表 B）；
3. 全部缺口均有替代方案或已明示降级，无「无解」项挂在关键路径上（§3）。

附带条件（触发前置）：补一次 Connect IQ SDK 编译 + 真机 / 模拟器走查——本机无 `monkeyc`、无 SDK、无开发者密钥，这是唯一未闭环环节（§3 G3）。当前编译器腿按环境槽 **SKIP-with-print**，打印手动命令与前置条件。

降级备选（若 T-M7-10 不愿承担）：只保留 L6 目标声明 + `experimental` + 本能力边界（不做打包器 / 不承诺示例页）——与「继续受限」同样兼容，代价是放弃「受限但可出包」的 Garmin 面。

## 1. 形态：为什么不是「又一个运行时 adapter」

| 维度 | adapter-dom / native / taro / tv / wear | adapter-garmin spike |
| --- | --- | --- |
| 渲染时机 | 运行时（react-dom / RN / Taro / 共享焦点系统） | **构建期**：wear 页模型（tokens / 布局盒 / 焦点序）→ Monkey C 源文件（Garmin 无 JS 运行时可托） |
| 逻辑层 | 页面 JS 原样运行 | **Monkey C 子集重写**（「核心不参与，仅 UI 子集 + 原生逻辑」字面落地） |
| 语义来源 | 共享焦点内核（JS，运行于宿主） | 内核**节点序移植**：`engine.order` 直接注入生成代码，零手写邻接 |
| 平台前置 | 宿主运行时 / 桥 | Connect IQ 编译器（`monkeyc`）+ 开发者密钥；本机均缺席 → SKIP-with-print |

spike 包：`src/page.js`（wear tokens / 安全区 / 能力面 / 焦点序的页模型）· `src/emit.js`（Monkey C 子集发射器，确定性六件套）· `src/subset.js`（语言 + API 白名单词法断言）· `src/check.js`（最小 XML 扫描器 + Connect IQ 工程结构自检 + `.iq` 条目断言）· `scripts/build-and-check.mjs`（构建 + 自检 + 编译器环境槽）。

示例页口径：取 adapter-wear 示例页 4 按钮核心（counter / reset / detail / exit）＋标题 / 版本 / 提示行；布局盒直接来自 wear `layoutPage(tokens)`，焦点序来自 wear `createWearFocus`（其内核 = adapter-native / tv 共享焦点内核）。故「同源可比」落在同一内核与同一布局函数的产物上，而非重写一套导航。

## 2. 能力（支持项）

### 表 A · 组件子集清单（wear 页模型 → Monkey C → 状态）

| 原语 | Monkey C 表达 | 状态 | 依据 |
| --- | --- | --- | --- |
| View（页面 / 卡片 / 安全区） | `Ui.View` 子类；`dc.clear` + `fillRectangle` + `drawRectangle`；安全区 = wear 内缩盒 | 实证 | 发射件 + 官方 `Graphics.Dc` 方法页逐字 |
| Text | `dc.drawText(x, y, font, text, Gfx.TEXT_JUSTIFY_*)` | 实证；字体近似 | 官方 `Graphics.Dc.drawText`；`FONT_SMALL` / `FONT_XTINY` 常量页 |
| Pressable | 复合：`fillRectangle` 底 + 焦点环（`setPenWidth(3)` + `drawRectangle`）+ 文字 | 实证 | 发射件；官方 `Dc.setPenWidth` / `drawRectangle` |
| 焦点环 | `mFocus.equals(id)` 时描 `COLOR_FOCUS` 矩形 | 实证（数据级） | 本包 `PolypackView.mc` `drawButton()`；环色取 wear `platformStyle.focusRing` |
| 颜色 | wear tokens `#rrggbb` → `0xRRGGBB`（确定性 literal） | 实证 | 发射件；颜色 = `surface` / `text` / `muted` / `brand` / `accent` 同源 |
| 圆角（包 radius） | **降级：方角**（Dc 无圆角矩形原语） | 明示降级 | §3 G5 |
| 图像 / 图标 / 启动图 | 未含（manifest 图标字段省略） | 缺口 G7 | 资产管线缺位 |

### 表 B · 交互子集清单（无指针核心任务）

| 交互 | 语义（共享内核口径） | 实现点 | 状态 |
| --- | --- | --- | --- |
| UP / DOWN | **线性循环**（`counter → reset → detail → exit → counter`），序 = 内核 `engine.order` | `BehaviorDelegate.onNextPage / onPreviousPage` → `move(±1)` | 数据级等价**实证**；行为级**待真机** |
| SELECT | 激活当前节点（计数 +1 / 归零 / 详情切换 / 退出请求） | `onSelect` → `activate(mFocus)` | 同上 |
| BACK | 焦点记忆回退（历史栈 pop）；空历史返回 `false` → 交宿主 / 系统退出 | `onBack` → `back()` | 同上 |
| 焦点序 | wear `createWearFocus` 的 `engine.order` 原样注入 `mOrder` / `mIndex` | `src/emit.js`；`test/emit.test.js` 钉住 | 实证 |
| 触摸 / 手势 | 不支持（`pointer: none` → `focusNav: true`） | `resolveCapabilities("wearos", {pointer:"none"})` | 明示降级（G6） |
| 四向方向键 | 未接线（Garmin 三键设备无左右；四向设备待后续） | `traversal` 快照已随页模型产出备用 | 缺口 G6 |

「数据级等价」= 生成代码里的焦点序与 wear 内核 `engine.order` 逐项相等（两种形态测试钉住）；「行为级待真机」= Monkey C 无本机解释器（G3）。

### 表 C · tokens / 布局 / 字体口径

- 配色 / 间距 / 圆角基值 = adapter-wear tokens **同源**（本包直 import `tokensByForm` / `layoutPage` / `platformStyle`）；`#rrggbb` → `0xRRGGBB` 转换实证。
- 画布 = wear 参考视口：圆 454×454（安全区 320×320，14.65% 内缩）/ 方 320×320（安全区 268×268，8% 内缩）。
- 运行时缩放：`scaleX = value * dc.getWidth() / REF_WIDTH`、`scaleY` 同形——布局盒坐标构建期已知，运行时只做线性缩放（无 flex / 无文本测量，G4）。
- 字体：`title → Gfx.FONT_SMALL`、正文 / 按钮 → `Gfx.FONT_XTINY`（内置位图字体，**近似** wear 16/14 px，G5）；`TEXT_JUSTIFY_CENTER | TEXT_JUSTIFY_VCENTER` 官方常量。
- 安全区：与 wear 紧凑口径一致（圆表边缘留白明显大于方表），无独立 Garmin 安全区表。

### 表 D · 打包（Connect IQ 官方工程结构）

| 项 | 形态 | 依据 |
| --- | --- | --- |
| 工程文件 | `manifest.xml` + `monkey.jungle` + `resources/strings/strings.xml` + `source/*.mc` | 官方 Jungle Reference（`project.manifest` / `base.sourcePath` / `base.resourcePath` 逐字） |
| manifest | namespace `http://www.garmin.com/xml/connectiq`、`version="3"`；application `id`（32 hex）/ `type="watch-app"` / `entry` / `name="@Strings.AppName"` / `version` / `minSdkVersion`；`iq:products` 至少 1 个 `iq:product` | 官方 manifest 样例与 Connect IQ 文档口径 |
| 参考设备 | 圆 `venu2` / 方 `venusq`（官方产品 id） | 官方样例 / 文档口径 |
| 应用 id | `sha256(identifier)` 前 32 hex（确定性；不冲突由 identifier 保证） | 本包实现 + manifest 32 hex 形态 |
| 编译 / 出包 | `monkeyc -e -w -r -o out.iq -y <devkey> -f monkey.jungle` → `.iq`（ZIP：`manifest.xml` + 按设备 `.prg`） | 社区 Makefile 逐字（文档口径）；`.iq` 结构由本包 `checkIqEntries` 断言 |
| 签名 | 开发者密钥（`-y`，仅从 `CIQ_DEVKEY` 取路径，零明文入库）；商店发布归 T-M7-10 | 官方文档口径 |
| 校验 | 转译确定性 + Monkey C 子集 / API 白名单 + 工程结构自检（真跑）；`.iq` 条目断言随编译器腿 | 本包 `npm test` / `npm run build` |

## 3. 缺口（影响 + 替代方案 / 明示无替代）

| # | 缺口 | 影响 | 替代方案 | 证据级 |
| --- | --- | --- | --- | --- |
| G1 | **无 JS 运行时**：React 组件函数体不可转换 | 页面逻辑无法复用；运行时 `RendererAdapter` 契约不适用 | 渲染器只承载声明式子集，逻辑按 Monkey C 子集重写（本 spike 即此形态）；适配器接口降阶为「子集发射器 + 逻辑移植手册」 | 官方（Monkey C 为编译型字节码语言）＋本 spike 形态 |
| G2 | **Monkey C 子集限制**：不做异常 / 枚举 / switch / for / while / module / 可见性修饰符 / 动态类型技巧 | 复杂页面逻辑需重构；生成器词汇封闭 | `src/subset.js` 词法白名单**失败即拒**（含 using 模块、Dc / Graphics / WatchUi / Application 成员、构造器、基类）；示例页仅用 if/else + 赋值 | 实证（29 项测试，含负例） |
| G3 | **本机无 Connect IQ SDK / `monkeyc` / 开发者密钥**（`~/.Garmin` 不存在；SDK 下载域 403） | `.iq` 真实编译与设备行为无法本机闭环 | 结构 / 子集 / 确定性真跑；编译器腿 SKIP-with-print（`CIQ_MONKEYC` / `CIQ_HOME` / `CIQ_DEVKEY` 环境槽就绪，打印手动命令）；真机实证列为 T-M7-10 触发前置 | 本机探测实证 |
| G4 | 无 flex / 文本测量：布局构建期固定 | 动态版式与文本自适应不可行 | 布局盒直接复用 wear `layoutPage`（构建期已算好）；运行时仅线性缩放；长文本 → 固定框（溢出未处理，待真机观察） | 实证 |
| G5 | Dc 无圆角 / 字体为内置位图字体（尺寸随设备） | 视觉与 wear 非逐像素一致 | 方角替代圆角；`FONT_SMALL` / `FONT_XTINY` 近似 16/14 px；环 / 卡边框宽度取平台整数 | 官方（Dc 方法页无圆角原语）＋实证 |
| G6 | 指针 / 手势缺失；三键设备无左右；2×2 网格右列不可达 | 交互语义与 wear 四向遍历不同 | `focusNav=true` + 内核 `engine.order` **线性循环**（四个按钮在三键设备全部可达）；`traversal` 快照保留给未来四向设备；不新写导航 | 实证（数据级） |
| G7 | 资产 / 图标 / 启动图缺位 | manifest 省略图标字段（开发侧载可运行形态） | 资产管线后续接入；本 spike 只发射 `AppName` 字符串资源 | 实证 |
| G8 | 每形态只声明参考设备（`venu2` / `venusq`） | 未做多设备产品矩阵；其他尺寸靠线性缩放，未登记 | `iq:products` 可按需扩展；缩放公式按 `dc.getWidth()/getHeight()` 已通用 | 实证 |
| G9 | 生命周期最小：无 `onStart/onStop` 状态持久化 | 退出 / 重启后计数归零 | 示例页明确为瞬时状态；持久化需 `Application.Storage`（不在当前白名单），列为后续扩展 | 实证 |
| G10 | 构建依赖 Node ≥20.11 + `unzip`（列 `.iq` 条目）；Rust 打包器未实现 | 不能由核心 `polypack-bundle` 直接出包 | 本 spike 为 Node 独立模块（与 M4 Roku 同形态）；Rust 打包器列为 T-M7-10 条件任务 | 本仓实证 |

无「无替代」硬项。G1 / G3 为影响最大两项：G1 收窄功能面（设计使然），G3 是唯一未闭环验证（前置条件已写明）。

## 4. TargetSpec 文案草案（experimental / notes，供 T-M7-10 采用）

- `id`: `garmin`；`tier`: 6（与 Roku / Tizen 同档实验目标）。
- `experimental`: **true**（必须——受限档）。
- `runtime`: `none`；`bridge`: `none`；`ui`: `["monkey-c-subset"]`。
- `package`: `{ format: "iq", builder: "garmin-monkeyc" }`（builder 命名待定）。
- `verify`: `["iq-structure-check"]`；`signing`: 开发者密钥 / Connect IQ 商店（发布线）。
- `publish`: `["connect-iq-store"]`（未接）。
- `notes`（中）：`实验：Monkey C 子集受限转译——仅 UI 子集 + 原生逻辑（Monkey C 重写，无 JS 运行时）；工程结构自检可用，Connect IQ 编译与真机实证待补`
- `notes`（en）：`Experimental: Monkey C subset transpile only — UI subset + native logic (Monkey C; no JS runtime); project structure self-check available, Connect IQ compile and on-device validation pending`

## 5. 证据索引

官方（developer.garmin.com，2026-10-18 抓取）：

- Monkey C Reference（`articles/reference-guides/Monkey_C_Reference.html`）：语言关键字 / 运算符 / 类与模块 / 数组与字典 / 常量作用域。
- Jungle Reference（`articles/reference-guides/Jungle_Reference.html`）：`project.manifest` / `base.sourcePath` / `base.resourcePath` 逐字。
- API Docs：`Toybox/Graphics/Dc.html`（`clear` / `setColor` / `fillRectangle` / `drawRectangle` / `drawText` / `setPenWidth` / `getWidth` / `getHeight`；`TEXT_JUSTIFY_*`）、`Graphics.html`（`FONT_XTINY` / `FONT_SMALL` 等常量）、`Toybox/WatchUi/View.html`、`Toybox/WatchUi/BehaviorDelegate.html`（`onSelect` / `onBack` / `onNextPage` / `onPreviousPage` 返回 Boolean）、`Toybox/Application/AppBase.html`（`getInitialView` 返回 `[view, delegate]`）。
- 社区导出命令（cedric-dufour Connect IQ Makefile，文档口径）：`monkeyc -e -w -r -o out.iq -y <devkey> -f monkey.jungle`；`.iq` = `manifest.xml` + 按设备 `.prg`。
- 产品 id `venu2` / `venusq`：官方样例与文档口径（未在本机 SDK 的 `devices.xml` 复核）。

本仓：

- `ui/adapters/wear/src/{tokens.js,layout.js,capabilities.js,focus.js,example.js}`（T-M7-01，已入 main）：tokens / 安全区 / `layoutPage` / 共享焦点内核 / 能力面。
- 本包 `test/` 29/29 绿（转译确定性、子集白名单正负例、XML / 工程 / `.iq` 条目断言、构建脚本集成）；`npm run build`：PASS 4 · FAIL 0 · SKIP 3（`monkeyc` 缺席 / `CIQ_DEVKEY` 未设 / 编译腿 SKIP-with-print）。
- 本机探测：`command -v monkeyc` 空；`~/.Garmin` 不存在；`/opt` 无 Connect IQ；`java` 18.0.2.1、`node` v22.17.0、`unzip` 存在；SDK 下载域返回 403（Cloudflare）。

诚实注记（未复核项）：Monkey C 未在本机编译（无 SDK），语法 / API 正确性为文档实证而非编译器实证；生成代码的 `String.equals` / 字典字符串键按官方 API 文档使用，未真机验证；字体尺寸 / 文本溢出 / 焦点环视觉效果未走查；`monkeyc` 参数组合来自社区 Makefile（文档口径），SDK 版本差异可能导致参数或 manifest 字段需微调。

## 6. English key points（README / ROADMAP 双语同步用）

- Garmin stays **experimental** (restricted tier): no JS runtime — a **build-time Monkey C subset emitter** replaces the runtime renderer; component logic must be **rewritten in Monkey C**.
- Supported UI subset: View / card / text / pressable composite with focus ring; colors from adapter-wear tokens; **no corner radius**; built-in bitmap fonts approximate wear px sizes.
- Interaction subset: three-key devices (UP / DOWN / SELECT / BACK) with a **linear focus cycle derived from the shared focus kernel's `engine.order`** (all four buttons reachable; no hand-written adjacency); Back history mirrors the kernel and falls through to host exit when empty.
- Packaging: Connect IQ project (`manifest.xml`, `monkey.jungle`, strings, `source/*.mc`); `monkeyc -e -w -r -o out.iq -y <devkey> -f monkey.jungle` produces a `.iq` (ZIP with `manifest.xml` + per-device `.prg`); dev-key signing only, no plaintext credentials.
- Verification: real-run transpile determinism, Monkey C subset / API allowlist assertions, project-structure self-check; compiler leg **SKIP-with-print** (no SDK / dev key locally) — Connect IQ compile and on-device validation are required before T-M7-10 triggers.
