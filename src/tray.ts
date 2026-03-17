import { TrayIcon } from "@tauri-apps/api/tray";
import { Menu, MenuItem } from "@tauri-apps/api/menu";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { defaultWindowIcon } from "@tauri-apps/api/app";

// Tracks the unlisten fn for focus-change listener so we can clean it up
let _focusUnlisten: (() => void) | null = null;

async function getPopup(): Promise<WebviewWindow | null> {
  return WebviewWindow.getByLabel("tray-popup");
}

async function hidePopup(): Promise<void> {
  const popup = await getPopup();
  if (!popup) return;
  await popup.hide();
  if (_focusUnlisten) {
    _focusUnlisten();
    _focusUnlisten = null;
  }
}

async function showPopup(
  trayRect: { position: { x: number; y: number }; size: { width: number; height: number } }
): Promise<void> {
  const popup = await getPopup();
  if (!popup) return;

  // Position below the tray icon, centered horizontally under it.
  // trayRect is in physical pixels; popup width is 300 logical px.
  const scaleFactor = await popup.scaleFactor();
  const x = Math.round(trayRect.position.x + trayRect.size.width / 2 - 150 * scaleFactor);
  const y = Math.round(trayRect.position.y + trayRect.size.height + 4 * scaleFactor);

  await popup.setPosition(new PhysicalPosition(x, y));
  await popup.show();
  await popup.setFocus();

  // Re-register focus listener (clean up previous first to avoid leaks)
  if (_focusUnlisten) {
    _focusUnlisten();
    _focusUnlisten = null;
  }
  _focusUnlisten = await popup.onFocusChanged(async ({ payload: focused }) => {
    if (!focused) await hidePopup();
  });
}

async function togglePopup(
  trayRect: { position: { x: number; y: number }; size: { width: number; height: number } }
): Promise<void> {
  const popup = await getPopup();
  if (!popup) return;
  // Use actual window visibility as source of truth (avoids JS state racing with focus-loss events)
  const isVisible = await popup.isVisible();
  if (isVisible) {
    await hidePopup();
  } else {
    await showPopup(trayRect);
  }
}

export async function initTray(): Promise<void> {
  // Guard: only initialize from the main window
  const win = getCurrentWindow();
  if (win.label !== "main") return;

  const menu = await Menu.new({
    items: [
      await MenuItem.new({
        text: "Open Token Watch",
        action: async () => {
          const main = await WebviewWindow.getByLabel("main");
          if (main) {
            await main.show();
            await main.setFocus();
          }
        },
      }),
      await MenuItem.new({
        text: "Quit",
        action: async () => {
          await invoke("quit_app");
        },
      }),
    ],
  });

  const icon = await defaultWindowIcon();

  await TrayIcon.new({
    id: "main",
    tooltip: "Token Watch",
    icon: icon ?? undefined,
    iconAsTemplate: true, // macOS: auto-inverts for light/dark menu bar
    menu,
    showMenuOnLeftClick: false, // Left-click toggles popup; right-click shows menu
    action: async (event) => {
      if (event.type === "Click" && event.button === "Left" && event.buttonState === "Up") {
        await togglePopup(event.rect);
      }
    },
  });
}
