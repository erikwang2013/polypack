const path = require('path');
const { mergeConfig, getDefaultConfig } = require('@react-native/metro-config');
const {
  createHarmonyMetroConfig,
} = require('@react-native-oh/react-native-harmony/metro.config');

/**
 * Metro configuration（RNOH 0.84.3 官方模板 + 本仓装配层两处增补）
 * https://reactnative.dev/docs/metro
 *
 * @type {import("metro-config").MetroConfig}
 */
const config = {
  transformer: {
    getTransformOptions: async () => ({
      transform: {
        experimentalImportSupport: false,
        inlineRequires: true,
      },
    }),
  },
  // @polypack/adapter-native 以 file:../.. 链接，真实路径在工程根之外 →
  // 必须纳入 watch 树，否则解析报 UnableToResolveError（与 android 宿主同因，T-M2-05 实测）
  watchFolders: [path.resolve(__dirname, '../..')],
};

module.exports = mergeConfig(
  getDefaultConfig(__dirname),
  createHarmonyMetroConfig({
    reactNativeHarmonyPackageName: '@react-native-oh/react-native-harmony',
  }),
  config,
);
