# polypack-app

`poly init` 生成的最小 PolyPack 示例应用：Rust 核心 + React 前端骨架 + 四端 L1 目标声明。

## 目录结构

```text
polypack.toml      工程配置（四端目标 windows / macos / linux / web + [retention]）
core/              Rust 核心 crate（宿主可执行，启动输出应用版本；独立工作区）
ui/                React 骨架（index.html / main.jsx / tokens.css / vite.config.js / public/icon.svg）
icons/             安装包图标（<边长>x<边长>.png / icon.ico / icon.icns；手工放置优先于派生）
.gitignore         忽略 node_modules / dist / target / .polypack
```

## 快速开始

```bash
# 1. 先装 UI 依赖（React + vite；不装的话 UI 会降级为「仅拷贝」的静态打包，见下）
cd ui && npm install && cd ..

# 2. 探测本机工具链与目标就绪度
poly doctor

# 3. 构建（compile 与 bundle(ui) 双支线，随后本机宿主目标进 package 阶段打包；
#    产物落 dist/、core/target/<triple>/release/，安装包落 .polypack/packages/<目标>/）
poly build --target all

# 4. 开发预览：改 UI 或核心源码后页面自动刷新
poly dev

# 5. 清理缓存与产物快照（默认 dry-run，--yes 执行）
poly clean --all --yes
```

## 前端构建与降级

UI 是 React + vite：`poly build` / `poly dev` 调 `vite build --base ./ --outDir <dist>`（用
`ui/node_modules` 里的 vite，或 PATH 上的 vite）。

**没装依赖时**会自动降级为内建静态打包，但那只是**原样拷贝**：不做打包、不做 JSX / TS 转译。
JSX 入口（`main.jsx`）这样拷出来的产物浏览器跑不了，`poly build` 会发告警说明；装齐依赖后
（`cd ui && npm install`）自动切回 vite 构建。

## 安装包图标

打包前按以下优先级取图标（构建期生成，不入库）：

1. `icons/` 里手工放置的图标——`<边长>x<边长>.png` / `icon.ico` / `icon.icns`。
   放了就不再派生，包内用你放的这枚（换图标最省事的办法）。
2. 否则从单一源 `ui/public/icon.svg`（Pax 徽章，品牌资产由 `tools/build-pax.mjs` 生成）
   现场派生 PNG 16–256 + ICO（T-M1-14；需要 `rsvg-convert`，缺 ICO / ICNS 转换器时
   只出 PNG 并在告警里说明）。
3. 再否则用工程根 `icon.svg`（把图标源放在 UI 目录之外时的落点）。

两个源都没有时桌面目标**不出半成品包**：发一条 package 告警说明原因并跳过该目标
（Web 的 PWA 图标走 `ui/public/icon.svg` → `dist/`，与之无关）。

## 签名与凭据

`polypack.toml` 只声明目标，不存放任何明文凭据；签名档案（如 `linux-repo-gpg`、`apple-codesign`）
由目标注册表引用，实际密钥走系统密钥库 / Vault / CI 注入（ROADMAP §7）。
