# 0004 — 插件市场分发：去中心化签名索引

- 状态（Status）：accepted
- 日期（Date）：2026-10-20
- 决策者（Deciders）：项目维护者

> **English abstract**: Third-party plugins are distributed through a decentralized, signed index rather than an official registry at GA. The index and its append-only revocation document are canonical JSON signed with detached GPG signatures; clients verify both through `poly-sign` (`ArtifactVerifier` + `CredentialRef` trust roots). Every indexed artifact must carry a publisher signature and a BLAKE3 SRI. Maintainers sign the index, a separately held revocation key owns revocation, and mirrors are untrusted replicas. No index or CLI code is added by this ADR.

## 背景（Context）

[ADR 0003](0003-wasm-plugins.md) 已确定 WASM 插件沙箱与 `target-spec/v1`，但插件目前只以文件形式分发：构建 `dist/plugin.wasm`、复制到应用目录、在 `polypack.toml` 的 `[[plugins]]` 中写 `path`，签名可选。`poly plugin digest` 能算 SRI，`poly plugin check` 能查脚手架，却没有索引、版本解析、来源展示与吊销机制。用户无法回答「有哪些插件、哪个版本可信、是否已被撤回」。

ROADMAP §14 把分发形态列为开放问题；INITIATIVES M9-D1 的触发条件是第三方插件 ≥ 3 个或出现第一个「如何发现 / 安装插件」的外部请求。本 ADR 先定治理与可验证格式，不等待触发条件，也不新增实现代码。约束来自既有决策：

- 不做付费市场、不做托管插件执行（INITIATIVES M9-D1 非目标）；
- 不改变 SRI 与 GPG 验签信任链（ADR 0003、GA §3.2）；
- 凭据只以 `CredentialRef` 引用出现，私钥与口令永不进入客户端路径（ROADMAP §7、[`crates/poly-sign/src/artifact.rs`](../../crates/poly-sign/src/artifact.rs)）；
- 插件能力仍由 ADR 0003 的沙箱按插件显式授权，索引不能扩大能力。

## 决策（Decision）

采用**去中心化签名索引**：索引是静态文件，可由任意静态主机、Git 仓库或对象存储镜像；客户端只信任签名与固定信任根，不信任托管方。官方策展注册表不在 GA 范围；若未来提供，只作为可选镜像，不获得额外信任或吊销权。

本 ADR 只记录契约与治理，不新增索引、CLI、TargetSpec 或 SDK 代码。实现任务为 INITIATIVES 的 T-M9-41（候选），官方镜像为 T-M9-42（条件任务）。

### 1. 分发单元与验证链

一个索引由两个独立签名的文档组成：

| 文档 | 作用 | 签名密钥 | 一致性 |
|---|---|---|---|
| `index.json` + `index.json.asc` | 发现、版本与发布者元数据 | 索引签名密钥 | `sequence` 单调递增 + `previous_digest` 链 |
| `revocations.json` + `revocations.json.asc` | 只追加的吊销快照 | 吊销密钥（与索引密钥分离） | `sequence` 单调递增，客户端保留最高值 |

`poly plugin add` 的验证顺序固定，任一步失败即拒绝安装，不做部分信任：

1. 索引分离签名（`poly-sign` 的 `ArtifactVerifier`，信任根来自 `CredentialRef`）；
2. 索引 `schema`、`sequence`、`previous_digest` 与 `expires_at`（防回滚、防分叉、防过期）；
3. 吊销快照验签与版本 / 密钥匹配（吊销优先于安装）；
4. 制品 SRI（`blake3-<hex>`）与字节数；
5. 发布者分离签名，且签名公钥指纹必须等于索引条目声明的 `publisher.fingerprint`；
6. `wit_version` 必须为宿主支持的 `target-spec/v1`；
7. 交给 ADR 0003 的沙箱实例化，能力授权仍按应用配置执行。

本地 `path` 安装维持 ADR 0003 的「签名可选 + 缺失时显式告警」；**经索引安装的第三方插件必须同时具备索引签名与发布者签名**。索引签名证明「这条 listing 由项目收录」，发布者签名证明「制品由该发布者签发」；两者不可互相替代。

### 2. `index.json` 顶层字段

| 字段 | 类型 | 语义 |
|---|---|---|
| `schema` | string | 固定 `polypack-plugin-index/v1`；破坏性变更发新版本 |
| `sequence` | integer | 单调递增；客户端持久化已见最高值 |
| `generated_at` | string | RFC 3339 UTC |
| `expires_at` | string | RFC 3339 UTC；有效期上限 30 天 |
| `previous_digest` | string / null | 上一版索引规范 JSON 的 `blake3-<hex>`；首个版本为 `null` |
| `signing_key_id` | string | 索引签名密钥的 GPG 指纹（40 hex） |
| `plugins` | array | 插件条目，见下表 |
| `revocations_digest` | string | 配套 `revocations.json` 的 `blake3-<hex>`，防止错配旧吊销快照 |

