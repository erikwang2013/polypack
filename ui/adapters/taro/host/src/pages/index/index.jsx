// 首页：装配链的最后一环——createExamplePage（原语注入）→ adapter.mount（路由绑定）
// → default export 即页面模块（Taro 编译器按 app.config 的 pages 清单接线）。
import { Button, Image, Text, View } from '@tarojs/components'
import { createExamplePage } from '@polypack/adapter-taro/example'

import { getAdapter } from '../../adapter'

const adapter = getAdapter()
// mount 契约：root.pagePath 须在 pages 清单内，未声明页在装配点即拒
const { Page } = createExamplePage({ View, Text, Button, Image }, adapter)

export default adapter.mount({ pagePath: 'pages/index/index' }, Page).app
