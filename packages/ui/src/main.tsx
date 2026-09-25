import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { createHostAdapter } from "./adapters/host";
import { applyTheme, resolveTheme } from "./theme";
import "./styles/app.css";

// 宿主快照到达前的开发预览默认（?theme=light 不持久化）；快照到达后以产品设置为准。
applyTheme(resolveTheme(window.location.search));

const container = document.getElementById("root");
if (!container) throw new Error("缺少 #root 容器");

createRoot(container).render(
  <StrictMode>
    <App adapter={createHostAdapter()} />
  </StrictMode>,
);
