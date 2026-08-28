// Settings screen: theme, "show snapshots" toggle, Play-tab-background toggle+blur, directory
// relocation (game_dir/instances_dir), and wipe-all-data.

import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import { el } from "../dom";
import { describeError, openFolder, setPathText } from "../helpers";
import { closeAllScreens, showErrorModal } from "../modals";
import { state } from "../state";
import type { DirectorySettings } from "../types";
import { loadVersions } from "../versions";
import * as play from "./play";

function openSettingsScreen() {
  closeAllScreens();
  el.settingsScreenEl.classList.add("is-open");
}

function closeSettingsScreen() {
  el.settingsScreenEl.classList.remove("is-open");
}

// ---------- theme ----------

const THEMES = ["beacon", "amber", "light", "amber-light", "starlight"] as const;
type Theme = (typeof THEMES)[number];
const DEFAULT_THEME: Theme = "beacon";
const THEME_KEY = "beacon:theme";

function isTheme(value: string): value is Theme {
  return (THEMES as readonly string[]).includes(value);
}

function readTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    return stored && isTheme(stored) ? stored : DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

function writeTheme(theme: Theme) {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Best-effort, same as the snapshots toggle -- just won't be remembered next launch.
  }
}

let currentTheme = readTheme();

function applyTheme() {
  // The default theme has no [data-theme] block (it lives on bare :root), so leave the
  // attribute off entirely rather than writing "beacon" -- keeps the inline
  // head script's early-apply logic (which only sets the attribute for a *non-default*
  // saved theme) and this in agreement about what "default" looks like in the DOM.
  if (currentTheme === DEFAULT_THEME) {
    document.documentElement.removeAttribute("data-theme");
  } else {
    document.documentElement.setAttribute("data-theme", currentTheme);
  }
  el.themeOptions.forEach((option) => {
    const selected = option.dataset.theme === currentTheme;
    option.classList.toggle("is-selected", selected);
    option.setAttribute("aria-checked", String(selected));
  });
}

// ---------- show snapshots ----------

const SHOW_SNAPSHOTS_KEY = "beacon:show-snapshots";

function readShowSnapshots(): boolean {
  try {
    return localStorage.getItem(SHOW_SNAPSHOTS_KEY) === "1";
  } catch {
    return false;
  }
}

function writeShowSnapshots(value: boolean) {
  try {
    localStorage.setItem(SHOW_SNAPSHOTS_KEY, value ? "1" : "0");
  } catch {
    // Best-effort -- a private/locked-down webview can throw here; the toggle still works
    // for the rest of the session, it just won't remember next launch.
  }
}

function renderSnapshotsToggle() {
  el.snapshotsToggle.classList.toggle("is-on", state.showSnapshots);
  el.snapshotsToggle.setAttribute("aria-checked", String(state.showSnapshots));
  el.snapshotsToggleLabel.textContent = state.showSnapshots ? "On" : "Off";
}

// ---------- Play tab screenshot background ----------
// Purely cosmetic, per-device preferences -- same localStorage treatment as the theme and
// snapshots toggle above, not config.json (that's reserved for data tied to the instance
// itself, like which screenshot is pinned).

const SCREENSHOTS_BG_ENABLED_KEY = "beacon:screenshots-bg-enabled";
const SCREENSHOTS_BG_BLUR_KEY = "beacon:screenshots-bg-blur";
const DEFAULT_SCREENSHOTS_BLUR = 6;

function readScreenshotsBgEnabled(): boolean {
  try {
    const stored = localStorage.getItem(SCREENSHOTS_BG_ENABLED_KEY);
    return stored === null ? true : stored === "1";
  } catch {
    return true;
  }
}

function writeScreenshotsBgEnabled(value: boolean) {
  try {
    localStorage.setItem(SCREENSHOTS_BG_ENABLED_KEY, value ? "1" : "0");
  } catch {
    // Best-effort, same as the other settings above.
  }
}

function readScreenshotsBgBlur(): number {
  try {
    const stored = Number(localStorage.getItem(SCREENSHOTS_BG_BLUR_KEY));
    return Number.isFinite(stored) && stored >= 0 && stored <= 20 ? stored : DEFAULT_SCREENSHOTS_BLUR;
  } catch {
    return DEFAULT_SCREENSHOTS_BLUR;
  }
}

function writeScreenshotsBgBlur(value: number) {
  try {
    localStorage.setItem(SCREENSHOTS_BG_BLUR_KEY, String(value));
  } catch {
    // Best-effort, same as the other settings above.
  }
}

