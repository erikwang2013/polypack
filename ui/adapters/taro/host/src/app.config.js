// 应用配置：pages 清单是路由映射（adapter-taro routes.js）与 mount 的**声明源**——
// 未声明的页面在装配 / 导航调用点即拒（T-M3-05 契约）。
export default defineAppConfig({
  pages: ['pages/index/index'],
  window: {
    backgroundTextStyle: 'light',
    navigationBarBackgroundColor: '#ffffff',
    navigationBarTitleText: 'PolyPack',
    navigationBarTextStyle: 'black'
  }
})
