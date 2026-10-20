# 环境槽关闭台账（ENV-SLOTS）

> 状态快照：2026-10-20 · 基线：`main`（与 `TASKS.md` / `GA.md` 当前版本同批）。
> 本台账只登记**环境槽**：仓内结构 / mock / 命令组装腿已真跑，但真实工具链、真机、真实云或人工签字尚未执行，因而只能 SKIP-with-print 的验收腿。
> 纪律：`SKIP-with-print` 不是 PASS；没有原始证据（命令、退出码、工具版本、产物摘要）的槽位不得在 TASKS / GA 标 ✅ 关闭。English: [ENV-SLOTS.en.md](ENV-SLOTS.en.md)

## 1. 口径与状态定义

环境槽与代码缺口分开记：环境槽可以用环境 / 凭据 / 宿主关闭；代码缺口必须先改代码或接线，单设环境变量无效。

| 状态 | 含义 |
| --- | --- |
| `OPEN` | 无真实执行证据；打开闸门时真实腿仍 SKIP 或会失败 |
| `PARTIAL` | 结构 / mock / 命令组装腿真跑，真实腿仍缺 |
| `CODE-GAP` | 缺代码接线，仅靠环境变量无法关闭 |
| `HUMAN` | 需人工或管理动作，无法由 runner 自动完成 |

| 模式 | 含义 |
| --- | --- |
| `CI` | 托管或自托管 runner 可完成，需凭据 / 工具链 / KVM |
| `HOST` | 需物理设备、GUI、局域网或交互登录 |
| `HUMAN` | 需组织流程或人工签字 |
| `CLOSED` | 真实证据已归档并复核，槽位关闭 |

## 2. 槽位总表
| ID | 关联任务 | 槽位 | 状态 | 模式 | 负责人 | 证据 | 现有 CI |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ENV-01 | T-M1-05 | Windows 真实 Tauri 样例 + 默认 Program Files 启动 | CLOSED | HOST | release-sec | run 39308849058 · windows-msi-real | rust-ci |
| ENV-02 | T-M2-10 | macOS `codesign` + `notarytool` 真执行 | PARTIAL | CI | release-sec | §3 ENV-02 | mobile-contract |
| ENV-03 | T-M2-13 / T-M8-09 | macOS 农场 worker → IPA | OPEN | HOST | infra + release-sec | §3 ENV-03 | — |
| ENV-04 | T-M3-05 / T-M3-06 | 微信 / 支付宝 / 抖音开发者工具预览 | PARTIAL | HOST | core + release-sec | §3 ENV-04 | miniprogram-contract |
| ENV-05 | T-M4-15 | Roku 真机侧载 | PARTIAL | HOST | core | §3 ENV-05 | — |
| ENV-06 | T-M5-07 | OpenBSD VM 安装冒烟 | PARTIAL | CI | infra | §3 ENV-06 | bsd-ci |
| ENV-07 | T-M5-08 | NetBSD `bmake` / pkgsrc | OPEN | CI | infra | §3 ENV-07 | — |
| ENV-08 | T-M6-03 / T-M6-09 | QNX SDP + QEMU 冒烟 | PARTIAL | CI | infra | §3 ENV-08 | embedded-ci |
| ENV-09 | T-M6-05 | VxWorks `wr-cc` 链接腿 | PARTIAL | CI | infra | §3 ENV-09 | embedded-ci |
| ENV-10 | T-M7-04 | Wear OS 设备 / 模拟器冒烟 | PARTIAL | CI | infra | §3 ENV-10 | wear-release-smoke |
| ENV-11 | T-M7-05 / T-M7-06 | watchOS `xcodebuild` 真构建 | PARTIAL | CI | core + release-sec | §3 ENV-11 | — |
| ENV-12 | T-M7-07 | Fitbit `fitbit-build` 自检 | PARTIAL | CI | infra | §3 ENV-12 | — |
| ENV-13 | T-M7-08 | Zepp `zeus` + 模拟器安装 | PARTIAL | HOST | core | §3 ENV-13 | — |
| ENV-14 | T-M7-09 | Garmin Connect IQ SDK / devkey 编译 | PARTIAL | CI | core | §3 ENV-14 | — |
| ENV-15 | T-M8-03 | keyring 信任根载体 | CLOSED | CI | release-sec | run 39409593528 · keyring-real | rust-ci · keyring-real |
| ENV-16 | T-M8-03 | Vault 信任根载体 | CLOSED | CI | release-sec + core | run 39414212765 · vault-real | rust-ci · vault-real |
| ENV-17 | T-M8-09 | 真实 S3 / OSS 往返与农场二跑 | CLOSED | CI | infra + release-sec | run 39425835979 · s3-real | rust-ci · s3-real |
| ENV-18 | T-M2-12 | Branch protection admin bypass | CLOSED | HUMAN | maintainer | §3 ENV-18 | secret-scan |
| ENV-19 | GA §7.2 | 人类外部开发者试用 | OPEN | HUMAN | docs | §3 ENV-19 | — |
| ENV-20 | T-M9-21 | 真机设备预览（Android + Android TV） | OPEN | HOST | core + infra | §3 ENV-20 | — |

负责人只使用五个角色：`release-sec` / `core` / `docs` / `infra` / `maintainer`；组合用 ` + `。证据列指向 §3 的执行卡；真实证据包仍按 §4.2 归档，本表不伪造证据。真实逐槽关闭仍是外部工作，未关闭槽位保持原状态。
## 3. 槽位执行卡

### ENV-01 · Windows 真实 Tauri 样例 + 默认 Program Files 启动

