# GUI 构建历史矩阵（M9-E6 · T-M9-18）

> 状态：T-M9-18 已交付（Tauri v2 + 静态前端）；真实 Tauri 窗口启动未在本环境执行，按环境门控披露。
> 数据源：`.polypack/history/events.jsonl`（与 `poly history` 共用 `HistoryRecord` schema）。

## 1. 是什么 / 不是什么

| 是 | 不是 |
| --- | --- |
| 只读桌面视图：目标矩阵 + 单目标最近构建详情 + 空状态引导 | 不是新的数据源：不重写、不删除、不轮转历史文件 |
| Tauri v2 command `history_records` 读取工程本地 JSONL，并在读取时再次脱敏路径 | 不是多用户服务端、云同步或远程农场调度 UI |
| 单工程根目录、单机查看，适合构建后复盘 | 不是安装器：`bundle.active=false`，不生成 deb / MSI / AppImage |

## 2. 数据来源与隐私

`poly build` 在工程根写入 `.polypack/history/events.jsonl`，一行一条 `HistoryRecord`（`schema_version`、`build_id`、`ts`、`target`、`event`、`status`、`duration_ms`、可选 `artifact` / `artifact_digest` / `error_code`）。GUI 通过 `poly_core::events::HistoryRecord` 反序列化，字段定义与 CLI 同源，不复制 schema。

路径处理有两道：CLI 落盘前把工程内路径转为相对路径、工程外绝对路径与 `..` 越界路径写 `<redacted>`；GUI 读取时对每条 `artifact` 再调用一次 `poly_core::events::redact_path`，防止手工编辑或旧版本记录绕过。

历史文件缺失视为空历史并显示引导；无法解析的行被跳过，界面显示跳过条数。

## 3. 构建与运行

### 3.1 前置依赖

Linux 需要 GTK3 与 WebKitGTK 4.1 开发包（Debian/Ubuntu：`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev`）；Windows 使用 WebView2；macOS 使用系统 WKWebView。`app/src-tauri` 是独立 Cargo workspace，不参与根 workspace 的 `crates/*` 构建。

### 3.2 运行

```bash
# 1) 在任意含 polypack.toml 的工程根目录生成历史
poly build

# 2) 从该工程根目录启动 GUI（保持当前目录 = 工程根）
cargo run --manifest-path /path/to/polypack/app/src-tauri/Cargo.toml
```

界面中的 `Project root` 留空即使用进程当前目录，也可以填绝对路径；`Target` 留空显示全部目标；`Per target` 控制每个目标返回的最近记录数（1–200，默认 20）。点击矩阵行或目标名进入详情，`Back to matrix` 返回。CLI 等价查询：`poly history [--target ID] [--limit N] [--json]`。

### 3.3 无 Tauri 环境时

若缺少 WebKitGTK / GTK 开发包，`cargo check` 会失败。此时可只读查看 `app/web/index.html` 的结构，但页面不会加载数据；本交付不提供 `history.json` 静态回退面板，因为 Tauri 构建在本仓库目标环境可用。需要纯静态面板时，可执行 `poly history --json > history.json` 并自行托管，字段与本 GUI 一致。

## 4. 视图快照

以下为文本示意，不是截图；数据取自单元测试夹具同形数据。真实窗口未在本环境启动（见 §5）。

### 4.1 矩阵

```text
TARGET   LATEST STATUS   TIME (UTC)             DURATION   ARTIFACT                                   ERROR CODE
linux    success         2026-10-20T10:42:11Z   1.25s      .polypack/packages/linux/app.AppImage       -
web      warning         2026-10-20T10:41:03Z   842ms      .polypack/packages/web/app.zip             -
tizen    failed          2026-10-20T10:39:55Z   2.10s      <redacted>                                 poly::E_COMPILE_0001
```

### 4.2 详情

```text
← Back to matrix     linux · 2 most recent record(s) · latest 2026-10-20T10:42:11Z
BUILD                  STATUS    TIME (UTC)             DURATION   ARTIFACT                              DIGEST        ERROR CODE
build-1750416131000    success   2026-10-20T10:42:11Z   1.25s      .polypack/packages/linux/app.AppImage  sha256:abc    -
build-1750416073000    failed    2026-10-20T10:41:13Z   980ms      <redacted>                             -             poly::E_COMPILE_0001
```

### 4.3 空状态

```text
No build history yet
Run a build in the project root, then refresh this view.
1. Run poly build in the project root.
2. History is appended to .polypack/history/events.jsonl.
3. Click Refresh; clear the target filter if one is set.
CLI equivalent: poly history --json
```

## 5. 测试与验证

- `cargo test --manifest-path app/src-tauri/Cargo.toml`：空历史、多目标聚合、每目标最新 N 条、读取时路径脱敏、损坏行跳过，以及前端矩阵 / 详情 / 空状态标记的静态结构检查；本环境 6 项全部通过。
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings`：本环境通过。
- 真实 Tauri 窗口启动属环境门控：需要可用的图形会话与 WebKitGTK 运行时；本交付未在维护者桌面弹出窗口，也不声称已执行 GUI 端到端点击测试。

## 6. 已知限制

- 只读、手动刷新：不监听文件变化，不自动刷新；没有分页或全文搜索。
- 一次一个工程根目录；根路径为手输，无原生目录选择器。
- 读取整个历史文件（CLI 已限制为 5 MiB / 10000 条）；无法解析的行只显示计数，不显示明细。
- `bundle.active=false`：不产出安装包；Windows / macOS 真机窗口未在本环境验证。
- 历史时间按 UTC 显示；界面不提供时区转换。
