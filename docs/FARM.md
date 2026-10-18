# 远程农场：发起地与构建地

> 配套阅读：[ROADMAP.md](ROADMAP.md) §7（安全与密钥）、§8（测试与 CI 矩阵）、§9 M2（风险 R3：Apple 系目标必须 macOS 宿主）。
> 部署指南：[FARM-DEPLOY.md](FARM-DEPLOY.md)（拓扑 / worker 池 / 隔离 / 缓存 / 凭据 / 演练）。
> English: [FARM.en.md](FARM.en.md)

## 为什么需要农场

Apple 系目标（iOS / macOS / tvOS）只能在 macOS 宿主上真出产物：`xcodebuild`、`lipo`、`codesign` 不随 Linux / Windows 提供。Windows 与 Linux 用户因此需要一个**构建地**：由一台 macOS 机器代跑打包链，产物回传本地。协议 v0（T-M2-13）把这件事拆成两个角色，并让两侧共用同一套 DAG 执行实现与同一缓存键公式——本地构建与远程构建不产生第二套语义。

## 发起地 / 构建地

| 角色 | 职责 | 需要什么 |
|---|---|---|
| **发起地**（origin） | 声明要构建什么（`polypack.toml` + `--target`），投递任务，回收产物清单 | `poly` 本体、工程源码、与构建地共享的农场目录；**不需要**目标平台工具链 |
| **构建地**（build site） | 执行任务：编译、打包、签名材料按需拉取 | `poly` 本体、平台工具链（Xcode / Android SDK 等）、凭据后端；**不需要**开发者本地的仓库状态 |

一句话边界：**发起地决定「构建什么」，构建地决定「怎么构建」并独占凭据**。凭据永远不跨这条边界传输（见下文「凭据模型」）。

## 协议 v0

线格式为 JSON Lines（每帧一行 UTF-8 JSON），版本串 `farm/v0`，两端版本不符即 `poly::E_FARM_0001`。

| 帧 | 方向 | 载荷 |
|---|---|---|
| `hello` | worker → 主端 | `protocol` + `worker`：`WorkerCaps { worker_id, triple, host, targets }` |
| `hello_ack` | 主端 → worker | `protocol` + `accepted` + `reason`（拒绝时给出原因） |
| `job` | 主端 → worker | `job_id` + `project_dir`（构建地路径）+ `spec`（`JobSpec` 序列化形态） |
| `result` | worker → 主端 | `job_id` + `ok` + `error`（`RemoteDiagnostic`：错误码 + 描述 + 修复提示）+ `artifacts`（`ArtifactRecord`：名称 / blake3 摘要 / 字节数） |
| `goodbye` | 主端 → worker | 结束会话（worker 据此退出拉取循环） |

帧序固定：`hello` → `hello_ack` → `job` → `result` → `goodbye`。`JobSpec` 含 `id / profile / targets / excluded / secrets / budget`——`secrets` 只装引用（见「凭据模型」）。

v0 传输为**共享目录双信箱**（`DirTransport`）：主端写 `<目录>/to-worker`、收 `<目录>/to-master`；worker 反之。帧文件名 `<tag>-<8 位序号>`（定宽 → 字典序即发送序），写入经临时文件原子改名，`.` 前缀文件不参与收件。HTTP / SSH 等远端后端在 M3+ 接入同一 `FarmTransport` trait——协议与传输解耦，届时命令行不变。

## 部署与 worker 注册

1. **准备共享目录**：一个发起地与构建地都能访问的目录（NFS / SMB / rsync 同步 / 同机同路径均可），例如 `/srv/polypack-farm`。协议只传路径与 `JobSpec`，帧本身是普通文件。
2. **工程可见性**：`job` 帧携带的 `project_dir` 由构建地本地解析，worker 会从该目录读 `polypack.toml` 并执行流水线。v0 因此要求**工程在两侧以同一路径可见**（共享挂载，或两侧同路径同步）；跨机自动同步工程不在 v0 范围内。
3. **启动构建地**：在 macOS 机器上装好 Xcode 与交叉编译 target（`rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`），然后注册并服务：

```bash
poly farm worker --dir /srv/polypack-farm --id mac-mini-1 --target ios --jobs 1 --idle 60
```

`--target` 声明本 worker 能构建的目标（可重复；缺省 = 按本机宿主可达的全部已声明目标）；`--jobs` 是处理多少个任务后退出，`--idle` 是空闲等待上限（超时即正常退出——无任务可做不是失败）。

4. **发起投递**：在 Linux 上指向同一目录投递：

```bash
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml --target ios --wait 600
```

显式 `--target <id>` 不做发起地可达性过滤——按注册表中各目标声明的宿主解析，因此 Linux 上也能投递 `ios` 去等一台 macOS worker；缺省 `--target all` 与 `poly build` 一致，只取发起地可达的子集（跨宿主投递请显式列出目标 id）。主端先等 `hello`，按任务目标的**声明宿主**做注册准入（宿主不在目标任务声明的宿主内、或 worker 未声明可构建全部任务目标 → `hello_ack accepted=false` + `poly::E_FARM_0002`）；准入通过后投递 `job` 并等 `result`（超时 → `poly::E_FARM_0003`）。worker 端失败不吞错：远端错误码与修复提示经 `RemoteDiagnostic` 完整回传，主端以 `poly::E_FARM_0003` 包装上抛并退出码 1。