- 关联任务：T-M1-05（Windows 打包器），T-M1-03（Tauri 布局）。 缺口：现有 NSIS 冒烟用 rustc 夹具 PE 和 `/D=` 临时目录；真实 Tauri 样例、默认 `$PROGRAMFILES64\Pax Notes`、开始菜单启动未验证。`real_tauri_core_binary_resolves_injected_resources` 目前固定 `Host::Linux`，不能产出 Windows 证据。
- 环境 / 工具链 / 凭据：windows-2022 或物理 Windows 10/11、WebView2 Runtime、NSIS、WiX v3.14、Tauri CLI、真实 Tauri 样例工程；无需云凭据。
- 开关：`POLY_TAURI_CORE_BIN=<真实 Tauri 可执行>`；MSI 真跑另需 `POLY_MSI_REAL=1` 与 `POLY_MSI_REAL_OUT=<证据目录>`；NSIS 测试无需额外开关。
- 命令：
```powershell
cargo test -p poly-bundle --test packagers_windows nsis_real_build_produces_pe_installer -- --nocapture
cargo test -p poly-bundle --test packagers_windows nsis_install_launch_uninstall_smoke -- --nocapture
$env:POLY_TAURI_CORE_BIN = "C:\work\pax-notes\target\release\pax-notes.exe"
cargo test -p poly-bundle --test tauri_layout real_tauri_core_binary_resolves_injected_resources -- --nocapture
$env:POLY_MSI_REAL = "1"; $env:POLY_MSI_REAL_OUT = "C:\work\msi-evidence"
cargo test -p poly-bundle --test windows_msi_real -- --nocapture
poly build --target windows
msiexec /i ".polypack\packages\windows\Pax Notes_1.2.3_x64_en-US.msi" /qn
& "$env:ProgramFiles\Pax Notes\pax-notes.exe" --version
```
- 进度（2026-10-20）：`examples/windows-msi-app/` 真实 Tauri v2 样例、`crates/poly-bundle/tests/windows_msi_real.rs` 与 rust-ci `windows-msi-real` 作业已落地；真实 MSI / 安装 / 启动 / 卸载 / 残留证据待 `windows-2022` 首跑，状态保持 `PARTIAL`。
- 证据：两条 NSIS 测试输出；真实样例安装到默认路径后的 `--version` 输出与窗口截图；`Pax Notes_1.2.3_x64-setup.exe` 的 SHA-256；卸载后目录消失。当前 `tauri_layout` 仅 Linux 断言，Windows 真实样例腿需新增测试或外部脚本，不得用 Linux 输出冒充。
- 现状：`CLOSED`（2026-10-20）。run 39308849058 的 `windows-msi-real` 作业在 windows-2022 真跑通过：构建 MSI → `msiexec /i /qn` → 启动安装副本 `--smoke` → `msiexec /x /qn` → 检查安装目录 / 开始菜单 / HKCU / ARP 残留；日志、MSI、`.wxs`、`smoke-marker.txt`、摘要作为 CI 工件归档。 负责人：release-sec。
- 下一步：无（该槽已关闭）；仅在新 Windows / WebView2 / WiX 版本变化时复跑。
- 阻塞：无；GUI 目视渲染仍需交互桌面，但安装 / 默认路径启动 / 退出 / 卸载 / 残留已由 CI 覆盖。

### ENV-02 · macOS `codesign` + `notarytool` 真执行

- 关联任务：T-M2-10（iOS 签名与公证）。 缺口：`ios_sign` 只做命令组装与凭据零明文断言；`ios_real_run` 产未签名 IPA；真实 `codesign`、`notarytool submit`、`stapler`、`spctl` 未在 Apple 凭据下跑过。
- 环境 / 工具链 / 凭据：macOS 14 + Xcode、Apple Distribution 证书、Team ID、Apple ID + app-specific password（或 notarytool keychain profile）、`xcrun` / `codesign` / `ditto` / `spctl`。
- 开关：`POLYPACK_IOS_IDENTITY`、`POLYPACK_IOS_TEAM_ID`、`POLYPACK_APPLE_ID`、`POLYPACK_APPLE_APP_PASSWORD`；或先写入 `apple-codesign` 档案。
- 命令：
```sh
xcrun notarytool store-credentials apple-codesign \
  --apple-id "$POLYPACK_APPLE_ID" --team-id "$POLYPACK_IOS_TEAM_ID" \
  --password "$POLYPACK_APPLE_APP_PASSWORD"
poly build --target ios --config /srv/proj/polypack.toml
APP="/srv/proj/.polypack/packages/ios/export/Payload/Pax Notes.app"
codesign --verify --deep --strict --verbose=2 "$APP"
ditto -c -k --keepParent "$APP" /tmp/pax-ios.zip
xcrun notarytool submit /tmp/pax-ios.zip --keychain-profile apple-codesign --wait
xcrun stapler staple "$APP" && xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=4 "$APP"
```
- 证据：notarytool 的 `status: Accepted` 与 submission id；`codesign --verify`、`stapler validate`、`spctl` 全绿输出；IPA / `.app` 的 SHA-256；证书指纹（不含私钥）。日志不得出现 app-specific password。
- 现状：`PARTIAL`。命令链与凭据引用已测；真实签名 / 公证待 Apple 门控。 负责人：release-sec。
- 下一步：在 macos-14 配 Apple secrets，按上述命令跑一次并把 submission id 写入 TASKS T-M2-10。
- 阻塞：Apple 开发者账号、证书与公证凭据由组织持有；无凭据无法伪造。 运行位置：CI（macos-14 + secrets）可关闭；不需物理设备。

### ENV-03 · macOS 农场 worker → IPA

- 关联任务：T-M2-13（远程农场 v0）、T-M8-09（农场部署指南）。 缺口：Linux 双 worker 演练已绿；macOS worker 注册、跨宿主 submit、IPA 落在共享农场目录、macOS 侧验签未真跑。
- 环境 / 工具链 / 凭据：macOS worker（完整 Xcode、`xcode-select`、`rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`）、Linux 发起机、两端同路径可见的农场目录 / 共享缓存、Apple 签名凭据。
- 开关：`POLYPACK_IOS_IDENTITY`、`POLYPACK_IOS_TEAM_ID`；农场侧 `--cache` / `--work-dir`。
- 命令：
```sh
# macOS worker
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
# Linux submit（工程与农场目录同路径可见）
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
APP=$(find .polypack/packages/ios -path '*/export/Payload/*.app' -print -quit)
codesign --verify --deep --strict --verbose=2 "$APP"
spctl --assess --type execute --verbose=4 "$APP"
```
- 证据：两侧日志出现 `worker 已注册：mac-01（aarch64-apple-darwin，宿主 Macos）` 与 `农场任务完成：1 个任务 / 1 个 worker`；IPA 路径与 SHA-256；`codesign` / `spctl` 输出；农场目录两侧 `ls -l` 对照。
- 现状：`OPEN`。FARM-DEPLOY §9 仍为 SKIP-with-print；macOS worker 未接入 nightly。 负责人：infra + release-sec。
- 下一步：准备 macOS 主机，按 FARM-DEPLOY §9 执行并把两侧 transcript 归档；随后把该槽从 GA §7.1 删除。
- 阻塞：需要 macOS 主机与跨宿主共享挂载；托管 Linux runner 无法代跑 Xcode / codesign。 运行位置：物理 macOS 宿主 + Linux 发起机；不在本仓 CI 内。

### ENV-04 · 微信 / 支付宝 / 抖音开发者工具预览

