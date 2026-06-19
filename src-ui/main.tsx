import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { installContextMenuGuard } from "./nativeChrome";
import "./styles.css";

installContextMenuGuard(document);

// Scope the tray styling: `view=tray` selects the popover-only rules, and
// `vibrancy` lets the panel go fully transparent so the native macOS material
// shows through. Vibrancy only exists on macOS (and only inside Tauri), so
// elsewhere — dev:web, Linux/Windows builds — the CSS keeps an opaque surface
// and the transparent window flag is harmless.
const root = document.documentElement;
if (new URLSearchParams(window.location.search).get("view") === "tray") {
  root.dataset.view = "tray";
}
if ("__TAURI_INTERNALS__" in window && navigator.userAgent.includes("Mac")) {
  root.dataset.vibrancy = "1";
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
