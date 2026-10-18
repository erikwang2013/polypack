# 远程农场部署指南

> 适用版本：农场协议 v1（`farm/v1`；v1 主端仍接受 `farm/v0` worker）。命令与参数以 `poly farm worker|submit --help` 和 `crates/poly-cli/src/farm.rs` 为准。
> English: [FARM-DEPLOY.en.md](FARM-DEPLOY.en.md)
> 配套阅读：[FARM.md](FARM.md)（协议帧 / 凭据模型 / 签名验证）、[ROADMAP.md](ROADMAP.md) §7（安全与密钥）、§8（测试与 CI）、§9（M2 风险 R3：Apple 系目标必须 macOS 宿主）。

## 1. 拓扑与边界

```text
Linux / Windows 发起地                     macOS 构建地
poly farm submit                           poly farm worker（mac-01、mac-02……多进程）
        │                                          │
        └──────────► /srv/polypack-farm ◄──────────┘   v1/<worker_id>/ 双信箱 + JSON Lines 帧
                     /srv/polypack-cache                共享目录 CAS（--cache；可选但推荐）
                     /srv/proj                          工程：v0 要求两侧同路径可见
```

**发起地决定「构建什么」**：读 `polypack.toml`、按目标拆分任务、投递 `JobSpec`、回收产物清单。**构建地决定「怎么构建」**：执行同一条 `Plan` DAG、按需拉取凭据，并独占平台工具链。协议帧只传路径、`JobSpec` 与产物记录（名称 / blake3 / 字节数），不传产物字节；v0 的产物留在构建地工程目录，经共享挂载或同步取回。

| 角色 | 职责 | 需要的条件 |
|---|---|---|
| 发起地（origin） | `poly farm submit`：声明目标、等待 worker 池、合并结果 | `poly` 本体、工程源码、农场目录；不需要 Xcode |
| 构建地（build site） | `poly farm worker`：注册、拉取、执行、回传 | `poly` 本体、平台工具链（Xcode / SDK）、农场目录、凭据后端 |

## 2. 前置条件

| 项 | 要求 |
|---|---|
| `poly` 版本 | 两端从**同一提交**构建（本仓 `poly-cli` 的 `publish = false`，尚未发布到 crates.io）；`poly --version` 应一致 |
| 共享文件系统 | 农场目录与缓存目录在两端以**同一路径**可见；缓存目录必须提供同目录 POSIX 原子 rename（NFS 需确认导出语义，SMB / 同步盘不保证时不要作为共享 CAS） |
| 工程可见性 | v0 要求工程在构建地以任务帧携带的路径可见（同路径挂载，或 rsync 到相同路径）；跨机自动同步不在 v0 范围 |
| 平台工具链 | macOS worker：完整 Xcode（`xcodebuild` / `plutil` / `ditto` / `codesign`）与三条 Apple Rust triple；Linux 发起地只需 Rust 与 `poly` |

## 3. 从零部署

### 3.1 共享目录

```bash
sudo mkdir -p /srv/polypack-farm /srv/polypack-cache /srv/proj
sudo chown "$USER": /srv/polypack-farm /srv/polypack-cache
chmod 2770 /srv/polypack-farm /srv/polypack-cache
```

农场目录承载协议信箱（`v1/<worker_id>/to-master`、`to-worker`）；缓存目录承载对象分片。两者**建议分开**，便于配额与清理。两端运行用户必须对这两个目录有读写权限（示例为同一部署用户；跨用户部署请用共享组或 NFS 映射）。

### 3.2 构建地（macOS）

从 App Store 或 developer.apple.com 安装**完整 Xcode**（仅 Command Line Tools 不含 iOS SDK），然后：

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
xcodebuild -version

# 同一提交的 poly（示例仓库路径 /srv/poly-src）
cd /srv/poly-src
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
cargo build --release -p poly-cli --locked
sudo install -m 755 target/release/poly /usr/local/bin/poly
poly --version

# 钥匙串档案：档案名必须等于 TargetSpec.signing（Apple 系为 apple-codesign）
export APPLE_ID='build@example.com'      # 非机密：Apple ID 邮箱
export TEAM_ID='TEAMID1234'              # 非机密：团队 ID
xcrun notarytool store-credentials apple-codesign --apple-id "$APPLE_ID" --team-id "$TEAM_ID"
# 按提示交互输入 App 专用密码：口令不进 argv、不进 shell 历史
export POLYPACK_IOS_IDENTITY='Apple Distribution: Example Ltd (TEAMID1234)'
export POLYPACK_IOS_TEAM_ID="$TEAM_ID"
```

启动 worker 池（每个进程一个信箱；池化 = 多进程）：

```bash
mkdir -p /var/tmp/polypack-work
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work

