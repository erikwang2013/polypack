// 示例应用 UI（T-M1-11）：React 骨架，样式取自 tokens.css（@polypack/ui 最小 token 集同形）。
// Vite 入口：`poly build` / `poly dev` 调 `vite build --base ./` 产出 dist/。

import { useState } from "react";
import { createRoot } from "react-dom/client";

import "./tokens.css";

function App() {
  const [clicks, setClicks] = useState(0);

  return (
    <section className="card">
      <img className="mark" src="./icon.svg" alt="Pax" width="64" height="64" />
      <h1>PolyPack App</h1>
      <p className="muted">版本 0.1.0 · 由 PolyPack 构建（poly build / poly dev）</p>
      <button type="button" onClick={() => setClicks(clicks + 1)}>
        点击计数：{clicks}
      </button>
      <p className="muted">编辑本文件并运行 poly dev，页面自动刷新。</p>
    </section>
  );
}

createRoot(document.querySelector("#app")).render(<App />);
