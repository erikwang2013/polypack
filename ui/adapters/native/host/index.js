// RN 宿主入口（T-M2-04）：装配层职责 = 注册组件 + 接事件源 + 原生模块注册位。
// app.json 的 name 即 AppRegistry 键，与 MainActivity.getMainComponentName() 一致。

import * as RN from "react-native";

import { createAdapter } from "@polypack/adapter-native";
import { createExampleApp } from "@polypack/adapter-native/example";

// pointer: "touch" = 触屏手机形态；TV / 车机 / 无指针调试传 "none" → 焦点环降级分支生效
const adapter = createAdapter({ target: "android", runtime: RN, host: { pointer: "touch" } });

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

// 原生模块注册位（T-M2-02 口径）：AAR 由 `poly bridge aar` 产出到 <工程根>/bridge/android/，
// entryClass = uniffi.<UDL namespace>.<Ns>Kt，libraryName = .so 基名（System.loadLibrary 入参）。
// 示例取 T-M2-01/02 真跑夹具（pax.udl + pax_core）：
// adapter.native.register({
//   artifact: "bridge/android/pax_core.aar",
//   entryClass: "uniffi.pax.PaxKt",
//   libraryName: "pax_core",
// });

// 方向键 / Back 事件源：Android 侧由原生按键事件（或调试模式）注入 onKey。
// 冒烟形态（同 T-M4-05）＝ 无指针完成核心任务：方向键遍历 → 激活 → Back 返回上一焦点。
export { adapter, onKey };
