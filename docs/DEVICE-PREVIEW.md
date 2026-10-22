# 发布前设备预览矩阵（M9-E7）

> 状态：T-M9-19（设备农场调研矩阵）与 T-M9-20（`DevicePool` / `DeviceLease` 协议 + `polypack preview --dry-run`）已交付；**T-M9-21 真机安装 / 启动 / 截图是外部环境工作，未执行、不标 ✅**。
> English: [DEVICE-PREVIEW.en.md](DEVICE-PREVIEW.en.md)

## 1. 状态与范围

本文件回答三件事：买还是自建设备农场；设备池与租约协议长什么样；真机演练需要哪些命令与证据。`polypack preview --dry-run` 只生成确定性计划，不租设备、不读凭据、不访问网络；真实执行留给 T-M9-21 的外部设备与人工证据归档。

非目标：完整设备农场产品、无人值守审核、为所有 49 个目标承诺真机覆盖。设备可用性（尤其 Android TV / Wear OS）随 provider 目录变化，租用前必须按当时目录核对。

## 2. 调研方法与证据状态

| 项目 | 取值 |
| --- | --- |
| 检索日期 | 2026-10-20 |
| 页面抓取状态 | 四家官方页面在本环境均被抓取层拦截，标记为 **not fetched**；下列细节仅来自检索结果摘要，**未验证**，不作为采购承诺。 |
| 价格口径 | 只记录计费单位与计费方式，不写任何具体金额；预算前必须打开官方价格页询价。 |
| 自建口径 | TCO 只列成本项（硬件、宿主、供电、运维工时、耗材），不虚构货币估算。 |

## 3. 供应商对比

| 选项 | 支持平台 / 设备（未验证） | API 模型 | 计费模型（单位） | 证据（检索 2026-10-20） |
| --- | --- | --- | --- | --- |
| Firebase Test Lab | Android 物理机 + 虚拟机、iOS 物理机；Robo / instrumentation / Game Loop / XCTest。TV / Wear 型号取决于目录，未核实。 | `gcloud firebase test android\|ios run` 与 Testing API 的 test matrix；结果落 Firebase 控制台 / Cloud Storage。 | 物理机与虚拟机分别按**设备小时**计费（单价不同），按分钟向上取整；计划含每日免费额度。不写具体费率。 | https://firebase.google.com/docs/test-lab 与 https://firebase.google.com/docs/test-lab/usage-quotas-pricing — **not fetched**，仅检索摘要。 |
| BrowserStack App Automate | Android / iOS 真机；Appium、Espresso、XCUITest、Flutter、Detox（仅 Android）。TV / Wear 目录未核实。 | REST API（`api-cloud.browserstack.com`）：上传应用后按框架启动会话；提供 CI 集成。 | 按**并行会话**计费（月付 / 年付计划），并行数决定并发上限；标准计划测试分钟不计费。不写具体费率。 | https://www.browserstack.com/app-automate 与 https://www.browserstack.com/pricing — **not fetched**，仅检索摘要。 |
| AWS Device Farm | Android / iOS 物理机；Web 应用经 Appium / Selenium 测试；支持私有专属设备池。TV / Wear 目录未核实。 | AWS API / CLI：`ScheduleRun`、项目与设备池；私有设备可选 VPC。 | 按**设备分钟**计量；或不限量**设备槽位月**（一个槽位 = 一台并发设备）。不写具体费率。 | https://aws.amazon.com/device-farm/ 与 https://aws.amazon.com/device-farm/pricing/ — **not fetched**，仅检索摘要。 |
| 自建设备池 | Android 经 adb / STF / Appium；iOS 需 macOS 宿主 + Xcode / WebDriverAgent；TV / Wear 取决于自购硬件。 | 目录或 HTTP 池管理器：租约 → adb / Appium 会话 → 安装 / 启动 / 截图 / logcat → 释放。 | 设备采购 + USB hub / 宿主 + 机架供电 + 运维工时 + 备件；iOS 另计 macOS 宿主。只列成本项。 | https://github.com/DeviceFarmer/stf 与 https://developer.android.com/tools/adb — **not fetched**，仅检索摘要。 |

## 4. Build vs Buy 建议