- 关联任务：T-M3-05（Taro adapter）、T-M3-06（小程序构建管线）。 缺口：`contract_devtools_real_preview` 在 `POLY_CONTRACT_REAL=1` 下调用三端 CLI；工具未安装或未登录时只 NOTE / SKIP。真实项目预览码 / 真机预览未产出。
- 环境 / 工具链 / 凭据：macOS（三端 devtools 官方支持最好）、WeChat DevTools CLI、Alipay Mini Program Studio `minidev`、Douyin DevTools `tma`、三端已登录账号（扫码 / 2FA）；抖音免密 Token 可走 CI。
- 开关：`POLY_CONTRACT_REAL=1`、`POLYPACK_WECHAT_DEVTOOLS_CLI`、`POLYPACK_ALIPAY_DEVTOOLS_CLI`、`POLYPACK_DOUYIN_DEVTOOLS_CLI`、`POLYPACK_WECHAT_DEVTOOLS_LOGIN`、`POLYPACK_ALIPAY_DEVTOOLS_LOGIN`、`POLYPACK_DOUYIN_DEVTOOLS_LOGIN`、`POLYPACK_DEVTOOLS_LOGIN_FILE`。
- 命令：
```sh
POLY_CONTRACT_REAL=1 POLYPACK_WECHAT_DEVTOOLS_CLI=/Applications/wechatwebdevtools.app/Contents/MacOS/cli \
POLYPACK_ALIPAY_DEVTOOLS_CLI=/Applications/minidev POLYPACK_DOUYIN_DEVTOOLS_CLI=/Applications/tma \
POLYPACK_WECHAT_DEVTOOLS_LOGIN="$POLYPACK_WECHAT_DEVTOOLS_LOGIN" \
POLYPACK_ALIPAY_DEVTOOLS_LOGIN="$POLYPACK_ALIPAY_DEVTOOLS_LOGIN" \
cargo test -p poly-cli --test contract_m3_devtools contract_devtools_real_preview -- --nocapture
```
- 证据：三端预览命令退出码 0；WeChat / Alipay 预览二维码截图或预览 URL；抖音远程预览返回；工具版本与登录账号（脱敏）；`contract_devtools_real_preview` 无 SKIP 的完整输出。
- 现状：`PARTIAL`。mock 与命令组装真跑；真实预览依赖 GUI 登录，miniprogram-contract `real` 作业在 vars / secrets 未设时只 NOTE。 负责人：core + release-sec。
- 下一步：在 macOS 上登录三端工具，设置 CLI 路径与登录态，跑模块测试并保存二维码 / URL；抖音优先走免密 Token 以进 CI。
- 阻塞：微信 / 支付宝登录态涉及扫码或 2FA，不能存进公开仓库；抖音账号资质另需审核。 运行位置：GUI 登录需物理 macOS；登录态就绪后命令腿可在 CI 跑。

### ENV-05 · Roku 真机侧载

- 关联任务：T-M4-15（Roku 频道包与校验器）。 缺口：结构自检 19 项真跑；`POLY_ROKU_REAL=1` 的 ECP 探测与 `plugin_install` 未对真实 Roku 执行。
- 环境 / 工具链 / 凭据：Roku 设备（开发者模式）、与设备同局域网的宿主、设备 IP、开发者密码、`curl`、`zip` / `unzip`。
- 开关：`POLY_ROKU_REAL=1`、`ROKU_DEV_TARGET=<ip>`、`ROKU_DEV_PASSWORD=<pwd>`。
- 命令：
```sh
cd ui/adapters/roku
POLY_ROKU_REAL=1 ROKU_DEV_TARGET=<ip> ROKU_DEV_PASSWORD=<pwd> node scripts/build-and-check.mjs
curl --connect-timeout 2 -s "http://<ip>:8060/"
```
- 证据：脚本输出 `ECP 设备: <name>` 与 `plugin_install 侧载成功`；Roku 屏幕上的频道启动截图；`ui/adapters/roku/dist/polypack-spike-channel.zip` 的 SHA-256；设备型号 / 固件版本。
- 现状：`PARTIAL`。结构自检与 zip 布局真跑；真机侧载按环境槽 SKIP-with-print，无 CI 作业。 负责人：core。
- 下一步：用一台开发者模式 Roku 执行上述命令，截图并归档；如设备可长期在线，再加自托管 runner 作业。
- 阻塞：需要局域网内的真实 Roku 与开发者密码；托管 runner 无设备网络。 运行位置：物理设备 + 同 LAN 宿主，不能进托管 CI。

### ENV-06 · OpenBSD VM 安装冒烟

- 关联任务：T-M5-07（OpenBSD ports 目标）。 缺口：bsd-ci 的 `openbsd` 作业存在但 `continue-on-error: true`；托管 runner 上 OpenBSD VM 在 TCG 下 SSH 启动失败，安装冒烟未产生绿色证据。
- 环境 / 工具链 / 凭据：支持 KVM 的 self-hosted runner（或物理 OpenBSD 主机）、OpenBSD 7.x 镜像、`pkg_add rust`、ports 树、`signify`。
- 开关：无专用开关；由 `vmactions/openbsd-vm` 作业执行。首绿后移除 `continue-on-error`。
- 命令：
```sh
pkg_add rust
cargo build -p poly-cli --ignore-rust-version
./target/debug/poly init /tmp/poly-openbsd && cd /tmp/poly-openbsd
./target/debug/poly build --target openbsd
cd /usr/ports/x11/<port> && make package PACKAGES=/tmp/openbsd-packages
pkg_add /tmp/openbsd-packages/<pkg>.tgz && pkg_info && pkg_check
```
- 证据：`make package` 与 `pkg_add` 退出码 0；`pkg_info` 显示包；`pkg_check` 无错误；`signify -V` 输出；bsd-ci `openbsd` 作业绿色日志与 artifact。
- 现状：`PARTIAL`。作业已接线但宿主虚拟化不满足，只有 SKIP / 失败日志，没有安装证据。 负责人：infra。
- 下一步：给 bsd-ci 加 KVM self-hosted runner 标签，跑绿一次后把 `continue-on-error` 改为 false。
- 阻塞：托管 ubuntu-latest 无 KVM；OpenBSD 在 TCG 下启动超时。 运行位置：CI（KVM self-hosted）可关闭。

### ENV-07 · NetBSD `bmake` / pkgsrc

- 关联任务：T-M5-08（BSD 家族 pkgsrc 目标）。 缺口：`bmake package` 与 NetBSD VM 真跑缺席；仓内只有 pkgsrc 文件结构 / 引号化断言。bsd-ci 无 NetBSD 作业。
- 环境 / 工具链 / 凭据：NetBSD 10 VM / 主机、`pkgin install rust bmake`、pkgsrc 树、`pkg_add`；无需签名凭据（自建仓库另配）。
- 开关：无专用开关；`poly build --target netbsd` 产出 pkgsrc 工程。
- 命令：
```sh
pkgin install rust bmake
cargo build -p poly-cli --ignore-rust-version
./target/debug/poly init /tmp/poly-netbsd && cd /tmp/poly-netbsd
./target/debug/poly build --target netbsd
cd <生成的 pkgsrc 目录> && bmake package
pkg_add <产物>.tgz && pkg_info && pkg_info -L <pkg>
```
- 证据：`bmake package` 退出码 0；`pkg_add` / `pkg_info` 输出；`.tgz` SHA-256；NetBSD 版本与 `bmake -V MAKE_VERSION`。
- 现状：`OPEN`。pkgsrc 生成与结构断言真跑；`bmake` 腿 SKIP-with-print，且无 CI 作业。 负责人：infra。
- 下一步：在 bsd-ci 新增 `netbsd` 作业（vmactions/netbsd-vm 或 self-hosted），先跑结构再跑 `bmake package`。
- 阻塞：无 NetBSD 宿主；pkgsrc 首次 bootstrap 与镜像带宽大。 运行位置：CI（NetBSD VM / self-hosted）可关闭。

