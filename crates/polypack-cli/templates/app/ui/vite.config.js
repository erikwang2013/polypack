// PolyPack 调起 `vite build --base ./ --outDir <dist>`；此处只固化目录约定与 React 插件。
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
