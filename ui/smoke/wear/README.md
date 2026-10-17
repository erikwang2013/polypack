# Wear 表单冒烟套件（T-M7-03）

`wearos` 目标的 §8 表单用例集：**输入（input）→ 滑动（swipe）→ 返回（back）**。用 adb
注入文本 / 滑动手势 / BACK 键，以 uiautomator dump 的可见文本为观测通道；`result.json`
（schema `polypack/wear-smoke/v1`）逐 case / 逐 step 记录 `expect` / `observed` / `dump`，
失败定位到**具体表单步骤**（例：`[swipe/swipe-to-note] 期望 step: note；实际 step: amount（dump: …）`）。

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
case fail → 1；SKIP 不红。`result.json` 恒写（全 SKIP 也写，空产物即异常）。

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