### ENV-08 · QNX SDP + QEMU 冒烟

- 关联任务：T-M6-03（QNX 目标）、T-M6-09（QNX QEMU 冒烟）。 缺口：embedded-ci 的 QNX 条目在 `vars.POLYPACK_QNX_SDP` 缺失时 SKIP；SDP 真交叉编译、`.ifs` 出包、QEMU 串口自检未跑。
- 环境 / 工具链 / 凭据：QNX SDP 7.1（`qcc` / `nto*-gcc`、`mkifs`）、QNX 许可证、`source qnxsdp-env.sh`、`qemu-system-x86_64`（或板级 QEMU）；可选 `POLYPACK_QNX_QEMU_ARGS`。
- 开关：`POLYPACK_QNX_SDP`、`POLYPACK_QNX_QEMU`、`POLYPACK_QNX_QEMU_ARGS`；CI 侧仓库变量 `vars.POLYPACK_QNX_SDP`。
- 命令：
```sh
export POLYPACK_QNX_SDP=/opt/qnx710
source "$POLYPACK_QNX_SDP/qnxsdp-env.sh"
poly doctor --json
poly init /tmp/poly-qnx && cd /tmp/poly-qnx
poly build --target qnx --json
POLYPACK_QNX_QEMU=/usr/bin/qemu-system-x86_64 poly verify --target qnx --json
```
- 证据：`doctor` 找到 `qcc` / `nto*-gcc`；build JSONL 出现 `qnx-image-structure` 的 `verified`；QEMU 串口日志与 `result.json`；`.ifs` / 静态库 SHA-256。
- 现状：`PARTIAL`。SDP 探测、结构校验与 CI 矩阵已接线；SDP 未提供，真实编译 / QEMU 冒烟 SKIP。 负责人：infra。
- 下一步：在 self-hosted runner 安装 SDP 7.1 并设 `vars.POLYPACK_QNX_SDP`；首绿后把 QNX 条目从 job summary 的 SKIP 列表移除。
- 阻塞：QNX SDP 需商业许可证，不能公开下载进托管 runner。 运行位置：CI（self-hosted runner + 许可证）可关闭。

### ENV-09 · VxWorks `wr-cc` 链接腿

- 关联任务：T-M6-05（VxWorks 目标）。 缺口：库 / 镜像打包与 `nm` 白名单真跑；`wr-cc` 缺席，链接自检腿 SKIP-with-print。
- 环境 / 工具链 / 凭据：Wind River Workbench / VxWorks 7 SDK、`wr-cc`、目标 triple（默认 `x86_64-wrs-vxworks`）、许可证；`-Z build-std` 的 nightly toolchain。
- 开关：`WR_CC`、`POLYPACK_VXWORKS_TRIPLE`。
- 命令：
```sh
export WR_CC=/opt/windriver/wr-cc
export POLYPACK_VXWORKS_TRIPLE=x86_64-wrs-vxworks
poly doctor --json
poly init /tmp/poly-vx && cd /tmp/poly-vx
poly build --target vxworks --json
```
- 证据：build JSONL 出现 `vxworks-ld-check` 的 `verified`；`check.sh` 的 `wr-cc -c` 与链接命令退出码 0；`nm` 无白名单外未解析符号；`.vxe` / 库 SHA-256；`wr-cc --version`。
- 现状：`PARTIAL`。打包器与结构断言真跑；`wr-cc` 链接腿 SKIP。 负责人：infra。
- 下一步：在 self-hosted runner 装 Workbench SDK，设 `WR_CC`，跑一次并归档 `check.sh` 输出。
- 阻塞：VxWorks SDK 与许可证需商业采购。 运行位置：CI（self-hosted runner）可关闭。

### ENV-10 · Wear OS 设备 / 模拟器冒烟

- 关联任务：T-M7-04（Wear OS 双冒烟）。 缺口：mock 腿真跑；真实 AVD 或手表上的 install → launch → 用例 → uninstall 未执行。`wear-release-smoke` 缺 `/dev/kvm` 或预构建 APK 时硬阻断。
- 环境 / 工具链 / 凭据：Linux KVM（`/dev/kvm`）或物理 Wear OS 设备 + ADB 授权、Android SDK / `avdmanager`、Wear OS 5 (API 34) x86_64 镜像或预构建 APK；`POLYPACK_WEAR_FORM_APK`。
- 开关：`POLYPACK_WEAR_FORM_APK`、`POLYPACK_WEAR_FORM_PACKAGE`（默认 `com.polypack.weatform`）、`POLYPACK_WEAR_SMOKE_LOGS`。
- 命令：
```sh
adb devices
POLYPACK_WEAR_FORM_APK=/path/to/weatform.apk \
POLYPACK_WEAR_FORM_PACKAGE=com.polypack.weatform \
POLYPACK_WEAR_SMOKE_LOGS=/tmp/wear-logs \
node ui/smoke/wear/run.mjs --leg android --serial emulator-5554 \
  --apk "$POLYPACK_WEAR_FORM_APK" --logs /tmp/wear-logs
```
- 证据：`result.json`（schema `polypack/wear-smoke/v1`）、`logcat.txt`、截图、UIAutomator dump、构建摘要含 commit 与 APK SHA-256；`wear-release-smoke` 作业绿色并上传 90 天 artifact。
- 现状：`PARTIAL`。mock / 夜跑作业就绪；KVM 与 APK 未提供，真实设备腿未跑。 负责人：infra。
- 下一步：给 runner 开 KVM，构建表单 APK 并设 `vars.POLYPACK_WEAR_FORM_APK`；或接一台长期在线手表。
- 阻塞：托管 runner 无 `/dev/kvm`；物理手表需 ADB 授权与固定连接。 运行位置：CI（KVM self-hosted）或物理设备。

### ENV-11 · watchOS `xcodebuild` 真构建

