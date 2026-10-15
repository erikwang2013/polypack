const path = require('path');
const { getDefaultConfig, mergeConfig } = require('@react-native/metro-config');

/**
 * Metro configuration
 * https://reactnative.dev/docs/metro
 *
 * @type {import('@react-native/metro-config').MetroConfig}
 */
const config = {
  // @polypack/adapter-native 以 file:.. 链接，真实路径在 Metro 项目根之外 →
  // 必须纳入 watch 树，否则解析报 UnableToResolveError（设备链实测，T-M2-05）
  watchFolders: [path.resolve(__dirname, '..')],
};

module.exports = mergeConfig(getDefaultConfig(__dirname), config);