function renderScreenshotsBgSettings() {
  el.screenshotsBgToggle.classList.toggle("is-on", state.screenshotsBgEnabled);
  el.screenshotsBgToggle.setAttribute("aria-checked", String(state.screenshotsBgEnabled));
  el.screenshotsBgToggleLabel.textContent = state.screenshotsBgEnabled ? "On" : "Off";
  el.screenshotsBgBlurInput.disabled = !state.screenshotsBgEnabled;
  el.screenshotsBgBlurInput.value = String(state.screenshotsBgBlur);
  document.documentElement.style.setProperty("--screenshot-blur", `${state.screenshotsBgBlur}px`);
}

// ---------- directory settings ----------

let directoriesBusy = false;

function renderDirectoriesBusyState() {
  el.gameDirBrowseBtn.disabled = directoriesBusy;
  el.gameDirOpenBtn.disabled = directoriesBusy;
  el.instancesDirBrowseBtn.disabled = directoriesBusy;
  el.instancesDirOpenBtn.disabled = directoriesBusy;
}

export async function loadDirectorySettings() {
  try {
    const settings = await invoke<DirectorySettings>("get_directory_settings");
    state.directorySettings = settings;
    setPathText(el.gameDirPathEl, settings.game_dir);
    setPathText(el.instancesDirPathEl, settings.instances_dir);
    setPathText(el.configDirPathEl, settings.config_dir);
  } catch (err) {
    console.error(err);
    el.gameDirPathEl.textContent = "Unknown";
    el.instancesDirPathEl.textContent = "Unknown";
    el.configDirPathEl.textContent = "Unknown";
  }
}

// Shared by both rows below -- picks a new folder, moves the actual files into it (not just
// the config pointer) via the given command, and reflects the result once it's done. Disabled
// while busy: relocating a large instances directory can take a while, and starting a second
// move (of either directory) before the first finishes isn't something the backend needs to
// handle if the UI simply doesn't offer it.
async function relocateDirectory(
  command: "set_game_dir_cmd" | "set_instances_dir_cmd",
  pathEl: HTMLElement,
  browseBtn: HTMLButtonElement,
  currentValue: string,
) {
  if (directoriesBusy) return;
  const picked = await openFileDialog({ directory: true, multiple: false, defaultPath: currentValue });
  if (!picked || Array.isArray(picked)) return;

  directoriesBusy = true;
  renderDirectoriesBusyState();
  const originalLabel = browseBtn.textContent;
  browseBtn.textContent = "Moving...";
  try {
    const newPath = await invoke<string>(command, { newPath: picked });
    setPathText(pathEl, newPath);
    // `game_dir` moving also moves `libraries_dir` (a subfolder of it) -- refresh the cached
    // settings so "Open libraries" on the instance screen doesn't open the old location.
    await loadDirectorySettings();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  } finally {
    directoriesBusy = false;
    browseBtn.textContent = originalLabel;
    renderDirectoriesBusyState();
  }
}

// ---------- CurseForge API key ----------
// Stored via the OS credential store (see beacon-core::secret_store), never round-tripped back to
// this renderer once saved -- only whether one exists, via has_curseforge_api_key_cmd.

const CURSEFORGE_KEY_REQUEST_URL = "https://console.curseforge.com/";

async function refreshCurseForgeKeyStatus() {
  try {
    const hasKey = await invoke<boolean>("has_curseforge_api_key_cmd");
    el.curseforgeKeyStatusEl.textContent = hasKey ? "Key set." : "Not set.";
  } catch (err) {
    console.error(err);
  }
}