### 3. 插件条目字段（`plugins[]`）

| 字段 | 类型 | 语义 |
|---|---|---|
| `id` | string | 稳定标识；第三方使用小写反向域名（如 `dev.example.pocket-notes`），官方插件可用 `dev.polypack.*` |
| `name` / `summary` | string | 仅供展示；不参与安全判定 |
| `version` | string | SemVer 2.0.0 |
| `wit_version` | string | 当前只接受 `target-spec/v1` |
| `min_host_version` | string | 兼容的最低 PolyPack 版本 |
| `published_at` | string | RFC 3339 UTC |
| `artifact.url` | string | `https://` 或 `file:`；URL 本身不受信任 |
| `artifact.sri` | string | `blake3-<64 hex>`，与宿主加载同口径 |
| `artifact.size` | integer | 字节数，用于预检与限额 |
| `publisher.key_id` | string | GPG 指纹（40 hex） |
| `publisher.display_name` | string | 展示名，不参与安全判定 |
| `signature.url` | string | 发布者对制品的分离签名（ASCII armor） |
| `status` | string | `active` / `yanked`；`yanked` 保留历史但不参与新安装解析 |
| `yank_reason` | string? | `status = yanked` 时必填 |
| `metadata` | object? | 可选展示字段（主页、许可证、关键词）；schema 固定，未知键拒绝 |

### 4. `revocations.json` 字段

| 字段 | 类型 | 语义 |
|---|---|---|
| `schema` | string | 固定 `polypack-plugin-revocations/v1` |
| `sequence` | integer | 单调递增；客户端保留最高值 |
| `issued_at` | string | RFC 3339 UTC |
| `revocations` | array | 吊销条目，见下 |

吊销条目按 `kind` 区分，命中任一条件即拒绝安装：

| `kind` | 匹配字段 | 含义 |
|---|---|---|
| `version` | `id` + `version` | 撤回单个版本；`replacement` 可指向修复版本 |
| `plugin` | `id` | 撤回该插件全部版本 |
| `key` | `fingerprint` | 撤回发布者密钥；该密钥签发的全部条目失效 |

条目字段：`kind`、`id`（`key` 类型可空）、`version`（仅 `version` 类型）、`fingerprint`（仅 `key` 类型）、`reason`（`compromised` / `malicious` / `withdrawn` / `superseded`）、`revoked_at`、`replacement`（可选）。吊销记录只追加，不提供「解除吊销」；纠正错误只能发布新版本并在 `replacement` 中说明。

### 5. 规范化与签名算法

- 两个文档均使用 RFC 8785 JSON Canonicalization Scheme（JCS）序列化为 UTF-8，无多余空白；签名对象是规范化字节，不是文件原始排版。
- 签名使用 GPG detached signature（ASCII armor）；摘要算法由 GPG 默认的安全摘要决定，客户端通过 `poly-sign` 的 `gpg --verify` 验证。
- 客户端不接触私钥。索引与吊销签名在离线发布步骤完成，密钥不进入 CI 的普通作业；发布 CI 只上传已签名的静态文件。
- `poly-sign` 的复用点：`ArtifactVerifier::from_reference` 从 `CredentialRef`（keyring / Vault / env / 内存注入）解析公钥，`verify_detached` 校验字节与签名。索引与制品使用同一验证器，不引入第二套密码学实现。

### 6. 信任根

信任根是两把固定的 GPG 公钥，分别用于索引签名与吊销，二者必须由不同维护者持有：

- 公钥指纹在首次生成密钥时登记到仓库信任根文件与 release notes，并以 `CredentialRef` 形式随客户端分发；在指纹登记前，索引功能保持关闭（fail closed）。信任根文件随 T-M9-41 落地，本 ADR 不创建该文件。
- 索引密钥由发布维护者离线持有；吊销密钥由安全维护者离线持有，并留有可审计的备份与恢复流程。
- 密钥轮换采用「旧钥 + 新钥双签同一版索引」的过渡方式，至少保留一个完整索引周期；轮换记录进入 release notes，并按 ADR 0001 双语披露。
- 客户端只接受显式配置的信任根。私有索引使用自己的信任根，不得自动继承公共信任根；配置项形态在实现 ADR / 任务中细化，但「显式信任」是本 ADR 的约束。

### 7. 吊销权与治理