# 第二个 worker（另开终端 / supervisor）
poly farm worker --dir /srv/polypack-farm --id mac-02 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

`--id` 只允许字母、数字、`-`、`_`（会用作信箱目录名）。启动后必须在 `--idle` 秒内等到主端注册帧；长期驻留池请把 `--idle` 设大（如 120–600），并用 supervisor / systemd `Restart=always` 拉起——每次 `submit` 结束主端都会发 `goodbye`，`--jobs 0` 的 worker 也会正常退出。

### 3.3 发起地（Linux）

```bash
cd /srv/poly-src
cargo build --release -p poly-cli --locked
sudo install -m 755 target/release/poly /usr/local/bin/poly
poly --version

poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
```

显式 `--target ios` **不做发起地宿主过滤**：任务按注册表中 iOS 声明的宿主（macOS）等待构建地；默认的 `--target all` 只取发起地可达子集，跨宿主投递必须显式列目标 id。`--wait` 同时是「等 worker 注册」与「等每个任务结果」的上限，IPA 构建应显著大于单次构建耗时。失败时主端以 `poly::E_FARM_0003` 包装远端错误码与修复提示，退出码 1。

### 3.4 产物与校验

结果回执里的 `artifacts` 是**核心二进制 + `dist/` 文件**的相对路径清单（名称 / blake3 / 字节数，供对账）。打包产物（IPA）落在**构建地**的工程目录，不经协议帧传输：

```bash
cd /srv/proj
unzip -o ".polypack/packages/ios/Pax Notes.ipa" -d /tmp/ipa
codesign --verify --deep --strict --verbose=2 "/tmp/ipa/Payload/Pax Notes.app"
spctl --assess --type execute --verbose=4 "/tmp/ipa/Payload/Pax Notes.app"
```

产物路径与产品名随工程而异；上例使用仓库示例产品 `Pax Notes`。若工程未在两侧同路径挂载，请自行用 rsync / scp 从构建地取回 `.polypack/packages/ios/`。

## 4. worker 池

### 4.1 worker 参数

| 参数 | 含义 | 缺省 |
|---|---|---|
| `--dir <DIR>` | 农场目录（双信箱根；主端与 worker 指向同一目录） | `.polypack-farm` |
| `--id <ID>` | worker 标识；多实例必须唯一（字母 / 数字 / `-` / `_`） | `<os>-<pid>` |
| `--target <ID>` | 声明可构建目标，可重复；缺省 = 本机宿主可达的全部已声明目标 | 本机可达集合 |
| `--jobs <N>` | 本次会话最多接多少个任务；`0` = 不限，直到 `goodbye` 或空闲超时 | `0` |
| `--idle <SECS>` | 注册 / 空闲等待上限；超时正常退出（无任务可做不是失败，但等不到注册帧是 `E_FARM_0002`） | `60` |
| `--cache <PATH>` | `local`（工程本地 CAS）或共享目录（跨 worker / 跨机复用） | `local` |
| `--work-dir <PATH>` | 一次性任务工作目录的父目录；每任务其下一个 0700 私有目录，任务结束即删 | 系统临时目录 |
| `--credential <NAME=env:VAR>` | 凭据档案名 → 环境变量名；可重复；只接受 `env:` 来源 | 无 |

### 4.2 submit 参数

| 参数 | 含义 | 缺省 |
|---|---|---|
| `--config <PATH>` | **构建地**的 `polypack.toml` | `polypack.toml` |
| `--dir <DIR>` | 农场目录（与 worker 一致） | `.polypack-farm` |
| `--target <ID>` | 目标 id，可重复；显式目标不受发起地宿主过滤 | 本机可达全部目标 |
| `--profile <local|ci>` | 构建 profile | `local` |
| `--wait <SECS>` | 等 worker 注册 / 等每个任务结果的上限 | `120` |
| `--retries <N>` | 任务超时 / worker 失联后的重试次数（每次换 worker；0 = 不重试） | `1` |