- 关联任务：T-M7-05（watchOS 模板）、T-M7-06（watchOS 打包与签名链）。 缺口：Linux 上 `xcodebuild` 返回 `ToolMissing`，模板构建与 `.xcarchive` → `Payload/*.app` 只在 mock 层断言；macOS 真构建未跑。
- 环境 / 工具链 / 凭据：macOS 14 + Xcode（含 watchOS SDK / 模拟器）、`xcodebuild`、`ditto`、`plutil`；签名另需 Apple 凭据。
- 开关：`POLY_CONTRACT_REAL=1`（Apple 链通用）；watchOS 打包器按宿主自动门控，无专用开关。
- 命令：
```sh
poly init --target watchos /tmp/pax-watch && cd /tmp/pax-watch
xcodebuild -project PolyPackWatch.xcodeproj -scheme PolyPackWatch \
  -destination 'generic/platform=watchOS' build CODE_SIGNING_ALLOWED=NO
poly build --target watchos
xcodebuild archive -project PolyPackWatch.xcodeproj -scheme PolyPackWatch \
  -configuration Release -destination 'generic/platform=watchOS' \
  -archivePath /tmp/PolyPackWatch.xcarchive \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=
```
- 证据：`xcodebuild` 的 `BUILD SUCCEEDED`；`.polypack/packages/watchos/export/Payload/<产品>.app` 存在；`Info.plist` / `PkgInfo` 结构断言；`.app` SHA-256；Xcode 版本。
- 现状：`PARTIAL`。模板 10 文件与打包器 mock 断言真跑；macOS 真构建缺失，无 CI 作业。 负责人：core。
- 下一步：在 macos-14 增加 watchOS 作业（安装 Xcode 后跑上面两条 `xcodebuild`），或手动执行并归档。
- 阻塞：需 macOS + Xcode；Linux 宿主只能返回 `poly::E_BND_0001` 与 R3 农场提示。 运行位置：CI（macos-14）可关闭。

### ENV-12 · Fitbit `fitbit-build` 自检

- 关联任务：T-M7-07（Fitbit OS 打包器）。 缺口：`.fba` 结构三层自检与金样真跑；`fitbit-build` 不在 PATH，CLI 自检 SKIP-with-print。
- 环境 / 工具链 / 凭据：Node 18+、`@fitbit/sdk-cli`（提供 `fitbit-build`）、npm 网络；无需硬件。
- 开关：无专用开关；工具在 PATH 即真跑。
- 命令：
```sh
npm install -g @fitbit/sdk-cli
fitbit-build --help
cargo test -p poly-cli --test contract_m7_wear_js -- --nocapture
```
- 证据：`fitbit-build --help` 退出码 0；`poly verify --target fitbit --json` 的 notes 不再含 `fitbit-build` 的 SKIP-with-print；`.fba` SHA-256 与结构断言输出。
- 现状：`PARTIAL`。打包器与结构检查真跑；官方 CLI 自检缺席。 负责人：infra。
- 下一步：在 CI 安装 `@fitbit/sdk-cli`，把 `contract_m7_wear_js` 的 Fitbit 腿从 SKIP 变 REAL；注意官方 CLI 只做在场自检，不重编译本项目（本项目不做 TS / rollup）。
- 阻塞：npm 包与账号可用性；无硬件要求。 运行位置：CI（ubuntu / macOS）可关闭。

### ENV-13 · Zepp `zeus` + 模拟器安装

- 关联任务：T-M7-08（Zepp OS 打包器）。 缺口：`.zab` 结构与 md5 命名真跑；`zeus --version` 因工具缺席 SKIP；模拟器安装腿在 verify 中恒 SKIP-with-print（代码明确不自动化）。
- 环境 / 工具链 / 凭据：Node + Zepp OS `zeus` CLI（`@zeppos/zeus-cli`，以官方文档为准）、Zepp OS 模拟器（GUI）、开发者账号（安装 / 预览按官方要求）。
- 开关：无专用开关；`zeus` 在 PATH 即自检。模拟器需人工操作。
- 命令：
```sh
npm install -g @zeppos/zeus-cli
zeus --version
cargo test -p poly-cli --test contract_m7_wear_js -- --nocapture
# 模拟器：手动把 .polypack/packages/zepp/*.zab 装入 Zepp OS 模拟器并截图
```
- 证据：`zeus --version` 输出；verify notes 不再含 `zeus` SKIP；模拟器安装 / 启动截图与 `.zab` SHA-256；模拟器版本。
- 现状：`PARTIAL`。打包器与结构断言真跑；`zeus` 与模拟器腿均缺席。 负责人：core。
- 下一步：CI 安装 zeus 关闭自检腿；模拟器腿由人工执行并把截图附到 ENV-13 issue。
- 阻塞：模拟器是 GUI 应用，无法在 headless runner 自动安装；`zeus` 安装包名与授权以官方为准。 运行位置：`zeus` 自检可在 CI；模拟器安装需物理 GUI 宿主。

### ENV-14 · Garmin Connect IQ SDK / devkey 编译

- 关联任务：T-M7-09（Garmin Monkey C 转译 spike）。 缺口：29/29 Node 测试与结构自检真跑；`monkeyc` / `CIQ_DEVKEY` 缺席，Monkey C 编译腿 SKIP（PASS 4 / FAIL 0 / SKIP 3）。
- 环境 / 工具链 / 凭据：Connect IQ SDK（`bin/monkeyc`）、开发者密钥 `.der`、Java 运行时；SDK 下载可能受 403 / 账号限制。
- 开关：`CIQ_MONKEYC`、`CIQ_HOME`、`CONNECTIQ_HOME`、`CIQ_DEVKEY`。
- 命令：
```sh
CIQ_HOME=/opt/connectiq-sdk CIQ_MONKEYC=/opt/connectiq-sdk/bin/monkeyc \
CIQ_DEVKEY=/secure/developer_key.der npm --prefix ui/adapters/garmin run build
cd <生成目录>/round && monkeyc -e -w -r -o ../garmin-round.iq -y "$CIQ_DEVKEY" -f monkey.jungle
cd <生成目录>/square && monkeyc -e -w -r -o ../garmin-square.iq -y "$CIQ_DEVKEY" -f monkey.jungle
```
- 证据：`npm run build` 输出 PASS 7 / FAIL 0 / SKIP 0；圆 / 方 `.iq` 文件与条目断言；`monkeyc --version`；devkey 指纹（不含私钥）；编译日志。
- 现状：`PARTIAL`。转译与结构自检真跑；编译器腿 SKIP；`poly build --target garmin` 仍无内建 iq 打包器（G10 条件任务）。 负责人：core。
- 下一步：在 CI 安装 Connect IQ SDK 并把 devkey 放 secret；关闭编译器腿。iq 打包器另立条件任务，不在本槽内伪装。
- 阻塞：SDK 获取可能需 Garmin 账号；devkey 由开发者生成，不能入库。 运行位置：CI（ubuntu / macOS + SDK）可关闭编译器腿。

### ENV-15 · keyring 信任根载体

