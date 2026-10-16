// 装配层接线（T-M3-05 契约 → Taro 宿主）：runtime 显式注入。
//
// - `config()` 读 app.config 的 pages 清单（路由映射的声明源）；
// - 路由五方法经 `Taro.*` 落地——方法名逐个在白名单内（apiguard 构建期预检的扫描面）。
import Taro from '@tarojs/taro'

import { TARO_PLATFORMS, createAdapter } from '@polypack/adapter-taro'

import appConfig from './app.config'

// Taro 编译平台（TARO_ENV）→ 装配层目标 id（TARO_PLATFORMS 的逆映射）
const TARGET_OF = Object.fromEntries(
  Object.entries(TARO_PLATFORMS).map(([target, platform]) => [platform, target]),
)

let adapter = null

/** 单例装配：app 与页面模块共享同一 adapter（mount 状态可观测）。 */
export function getAdapter() {
  adapter ??= createAdapter({
    // TARO_ENV 由 Taro 编译器静态注入（weapp / alipay / tt）
    target: TARGET_OF[process.env.TARO_ENV] ?? 'wechat',
    runtime: {
      config: () => appConfig,
      navigateTo: (options) => Taro.navigateTo(options),
      redirectTo: (options) => Taro.redirectTo(options),
      switchTab: (options) => Taro.switchTab(options),
      reLaunch: (options) => Taro.reLaunch(options),
      navigateBack: (options) => Taro.navigateBack(options),
    },
    host: { pointer: 'touch' },
  })
  return adapter
}
