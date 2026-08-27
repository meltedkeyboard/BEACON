// Slim bootstrap: wires up the DOM refs, generic UI (modals/tabs/window controls), each
// feature module's own event listeners, then kicks off the startup data load. Actual feature
// logic lives under `./features/*` (plus `./versions` for the shared version list) -- see each
// file's own header comment for what it owns.

import { getCurrentWindow } from "@tauri-apps/api/window";

import { initDom } from "./dom";
import * as accounts from "./features/accounts";
import * as instanceContent from "./features/instance-content";
import * as instances from "./features/instances";
import * as modBrowser from "./features/mod-browser";
import * as modLoader from "./features/mod-loader";
import * as play from "./features/play";
import * as settings from "./features/settings";
import * as skins from "./features/skins";
import { initModals } from "./modals";
import { initTabs } from "./tabs";
import { loadVersions } from "./versions";

async function main() {
  initDom();

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

  initTabs();
  initModals();
  settings.init();
  instances.init();
  instanceContent.init();
  await modLoader.init();
  modBrowser.init();
  skins.init(accounts.startSignIn);
  await play.init(accounts.startSignIn);
  await accounts.init();

  accounts.renderAccount();
  play.renderPlayButton();
  await Promise.all([accounts.loadAccounts(), instances.loadInstances(), loadVersions(), settings.loadDirectorySettings()]);
}

window.addEventListener("DOMContentLoaded", () => void main());
