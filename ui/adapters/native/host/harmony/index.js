// RN 宿主入口（T-M3-01 宿主模板 + T-M3-02 承载分支接线：target 口径修正 + 组件映射）：
// 装配层职责 = 注册组件 + 接事件源 + 原生模块注册位。
// app.json 的 name 即 AppRegistry 键，与 harmony/entry 的 RNApp.appKey（PaxApp）一致。

import { createElement } from "react";
import * as RN from "react-native";

import { bindPrimitives, createAdapter } from "@polypack/adapter-native";
import { createExampleApp } from "@polypack/adapter-native/example";

import appConfig from "./app.json";

// pointer：app.json 的 polypack.pointer（默认 "touch" = 触屏手机形态）。设备走查（T-M3-09）
// 设 "none" → 无指针形态生效（焦点环降级分支）。用 app.json 而非 process.env：RN 运行时
// process.env 只保证 NODE_ENV（Libraries/Core/setUpGlobals.js），自定义 env 不会被打包内联。
const pointer = appConfig.polypack?.pointer ?? "touch";
// target = TargetSpec id（crates/poly-targets/src/builtin.rs / capabilities 矩阵口径 "harmonyos"；
// 与本仓 <工程根>/harmony/ 目录名无关——目录名是 DevEco 工程路径，不是目标 id，T-M3-02 修正）
const adapter = createAdapter({ target: "harmonyos", runtime: RN, host: { pointer } });

// 原语显式取用（不展开 RN 命名空间：避免握手期 getter 全量求值）
// iconSource 待 Pax 派生管线资产（T-M1-14）就位后填入，当前省略图标节点
// 组件映射（T-M3-02）：同一页面组件树经 bindPrimitives 落到 RNOH —— 页面照写 Android 焦点锚点
// （hasTVPreferredFocus），RNOH 无对应 prop，由装配边界按 HOST_PROP_OMISSIONS 剔除（页面零改动）
const primitives = bindPrimitives(
  {
    View: RN.View,
    Text: RN.Text,
    Pressable: RN.Pressable,
    Image: RN.Image,
    iconSource: null,
  },
  "harmonyos",
  createElement,
);
const { App, onKey } = createExampleApp(primitives, adapter);

adapter.mount({ appKey: "PaxApp" }, App);

// 硬件键事件源（T-M3-01 约定）：ArkTS 根容器 onKeyEvent → rnInstance.emitDeviceEvent
// ('polyKeyEvent', {keyCode, keyAction})（见 harmony/entry/src/main/ets/pages/Index.ets）。
// KeyCode（OHOS @ohos.multimodalInput.keyCode）：DPAD_UP=2012 DOWN=2013 LEFT=2014
// RIGHT=2015 CENTER=2016、ENTER=2054、SPACE=2050；Back=2 不走这里的表（见下方 BackHandler）。
// keyAction（KeyType）：Down=0 / Up=1 —— 须只消费 Down：一次物理按键派发 Down+Up 两帧，
// 全收则中心键激活双计、Back 双弹（与 android 宿主 ACTION_DOWN 过滤同因，走查期望才对得上）。
const KEYCODE_TO_KERNEL = {
  2012: "DPAD_UP",
  2013: "DPAD_DOWN",
  2014: "DPAD_LEFT",
  2015: "DPAD_RIGHT",
  2016: "DPAD_CENTER",
  2054: "DPAD_CENTER",
  2050: "DPAD_CENTER",
};

/** 键入内核，并把 handleKey 结果写成独立日志行（hilog tag ReactNativeJS；走查比对用）。
 *  单参数形式：多参数会被 RN console 加引号拼接，破坏 device-walks「POLYPACK_KEY <json>」
 *  的通道契约（android 设备链实测，T-M2-05）。 */
const dispatch = (key) => {
  const result = onKey(key);
  console.log("POLYPACK_KEY " + JSON.stringify(result));
  return result;
};

RN.DeviceEventEmitter.addListener("polyKeyEvent", (event) => {
  if (event.keyAction !== 0) return;
  const key = KEYCODE_TO_KERNEL[event.keyCode];
  if (key !== undefined) dispatch(key);
});

RN.BackHandler.addEventListener("hardwareBackPress", () => {
  // 源：harmony/entry EntryAbility.onBackPress → RNOH dispatchBackPress →
  // rnInstance.onBackPress → 本设备事件（与 android 核心 BackHandler 同语义）。
  // 始终消费（返回 true = 已处理）：栈空时内核给 to: null——退出语义属宿主策略，
  // 走查期间不退出更稳（precondition 靠重启回初始态），M4 再定是否退到桌面。
  dispatch("Back");
  return true;
});

// 原生模块注册位（T-M3-01 口径）：NAPI 模块由 `poly bridge ohos` 产出到
// <工程根>/bridge/harmonyos/<abi>/libpax.so，注入 harmony/entry/libs/<abi>/ 后随 HAP 打包。
// 模块名 = UDL namespace（示例取 T-M3-01 真跑夹具 pax.udl + pax_core）；
// ArkTS 侧 import 语法为 `import { greet } from 'libpax.so'`（host/harmony 仓库内解析不到
// bridge/ 属正常——模板按工程根铺开，见 README「桥产物挂载点」）。
// adapter.native.register({
//   artifact: "bridge/harmonyos/arm64-v8a/libpax.so",
//   moduleName: "pax",
// });

// 冒烟形态（同 T-M4-05）＝ 无指针完成核心任务：方向键遍历 → 激活 → Back 返回上一焦点。
export { adapter, onKey };