另有 `--project-dir <PATH>`（构建地工程路径；跨机布局不同时显式给出，相对路径按各 worker 的当前目录解析）与 `--cache <PATH>`（只回显在结果报告里便于对账；**缓存后端由 worker 的 `--cache` 决定**）。

调度语义：主端按目标拆任务，池内每个空闲 worker 取一条自己能构建的目标并发执行；注册准入要求 worker 宿主落在目标声明内且至少声明一个任务目标，逐任务派发时再复核。超时（`E_FARM_0004`）与失联（`E_FARM_0005`）按 `--retries` 换 worker 重试；worker 回传的确定性构建失败不重试，错误码与修复提示完整上抛。

## 5. 隔离

### 5.1 进程边界与一次性工作目录

隔离由两层组成，均由 worker 本体实现：

- **进程边界**：每个任务在一个 worker 进程内执行；主端只能经帧投递任务，无法直接触碰构建地文件系统。并发靠多 worker 进程，进程内一次一个任务。
- **一次性工作目录**：worker 启动时在 `--work-dir`（缺省 `std::env::temp_dir()`）下建 0700 私有工作根；每任务再建 0700 子目录，目录名只含 pid + 进程内序号，**不含任务标识 / 网络路径**，创建用 `create_dir`（预置同名符号链接无法重定向）。任务结束整目录 `remove_dir_all`（不跟随目录内符号链接），worker 退出删除工作根；失败以 `poly::E_FARM_0006` 发声并给出清理提示。

`--work-dir` 应是 worker 用户私有、本机的可写目录（不要指向共享农场目录或共享缓存）。签名临时材料经 `CredentialScope` 重定向到该任务目录，与工作目录同生共死。

### 5.2 容器边界（可选强化层）

仓库**不启动也不管理容器**：容器边界是部署者可选的强化层，不是 `poly` 的内建隔离。需要更强边界时，由部署者把 worker 进程放进自管容器，并自行承担以下约束与验证：

- 只挂载必需的农场目录、缓存目录与工程目录；工具链与基础镜像只读；`--work-dir` 指向容器内 tmpfs / 临时卷。
- 凭据只经进程环境变量注入（镜像、`docker run -e` 的命令行、compose 文件与日志都不得出现明文）；Apple 钥匙串在容器内不可用时改用 §7.3 的 env 内存注入或宿主侧钥匙串。
- 每 worker 一个容器进程，`--id` 唯一；容器的资源限额、网络与重启策略由部署层负责。
- 该层**不在本仓测试覆盖内**；未在部署环境验证前，不要宣称「容器隔离已验证」。

## 6. 缓存后端

### 6.1 共享 CAS 语义

`--cache local`（缺省）用工程本地 CAS：`<工程根>/.polypack/cache`。`--cache <共享目录>` 换成共享目录对象存储，所有 worker 指向同一目录即可跨机复用——缓存键公式不含绝对路径，同一输入在不同构建地产生同一键。

共享对象布局与格式（`crates/poly-cache/src/store.rs`，`DirSharedStore`）：

```text
<root>/<hex 前 2 位>/<hex>            # 对象文件
POLYPACK-FARM-OBJECT v0               # 头行（格式版本）
<blake3(payload) 64 位十六进制>        # 第二行：载荷摘要
<payload 字节>                         # 其余为载荷
```

写入先落到**同目录**的 `.` 前缀临时文件（`.<hex>.tmp-<pid>-<序号>`，不会被当作对象），再 `rename(2)` 原子改名；同名键的多进程并发写不会互相截断。读取校验头行与摘要，不符视为未命中并删除损坏条目。因此共享文件系统必须提供**同目录原子 rename**；NFS 部署前确认导出语义，SMB / 同步盘若破坏 rename 语义则不要用作共享 CAS。

### 6.2 降级

缓存是优化面，不是构建前置。共享目录缺席 / 不可写 / 运行中掉线时，worker 告警一次并降级到工程本地 CAS，构建继续：

- 构造期不可用（未挂载 / 路径被文件占用 / 权限不足）：`共享缓存目录不可用（<路径>），已降级为工程本地缓存（构建继续）: <原因>`（阶段 `cache`）。
- 运行期 IO 失败：`对象存储后端不可用，已降级到后备缓存: <原因>`，写操作继续落本地，保证本机下次仍可命中。

