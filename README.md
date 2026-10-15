# PolyPack

**多端打包，一次送达** — Rust 核心 × React UI 适配层 × 50+ 目标平台

[English](README.en.md) · [详细规划](docs/ROADMAP.md) · [架构](#项目架构) · [平台矩阵](#支持平台矩阵)

> **项目状态：M2 移动与桥生态已交付**（2026-10-15）。可用命令：`poly doctor` / `poly init` / `poly build`（真出 deb / AppDir / Web 静态包；**Android 直出 AAB/APK**；**iOS 未签名 IPA**（macOS 宿主））/ `poly bridge`（uniffi 桥生成）/ `poly farm worker|submit`（远程农场）/ `poly scan`（密钥扫描）/ `poly verify --reproducible` / `poly retry --from sign` / `poly dev` / `poly clean`。五工作流三宿主全绿（含真模式 nightly 与真产物留档）。后续里程碑按 [docs/ROADMAP.md](docs/ROADMAP.md) 推进。

---

## 认识 Pax

![Pax 的动态表情循环：笑 → 眨眼 → 疑惑 → 哭 → 惊恐](docs/assets/pax.svg)

**Pax（帕克斯）** 是 PolyPack 的项目宠物——一只邮差蟹。Rust 橙的蟹壳致敬 Ferris，头顶的邮政帽是它的工种：把你的应用打包成一件快递件，贴上 React 轨道邮票，用钳子一件件送达手机、手表、电视们手里。它抱着的那件包裹，就是同一份代码库编译出的几十种产物。

Pax 是动态多形态的：空闲时轻轻浮动、定时眨眼，并循环展示 **笑 / 眨眼 / 疑惑 / 哭 / 惊恐** 五种表情；每一个单独的表情也都有自己的动态——哭是泪滴下落、惊恐是发抖加汗滴、疑惑是问号浮动、眨眼是定期眨眼、笑是轻盈浮动。全部为纯 CSS 动画、无脚本；系统开启「减弱动态效果」时各自定格为对应的静态形态。五种表情一览（格子里每一只都在动）：

![Pax 的五种表情（各自带动态）：笑 / 哭 / 惊恐 / 疑惑 / 眨眼](docs/assets/pax-moods.svg)

独立表情文件（每个都带自己的动态）：[笑](docs/assets/pax-happy.svg) · [哭](docs/assets/pax-cry.svg) · [惊恐](docs/assets/pax-shock.svg) · [疑惑](docs/assets/pax-confused.svg) · [眨眼](docs/assets/pax-blink.svg)；本地打开 [docs/pax.html](docs/pax.html) 可手动切换表情。

**品牌约定**：Pax 就是 PolyPack 的项目图标——文档 favicon（[pax-mark.svg](docs/assets/pax-mark.svg)）、生成图示右下角的微缩角标、未来的 CLI / GUI / 安装包图标都用它；五种表情同时映射构建状态（成功 = 笑 · 失败 = 哭 · 异常 = 惊恐 · 待确认 = 疑惑 · 空闲 = 眨眼）。全部形态由 `tools/build-pax.mjs` 单一源生成。

Pax 与全部文档图示共用同一套色彩变量，你会在架构图里再次见到它身上的锈橙与轨道蓝。

## 项目简介

PolyPack 是一个**「一次编写、全端打包」的通用打包工具**：以 Rust 为单一核心，通过声明式目标注册表（TargetSpec）把同一份 **Rust 业务核心 + React 界面** 编译、签名、封装并发布到 **50+ 个目标平台**——从 Android / iOS / HarmonyOS / Windows / macOS / Linux / Web，到小程序、快应用、电视、车机、穿戴、嵌入式 RTOS，乃至主机平台。

### 为什么需要它

- **目标多而散**：多端构建目前各自为战——Android 用 Gradle、iOS 用 Xcode、小程序用各家 IDE、桌面各写一套打包脚本。PolyPack 把它们收敛成一条 `poly` 命令。
- **栈要统一**：业务逻辑用 Rust 写一次，跨端复用；界面用 React 写一次，经平台适配层换成各端组件体系。
- **流程要成链**：探测工具链 → 交叉编译 → UI 打包 → 签名/公证/加固 → 封装 → 商店提审，全链路可编排、可缓存、可复现。
- **生态要可扩展**：新平台 = 一个 TargetSpec + 一个 WASM 插件，不必等官方发版。

### 工作原理（90 秒版）

```bash
poly release --target android,ios,wechat,tizen --publish
```

1. 装载 `polypack.toml`，为目标列表解析出 **TargetSpec**（运行时、打包格式、签名要求、分发渠道）；
2. **工具链探测**：本机缺什么报什么（SDK/NDK/Xcode/emsdk…），构建缓存按内容寻址增量复用；
3. **Rust 交叉编译** + **React 适配层打包**并行展开，产物汇入打包器矩阵；
4. **签名 → 公证 → 加固 → 校验**，最后按目标封装为 APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / 小程序包 …… 并生成构建报告。

完整的 13 步旅程见下方[请求周期](#请求周期)。

---

## 支持平台矩阵

| 等级 | 平台 | UI 运行时 | 打包 / 分发 |
|---|---|---|---|
| L1 | Android | React Native · Material 3 | AAB / APK → Google Play |
| L1 | iOS | React Native · HIG | IPA → App Store |
| L1 | HarmonyOS | RN for OpenHarmony · HarmonyOS Design | HAP → AppGallery |
| L1 | Windows | react-dom（Tauri） | MSI / NSIS → MS Store |
| L1 | macOS | react-dom（Tauri） | DMG / PKG + 公证 |
| L1 | Linux | react-dom（Tauri） | AppImage · deb · rpm · Flatpak · Snap |
| L1 | Web | react-dom | 静态包 + PWA |
| L2 | 微信小程序 | Taro（React 语法） | 小程序代码包 → 微信审核 |
| L2 | 支付宝小程序 | Taro | 小程序包 → 支付宝审核 |
| L2 | 抖音小程序 | Taro | 小程序包 → 抖音审核 |
| L2 | 快应用 | Taro / 快应用框架 | .rpk → 联盟商店 |
| L2 | TikTok Minis | HTML5 + Minis 桥 | zip → TikTok 平台 |
| L2 | Google Play Instant | React Native | Instant AAB |
| L2 | Facebook Instant Games | HTML5 | zip → Meta 平台 |
| L2 | Snapchat Mini | HTML5 | zip → Snap 平台 |
| L2 | LINE Mini App | LIFF（web） | LIFF 应用 → LINE 平台 |
| L3 | Android TV | RN · Leanback | APK → Play（TV 轨道） |
| L3 | Apple tvOS | react-native-tvos | TV 包 → App Store |
| L3 | HarmonyOS TV | RNOH | HAP → 华为视频 |
| L3 | Samsung Tizen（TV） | react-dom（web 包） | .wgt → Seller Office |
| L3 | LG webOS | react-dom（web 包） | .ipk → LG 内容商店 |
| L3 | Roku OS | 受限渲染器（SceneGraph 子集）※实验 | 频道包 → Roku 渠道 |
| L3 | Amazon Fire TV | React Native（Android） | APK → 亚马逊应用商店 |
| L3 | Vizio SmartCast | react-dom（web 包） | 应用包 → Vizio 平台 |
| L3 | Hisense VIDAA | react-dom（web 包） | 应用包 → VIDAA 商店 |
| L4 | GrapheneOS | React Native（无 GMS 构建） | APK → 自签名 / F-Droid |
| L4 | KaiOS | react-dom（web 栈） | zip → KaiStore |
| L4 | Sailfish OS | Qt / Silica 壳 + WebView | RPM → Jolla 商店 |
| L4 | Ubuntu Touch | WebView 壳（Clickable） | .click → OpenStore |
| L5 | Ubuntu | react-dom / Tauri | deb · Snap · Flatpak |
| L5 | Fedora | react-dom / Tauri | rpm · Flatpak |
| L5 | Linux Mint | react-dom / Tauri | deb · Flatpak |
| L5 | Chrome OS | Linux（Crostini）+ PWA | deb + PWA |
| L5 | FreeBSD | 原生构建 | pkg |
| L5 | OpenBSD | 原生构建 | ports |
| L5 | BSD 家族（NetBSD / DragonFly 等） | 原生构建 | pkgsrc 等 |
| L6 | Apple watchOS | 伴生应用（SwiftUI 模板）※实验 | watch 包 |
| L6 | Wear OS | RN / 轻量运行时 | APK → Play |
| L6 | Android Automotive | React Native（Android） | APK → 车厂渠道 |
| L6 | QNX | Rust 核心 + 可选 HMI 层 | 镜像 / 静态库 |
| L6 | AUTOSAR（Adaptive） | 无 UI（核心库） | ARXML / 静态库 |
| L6 | VxWorks | 无 UI（核心库） | 静态库 / 镜像 |
| L6 | RTOS（Zephyr / FreeRTOS） | 无 UI（no_std 核心） | 静态库 |
| L6 | Fitbit OS | Fitbit JS SDK | Fitbit 应用包 |
| L6 | Garmin（Connect IQ） | Monkey C 转译 ※实验 | .iq 包 |
| L6 | Zepp OS | Zepp JS API | Zepp 小程序 |
| L7 | PlayStation | 官方 SDK 插件（需授权） | 平台包 |
| L7 | Xbox | GDK 插件（需授权） | 平台包 |
| L7 | Nintendo Switch | NintendoSDK 插件（需授权） | 平台包 |

> 等级说明：**L1** 一等公民，全链路官方支持；**L2** 小程序与即时应用，经 Taro / HTML5 适配层；**L3** 电视与大屏；**L4** 移动系统变体；**L5** 桌面发行版与 BSD；**L6** 穿戴 / 车机 / 嵌入式（部分为无 UI 核心库）；**L7** 主机平台，受厂商授权约束，仅以官方 SDK 插件形式提供。
>
> 清单共 **49 个明确平台**；计入 BSD 家族、RTOS 家族与 Linux 发行版家族后为 **50+ 端**。标记 ※实验 的目标其运行时受限，风险与对策见 [docs/ROADMAP.md](docs/ROADMAP.md) 的风险登记册。

---

## 项目架构

![PolyPack 架构总览](docs/assets/architecture.svg)

五个层次，自上而下：

1. **入口层**——`poly` CLI（单二进制）、`poly-gui` 控制台（Tauri + React）与 CI 管线，共同向核心提交构建请求；
2. **Rust 核心工作区（poly workspace）**——`poly-core` 流水线编排、`poly-targets` 目标注册表（50+ TargetSpec）、`poly-toolchain` 工具链探测、`poly-bundle` 打包器矩阵、`poly-sign` 签名 / 公证 / 加固、`poly-publish` 商店发布；
3. **旁路资源**——构建缓存（内容寻址、增量复用）、远程构建农场（Apple 系目标必须 macOS 宿主，由 macOS Worker 池承接）、密钥库（Keychain / Vault 引用，密钥永不入库）；
4. **React UI 适配层**——`@polypack/ui` 设计系统（tokens + headless 原语）+ 平台渲染适配器（react-dom / react-native / Taro / TV / 穿戴），把同一份界面按端换皮；
5. **产物与收件人**——APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / 小程序包 …… 送达 50+ 个平台的商店与渠道。

Rust 与 React 之间通过四座桥通讯：**napi-rs**（Node / Taro 侧）、**uniffi**（Kotlin / Swift）、**wasm-bindgen**（Web / WebView 侧）、**cffi**（QNX / RTOS 等嵌入式侧）。

## 请求周期

![一次 poly release 的请求周期](docs/assets/request-cycle.svg)

一次 `poly release` 的完整旅程，13 步、5 个阶段：

| 阶段 | 步骤 |
|---|---|
| 01 解析探测 | 命令进入 CLI → 装载 `polypack.toml` 生成 JobSpec → 探测 SDK / NDK → 工具链就绪（缓存命中直接复用） |
| 02 编译与 UI | Rust 交叉编译出各端库（.a / .so / .wasm）→ React 打包 + 适配器变换出 bundle.js 与资源 |
| 03 签名 | 向密钥库引用凭据 → 签名 / 公证 / 加固完成 |
| 04 封装校验 | 按目标封装 APK / IPA / HAP / wgt… → 产出校验清单 |
| 05 交付 | 构建报告 + 产物路径返回；`--publish` 时继续向商店提审 |

## 项目功能

![PolyPack 功能地图](docs/assets/features.svg)

五大能力族：

- **多端目标与覆盖**——声明式目标注册表（50+ TargetSpec）、`poly release` 一条命令全出、系统变体支持（GrapheneOS / KaiOS / 鸿蒙族）；
- **构建与缓存**——工具链自动探测与安装、内容寻址缓存（增量复用、可复现）、远程构建农场（macOS Worker 池）；
- **跨平台 UI 体系**——`@polypack/ui` 设计系统、平台组件适配（Material / HIG / HarmonyOS 风格）、`poly dev` 热预览；
- **签名与发布**——密钥库集成（Keychain / Vault 引用）、签名 · 公证 · 加固（notarize / AVB）、商店流水线（Play / ASC / AGC / 小程序平台）；
- **生态与运维**——WASM 插件（自定义目标、沙箱执行）、CI 集成、诊断报告（构建解释、失败定位）。

## 生命周期

![项目与产物生命周期](docs/assets/lifecycle.svg)

从初始化到归档的状态机，三条泳道之外还有两条**恢复回路**：

- **主链路**：初始化 → 开发迭代 → 构建 → 签名与校验 → 发布；
- **等待态**：签名缺密钥（等 Vault / Keychain 解锁）、商店与小程序人工审核（等待期不消耗构建资源）；
- **恢复回路**：构建失败可"修复后重试"回到构建；审核驳回可"整改后重建"回到开发迭代——两条回路保证任何失败都有明确的返程；
- **终态**：归档 / EOL 是唯一的退役出口，停止维护但保留产物可追溯。

## UI 组件策略

PolyPack 的界面只写一遍（React），由适配层按端映射到**该平台自己的组件体系与插件**：

| 平台族 | React 运行时 | 组件体系 |
|---|---|---|
| Web / 桌面（Win / macOS / Linux） | react-dom（Tauri WebView） | `@polypack/ui` 桌面 tokens |
| Android 族（含 GrapheneOS / Wear / Auto / Fire TV / Android TV） | React Native（Fabric + JSI） | Material 3 变体 |
| Apple 族（iOS / macOS / tvOS） | React Native / react-native-tvos | HIG 变体 |
| HarmonyOS 族（含 TV） | React Native for OpenHarmony | HarmonyOS Design |
| 小程序族（微信 / 支付宝 / 抖音 / 快应用） | Taro（React 语法编译） | 各平台原生组件 |
| 电视 Web 端（Tizen / webOS / VIDAA / Vizio / KaiOS） | react-dom（打包为 wgt / ipk / 应用包） | 10-foot UI + 焦点导航系统 |
| 穿戴（Wear OS / watchOS / Fitbit / Zepp） | RN / JS 轻量运行时（实验） | 紧凑型 tokens |
| 嵌入式 / 车机（QNX / AUTOSAR / RTOS） | headless 核心（可选 HMI 层） | 平台 HMI 规范 |

## 快速开始（规划中 API）

```bash
# 已可用（M1）：init / build / dev / clean / doctor（见下方说明）；release / publish 为目标形态
cargo install polypack      # 安装 poly CLI
poly init                   # 生成 polypack.toml
poly dev                    # 启动热预览
poly release --target android,ios,wechat
poly release --target all --publish
```

> `--target all` = 当前环境**可构建**的全部目标；排除项（需厂商授权、宿主 / 凭据不可达）与原因在 plan 阶段列明。
>
> M1 已可用：`cargo run -p poly-cli -- init|build|dev|clean|doctor`（本地工具链探测、示例应用脚手架、带内容寻址缓存的多端构建与打包、整页重载热预览、缓存与产物清理）。

`polypack.toml` 示意：

```toml
schema = 1

[package]
name = "my-app"
version = "1.2.0"

[ui]
framework = "react"
entry = "ui/src/main.tsx"

[core]
crate = "crates/app-core"    # Rust 业务核心，编译为各端库

[targets.android]
min-sdk = 24
signing = "keystore"          # 引用密钥库条目，密钥不入库

[targets.ios]
bundle-id = "dev.example.polypack"
team = "ABCDE12345"

[targets.wechat]
appid = "wx0000000000000000"

[publish]
stores = ["play", "appstore", "appgallery"]
```

## 仓库结构

```text
polypack/
├── crates/
│   ├── poly-cli/          # 命令行入口（clap）
│   ├── poly-core/         # 流水线编排、JobSpec、事件总线
│   ├── poly-targets/      # TargetSpec 注册表（50+ 目标声明）
│   ├── poly-toolchain/    # SDK / NDK / Xcode / emsdk 探测与安装
│   ├── poly-bundle/       # 打包器矩阵（apk / ipa / hap / msi / dmg / deb / rpm / wgt / …）
│   ├── poly-sign/         # 签名、公证、加固
│   ├── poly-publish/      # 商店与小程序平台发布
│   ├── poly-cache/        # 内容寻址构建缓存
│   └── poly-plugin/       # WASM 插件宿主（自定义目标）
├── ui/
│   ├── polypack-ui/       # @polypack/ui 设计系统（tokens + headless 原语）
│   └── adapters/          # react-dom / react-native / taro / tv / wear 适配器
├── app/                   # poly-gui 控制台（Tauri + React）
├── docs/
│   ├── ROADMAP.md         # 详细规划（里程碑、风险、支持矩阵）
│   ├── TASKS.md           # 任务计划（116 任务 / 283 人日）
│   ├── FARM.md            # 远程农场：发起地 / 构建地（部署、凭据与签名验证）
│   ├── assets/            # 本文档内嵌的全部 SVG（含 Pax）
│   └── diagrams/          # 图示源文件（JSON IR）与重建流水线
└── tools/
    ├── render-diagrams.mjs  # 图示流水线: JSON IR → SVG (含右下角 Pax 角标)
    └── build-pax.mjs        # Pax 生成器: 多形态 + 图标 + 联系表 + 游乐场
```

（M0 已交付 9-crate 工作区骨架与 `poly-cli` / `poly-core` / `poly-targets` / `poly-toolchain`；M2 起 `ui/` 已落地——`ui/adapters/native` 原生适配层 v0 + RN 宿主骨架（T-M2-04/05）；`app/` 仍为目标形态。）

## 文档与图示同步约定

文档即产品的一部分，图示与正文必须同源同步：

1. **`docs/diagrams/*.json` 是图示的唯一事实源**——修改架构 / 功能 / 流程时先改 JSON，再运行：

   ```bash
   node tools/render-diagrams.mjs   # 校验 → 渲染 → 提取为 docs/assets/*.svg
   ```

   CI 侧由 `render-diagrams.mjs --check`、`build-pax.mjs --check` 与 `tools/check-docs.mjs` 自动拦截漂移（见 `.github/workflows/docs-check.yml`）。

2. `README.md` 与 [`README.en.md`](README.en.md) 必须**同一次提交内同步**（双语镜像）；
3. 里程碑与支持矩阵的变化同步到 `docs/ROADMAP.md` / [`ROADMAP.en.md`](docs/ROADMAP.en.md)；
4. Pax 形象或文案变化时更新 `docs/assets/pax.svg`，保持与设计系统一致。

## 路线图

自 v1.0.0 起，本仓库的 tag 与 GitHub Release 名称统一按 `vX.Y.Z` 语义化递增（v1.0.0 = 本文档与规划基线）。

已立项的扩展方向（SBOM / monorepo / 增量发布 / OTA / 商店元数据 / GUI 仪表盘 / 设备预览 / ADR）见 [docs/ROADMAP.md](docs/ROADMAP.md) 第 13 节。

| 里程碑 | 内容 |
|---|---|
| M0 | 仓库骨架、`polypack.toml` schema、TargetSpec DSL、`poly doctor` |
| M1 | Windows / macOS / Linux / Web 全链路打通 + 内容寻址缓存 + **英文版图示** |
| M2 | Android / iOS（RN 桥 + 签名管线） |
| M3 | HarmonyOS（RNOH）+ 微信 / 支付宝 / 抖音小程序（Taro） |
| M4 | TV 家族（Android TV / Fire TV / tvOS / Tizen / webOS / VIDAA / Vizio；Roku 受限适配） |
| M5 | 桌面发行版与 BSD（deb / rpm / Flatpak / Snap / AppImage / pkg / ports / ChromeOS） |
| M6 | 嵌入式与车机（headless 核心：QNX / AUTOSAR / VxWorks / RTOS / Android Automotive） |
| M7 | 穿戴（Wear OS / watchOS 伴生 / Fitbit / Garmin / Zepp，实验） |
| M8 | WASM 插件 SDK、远程构建农场、商店发布流水线、生态 GA |

完整规划（含逐平台支持细节、风险与对策）见 **[docs/ROADMAP.md](docs/ROADMAP.md)**；任务级分解（116 任务 / 283 人日，含角色、依赖、验收标准与排期备注）见 **[docs/TASKS.md](docs/TASKS.md)**。

跨宿主构建（Linux 发起 → macOS worker 出 IPA）的部署方式、凭据模型与签名验证指引见 **[docs/FARM.md](docs/FARM.md)**。

## 许可

[Apache-2.0](LICENSE)（`LICENSE` 已落地；贡献规则随代码化完善）。
