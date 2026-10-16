/**
 * Copyright (c) 2025 Huawei Technologies Co., Ltd.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  AnyJSBundleProvider,
  MetroJSBundleProvider,
  RNApp,
  RNOHErrorDialog,
  ResourceJSBundleProvider,
  RNOHCoreContext,
  RNInstance,
} from '@rnoh/react-native-openharmony';
import { getRNOHPackages } from '../PackageProvider';

@Entry
@Component
struct Index {
  @StorageLink('RNOHCoreContext') private rnohCoreContext: RNOHCoreContext | undefined = undefined
  // onSetUp 钩子把 RNInstance 存下来：下方根容器 onKeyEvent 的转发要用（T-M3-01 键事件约定）
  private rnInstance: RNInstance | undefined = undefined

  build() {
    Column() {
      if (this.rnohCoreContext) {
        if (this.rnohCoreContext?.isDebugModeEnabled) {
          RNOHErrorDialog({ ctx: this.rnohCoreContext })
        }
        RNApp({
          rnInstanceConfig: {
            name: "PaxApp",
            createRNPackages: getRNOHPackages,
            fontResourceByFontFamily: {},
            enableDebugger: this.rnohCoreContext?.isDebugModeEnabled,
          },
          appKey: "PaxApp",
          onSetUp: (rnInstance) => {
            this.rnInstance = rnInstance
          },
          jsBundleProvider: this.rnohCoreContext?.isDebugModeEnabled ?
            new AnyJSBundleProvider([
              new MetroJSBundleProvider(),
              new ResourceJSBundleProvider(this.rnohCoreContext.uiAbilityContext.resourceManager, 'hermes_bundle.hbc'),
            ]) :
            /**
             * NOTE: The HBC format is recommended for Hermes in a production environment. HBC provides better performance and loading times.
             */
            new ResourceJSBundleProvider(this.rnohCoreContext.uiAbilityContext.resourceManager, 'hermes_bundle.hbc'),
        })
      }
    }
    .height('100%')
    .width('100%')
    // 硬件键转发（T-M3-01 约定）：ArkTS 只收不算，keyCode/keyAction(KeyType) 原样给 JS——
    // 映射（KeyCode→内核键）与 DOWN-only 过滤都在 host/harmony/index.js 的 polyKeyEvent 通道。
    // Back 不走这里（见 EntryAbility.onBackPress → 'hardwareBackPress'）。
    .onKeyEvent((event: KeyEvent) => {
      this.rnInstance?.emitDeviceEvent('polyKeyEvent', {
        keyCode: event.keyCode,
        keyAction: event.type,
      })
    })
  }
}
