import { getCurrentWindow } from "@tauri-apps/api/window";

import { initContextMenu } from "./contextmenu";
import { initDom } from "./dom";
import { applyI18n } from "./i18n";
import { initKeyboard } from "./keyboard";
import * as accounts from "./features/accounts";
import * as contentBrowser from "./features/content-browser";
import * as instanceContent from "./features/instance-content";
import * as instances from "./features/instances";
import * as modLoader from "./features/mod-loader";
import * as play from "./features/play";
import * as settings from "./features/settings";
import * as skins from "./features/skins";
import { initModals } from "./modals";
import { initTabs } from "./tabs";
import { loadVersions } from "./versions";

async function main() {
  initDom();
  applyI18n();

  const appWindow = getCurrentWindow();
  document.querySelectorAll<HTMLButtonElement>("[data-window-action]").forEach((button) => {
    button.addEventListener("click", () => {
      switch (button.dataset.windowAction) {
        case "minimize":
          void appWindow.minimize();
          break;
        case "maximize":
          void appWindow.toggleMaximize();
          break;
        case "close":
          void appWindow.close();
          break;
      }
    });
  });
  // Double-clicking the drag region toggles maximize/restore, matching every native titlebar
  // (Windows, GNOME, Qt) -- `data-tauri-drag-region` alone only handles the drag itself.
  document.querySelectorAll<HTMLElement>("[data-tauri-drag-region]").forEach((region) => {
    region.addEventListener("dblclick", () => void appWindow.toggleMaximize());
  });

  initTabs();
  initModals();
  initContextMenu();
  initKeyboard();
  settings.init();
  instances.init();
  instanceContent.init();
  await modLoader.init();
  contentBrowser.init();
  skins.init(accounts.startSignIn);
  await play.init(accounts.startSignIn);
  await accounts.init();

  accounts.renderAccount();
  play.renderPlayButton();
  await Promise.all([accounts.loadAccounts(), instances.loadInstances(), loadVersions(), settings.loadDirectorySettings()]);
}

window.addEventListener("DOMContentLoaded", () => void main());