- 关联任务：T-M8-03（插件信任根）。 缺口：env 载体已由 `plugin_trust_root_env` 自动化；keyring 后端单测（`keyring_real`）与 CLI E2E（`plugin_trust_root_keyring`，只换 `source = "keyring"`）已接 rust-ci `keyring-real` 作业，真实 Secret Service 首跑证据待归档。
- 环境 / 工具链 / 凭据：Linux Secret Service（dbus + gnome-keyring）、macOS Keychain 或 Windows Credential Manager；`secret-tool` 或等价写入工具；GPG 密钥对（信任根载荷）。
- 开关：`POLY_KEYRING_REAL=1`、`DBUS_SESSION_BUS_ADDRESS`；CLI 配置 `trust_root = { name = "plugin-trust-root", source = "keyring" }`。
- 命令：
```sh
mkdir -p -m 700 /tmp/poly-keyring-rt /tmp/poly-keyring-home/keyrings
dbus-run-session -- bash -c '
  export HOME=/tmp/poly-keyring-home XDG_RUNTIME_DIR=/tmp/poly-keyring-rt \
         XDG_DATA_HOME=/tmp/poly-keyring-home/.local/share \
         GNOME_KEYRING_CUSTOM_LOCAL_PATH=/tmp/poly-keyring-home/keyrings
  printf "poly-it-pass\n" | gnome-keyring-daemon --unlock --components=secrets --foreground &
  sleep 3
  POLY_KEYRING_REAL=1 cargo test -p poly-sign --test keyring_real -- --nocapture
  POLY_KEYRING_REAL=1 cargo test -p poly-cli --test plugin_trust_root_keyring -- --nocapture'
```
- 证据：`keyring_real` 的真写 / 真读 / 真删输出；`gdbus` 交叉查询条目存在；`plugin_trust_root_keyring` 的 `poly build --target linux-reference --json` 出现 `plugin_granted` / `packaged`，删除条目后为 `E_PLUGIN_0010`；日志无载荷明文。
- 现状：`CLOSED`（2026-10-20）。rust-ci `keyring-real`（run 39409593528）在隔离 Secret Service 上真跑 `keyring_real` 与 `plugin_trust_root_keyring` 通过，作业内 SKIP 即失败。 负责人：release-sec。
- 下一步：macOS / Windows 真钥匙串宿主另行立槽；Linux 腿已关，不能用本证据冒充其他平台。
- 阻塞：headless Secret Service 需按文件头配方隔离；macOS / Windows 需各自真钥匙串宿主，不能拿 Linux 结果冒充。 运行位置：CI（ubuntu + dbus / gnome-keyring）可关闭 Linux 腿；macOS / Windows 另行。

### ENV-16 · Vault 信任根载体

- 关联任务：T-M8-03（插件信任根）。 缺口：`vault_real` 验证 `VaultBackend` 自身的 KV v2 读写；`default_backends()` 现按 `VAULT_ADDR` / `VAULT_TOKEN` 自动加入 `VaultBackend<HttpVaultClient>`（插件信任根、发布、签名共用同一条后端链），代码缺口已关闭。CLI E2E `plugin_trust_root_vault`（只换 `source = "vault"`）已接 rust-ci `vault-real` 作业，首个真实 Vault 服务运行（run 39414212765）已通过。
- 环境 / 工具链 / 凭据：Vault 服务（dev 或真实集群）、`VAULT_ADDR`、`VAULT_TOKEN`、KV v2 路径；插件公钥载荷 `base64(JSON)`。
- 开关：`POLY_VAULT_REAL=1`、`VAULT_ADDR`、`VAULT_TOKEN`；CLI 配置 `trust_root = { name = "plugin-trust-root", source = "vault" }`。
- 命令：
```sh
vault server -dev -dev-root-token-id=root -dev-listen-address=127.0.0.1:8200 &
export VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN=root
POLY_VAULT_REAL=1 cargo test -p poly-sign --test vault_real -- --nocapture
POLY_VAULT_REAL=1 cargo test -p poly-cli --test plugin_trust_root_vault -- --nocapture
```
- 证据：`vault_real` 的真 KV v2 往返 / 404 / 403 脱敏输出；`plugin_trust_root_vault` 的 `poly build --target linux-reference --json` 出现 `plugin_granted` / `packaged`，删除 KV 条目后为 `E_PLUGIN_0010`；日志无 token。rust-ci `vault-real` run 39414212765 首跑通过。
- 现状：`CLOSED`（2026-10-20）。CI 作业与门控 E2E 在真实 Vault dev 服务上通过，作业内 SKIP 即失败。 负责人：release-sec + core。
- 下一步：真实集群 / 私有 CA 场景另行立槽；dev 服务证据不得冒充生产 Vault。
- 阻塞：无（CI dev server 已提供）；代码缺口已关闭。

### ENV-17 · 真实 S3 / OSS 往返与农场二跑

- 关联任务：T-M8-09（远程农场部署指南）。 缺口：S3 兼容后端与 mock 契约已验证；真实桶上的 PUT / GET / LIST / DELETE 与两 worker 二跑命中未跑。2026-10-20：rust-ci `s3-real`（run 39425835979）已通过（MinIO，Linux 腿），`s3_real` 往返与 `farm_cache` 的 S3 二跑测试（复用仓库现有静态 Web 夹具）都在作业内真实运行。
- 环境 / 工具链 / 凭据：CI 用 MinIO（server 由归档官方源码构建，mc 用官方 GitHub release 二进制并核对已发布 SHA-256）；也可指向 AWS S3 / 阿里云 OSS S3 兼容端点，endpoint / region / access key / secret / 可选 session token、桶写权限。
- 开关：`POLYPACK_S3_REAL=1`、`POLYPACK_S3_TEST_URI=s3://<bucket>[/<prefix>]`、`POLYPACK_S3_ENDPOINT`、`POLYPACK_S3_REGION`、`POLYPACK_S3_ACCESS_KEY_ID`、`POLYPACK_S3_SECRET_ACCESS_KEY`、`POLYPACK_S3_SESSION_TOKEN`、`POLYPACK_S3_PATH_STYLE`、`POLYPACK_S3_OSS_COMPAT=1`（OSS）。
- 命令：
```sh
# CI（rust-ci s3-real）：MinIO 监听 127.0.0.1:9000，桶 polypack-ci
POLYPACK_S3_REAL=1 POLYPACK_S3_TEST_URI=s3://polypack-ci/ci-prefix \
POLYPACK_S3_ENDPOINT=http://127.0.0.1:9000 POLYPACK_S3_REGION=us-east-1 \
POLYPACK_S3_ACCESS_KEY_ID=... POLYPACK_S3_SECRET_ACCESS_KEY=... \
cargo test -p poly-cache --test s3_real -- --nocapture
# 两 worker 二跑：同一桶前缀作为共享缓存，二跑必须 1/1 命中
POLYPACK_S3_REAL=1 POLYPACK_S3_TEST_URI=s3://polypack-ci/ci-prefix \
POLYPACK_S3_ENDPOINT=http://127.0.0.1:9000 POLYPACK_S3_REGION=us-east-1 \
POLYPACK_S3_ACCESS_KEY_ID=... POLYPACK_S3_SECRET_ACCESS_KEY=... \
cargo test -p poly-cli --test farm_cache \
  shared_cache_hits_across_worker_processes_on_s3 -- --nocapture
```
- 证据：`s3_real` 的 PUT / GET / LIST / DELETE 全绿；`farm_cache` S3 腿首跑 `缓存命中 0/1`、二跑 `1/1` 且两侧缓存键相同；作业内 SKIP 即失败，并要求两条 `... ok`；对象列表由 `mc ls` 记录。rust-ci `s3-real` run 39425835979 首跑通过。
- 现状：`CLOSED`（2026-10-20）。两条真实腿已在 rust-ci `s3-real` 的 MinIO 上通过，作业内 SKIP 即失败。 负责人：infra + release-sec。
- 下一步：真实云（AWS / 阿里云 OSS）另行取证，不得用 MinIO 结果冒充。
- 阻塞：dl.min.io 已停止分发社区版 MinIO（所有 release 与 hotfix 均 410 Gone），server 只能从归档源码构建；真实云账号 / 桶 / 凭据仍需另行提供。 运行位置：CI（MinIO 本地环回）可关闭 S3 兼容腿。

