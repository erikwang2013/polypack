# Wear 表单冒烟套件（T-M7-03/04）

`wearos` 目标的 §8 表单用例集：**输入（input）→ 滑动（swipe）→ 返回（back）**。用 adb
注入文本 / 滑动手势 / BACK 键，以 uiautomator dump 的可见文本为观测通道；`result.json`
（schema `polypack/wear-smoke/v1`）逐 case / 逐 step 记录 `expect` / `observed` / `dump`，
失败定位到**具体表单步骤**（例：`[swipe/swipe-to-note] 期望 step: note；实际 step: amount（dump: …）`）。

T-M7-04 补齐**有设备冒烟生命周期与归档**：安装（`adb install -r`）→ 启动（每 case
force-stop + monkey 冷启动）→ 表单用例 → 卸载（`finally`，失败也执行，设备不残留被测包）；
`result.json` 含 `startedAt` / `finishedAt` 时间戳与 `build`（源码 `commit` / `dirty`、
APK `bytes` / `sha256`、node / platform），失败记录额外带自描述的 `failure.locate`；
同目录归档 `logcat.txt`、逐 case 终态截图 `<case>.png`、失败步截图 `<case>-<step>.png`、
逐 step `uiautomator` dump `<case>-<step>.xml`。发版门见 `.github/workflows/wear-release-smoke.yml`。

本套件只覆盖**真机/模拟器腿**；APK 产物与 `bundletool validate` 由 Rust 侧
`crates/poly-bundle/tests/packagers_android_wear.rs` 真跑（本机无 bundletool 时该腿
SKIP-with-print）。

## 被测应用契约（表单锚点）

APK 由仓库外预构建（RN Wear 宿主，同 TV 冒烟的预构建纪律：托管 runner 装不动整条 RN
gradle 链）。宿主须实现以下可观测契约，用例只依赖它：

| 锚点 | 含义 |
|---|---|
| `Wear Form` | 表单标题（每个用例冷启动后首个断言） |
| `step: amount` / `step: note` | 当前表单页状态行 |
| `amount: <n>` | 金额回显（输入后 / 滑动后 / 返回后都须保留） |
| 金额输入框启动即聚焦 | 免按坐标点按，圆屏 / 方屏同一用例 |
| 内容区左滑 | `amount` 页 → `note` 页 |
| 系统 BACK（KEYCODE_BACK） | `note` 页 → `amount` 页，金额保留 |

锚点唯一来源：`forms.mjs` 的 `FORM_ANCHORS`；宿主改文案须同步该文件（mock 腿钉住用例形态）。

## 用法

```sh
# mock 腿（每提交 / PR，零 npm install）：用例集 + adb 映射 + 定位格式
node --test "ui/smoke/wear/test/*.test.js"    # 或 cd ui/smoke/wear && npm test

# 真跑腿（Wear 模拟器 + 预构建 APK 同时在场才执行；任一缺席 SKIP-with-print）
node ui/smoke/wear/run.mjs --leg android \
  --apk "$POLYPACK_WEAR_FORM_APK" --package "${POLYPACK_WEAR_FORM_PACKAGE:-com.polypack.weatform}" \
  [--serial emulator-5554] [--logs target/wear-smoke-logs]
```

缺省 `--logs $POLYPACK_WEAR_SMOKE_LOGS`（否则 `target/wear-smoke-logs`）。退出码：任一
case / 卸载 fail → 1；SKIP 不红（发布门对 SKIP 另有阻断，见下）。`result.json` 恒写（全
SKIP 也写，空产物即异常）。

归档布局（同一 `--logs` 目录）：`result.json`、`logcat.txt`、`<case>.png`（终态）、
`<case>-<step>.xml`（dump）、失败时 `<case>-<step>.png`。`result.json` 的 `build` 字段即
构建摘要（commit + APK SHA-256），失败时 `targets[].failure.locate` 是可直接打印的步骤定位。

## 本机复现（Wear AVD）

