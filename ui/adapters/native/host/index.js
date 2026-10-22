// RN 宿主入口（T-M2-04）：装配层职责 = 注册组件 + 接事件源 + 原生模块注册位。
// app.json 的 name 即 AppRegistry 键，与 MainActivity.getMainComponentName() 一致。

import * as RN from "react-native";

import { createAdapter } from "@polypack/adapter-native";
import { createExampleApp } from "@polypack/adapter-native/example";

import appConfig from "./app.json";

// pointer：app.json 的 polypack.pointer（默认 "touch" = 触屏手机形态）。设备走查（T-M2-05）
// 设 "none" → 无指针形态生效（焦点环降级分支）。用 app.json 而非 process.env：RN 运行时
// process.env 只保证 NODE_ENV（Libraries/Core/setUpGlobals.js），自定义 env 不会被打包内联。
const pointer = appConfig.polypack?.pointer ?? "touch";
const adapter = createAdapter({ target: "android", runtime: RN, host: { pointer } });

// 原语显式取用（不展开 RN 命名空间：避免握手期 getter 全量求值）
// iconSource 待 Pax 派生管线资产（T-M1-14）就位后填入，当前省略图标节点
const primitives = {
  View: RN.View,
  Text: RN.Text,
  Pressable: RN.Pressable,
  Image: RN.Image,
  iconSource: null,
};
const { App, onKey } = createExampleApp(primitives, adapter);

adapter.mount({ appKey: "PaxApp" }, App);

// 方向键 / Back 事件源（T-M2-05 走查前提）：RN 0.87 核心在 ReactRootView.dispatchKeyEvent
// 无条件派发 onHWKeyEvent 设备事件（DPAD 四向 → up/down/left/right，DPAD_CENTER/ENTER/SPACE
// → select；核心 JS 无消费者，故默认无人接）；Back 不在该表内，走核心 BackHandler。
const HW_TO_KERNEL = {
  up: "DPAD_UP",
  down: "DPAD_DOWN",
  left: "DPAD_LEFT",
  right: "DPAD_RIGHT",
  select: "DPAD_CENTER",
};

/** 键入内核，并把 handleKey 结果写成 logcat 通道（ReactNativeJS tag；T-M2-05 走查比对用）。
 *  单参数形式：多参数会被 RN console 加引号拼接（'POLYPACK_KEY', '{…}'），破坏
 *  device-walks/README「POLYPACK_KEY <json>」的通道契约（设备链实测）。 */
const dispatch = (key) => {
  const result = onKey(key);
  console.log("POLYPACK_KEY " + JSON.stringify(result));
  return result;
};

RN.DeviceEventEmitter.addListener("onHWKeyEvent", (event) => {
  // 一次 `adb shell input keyevent` 派发 ACTION_DOWN(0) + ACTION_UP(1) 两帧，须只消费 DOWN：
  // 否则中心键激活双计、Back 双弹，走查期望（device-walks/expectations.json）必对不上。
  if (event.eventKeyAction !== 0) return;
  const key = HW_TO_KERNEL[event.eventType];
  if (key !== undefined) dispatch(key);
});

RN.BackHandler.addEventListener("hardwareBackPress", () => {
  // 始终消费（返回 true = 已处理）：栈空时内核给 to: null——退出语义属宿主策略，
  // 走查期间不退出更稳（precondition 靠重启回初始态），M4 再定是否退到桌面。
  dispatch("Back");
  return true;
});

// 原生模块注册位（T-M2-02 口径）：AAR 由 `polypack bridge aar` 产出到 <工程根>/bridge/android/，
// entryClass = uniffi.<UDL namespace>.<Ns>Kt，libraryName = .so 基名（System.loadLibrary 入参）。
// 示例取 T-M2-01/02 真跑夹具（pax.udl + pax_core）：
// adapter.native.register({
//   artifact: "bridge/android/pax_core.aar",
//   entryClass: "uniffi.pax.PaxKt",
//   libraryName: "pax_core",
// });

// 冒烟形态（同 T-M4-05）＝ 无指针完成核心任务：方向键遍历 → 激活 → Back 返回上一焦点。
export { adapter, onKey };