### ENV-18 · Branch protection admin bypass

- 关联任务：T-M2-12（secret-scan 与必过检查）。 原缺口：3 宿主 secret-scan 已是 required checks，但 `enforce_admins=false`，管理员可直推绕过 required checks；2026-10-20 已启用管理员同等约束。
- 环境 / 工具链 / 凭据：GitHub 仓库管理员权限、`gh` CLI 或网页设置。
- 开关：无环境变量；GitHub 设置项。
- 命令：
```sh
gh api repos/erikwang2013/polypack/branches/main/protection --jq '.enforce_admins.enabled'
gh api repos/erikwang2013/polypack/branches/main/protection/required_status_checks --jq '.contexts'
# 目标：enforce_admins.enabled == true；或在 GA 记录有时限的例外与四眼复核
```
- 证据：GitHub branch protection API 报告 `enforce_admins.enabled = true` 与 9 个 required contexts（含 `secret-scan / ubuntu-latest`、`secret-scan / macos-14`、`secret-scan / windows-2022`）；核实日期 2026-10-20，结果记录于 T-M2-12 标题与本节。
- 现状：`CLOSED`（2026-10-20）。9 个 required contexts 生效，管理员直推同样受 required checks 约束。 负责人：maintainer。
- 下一步：保持规则不变；如启用紧急 break-glass 通道，须按 GA §7 记录例外条件、审批人与四眼复核记录，并在事后恢复 `enforce_admins=true`。
- 阻塞：无（已关闭）；后续修改仍需仓库管理员权限。 运行位置：HUMAN / GitHub 设置，不在代码仓内。

### ENV-19 · 人类外部开发者试用

- 关联任务：GA §7.2「人类外部开发者试用」。 缺口：未招募真实外部开发者按 GA §2 在 1 天内新增一个自定义目标并出包；卡点未记录。
- 环境 / 工具链 / 凭据：3–5 名未参与本仓开发的外部开发者、干净机器（Linux / macOS / Windows 至少各一）、GA / README / 插件模板、反馈模板与计时表。
- 开关：无；纯人工流程。
- 命令：
```sh
# 每位试用者从零开始，仅按 docs/GA.md §2 操作；记录命令、耗时与卡点
poly plugin new pocket-notes
poly plugin check <dir>
poly build --config <app>/polypack.toml --target pocket-notes
```
- 证据：每位试用者的试用报告（环境、开始 / 结束时间、完成的命令、卡点原文、是否 1 天内出包）；汇总 issue；文档修订 PR 链接。
- 现状：`OPEN`。尚无外部试用记录。 负责人：docs。
- 下一步：GA 发布后招募 3–5 人，按模板收集报告，把卡点回写到 GA / README / 模板。
- 阻塞：需要真实外部人员与时间，不能由 agent 代跑。 运行位置：HUMAN；不在 CI。

### ENV-20 · 真机设备预览（Android + Android TV）

- 关联任务：T-M9-21（设备预览矩阵）。 缺口：`poly preview --dry-run` 已交付（DevicePool / DeviceLease、确定性计划、超时 / 清理），但真实租约、安装、启动、截图、日志与清理未执行。
- 环境 / 工具链 / 凭据：两台真实设备（Android 手机 + Android TV）或等价云设备；`adb`；夹具 APK；证据目录；云厂商账号（若走云）。
- 开关：无（`poly preview --dry-run` 不需要凭据；真实执行由 provider 适配层接线）。
- 命令：
```sh
poly preview --dry-run --target android --device android-phone --device android-tv
# 真实执行命令与 provider 适配见 docs/DEVICE-PREVIEW.md §8.1
```
- 证据：plan JSON、APK blake3、`adb devices -l` / API level、安装 / 启动退出码、两张 PNG + SHA-256、logcat / crash、卸载后设备列表。
- 现状：`OPEN`。dry-run 已交付；真实执行未接线。 负责人：core + infra。
- 下一步：接 provider 适配层并在两台真实设备上执行；两台都通过才关闭。
- 阻塞：需要真实设备 / 云账号与 adb 授权。 运行位置：HOST / 云设备农场。

## 4. 关闭协议

### 4.1 角色与职责

| 角色 | 职责 |
| --- | --- |
| 槽位负责人 | 准备环境、执行命令、生成证据包、起草 TASKS / GA 回写 |
| 复核人 | 核对原始日志确有真实调用、无 SKIP、退出码 0、证据绑定 commit |
| release-manager | 合并回写、维护 GA §7、确认不会用 SKIP 冒充 PASS |

### 4.2 取证步骤

1. 建 issue：标题 `[ENV-XX] <槽位>`，正文写关联任务、计划命令、预期证据、负责人。
2. 从目标 commit 构建 CLI 并记录 `git rev-parse HEAD`、`poly --version`、宿主 `uname -a` / `winver` / `sw_vers`。
3. 按执行卡设置环境变量；凭据只从 secret / 本机钥匙串读，绝不写进命令历史或仓库。
4. 用 `tee` 或 runner 日志捕获 stdout / stderr / 退出码；命令本身也要记录（脱敏后）。
5. 生成证据包并计算产物 SHA-256；截图只拍必要区域，遮住账号 / token / 设备密码。
6. 跑一次 `secret-scan` 或目检证据，确认无密钥、无私钥、无 app-specific password、无 Vault token。
7. 把证据包上传到 issue（小文件）或 GitHub Actions artifact / release asset（大文件）；在 TASKS / GA 只放链接，不贴日志。
8. 复核人按 4.3 清单验收；通过后同一 PR 回写 TASKS.md、TASKS.en.md、GA.md、GA.en.md。