```sh
SDK="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export PATH="$SDK/cmdline-tools/latest/bin:$SDK/emulator:$SDK/platform-tools:$PATH"
sdkmanager "platform-tools" "emulator" "system-images;android-34;wear;x86_64"
echo no | avdmanager create avd -n polypack-wear-smoke \
  -k "system-images;android-34;wear;x86_64" -d wearos_small_round --force
# 设备形态本地实测有 wearos_small_round / wearos_large_round / wearos_rect / wearos_square；
# 省略 -d 亦可（watch 特性来自系统镜像的 ro.build.characteristics）
emulator -avd polypack-wear-smoke -no-window -no-audio -no-boot-anim -no-snapshot \
  -gpu swiftshader_indirect -port 5554 &
adb wait-for-device
until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do sleep 5; done
node ui/smoke/wear/run.mjs --leg android --serial emulator-5554 --apk /path/to/wear-form.apk
```

Wear 镜像只有 x86_64 形态（与 google-tv 的 x86 相反）；`ro.build.characteristics` 含
`watch` 是设备门的判据，非 Wear 设备直接 SKIP 并注明，绝不冒充。

本机当前探测（2026-10-18）：SDK 有 `platform-tools/adb`、build-tools 34/36/37，但
**无 Wear system image、无 AVD、无在线设备、无 bundletool** → 真跑腿一律 SKIP-with-print；
`aapt2` 打包与 APK 结构断言已由 Rust 侧真跑。

## CI 接线（`.github/workflows/wear-smoke.yml`，新增文件）

- **mock 腿**：push main / PR，`node --test ui/smoke/wear/test/*.test.js`，秒级、零依赖。
- **真跑腿**：每夜 schedule + 手动触发。门 = `/dev/kvm` + 仓库变量
  `vars.POLYPACK_WEAR_FORM_APK`（预构建表单 APK 路径；包名可用
  `vars.POLYPACK_WEAR_FORM_PACKAGE` 覆盖）。门通过 → `android-actions/setup-android` 装
  `system-images;android-34;wear;x86_64`，起 `polypack-wear-smoke` AVD 后跑
  `run.mjs --leg android`；门缺席 → run 步仍执行，驱动打印 SKIP 并写全 SKIP 的
  `result.json`，同时发 `::warning::` 注记（不静默降级）。证据（result.json + dump +
  失败截图）保留 14 天。

## 发布门（`.github/workflows/wear-release-smoke.yml`，T-M7-04 新增文件）

- **触发**：`workflow_dispatch`、`release: published`；并暴露 `workflow_call`，发版流水线可
  直接 `uses: ./.github/workflows/wear-release-smoke.yml` 把 `release-gate` job 作为前置依赖。
- **与夜跑的区别**：缺 `/dev/kvm` 或 `vars.POLYPACK_WEAR_FORM_APK` 时打印 SKIP-with-print
  原因后以 `::error::` + exit 1 **阻断**（发版前必跑，SKIP 不放行）；run.mjs 表单失败 →
  exit 1；驱动内部 SKIP（如设备非 Wear）由「发布门判定」步骤读 `result.json` 判为不过。
- **失败定位**：`result.json` 的 `failure.locate`（例 `[swipe/swipe-to-note] 期望 …；实际 …`）
  在 job 日志以 `::error::` 打印；artifact `wear-release-smoke-evidence`（90 天）含
  `result.json`（时间戳 + build 摘要）、`logcat.txt`、截图与 dump。
- **release-gate job**：`needs: wear-device-smoke` + `if: always()`；设备 job 失败 / 取消 /
  未真跑 → gate job 红，依赖它的发版流水线被阻断。

### blessed runner / 模拟器复跑（发布门同款）

```sh
# 1) 在 blessed runner（有 /dev/kvm）或本机装好 Wear 镜像后启动 AVD（命令同上一节）；
# 2) 用发布门同款参数真跑（APK 与包名与 CI 变量一致）：
POLYPACK_WEAR_FORM_PACKAGE="${POLYPACK_WEAR_FORM_PACKAGE:-com.polypack.weatform}" \
node ui/smoke/wear/run.mjs --leg android --serial emulator-5554 \
  --apk "$POLYPACK_WEAR_FORM_APK" --logs /tmp/wear-release-smoke-logs
# 3) 发布门判定在 workflow 内联：读 result.json，仅 status=pass 且 pass=1 / fail=0 / skip=0
#    放行；skip / fail / 缺 result.json 均 exit 1 并打印 failure.locate。
```

本机无 Wear system image / AVD / 在线设备时，第 2 步打印 `SKIP wearos（android）：…` 并写
全 SKIP 的 `result.json`（exit 0）；发布门对同一结果判为阻断——这是有意的两种语义。