5. **跨机缓存**：`--cache local` 用工程本地 CAS（`<工程根>/.polypack/cache`，默认）；`--cache <共享目录>` 换成共享对象存储后端。缓存键公式不含绝对路径（ROADMAP §5.2），因此同一工程在两个构建地之间可直接命中；命中情况在任务完成事件里如实报告（`缓存命中 N/M`）。

## 凭据模型（只在构建地按需拉取）

- `JobSpec.secrets` 里只有 `CredentialRef { name, source }`：`name` = 目标声明的签名档案（`apple-codesign` / `android-keystore`），`source` ∈ `keyring`（系统密钥库）/ `vault` / `env { variable }`（CI 注入的环境变量名）。
- **明文永不进入协议帧、命令行、计划或日志**：worker 在执行时按引用向本机后端取值。Android keystore 口令只以 `env:POLYPACK_KS_PASS` / `env:POLYPACK_KEY_PASS` 形式引用；iOS 公证口令只以 `env:POLYPACK_APPLE_APP_PASSWORD` 形式引用，签名身份留在构建地的登录钥匙串（Apple 推荐的 `xcrun notarytool store-credentials <档案名>` 档案形态）。
- 仓库内出现明文凭据会让 `poly scan`（secret-scan，提交与构建双重检查）失败；CI 侧另有独立的 secret-scan 作业。

## 端到端示例：Linux 发起 → macOS worker 出 IPA

前置：macOS worker 已装 Xcode 与三个 Apple triple；工程位于两侧同路径 `/srv/proj`；共享目录 `/srv/polypack-farm`。

```bash
# 构建地（macOS）：注册为 worker，处理 1 个任务后退出
poly farm worker --dir /srv/polypack-farm --id mac-mini-1 --target ios --jobs 1 --idle 60

# 发起地（Linux）：投递 ios 目标，等结果
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml --target ios --wait 600
```

预期输出（发起地侧，`--json` 时 stdout 为单行 JSON）：

```
worker 已注册：mac-mini-1（aarch64-apple-darwin，宿主 Macos）
任务 <job-id> 完成：1 个产物
  .polypack/packages/ios/Pax Notes.ipa（1234567 字节，<blake3>）
```

产物落在构建地的工程目录（`.polypack/packages/ios/`），经共享目录取回发起地后按下一节验证。若 worker 端失败，主端输出形如 `[poly::E_FARM_0003] worker 执行失败：[<远端错误码>] <描述>` 并带上修复提示——在构建地复现即可看到同一错误码。

## 签名验证指引

| 平台 | 命令 | 判定依据 |
|---|---|---|
| iOS（应用包） | `codesign --verify --deep --strict --verbose=2 "Payload/Pax Notes.app"` | 通过时输出 `valid on disk` 与 `satisfies its Designated Requirement`；失败信息给出具体失效原因 |
| iOS（分发评估） | `spctl --assess --type execute --verbose=4 "Payload/Pax Notes.app"`（`.pkg` / `.dmg` 用 `--type install`） | `accepted`（未公证或未签名则 `rejected`，`source=` 指明来源） |
| iOS（公证票据） | `xcrun stapler validate <产物>` | 通过时 `The validate action worked!`；未装订提示重新 `stapler staple` |
| iOS（IPA） | `unzip -o app.ipa -d /tmp/ipa` 后对 `Payload/*.app` 跑上面前两条 | IPA 是 zip 容器，验证对象是里面的 `.app` |
| Android（APK） | `apksigner verify --print-certs --verbose app-release.apk` | `Verifies` + `Signer #1 certificate DN:`（poly-sign 的验签证据即取这两类行） |
| Android（AAB） | `jarsigner -verify -verbose -certs app-release.aab` + `bundletool validate --bundle=app-release.aab` | AAB 是签名 JAR（jarsigner 验签），bundletool 校验包结构 |

上面的命令与目标注册表声明的校验器一一对应：iOS 为 `codesign-verify` / `spctl`，Android 为 `apksigner-verify` / `bundletool-validate`（金样见 `crates/poly-cli/tests/fixtures/m2/*.artifact.json`）。移动目标的契约测试在 [`.github/workflows/mobile-contract.yml`](../.github/workflows/mobile-contract.yml)：每提交跑 mock、每晚 `POLY_CONTRACT_REAL=1` 跑真实链路并留产物（ROADMAP §8 两档）。

## 已知限制与排期

- 传输只有共享目录双信箱（HTTP / SSH 在 M3+ 接入同一 trait）；协议 v0 假定工程已在构建地同路径可见。
- 签名链（`poly-sign`：codesign / notarytool / stapler、apksigner）尚未接入 package 阶段——当前农场链路产出的是**未签名**产物（iOS 归档阶段 `CODE_SIGNING_ALLOWED=NO`），签名与公证随 T-M2-10 接线；在此之前请把上表当作产物核验清单。
- 农场 v1（部署指南 + 并发 / 隔离 / 缓存共享）在 M8（ROADMAP §9）。