1. **默认先买按需**：T-M9-21 演练只租一次，不买常驻云套餐、不先建机架；拿到真实截图与日志后再决定长期方案。
2. **已有 Android / TV 硬件时优先自建**：无按分钟计费，adb 截图 / logcat 证据完整，卸载与释放完全可控；成本主要是运维工时与备件。
3. **iOS 与设备广度优先买**：需要 iOS 或大量并发时选云。GCP 为主选 Firebase；需要多框架与并行会话选 BrowserStack；AWS 原生与私有设备池选 Device Farm。先接一家，其余作为回退。
4. **协议保持可逆**：`preview/v1` 的 `provider` 字段隔离供应商差异，dry-run 计划与供应商无关；换供应商不改任务模型。
5. **用证据复盘**：统计每月预览次数、排队时间与运维工时，再与云报价比较；没有真实报价前不给数字盈亏平衡点。

## 5. DevicePool / DeviceLease 协议（preview/v1）

| 类型 | 字段 | 含义 |
| --- | --- | --- |
| DeviceSpec | provider | 设备来源：`firebase-test-lab` / `browserstack-app-automate` / `aws-device-farm` / `self-hosted` |
| DeviceSpec | device_id | provider 内设备标识（型号、序列号或 `host:port`） |
| DeviceSpec | os | 设备操作系统（`android` / `ios` / `tvos` 等） |
| DeviceSpec | target | PolyPack 目标 id；缺省等于 `os` |
| DevicePool | devices | 按声明序保存的设备列表；空池、重复 provider+device 被拒绝 |
| DeviceLease | lease_id | 计划内稳定编号：`lease-0001`、`lease-0002`…… |
| DeviceLease | provider | 设备来源 |
| DeviceLease | device_id | provider 内设备标识 |
| DeviceLease | os | 设备操作系统 |
| DeviceLease | target | PolyPack 目标 id |
| DeviceLease | acquired_at | 租约起点（Unix 秒） |
| DeviceLease | expires_at | 租约到期（Unix 秒 = acquired_at + timeout_secs） |
| DeviceLease | timeout_secs | 租约时长（秒） |
| DeviceLease | cleanup | 清理语义（见下） |
| CleanupPolicy | on_success | 成功路径：`release` |
| CleanupPolicy | on_failure | 失败路径：`release_and_archive` |
| CleanupPolicy | on_timeout | 超时路径：`force_release` |
| CleanupPolicy | uninstall | 释放前是否卸载夹具应用 |
| CleanupPolicy | evidence_dir | 证据目录：`.polypack/preview/<target>/<device>/` |
| PreviewPlan | protocol | `preview/v1` |
| PreviewPlan | farm_protocol | `farm/v1`（设备池是农场协议 v1 的扩展） |
| PreviewPlan | job_kind | `preview`（对应农场 job 帧的任务类型） |
| PreviewPlan | dry_run | 固定 `true`：计划不接触设备或网络 |
| PreviewPlan | leases | 按设备声明序生成的租约数组 |

## 6. Dry-run 命令与计划

```bash
polypack preview --dry-run --json --at 0 --timeout 900 \
  --device "firebase-test-lab/Pixel 7/android" \
  --device "self-hosted/tv-lab-01/android/android-tv"
```

`--at 0` 让输出可复现；`--timeout` 是每份租约的秒数；设备表达式为 `<provider>/<device-id>/<os>[/<target>]`，target 缺省等于 os。`--json` 输出单行 JSON；不带 `--json` 时输出同内容的缩进 JSON。

```json
{
  "protocol": "preview/v1",
  "farm_protocol": "farm/v1",
  "job_kind": "preview",
  "dry_run": true,
  "leases": [
    {
      "lease_id": "lease-0001",
      "provider": "firebase-test-lab",
      "device_id": "Pixel 7",
      "os": "android",
      "target": "android",
      "acquired_at": 0,
      "expires_at": 900,
      "timeout_secs": 900,
      "cleanup": {
        "on_success": "release",
        "on_failure": "release_and_archive",
        "on_timeout": "force_release",
        "uninstall": true,
        "evidence_dir": ".polypack/preview/android/Pixel 7/"
      }
    }
  ]
}
```

## 7. 租约、超时与清理语义

