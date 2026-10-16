// RN-tvOS 宿主入口（T-M4-06）：装配层职责 = 注册组件 + 接事件源（遥控器）+ 原生模块注册位。
// 与 android / harmony 宿主两处形态差异：①事件源是 useTVEventHandler（hook，须在组件内注册）
// 且事件名是字符串 eventType（up/down/left/right/select/menu/playPause…），不是 keyCode 数字表；
// ②Native 桥不经 JS register（Apple 侧 = uniffi Swift 绑定 + 核心静态库，随 Xcode 链编入 App）。
// app.json 的 name 即 AppRegistry 键，与 ios/ 工程位（T-M4-07）的 scheme / App key 一致。

import { createElement, useEffect } from "react";
import * as RN from "react-native";

import { bindPrimitives, createAdapter } from "@polypack/adapter-native";
import { createExampleApp } from "@polypack/adapter-native/example";

import appConfig from "./app.json";

// pointer：TV 恒无指针 → app.json 默认即 "none"（与 android / harmony 宿主「默认 touch、
// 走查会话改 none」的形态不同）。用 app.json 而非 process.env：RN 运行时 process.env 只保证
// NODE_ENV，自定义 env 不会被打包内联（android 宿主同因）。
const pointer = appConfig.polypack?.pointer ?? "none";
// target = TargetSpec id（crates/poly-targets/src/builtin.rs 的 "tvos"）
const adapter = createAdapter({ target: "tvos", runtime: RN, host: { pointer } });

// 原语显式取用（不展开 RN 命名空间：避免握手期 getter 全量求值）
// iconSource 待 Pax 派生管线资产（T-M1-14）就位后填入，当前省略图标节点
// 组件映射：tvos 无剔除项（hasTVPreferredFocus 由 RN-tvOS 原生支持，焦点锚点原样保留），
// bindPrimitives 对空表目标返回原语同一性——示例页组件树对 tvos 零改动。
const primitives = bindPrimitives(
  {
    View: RN.View,
    Text: RN.Text,
    Pressable: RN.Pressable,
    Image: RN.Image,
    iconSource: null,
  },
  "tvos",
  createElement,
);
const { App, onKey } = createExampleApp(primitives, adapter);

// eventType（RN-tvOS 原生常量，RCTTVRemoteHandlerConstants）→ 内核键名。
// menu 不入表：tvOS 的 Back 走核心 BackHandler（'hardwareBackPress'，BackHandler.ios.js 在
// Platform.isTV 下即对 eventType=menu 的封装）——两路都接会双计（同 harmony 宿主「Back 不走键表」）。
const EVENTTYPE_TO_KERNEL = {
  up: "DPAD_UP",
  down: "DPAD_DOWN",
  left: "DPAD_LEFT",
  right: "DPAD_RIGHT",
  select: "DPAD_CENTER",
};

/** 键入内核，并把 handleKey 结果写成独立日志行（Metro 终端 / Console.app；走查比对用）。
 *  单参数形式：多参数会被 RN console 加引号拼接，破坏 device-walks「POLYPACK_KEY <json>」
 *  的通道契约（android 设备链实测，T-M2-05）。 */
const dispatch = (key) => {
  const result = onKey(key);
  console.log("POLYPACK_KEY " + JSON.stringify(result));
  return result;
};

/** 根组件（T-M4-06）：遥控器事件源桥——useTVEventHandler 是 hook，注册的根须是组件而非 App 本体。 */
function TvKeyBridge() {
  // menu（Back）默认被系统吞（Apple 指南，RN #18930）：显式开闸后 menu 才进事件系统 →
  // BackHandler('hardwareBackPress')；卸载对偶关闭防泄漏。
  useEffect(() => {
    RN.TVEventControl.enableTVMenuKey();
    return () => RN.TVEventControl.disableTVMenuKey();
  }, []);

  // 只消费 Down（eventKeyAction：0=Down / 1=Up）：一次物理按键派发两帧，全收则中心键激活双计
  // （同 android ACTION_DOWN、RNOH keyAction 过滤）。
  RN.useTVEventHandler((event) => {
    if (event.eventKeyAction !== 0) return;
    const key = EVENTTYPE_TO_KERNEL[event.eventType];
    if (key !== undefined) dispatch(key);
  });

  return createElement(App, null);
}

adapter.mount({ appKey: "PaxApp" }, TvKeyBridge);

RN.BackHandler.addEventListener("hardwareBackPress", () => {
  // 始终消费（返回 true = 已处理）：栈空时内核给 to: null——退出语义属宿主策略，
  // 走查期间不退出更稳（precondition 靠重启回初始态）；与 android / harmony 宿主同策略。
  dispatch("Back");
  return true;
});

// 原生模块注册位（T-M4-07 口径）：Apple 侧无 AAR / .so 形态的注册位——桥产物是 uniffi Swift
// 绑定 + 核心 staticlib，经 host/tvos/ios/ 的 Xcode 工程（或打包链骨架工程）编入 App，
// 不经 adapter.native.register（android / harmony 专属口径，见 native-registry.js）。

// 冒烟形态（同 T-M4-05）＝ 无指针完成核心任务：方向键遍历 → 激活 → Back 返回上一焦点。
export { adapter, onKey };
