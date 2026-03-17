import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./App.css";
import { initTray } from "./tray";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setDockVisibility } from "@tauri-apps/api/app";

// Only run main-window setup for the "main" window (not tray-popup)
const win = getCurrentWindow();
if (win.label === "main") {
  // Hide from Dock — this is a menu bar app
  setDockVisibility(false).catch(console.error);

  // Hide to tray on close instead of quitting
  win.onCloseRequested(async (event) => {
    event.preventDefault();
    await win.hide();
  });

  // Initialize system tray icon and handlers
  initTray().catch(console.error);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
