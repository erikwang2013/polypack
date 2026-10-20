# 安装 PolyPack CLI

> 适用版本：GitHub Releases 预编译产物（Linux x86_64 / macOS arm64 / Windows x86_64）与源码构建。
> 本文同时说明 crates.io 的当前阻塞项（T-M9-45）：全部 workspace crate 均为 `publish = false`，不要在维护者决策前改动。
> English: [INSTALL.en.md](INSTALL.en.md)

## 1. 支持矩阵

| 平台 | 架构 | Rust target | Release 资产 |
| --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | `poly-x86_64-unknown-linux-gnu.tar.gz` |
| macOS | Apple Silicon（arm64） | `aarch64-apple-darwin` | `poly-aarch64-apple-darwin.tar.gz` |
| Windows（Git Bash） | x86_64 | `x86_64-pc-windows-msvc` | `poly-x86_64-pc-windows-msvc.zip` |

安装脚本只覆盖上表；其他组合（Linux aarch64、macOS x86_64、Windows arm64 等）没有预编译产物，脚本会明确报错退出，不会回退到未校验下载。

## 2. 源码安装

前置：Rust 1.98+（以 `rust-toolchain.toml` 为准）与平台 C 工具链。

```bash
git clone https://github.com/erikwang2013/polypack.git
cd polypack
cargo install --path crates/poly-cli --locked
poly --version
```

`cargo install` 只安装 bin `poly`；首次编译较慢，后续可复用 `target/`。

## 3. 预编译二进制

从 GitHub Release 手动下载对应资产并解压：

```bash
TAG=v1.11.0
TARGET=x86_64-unknown-linux-gnu
curl -fsSLO "https://github.com/erikwang2013/polypack/releases/download/$TAG/poly-$TARGET.tar.gz"
tar -xzf "poly-$TARGET.tar.gz"
install -m 0755 "poly-$TARGET" "$HOME/.local/bin/poly"
```

Windows 资产是 `.zip`，解压后把 `poly-x86_64-pc-windows-msvc.exe` 放到 PATH 中并改名为 `poly.exe`。

## 4. 校验和

每个 Release 附带 `SHA256SUMS.txt`。下载资产与校验和后，在资产同目录执行：

```bash
sha256sum --check --ignore-missing SHA256SUMS.txt
```

macOS 没有 `sha256sum` 时用 `shasum -a 256 -c SHA256SUMS.txt`。校验和只能证明下载内容与发布清单一致；发布者身份需要签名（见 §5）。

## 5. 安装脚本

推荐先下载审阅再执行，不要直接 `curl | bash`：

```bash
curl -fsSLO https://raw.githubusercontent.com/erikwang2013/polypack/main/scripts/install.sh
less install.sh
bash install.sh
```

环境变量：

| 变量 | 默认值 | 作用 |
| --- | --- | --- |
| `POLY_VERSION` | 最新 Release | 指定 tag（`v1.11.0` 或 `1.11.0`） |
| `POLY_INSTALL_DIR` | `~/.local/bin` | 安装目录 |
| `POLY_REPO` | `erikwang2013/polypack` | GitHub 仓库 |
| `POLY_RELEASE_BASE_URL` | GitHub Release URL | 镜像 / 离线测试基础 URL |
| `POLY_REQUIRE_SIGNATURE` | 空 | 设为 `1` 时强制验证 minisign 签名 |
| `POLY_MINISIGN_PUBKEY` | 空 | minisign 公钥文件路径（严格模式必填） |

脚本行为：识别平台 → 解析 tag → 下载资产与 `SHA256SUMS.txt` → 强制 SHA-256 校验 → 安装 → 执行 `poly --version` 打印版本。没有跳过校验的开关；校验文件缺失、资产条目缺失或摘要不匹配都会在落盘前失败。严格签名模式还要求 `SHA256SUMS.txt.minisig` 与 `minisign` 可用，缺任何一项都不会安装。

签名是可选发布步骤：Release 工作流仅在配置 `MINISIGN_SECRET_KEY` secret（base64 编码的 minisign 私钥文件）时签名并上传 `SHA256SUMS.txt.minisig`；未配置时明确跳过，发布物只有校验和。公钥由维护者随文档发布，不要自行生成或编造发布密钥。

## 6. crates.io 阻塞项（T-M9-45）

| 检查项 | 现状 | 结论 |
| --- | --- | --- |
| workspace crate `publish` | 全部为 `false` | `cargo install poly-cli` 不可用 |
| 根清单版本 | `1.13.0` | 已与最新 tag 对齐 |
| 发布审计 | `cargo package --list` 9/9 通过；`cargo publish --dry-run` 被 `publish = false` 拒绝 | 审计已完成，实际发布关闭 |
| 名称可用性 | `poly-cli`、`poly-core` 已被他人占用 | 发布前需改名或取得所有权 |
| crates.io token | 仓库与 CI 均无 | 未配置发布 secret |
| 安装入口 | 源码路径 / GitHub Release | 当前唯一受支持路径 |

结论：审计（T-M9-45）已完成，版本与元数据已就绪，但全部 workspace crate 仍为 `publish = false`，且 `poly-cli` / `poly-core` 名称被占用、无发布 token；在这些维护者决策完成前，不要改动该标志，也不要声称支持 crates.io 安装。细节见 `docs/CRATES-IO.md`。

## 7. 验证与卸载

```bash
poly --version
poly doctor
```

卸载：删除 `POLY_INSTALL_DIR` 下的 `poly`（Windows 为 `poly.exe`）。安装脚本不写系统目录、不改 shell 配置。
