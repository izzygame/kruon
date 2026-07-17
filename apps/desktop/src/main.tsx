import { StrictMode, Suspense, lazy } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("Root element not found");

const isWorldView = getCurrentWebviewWindow().label === "world";
const LazyWorldApp = lazy(() => import("./world/WorldApp").then((module) => ({ default: module.WorldApp })));

createRoot(root).render(
  <StrictMode>
    {isWorldView ? (
      <Suspense fallback={null}>
        <LazyWorldApp />
      </Suspense>
    ) : (
      <App />
    )}
  </StrictMode>,
);