- **吊销权属于 PolyPack 维护者团队**；日常由安全维护者用吊销密钥签发，索引维护者不能单独吊销，也不能删除吊销记录。
- 吊销必须附公开事件说明（受影响 `id` / `version` / 指纹、原因、替代版本、时间）；紧急吊销可先发布签名快照，再在 72 小时内补事件说明。
- 官方策展镜像只能选择镜像哪些条目，不能修改条目、签发索引或吊销；镜像与主索引不一致时，客户端以签名和 `sequence` 为准。
- 镜像分叉（同一 `sequence` 不同 `previous_digest` / 内容）视为攻击信号：拒绝该索引、保留双方摘要并提示用户切换镜像。
- 客户端吊销缓存 TTL 为 7 天；超过 30 天未刷新时拒绝新安装，已安装插件仍可离线构建但输出告警。实现可提供显式 `--allow-stale` 逃生阀，但必须打印风险。

### 8. 可用性与回滚

- 客户端可配置多个索引 URL，按顺序尝试；只有通过完整验签与新鲜度检查的索引才进入候选。
- 已缓存索引在 `expires_at` 前可作为镜像全部不可达时的回退；过期索引只允许查询已安装插件的元数据，不允许新安装。
- `sequence` 低于本地最高值、或同 `sequence` 内容不一致时拒绝；不提供静默降级。
- 搜索在本地对索引做过滤与排序（v1 不做服务端排名）；版本解析选择满足约束的最新 `active` 且未被吊销版本。
- 建议在实现时预留错误码 `E_PLUGIN_0011`（索引格式 / 签名 / 回滚）、`E_PLUGIN_0012`（索引过期 / 分叉）、`E_PLUGIN_0013`（已吊销）；本 ADR 不写代码，只固定语义。

### 9. 威胁模型

| 威胁 | 缓解 | 残余风险 |
|---|---|---|
| 索引签名密钥泄漏，攻击者发布恶意 listing | 离线密钥、独立吊销密钥、`sequence` 链与镜像分叉检测、双签轮换 | 吊销密钥也失陷时只能靠信任根轮换；需事件响应 |
| 镜像篡改或替换制品 | 索引签名 + 条目 SRI + 发布者签名；URL 不受信任 | 发布者密钥失陷仍需吊销 |
| 发布者密钥失陷 | `kind = key` 吊销；客户端 7 天 TTL 刷新 | 吊销窗口内的安装可能受影响 |
| 回滚到旧索引 / 旧版本 | `sequence` + `previous_digest` + `expires_at`；拒绝降级 | 完全离线且缓存过期时无法新装 |
| 分叉视图（不同镜像给不同索引） | 同 `sequence` 内容不一致即拒绝并告警 | 用户可能只看到一个镜像；需多镜像配置 |
| 仿冒 / 近似插件名 | 反向域名 `id`、展示发布者指纹、`display_name` 不参与安全判定 | 社会工程仍可能成功；文档需强调核对指纹 |
| 恶意插件逻辑 | 沿用 ADR 0003：默认零能力、显式授权、fuel / epoch / 内存上限 | 授权能力范围内的行为仍需审计 |
| 索引泄露用户查询 | 索引为静态文件；客户端不向索引方发送查询 | 镜像可见客户端 IP；可自建镜像 |
| 供应链构建被篡改 | 索引构建可复现、CI 只发布已签名文件、发布者签名覆盖制品 | 发布者本地构建环境仍属其责任 |

### 10. 规范边界

- 本 ADR 不实现 `poly plugin search/add`、不新增索引文件、不修改 `crates/poly-plugin` 或 `crates/poly-cli`。
- 不新增 TargetSpec；插件仍是 `target-spec/v1` 的 WASM 组件。
- 不改变 [ADR 0003](0003-wasm-plugins.md) 的沙箱、SRI、签名加载顺序与错误码语义，只在其上增加索引层的治理规则。
- 官方注册表 / 镜像若启动，必须另立 ADR 说明运维、可用性与治理；本 ADR 不承诺该服务。

## 备选方案（Alternatives）

| 方案 | 未采用的原因 |
|---|---|
| 官方集中注册表 | 需要持续运维、滥用处理与可用性承诺；与「不托管」边界和当前维护规模不符；签名索引已能提供来源与吊销 |
| Git 仓库索引，无签名语义 | 仓库权限不等于发布者身份；无法离线验证来源，也无法表达吊销 |
| OCI registry | 生态与认证链路重；客户端需引入额外 registry 语义，收益不足以抵消复杂度 |
| 维持文件分发（现状） | 无发现、无版本解析、无吊销与来源展示；第三方插件增长后不可治理 |
| 只做官方策展列表（无签名） | 托管方可以静默替换 URL 或条目；无法满足供应链可验证性 |

## 影响（Consequences）