命中统计仍如实累计（任务完成事件里的 `缓存命中 N/M`）。真实云对象存储（S3 / OSS）尚未实现；接入点是实现 `ObjectStore` trait 并在 `CacheBackend` 构造点替换后端。

## 7. 密钥库接入

### 7.1 CredentialRef 边界

`JobSpec.secrets` 只装 `CredentialRef { name, source }`：`name` = `TargetSpec.signing` 的档案名（`apple-codesign` / `android-keystore` / `harmonyos-hap-sign` / `tizen-author` 等），`source` ∈ `keyring` / `env { variable }` / `vault`。明文永不进入农场帧、命令行、计划或日志。当前配置面把目标签名档案映射为 `keyring` 引用；`poly-sign` 的解析层按来源选择后端，worker 侧内存注入优先于后端链。

### 7.2 macOS 钥匙串

Apple 系（`apple-codesign`）按 Apple 推荐形态把材料留在登录钥匙串：

```bash
xcrun notarytool store-credentials apple-codesign --apple-id "$APPLE_ID" --team-id "$TEAM_ID"
# 交互输入 App 专用密码；profile 名 = apple-codesign = TargetSpec.signing
export POLYPACK_IOS_IDENTITY='Apple Distribution: Example Ltd (TEAMID1234)'  # 非机密
export POLYPACK_IOS_TEAM_ID='TEAMID1234'                                      # 非机密
```

这两个环境变量必须在 **worker 进程**的环境里可见。公证链（notarytool / stapler）当前不在 DAG 内（M2 边界）；DAG 内的 `sign` 阶段用 `codesign` 签 `export/Payload/<产品>.app`，identity / team 取自上述非机密变量。

### 7.3 worker 侧 env 内存注入

CI / Vault 场景用 `--credential` 把档案名绑定到环境变量；worker 在**任务开始时**读入内存并经 `CredentialScope` 注入，任务结束随作用域清空：

```bash
# 变量值 = poly-sign 后端载荷，按 secret.rs 契约 base64(JSON)；变量只存在于 worker 环境
export POLYPACK_ANDROID_KEYSTORE_B64='<base64(JSON) 载荷>'   # 占位，绝不在仓库 / 文档写真实值

poly farm worker --dir /srv/polypack-farm --id build-01 --target android \
  --credential android-keystore=env:POLYPACK_ANDROID_KEYSTORE_B64 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

约束与行为（`crates/poly-cli/src/farm.rs`、`crates/poly-sign/src/inject.rs`）：

- 只接受 `<档案名>=env:<变量名>`；参数非法报 `poly::E_FARM_0002`，且**不回显参数原文**（误放明文也不进日志）。
- 值只经环境变量下发：不进 argv、不落盘、不进事件 / 日志（注入告警只列档案名与条数）；任务结束即清空，跨任务零残留。
- 变量缺失不失败：worker 告警「凭据档案 X 的注入变量 VAR 不可用」，签名阶段按凭据不可用 skip-if-missing 处理。
- 载荷契约见 `crates/poly-sign/src/secret.rs`（Android：`keystore` / `store_password` / `key_alias` / `key_password`；HarmonyOS 另有 `app_cert` / `profile`）。Vault 来源需要调用方注入 Vault 客户端；farm CLI 当前未接线，请经 env 注入或先把材料注入钥匙串。

## 8. Linux 双 worker 演练

本机没有 macOS 与跨机农场，验收的 macOS / IPA 腿按环境槽 SKIP-with-print（§9）；真跑替代证据是一套可脚本化的 Linux 双 worker 本地演练：两个真实 worker 进程、`poly farm submit` 多目标并发、共享缓存二跑命中、凭据内存注入与缓存降级。

### 8.1 运行

```bash
cd /srv/poly-src
cargo build -p poly-cli
POLY=target/debug/poly DRILL_OUT=target/farm-drill tools/farm-drill.sh
```

脚本自建静态 UI 夹具（三个独立工程副本，无 node / 网络依赖），输出归档在 `$DRILL_OUT`（`transcript.log` + `logs/`）。逐条命令都带实际路径打印在日志里，可直接复制复跑。

### 8.2 通过判据

演练日志应以以下行结尾（全部为真实执行）：

```text
PASS round 1: two targets completed on two workers
PASS round 2: shared-cache hit 1/1 on a fresh project copy
PASS cache format: POLYPACK-FARM-OBJECT v0
PASS round 3: credential injected from env; zero plaintext on disk/logs
PASS round 4: unavailable shared cache degraded to local cache with a warning
PASS isolation: no task work directories left behind
PASS SKIP printed (environment slot); no macOS claims made
```

- Round 1：`web` 与 `vidaa-tv` 两个任务落在两个不同 worker 上，产物 `dist/index.html` 合并回执。
- Round 2：**全新**工程副本（本地缓存为空）在另一个 worker 上 `缓存命中 1/1`，命中只能来自共享 CAS。
- Round 3：哨兵值只经 `env:` 变量下发，扫描农场目录 / 工作目录 / 工程目录 / 两侧日志零命中。
- Round 4：共享缓存路径不可用，worker 告警并降级到本地缓存，任务仍成功。
- 隔离：worker 退出后无工作根 / 任务目录残留。
- CI 接线建议（本任务未改 workflow）：Linux 构建作业跑完 `cargo build -p poly-cli` 后执行本脚本，把 `$DRILL_OUT` 作为 artifact 上传，即可把演练日志纳入 CI。

## 9. macOS / IPA 环境槽

### 9.1 环境槽状态

**SKIP**：当前执行环境为 Linux，无 macOS 主机、无 Xcode / `codesign` / `notarytool`，因此「从零部署 macOS worker 池并从 Linux 完成一次 IPA 构建（R3）」未在本机真跑。演练脚本会把下述命令与预期输出**打印进日志**（SKIP-with-print），环境槽保持开放；在 macOS 主机上执行后以真实日志替换该槽。

### 9.2 在 macOS 主机上执行

构建地（macOS，按 §3.2 准备好 Xcode / triple / poly / 钥匙串）：

```bash
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