### 4.3 证据包与脱敏

| 文件 | 内容 |
| --- | --- |
| `cmd.txt` | 实际命令（token / 密码以 `<redacted>` 替换） |
| `versions.txt` | 宿主、OS、工具链、SDK、`poly --version`、commit |
| `stdout.log` / `stderr.log` | 原始输出与退出码；必须能看到真实工具调用，不能只有 SKIP |
| `result.json` | 机器可读结果（Wear / QEMU / 农场等有 schema 的槽位） |
| `sha256.txt` | 产物与证据文件的 SHA-256 |
| `screenshot.png` | GUI / 设备 / 预览二维码（按需，脱敏） |

证据目录建议：`env-slots/<ENV-XX>/<UTC 时间戳>/`。仓库不提交二进制、日志、截图；用 issue / artifact 链接。证据必须绑定 commit；工具链大版本升级后需重跑。

### 4.4 回写 TASKS / GA

- TASKS 中把任务标题的注记从「环境槽 SKIP-with-print」改为「ENV-XX 已关：<证据链接>（YYYY-MM-DD，<工具版本>）」；验收行补同一链接。中英文同一次提交。
- GA §7.1 删除对应缺口行（或标「已关（ENV-XX）」并保留历史）；§7.2 删除对应动作或标完成。中英文同一次提交。
- 不得只改文档不附证据；不得把 SKIP 日志当关闭证据；不得用另一平台 / 另一工具链的输出冒充。
- 提交信息：`test(env): close ENV-XX <slot>`；回写提交：`docs(ga): record ENV-XX closure`。

## 5. 槽位 → CI 工作流矩阵

| 槽位 | 工作流 | 作业 | 覆盖与缺口 |
| --- | --- | --- | --- |
| ENV-01 | `.github/workflows/rust-ci.yml` | `rust / windows-2022` | NSIS 真编译 / 安装 / 启动；无真实 Tauri 样例、无默认 Program Files |
| ENV-02 | `.github/workflows/mobile-contract.yml` | `ios / macos-14` | Apple 链真跑；无 Apple 签名 / 公证 secrets |
| ENV-03 | — | — | 无农场作业；FARM-DEPLOY §9 人工 |
| ENV-04 | `.github/workflows/miniprogram-contract.yml` | `real / macos-14` | CLI 预览测试接线；vars / secrets 未设时 NOTE |
| ENV-05 | — | — | 无 Roku 作业 |
| ENV-06 | `.github/workflows/bsd-ci.yml` | `openbsd` | VM 作业存在，`continue-on-error` 且 TCG 启动失败 |
| ENV-07 | — | — | 无 NetBSD 作业 |
| ENV-08 | `.github/workflows/embedded-ci.yml` | `embedded`（matrix `qnx`） | 构建 / doctor / QEMU 接线；SDP 缺失即 SKIP |
| ENV-09 | `.github/workflows/embedded-ci.yml` | `embedded`（matrix `vxworks`） | 构建接线；`wr-cc` 缺失即 SKIP |
| ENV-10 | `.github/workflows/wear-release-smoke.yml` | `wear-device-smoke` | 发版硬门；缺 KVM / APK 阻断 |
| ENV-11 | — | — | 无 watchOS 作业；mobile-contract ios 不跑 xcodebuild watchOS |
| ENV-12 | — | — | 无 `fitbit-build` 作业 |
| ENV-13 | — | — | 无 `zeus` 作业 |
| ENV-14 | — | — | 无 Connect IQ 作业 |
| ENV-15 | `.github/workflows/rust-ci.yml` | `keyring-real` | Secret Service 真跑（run 39409593528）：`keyring_real` 真写 / 真读 / 真删 + CLI keyring 信任根 `plugin_granted` / `packaged` / `E_PLUGIN_0010`；作业内 SKIP 即失败 |
| ENV-16 | `.github/workflows/rust-ci.yml` | `vault-real` | Vault dev（KV v2，run 39414212765）：`vault_real` 真读写 / 404 / 403 脱敏 + CLI Vault 信任根 `plugin_granted` / `packaged` / `E_PLUGIN_0010`；作业内 SKIP 即失败 |
| ENV-17 | `.github/workflows/rust-ci.yml` | `s3-real` | MinIO（127.0.0.1:9000，run 39425835979）：`s3_real` PUT / GET / LIST / DELETE + `farm_cache` 两 worker 共享桶二跑 1/1；作业内 SKIP 即失败 |
| ENV-18 | `.github/workflows/secret-scan.yml` | `scan`（3 宿主） | 提供 required checks；admin bypass 已于 2026-10-20 关闭，设置不在工作流 |
| ENV-19 | — | — | 人工试用，无工作流 |
| ENV-20 | — | — | 无设备预览作业；`poly preview --dry-run` 已交付 |

## 6. 仓库内不可关闭清单

| 槽位 | 仓库外资源 | 为什么仓内不能关闭 |
| --- | --- | --- |
| ENV-01（GUI 腿） | 物理 Windows 桌面 | 开始菜单 / 默认安装目录的交互启动无法在服务会话观察 |
| ENV-03 | macOS 主机 + 共享农场目录 | 需 Xcode / codesign 与跨宿主挂载，Linux runner 不能代跑 |
| ENV-04 | 三端开发者账号与 GUI 登录 | 扫码 / 2FA / 账号资质不在仓库控制内 |
| ENV-05 | 局域网 Roku 设备 | ECP 侧载需要真实设备 IP 与开发者密码 |
| ENV-10 | KVM 或物理 Wear 设备 | ADB 授权与设备 / 模拟器属于宿主资源 |
| ENV-13（模拟器腿） | GUI 模拟器 | 安装 / 启动是桌面应用，verify 明确不自动化 |
| ENV-18 | GitHub 管理员设置 | Branch protection 是仓库平台配置，不是代码 |
| ENV-19 | 外部开发者 | 人类试用无法由 agent 或 CI 代替 |
| ENV-20 | 真实 Android / Android TV 设备或云农场 | 真机安装 / 启动 / 截图需要设备、adb 授权或厂商账号 |

## 7. 开放决策与代码缺口

| 项 | 类型 | 处置 |
| --- | --- | --- |
| Vault 默认后端链 | 已关闭（代码） | `default_backends()` 已按环境装配 Vault；ENV-16 只剩真实服务 E2E |
| Garmin 内建 iq 打包器 | 已交付 | `poly build --target garmin` 已有内建打包器；ENV-14 只关编译器 / 真机腿 |
| 缺失的 CI 作业 | 基建 | NetBSD / watchOS / Fitbit / Zepp / Garmin / Roku 目前只有执行卡，没有工作流 |

## 8. 参考

- [TASKS](TASKS.md) · [GA](GA.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md) · 英文版：[ENV-SLOTS.en.md](ENV-SLOTS.en.md)