- 项目不需要为 GA 运行注册表；镜像可由社区或官方以静态文件方式提供，成本低。
- 维护者承担两把离线密钥的生成、备份、轮换与事件响应责任；密钥管理成为发布流程的一部分。
- 客户端必须实现规范化 JSON、序列 / 过期校验、多镜像回退与吊销缓存；实现复杂度高于「下载一个 wasm」。
- 插件发现是本地过滤，没有服务端排名与推荐；索引体积增长后需要分页 / 分片，属于后续实现决策。
- 发布者必须持有 GPG 密钥并签名制品；比当前「路径 + 可选签名」门槛高，但这是第三方市场可追溯的最低要求。
- 吊销不能「撤销」，错误吊销只能靠新版本与说明修复；治理流程必须克制。

## 参考（References）

- [INITIATIVES](../INITIATIVES.md) §4 M9-D1（[English](../INITIATIVES.en.md)）
- [ADR 0003：WASM 插件沙箱与 `target-spec/v1` WIT](0003-wasm-plugins.md)
- [ROADMAP](../ROADMAP.md) §7（安全与密钥）、§14（开放问题，[English](../ROADMAP.en.md)）
- [GA](../GA.md) §3.2（制品加载顺序）、§3.4（`E_PLUGIN_0001`–`0010`）
- [`crates/poly-sign/src/artifact.rs`](../../crates/poly-sign/src/artifact.rs)（`ArtifactVerifier` 与 `CredentialRef` 信任根）
- [`crates/poly-plugin`](../../crates/poly-plugin/)（沙箱与 WIT 契约）
- RFC 8785（JSON Canonicalization Scheme）

## 实现后记（T-M9-41，2026-10）

本节只记录落地差异与限制，不改变上文决策。

- **代码位置**：`crates/poly-cli/src/plugin/index/{schema,rules,verify,select,mod}.rs`；`poly plugin add` / `poly plugin search` 复用 `poly-sign` 的 `ArtifactVerifier` / `CredentialRef`，不新增密码学实现。
- **命令行**：`--index`（文件或目录，目录读 `index.json`）、`--index-signature`、`--revocations`、`--revocations-signature`、`--trust-root`、`--revocations-trust-root`、`--state`（默认 `.polypack/plugin-index`）；`add` 另需 `--publisher-key`。省略签名/吊销路径时按同目录 `.asc` / `revocations.json` 推导。
- **默认离线**：`add` 只解析 `file:` 制品与签名 URL，`https://` 条目会以 `E_PLUGIN_0011` 拒绝并提示先手动下载；未实现镜像下载、多索引回退或网络探测。`search` 完全不触网。
- **规范化**：签名对象是 `poly-publish` 既有 `metadata::canonical_json` 的紧凑 UTF-8 字节；该 schema 只有整数与字符串，输出与 RFC 8785 JCS 一致。索引/吊销文件无需按规范化排版落盘；客户端解析后重新规范化再验签，不依赖磁盘排版。
- **验证顺序**：索引分离签名与 `signing_key_id` 指纹绑定 → schema / sequence / `previous_digest` / `expires_at` → 吊销快照分离签名 → 吊销密钥必须与索引密钥不同 → 吊销 schema / `revocations_digest` 绑定 / sequence → 新鲜度 → 条目选择 → 制品大小与 `blake3` SRI → 发布者分离签名与指纹。任一步失败即拒绝，`add` 不执行插件。
- **吊销快照为必需输入（fail closed）**；超过 30 天未刷新拒绝新安装，与 §7 一致。尚未实现 §7 的 7 天本地缓存 TTL 与 `--allow-stale`，当前过期索引在 `search` 阶段同样拒绝（比「允许查询已安装元数据」更保守）。
- **回滚基线**：`<state>/state.json` 记录索引与吊销的 `sequence` + 规范化摘要；同 sequence 不同摘要 → `E_PLUGIN_0012`，低 sequence → `E_PLUGIN_0011`，紧邻上一版还会校验 `previous_digest`。基线在 `add` 安装成功后或 `search` 验证通过后推进（已见即记录，搜索本身也会失败关闭）。跨版本跳号依赖单调 sequence，不做逐版链验证。
- **错误码**：`E_PLUGIN_0011` 索引/吊销格式、签名、回滚；`E_PLUGIN_0012` 过期、镜像分叉；`E_PLUGIN_0013` 版本/插件/发布者密钥吊销与 yanked；制品 SRI/大小沿用 `E_PLUGIN_0006`，发布者签名沿用 `E_PLUGIN_0009`，信任根/工具沿用 `E_PLUGIN_0010`。
- **生产信任根仍为外部事项**：仓库未生成、未内置任何生产索引/吊销公钥，也未执行密钥仪式；测试使用一次性 GPG 密钥。首次登记指纹前，索引功能不默认启用。
- **发现语义**：`search` 只返回 `active`、未被吊销、`wit_version` 受支持且满足 `min_host_version` 的条目，按 id 保留最新 SemVer；查询为 id / name / summary / keywords 的本地子串匹配。
