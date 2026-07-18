import { StrictMode, Suspense, lazy } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("Root element not found");
const appRoot = root;

// Vite previews do not provide Tauri's window metadata. Falling back to the
// authoritative 2D shell keeps the visual surface reviewable without changing
// the production main/world split.
const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const isBrowserPreview = import.meta.env.DEV && !isTauriRuntime;
let isWorldView = false;
if (isTauriRuntime) {
  try {
    isWorldView = getCurrentWebviewWindow().label === "world";
  } catch {
    isWorldView = false;
  }
}
const LazyWorldApp = lazy(() => import("./world/WorldApp").then((module) => ({ default: module.WorldApp })));

async function renderDesktop() {
  const client = isBrowserPreview
    ? (await import("./lib/browserPreview")).browserPreviewClient
    : undefined;
  createRoot(appRoot).render(
    <StrictMode>
      {isWorldView ? (
        <Suspense fallback={null}>
          <LazyWorldApp />
        </Suspense>
      ) : (
        <App client={client} />
      )}
    </StrictMode>,
  );
}

void renderDesktop();
