import { defineConfig } from '@tarojs/cli'

import devConfig from './dev'
import prodConfig from './prod'

// Taro 编译配置（T-M3-06）：四目标共用一份配置，平台经 `taro build --type <platform>` 切换
// （weapp / alipay / tt；快应用无 Taro 平台插件，口径见 test/host-structure.test.js 的注记）。
//
// outputRoot 走 `POLYPACK_OUTPUT_ROOT` 注入：poly-bundle 的小程序打包器按目标给目录，
// 四份产物互不覆盖；本地手跑回落 `dist/`（Taro 模板默认）。
export default defineConfig(async (merge, { command, mode }) => {
  const baseConfig = {
    projectName: 'polypack-host-taro',
    date: '2026-10-16',
    // 设计稿 750 宽（Taro 默认），px 由 pxtransform 折半到 CSS px
    designWidth: 750,
    deviceRatio: {
      640: 2.34 / 2,
      750: 1,
      375: 2,
      828: 1.81 / 2
    },
    sourceRoot: 'src',
    outputRoot: process.env.POLYPACK_OUTPUT_ROOT || 'dist',
    plugins: [],
    defineConstants: {},
    copy: {
      patterns: [],
      options: {}
    },
    framework: 'react',
    compiler: 'webpack5',
    // 持久化缓存：二跑缓存命中（T-M3-06 验收项之一）
    cache: {
      enable: true
    },
    mini: {
      // 宿主以 `file:..` 软链 adapter：webpack 默认会解析到仓内真实路径，从那里向上找不到
      // 宿主 node_modules 里的 react / @tarojs/taro（真跑件在 mac/linux 同错）。保留软链路径。
      webpackChain: (chain) => {
        chain.resolve.symlinks(false)
      },
      postcss: {
        pxtransform: {
          enable: true,
          config: {}
        },
        cssModules: {
          enable: false,
          config: {
            namingPattern: 'module',
            generateScopedName: '[name]__[local]___[hash:base64:5]'
          }
        }
      }
    }
  }

  if (process.env.NODE_ENV === 'development') {
    return merge({}, baseConfig, devConfig)
  }
  return merge({}, baseConfig, prodConfig)
})
