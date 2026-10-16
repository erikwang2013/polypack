// 应用入口：Taro 的 App 组件只做一件事——装配自检（T-M3-06 证据链的一环：
// 编译产物里带上装配层的目标 / 平台 / 路由声明，devtools console 与产物扫描都可验）。
import { useLaunch } from '@tarojs/taro'

import { getAdapter } from './adapter'

import './app.css'

export default function App({ children }) {
  useLaunch(() => {
    const adapter = getAdapter()
    // 路由映射自检：声明源缺席 / pages 为空会在装配点即拒（不是运行期 404）
    console.log(
      `POLYPACK_TARO target=${adapter.target} platform=${adapter.platform} pages=${adapter.router().pages.join(',')}`,
    )
  })

  // children 是将要渲染的页面
  return children
}