- 租约按 `--device` 声明序编号，同一输入与 `--at` 生成逐字节相同的 JSON；重跑可对比差异。
- `expires_at = acquired_at + timeout_secs`，加法做溢出检查；`--timeout 0` 与溢出都以 `polypack::E_PLAN_0001` 拒绝。
- 成功释放；失败释放并保留证据；超时强制释放。三条路径都要求卸载夹具应用并从 provider 释放设备。
- 证据目录固定在 `.polypack/preview/<target>/<device>/`；device / os / target 含 `/`、`\`、控制字符或为 `.` / `..` 时拒绝，避免证据路径逃逸。
- dry-run 是纯函数路径：不读凭据、不起子进程、不建 socket。真实执行未接线，不带 `--dry-run` 直接以 `polypack::E_FARM_0005` 拒绝，避免伪造结果。

| 条件 | 稳定错误码 | 状态 |
| --- | --- | --- |
| provider / 设备表达式 / 空列表 / 重复设备非法 | `polypack::E_FARM_0002` | 已实现 |
| 租约时长非法或溢出 | `polypack::E_PLAN_0001` | 已实现 |
| 未带 `--dry-run` 的真实执行 | `polypack::E_FARM_0005` | 已实现（明确拒绝） |
| 租约到期仍未完成 | `polypack::E_FARM_0004` | 计划中，未实现 |
| 远端安装 / 启动失败 | `polypack::E_FARM_0003` | 计划中，未实现 |

## 8. T-M9-21 外部真机演练（未交付）

状态：**EXTERNAL / OPEN，未执行，不标 ✅**。需要 Android 与 Android TV 各一台真实设备（或云农场等价设备）、adb、夹具 APK 与可写证据目录。以下命令以自建设备池为例；云农场按其 API 执行同一组安装 / 启动 / 截图 / 日志步骤。

### 8.1 命令（自建 Android / Android TV）

```bash
# 0. 确定性租约计划（先归档，证明设备与时间窗）
polypack preview --dry-run --json --at 0 --timeout 900 \
  --device "self-hosted/<android-serial>/android" \
  --device "self-hosted/<android-tv-serial>/android/android-tv"

# 1. 设备与夹具
adb devices -l
adb -s <serial> shell getprop ro.build.version.sdk
adb -s <serial> install -r -t .polypack/packages/android/app-release.apk

# 2. 启动并截图（Android 与 Android TV 相同；TV 可用方向键确认焦点）
adb -s <serial> shell am start -W -n <applicationId>/<activity>
adb -s <serial> exec-out screencap -p > .polypack/preview/<target>/<device>/launch.png
adb -s <serial> shell dumpsys window | grep -E 'mCurrentFocus|mFocusedApp' \
  > .polypack/preview/<target>/<device>/focus.txt

# 3. 日志
adb -s <serial> logcat -d > .polypack/preview/<target>/<device>/logcat.txt
adb -s <serial> logcat -d -b crash > .polypack/preview/<target>/<device>/crash.txt

# 4. 清理
adb -s <serial> shell pm uninstall <applicationId>
adb devices -l
```

### 8.2 必需证据与错误码

| 证据 | 要求 |
| --- | --- |
| 计划 | `polypack preview --dry-run --json` 原始输出，含 lease_id 与时间窗 |
| 产物 | APK 路径 + 构建回执中的 blake3 摘要（`polypack build --json`） |
| 设备 | `adb devices -l` 输出、型号、API level（`getprop ro.build.version.sdk`） |
| 安装 | 完整 `adb install` 命令与退出码 |
| 启动 | `am start -W` 输出含 `Status: ok`，且焦点活动等于目标 activity |
| 截图 | Android 与 Android TV 各一张 PNG，记录路径与 SHA-256 |
| 日志 | 完整 logcat 与 crash buffer 归档到证据目录 |
| 清理 | 卸载退出码、`pm list packages` 确认已移除、演练后 `adb devices -l` |

失败必须保留原始命令、退出码与日志，并映射到第 7 节稳定错误码；不得把模拟器或页面截图冒充真机证据。

## 9. 风险与限制

- 四家官方页面本次均未抓到，矩阵细节未经原文核验；采购前必须以官方价格页与设备目录复核。
- 云农场目录随时变化，TV / Wear 设备不保证存在；租约生成不代表设备可租。
- 自建设备池的隐性成本是运维、供电、USB 稳定性与备件，不是零成本。
- 截图可能含用户数据：只跑夹具应用，证据目录按保留期清理。
- T-M9-21 未完成前，本矩阵不能作为「发布前真机回归已覆盖」的证据。
