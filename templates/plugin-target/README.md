# __polypack_plugin_name__

PolyPack 插件模板（`target-spec/v1`）：用 Rust + WIT 实现自定义打包目标，
编译为 wasm 组件后在 PolyPack 沙箱中运行。

## 目录

| 路径 | 说明 |
|------|------|
| `plugin/` | 插件本体（`cdylib`，`world polypack-target`）；`describe` 声明目标规格，`package` 产出打包结果 |
| `componentize/` | core wasm → 组件模型封装器（`wit-component`，无需全局安装 wasm-tools） |
| `contract/` | 宿主侧契约测试：wasmtime 加载组件并调用 `describe` / `package` |
| `build.sh` | 构建入口，产物落在 `dist/plugin.wasm` |

## 构建

前置：Rust 1.85+ 与 wasm 目标。

```bash
rustup target add wasm32-unknown-unknown
./build.sh
```

## 契约测试

```bash
cargo test -- --nocapture
```

`cargo test` 会先跑 `build.sh` 再加载产物；也可设置
`POLYPACK_PLUGIN_WASM=dist/plugin.wasm` 复用已构建产物。未安装
`wasm32-unknown-unknown` 时按环境槽 SKIP-with-print。CI 步骤见
`.github/workflows/ci.yml`，本地等价命令即上面的 `cargo test`。

自定义 `describe` / `package` 后，同步 `contract/src/lib.rs` 顶部的
`EXPECTED_FORMAT` 与 `EXPECTED_PACKAGE`（唯一需要随插件修改的断言）；
忘记同步会让契约测试以退出码 101 失败。

## 在应用中注册

```toml
[[plugins]]
id = "__polypack_plugin_id__"
path = "plugins/__polypack_plugin_id__.wasm"
# 可选：制品内容摘要；设置后加载前强制校验（poly plugin digest 产出）
# sri = "blake3-<hex>"
# 默认零能力（无文件系统、无网络）；确需时按最小权限显式授权：
# fs_read = ["data"]
# fs_write = ["out"]
# net = ["api.example.com"]
```

SRI（加载前校验的制品摘要）：

```bash
poly plugin digest dist/plugin.wasm
# blake3-<hex>
poly --json plugin digest dist/plugin.wasm   # 脚本用（不混入横幅）
```

该值与宿主 `Sandbox::load` 的 SRI 口径一致；插件重新构建后需重新计算。
`poly plugin new` 只生成插件工程，不写入应用目录；构建后把产物复制过去：

```bash
mkdir -p <app>/plugins
cp dist/plugin.wasm <app>/plugins/__polypack_plugin_id__.wasm
```

插件目标同样要求应用 `polypack.toml` 具备 `[ui]` 与 `[core]` 段（插件可以
不使用它们，但 schema 必填）；最小占位与构建命令：

```toml
[ui]
framework = "react"
entry = "index.html"

[core]
crate = "core"
```

```bash
poly build --config <app>/polypack.toml --target __polypack_plugin_id__
```

## 占位符

`poly plugin new <id>` 会自动替换以下占位符；手工复制本模板时请一并替换：

- `__polypack_plugin_id__`：插件 id（kebab-case）
- `__polypack_plugin_name__`：显示名
- `__polypack_plugin_crate__`：Cargo 包名

接口契约以 `plugin/wit/polypack-target.wit` 为准，`spec-version` 必须保持
`target-spec/v1`；破坏性变更由宿主以新版本号发布。