async function saveCurseForgeKey() {
  const key = el.curseforgeKeyInput.value.trim();
  if (!key) return;
  try {
    await invoke("set_curseforge_api_key_cmd", { key });
    el.curseforgeKeyInput.value = "";
    await refreshCurseForgeKeyStatus();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function clearCurseForgeKey() {
  try {
    await invoke("set_curseforge_api_key_cmd", { key: null });
    el.curseforgeKeyInput.value = "";
    await refreshCurseForgeKeyStatus();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

// ---------- wipe all data ----------
//
// Deliberately not reusing the generic single-click confirm modal used for deleting one
// instance/world -- this deletes every account, instance and setting at once, so it's gated
// behind typing a confirmation word rather than a single click.

const WIPE_CONFIRM_WORD = "WIPE";

function openWipeModal() {
  el.wipeConfirmInput.value = "";
  el.wipeConfirmBtn.disabled = true;
  el.wipeConfirmBtn.textContent = "Wipe everything";
  el.wipeCancelBtn.disabled = false;
  el.wipeModalEl.classList.add("is-open");
  el.wipeConfirmInput.focus();
}

function hideWipeModal() {
  el.wipeModalEl.classList.remove("is-open");
}

async function performWipe() {
  if (el.wipeConfirmInput.value.trim().toUpperCase() !== WIPE_CONFIRM_WORD) return;
  el.wipeConfirmBtn.disabled = true;
  el.wipeCancelBtn.disabled = true;
  el.wipeConfirmBtn.textContent = "Wiping...";
  try {
    // On success the backend deletes everything and exits the whole process -- this call
    // never resolves, so there's nothing to handle after it.
    await invoke("wipe_all_data_cmd");
  } catch (err) {
    console.error(err);
    hideWipeModal();
    showErrorModal(describeError(err));
  }
}

export function init() {
  state.showSnapshots = readShowSnapshots();
  state.screenshotsBgEnabled = readScreenshotsBgEnabled();
  state.screenshotsBgBlur = readScreenshotsBgBlur();

  el.settingsNavBtn.addEventListener("click", openSettingsScreen);
  el.settingsBackBtn.addEventListener("click", closeSettingsScreen);

  el.themeOptions.forEach((option) => {
    option.addEventListener("click", () => {
      const theme = option.dataset.theme;
      if (!theme || !isTheme(theme) || theme === currentTheme) return;
      currentTheme = theme;
      writeTheme(currentTheme);
      applyTheme();
    });
  });
  applyTheme();

  el.snapshotsToggle.addEventListener("click", () => {
    state.showSnapshots = !state.showSnapshots;
    writeShowSnapshots(state.showSnapshots);
    renderSnapshotsToggle();
    void loadVersions();
  });
  renderSnapshotsToggle();

  el.screenshotsBgToggle.addEventListener("click", () => {
    state.screenshotsBgEnabled = !state.screenshotsBgEnabled;
    writeScreenshotsBgEnabled(state.screenshotsBgEnabled);
    renderScreenshotsBgSettings();
    void play.refreshPlayBackdrop();
  });
  el.screenshotsBgBlurInput.addEventListener("input", () => {
    state.screenshotsBgBlur = Number(el.screenshotsBgBlurInput.value);
    writeScreenshotsBgBlur(state.screenshotsBgBlur);
    renderScreenshotsBgSettings();
  });
  renderScreenshotsBgSettings();

  el.curseforgeKeySaveBtn.addEventListener("click", () => void saveCurseForgeKey());
  el.curseforgeKeyClearBtn.addEventListener("click", () => void clearCurseForgeKey());
  el.curseforgeKeyRequestBtn.addEventListener("click", () => void openUrl(CURSEFORGE_KEY_REQUEST_URL));
  void refreshCurseForgeKeyStatus();

  el.gameDirOpenBtn.addEventListener("click", () => void openFolder(el.gameDirPathEl.textContent ?? ""));
  el.gameDirBrowseBtn.addEventListener("click", () =>
    void relocateDirectory("set_game_dir_cmd", el.gameDirPathEl, el.gameDirBrowseBtn, el.gameDirPathEl.textContent ?? ""),
  );

  el.instancesDirOpenBtn.addEventListener("click", () => void openFolder(el.instancesDirPathEl.textContent ?? ""));
  el.instancesDirBrowseBtn.addEventListener("click", () =>
    void relocateDirectory(
      "set_instances_dir_cmd",
      el.instancesDirPathEl,
      el.instancesDirBrowseBtn,
      el.instancesDirPathEl.textContent ?? "",
    ),
  );

  // Read-only -- see `DirectorySettings.config_dir` on the Rust side for why this one has no
  // Browse button.
  el.configDirOpenBtn.addEventListener("click", () => void openFolder(el.configDirPathEl.textContent ?? ""));

  el.wipeAllBtn.addEventListener("click", openWipeModal);
  el.wipeCancelBtn.addEventListener("click", hideWipeModal);
  el.wipeConfirmBtn.addEventListener("click", () => void performWipe());
  el.wipeConfirmInput.addEventListener("input", () => {
    el.wipeConfirmBtn.disabled = el.wipeConfirmInput.value.trim().toUpperCase() !== WIPE_CONFIRM_WORD;
  });
}