发起地（Linux，工程与农场目录已同路径可见）：

```bash
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
```

通过判据：发起地出现 `worker 已注册：mac-01（aarch64-apple-darwin，宿主 Macos）` 与 `农场任务完成：1 个任务 / 1 个 worker`；构建地 `/srv/proj/.polypack/packages/ios/` 出现 IPA；在 macOS 上按 §3.4 跑 `codesign --verify` / `spctl --assess`。把两侧日志与产物摘要归档后，本环境槽才算关闭。

## 10. 故障排查

| 错误码 | 含义 | 处置 |
|---|---|---|
| `poly::E_FARM_0001` | 协议帧解析失败 / 版本不受支持 / 非预期帧 | 两端升级到同一 `poly` 版本；核对帧序（`hello` → `hello_ack` → `job` → `result` → `goodbye`） |
| `poly::E_FARM_0002` | 等注册超时 / 注册被拒 / worker 标识非法 / `--credential` 参数非法 | 先起 worker 再 submit；核对目标声明的宿主与 worker `--target`；`--id` 只用字母数字 `-` `_`；凭据写成 `NAME=env:VAR` |
| `poly::E_FARM_0003` | worker 执行失败（消息内嵌远端错误码） | 在构建地复现同一错误码；按远端 hint 修复后重投 |
| `poly::E_FARM_0004` | 等任务结果超时 | 确认 worker 存活；提高 `--wait`；增加 worker 或 `--retries` |
| `poly::E_FARM_0005` | worker 失联 / 无可用 worker | 检查农场目录权限与 worker 进程；启动声明对应目标的 worker |
| `poly::E_FARM_0006` | 工作根 / 任务工作目录创建或删除失败 | 检查 `--work-dir` 权限；用 worker 私有可写目录；人工清理 `polypack-farm-work-*` 残留 |

农场目录 IO 失败报 `poly::E_IO_0001`（检查目录存在性与两端权限）。共享缓存告警（§6.2）不是构建失败；若持续出现，先修好挂载 / 权限再看命中率。

## 11. 参考

- [FARM.md](FARM.md)：协议帧、发起地 / 构建地模型、签名验证指引。
- `crates/poly-cli/src/farm.rs`：`worker` / `submit` 参数与调度入口。
- `crates/poly-core/src/farm/`：协议、池调度、一次性工作目录。
- `crates/poly-cache/src/store.rs`：共享目录 CAS 与降级后端。
- `crates/poly-sign/src/secret.rs`、`crates/poly-sign/src/inject.rs`：凭据后端契约与内存注入。
- `tools/farm-drill.sh`：§8 的可执行演练。
