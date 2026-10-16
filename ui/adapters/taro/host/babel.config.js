// babel-preset-taro（React 框架）；见 https://docs.taro.zone/docs/next/babel-config
module.exports = {
  presets: [
    ['taro', {
      framework: 'react',
      ts: false,
      compiler: 'webpack5'
    }]
  ]
}
