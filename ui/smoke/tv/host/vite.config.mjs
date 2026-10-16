// web 腿宿主出包配置：零插件（宿主与示例页均为 createElement，无 JSX → 不需要 react 插件）。
// root = 本目录（index.html 同址），跑法无关 cwd：vite build --config host/vite.config.mjs。
import { defineConfig } from "vite";

export default defineConfig({
  root: import.meta.dirname,
  build: { outDir: "dist", emptyOutDir: true, target: "es2022" },
  // react 单实例：示例页（ui/adapters/tv）与宿主各自树上的 react 副本须去重，
  // 否则 hooks（useSyncExternalStore）抛 Invalid hook call。
  resolve: { dedupe: ["react", "react-dom"] },
});
