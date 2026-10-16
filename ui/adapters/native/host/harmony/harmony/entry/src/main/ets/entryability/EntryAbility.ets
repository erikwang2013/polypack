/**
 * Copyright (c) 2025 Huawei Technologies Co., Ltd.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import { RNAbility, RNOHCoreContext } from '@rnoh/react-native-openharmony';

export default class EntryAbility extends RNAbility {
  override getPagePath() {
    return 'pages/Index';
  }

  override getRNOHWorkerScriptUrl() {
    return "entry/ets/workers/RNOHWorker.ets"
  }

  /**
   * 系统 Back（KEYCODE_BACK=2 的框架级事件，T-M3-01 键事件约定）走 RN 核心 BackHandler
   * 通道，与 android 宿主同语义：dispatchBackPress → rnInstance.onBackPress →
   * JS 设备事件 'hardwareBackPress'（RNAbility 无此转发，宿主须显式接）。
   * 未被 JS 消费时 RNOH 的 invokeDefaultBackPressHandler → RNAbility 的
   * defaultBackPressHandler（默认收起/退出，宿主策略留 M4 定）。
   *
   * 回调名以本机 SDK（5.1.1(19)）的 UIAbility d.ts 为准 = `onBackPressed`：
   * 编译期实证 RNAbility 基类链上无 `onBackPress`（ArkTS 报 override 不存在），
   * 新 SDK 文档里的 `onBackPress` 属更新 API 线的命名，此处不可用。
   */
  override onBackPressed(): boolean {
    AppStorage.get<RNOHCoreContext>('RNOHCoreContext')?.dispatchBackPress();
    return true;
  }
}
