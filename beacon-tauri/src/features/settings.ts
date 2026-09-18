import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import { el } from "../dom";
import { describeError, openFolder, setPathText } from "../helpers";
import { getLang, setLang, t, type Lang } from "../i18n";
import { closeAllScreens, showErrorModal } from "../modals";
import { state } from "../state";
import { showTab } from "../tabs";
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

function renderLanguagePicker() {
  const lang = getLang();
  el.languageOptions.forEach((option) => {
    const selected = option.dataset.lang === lang;
    option.classList.toggle("is-selected", selected);
    option.setAttribute("aria-checked", String(selected));
  });
}

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
    // best-effort: locked-down webview, just won't persist
  }
}

let currentTheme = readTheme();

function applyTheme() {
  // must match the inline head script's early-apply logic in index.html
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
  } catch {}
}

function renderSnapshotsToggle() {
  el.snapshotsToggle.classList.toggle("is-on", state.showSnapshots);
  el.snapshotsToggle.setAttribute("aria-checked", String(state.showSnapshots));
  el.snapshotsToggleLabel.textContent = state.showSnapshots ? t("toggle.on") : t("toggle.off");
}

const MOMENTS_TAB_ENABLED_KEY = "beacon:moments-tab-enabled";

function readMomentsTabEnabled(): boolean {
  try {
    return localStorage.getItem(MOMENTS_TAB_ENABLED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeMomentsTabEnabled(value: boolean) {
  try {
    localStorage.setItem(MOMENTS_TAB_ENABLED_KEY, value ? "1" : "0");
  } catch {}
}

function renderMomentsTabToggle() {
  el.momentsTabToggle.classList.toggle("is-on", state.momentsTabEnabled);
  el.momentsTabToggle.setAttribute("aria-checked", String(state.momentsTabEnabled));
  el.momentsTabToggleLabel.textContent = state.momentsTabEnabled ? t("toggle.on") : t("toggle.off");
  el.momentsTabBtn.hidden = !state.momentsTabEnabled;
  if (!state.momentsTabEnabled && el.momentsTabBtn.classList.contains("is-active")) showTab("installations");
}

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
  } catch {}
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
  } catch {}
}

function renderScreenshotsBgSettings() {
  el.screenshotsBgToggle.classList.toggle("is-on", state.screenshotsBgEnabled);
  el.screenshotsBgToggle.setAttribute("aria-checked", String(state.screenshotsBgEnabled));
  el.screenshotsBgToggleLabel.textContent = state.screenshotsBgEnabled ? t("toggle.on") : t("toggle.off");
  el.screenshotsBgBlurInput.disabled = !state.screenshotsBgEnabled;
  el.screenshotsBgBlurInput.value = String(state.screenshotsBgBlur);
  document.documentElement.style.setProperty("--screenshot-blur", `${state.screenshotsBgBlur}px`);
}

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
    el.gameDirPathEl.textContent = t("settings.unknown");
    el.instancesDirPathEl.textContent = t("settings.unknown");
    el.configDirPathEl.textContent = t("settings.unknown");
  }
}

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
  browseBtn.textContent = t("settings.moving");
  try {
    const newPath = await invoke<string>(command, { newPath: picked });
    setPathText(pathEl, newPath);
    await loadDirectorySettings(); // libraries_dir is a subfolder of game_dir, needs refresh too
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  } finally {
    directoriesBusy = false;
    browseBtn.textContent = originalLabel;
    renderDirectoriesBusyState();
  }
}

const CURSEFORGE_KEY_REQUEST_URL = "https://console.curseforge.com/";

async function refreshCurseForgeKeyStatus() {
  try {
    const hasKey = await invoke<boolean>("has_curseforge_api_key_cmd");
    el.curseforgeKeyStatusEl.textContent = hasKey ? t("settings.curseforge.set") : t("settings.curseforge.notSet");
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

const WIPE_CONFIRM_WORD = "WIPE";

function openWipeModal() {
  el.wipeConfirmInput.value = "";
  el.wipeConfirmBtn.disabled = true;
  el.wipeConfirmBtn.textContent = t("wipe.confirm");
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
  el.wipeConfirmBtn.textContent = t("wipe.wiping");
  try {
    // On success the backend exits the whole process; this call never resolves.
    await invoke("wipe_all_data_cmd");
  } catch (err) {
    console.error(err);
    hideWipeModal();
    showErrorModal(describeError(err));
  }
}

export function init() {
  state.showSnapshots = readShowSnapshots();
  state.momentsTabEnabled = readMomentsTabEnabled();
  state.screenshotsBgEnabled = readScreenshotsBgEnabled();
  state.screenshotsBgBlur = readScreenshotsBgBlur();

  el.settingsNavBtn.addEventListener("click", openSettingsScreen);
  el.settingsBackBtn.addEventListener("click", closeSettingsScreen);

  el.languageOptions.forEach((option) => {
    option.addEventListener("click", () => {
      const lang = option.dataset.lang;
      if (!lang || (lang !== "en" && lang !== "ru") || lang === getLang()) return;
      setLang(lang as Lang);
      renderLanguagePicker();
      renderSnapshotsToggle(); // not data-i18n, setLang() alone won't refresh these
      renderMomentsTabToggle();
      renderScreenshotsBgSettings();
      void refreshCurseForgeKeyStatus();
    });
  });
  renderLanguagePicker();

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

  el.momentsTabToggle.addEventListener("click", () => {
    state.momentsTabEnabled = !state.momentsTabEnabled;
    writeMomentsTabEnabled(state.momentsTabEnabled);
    renderMomentsTabToggle();
  });
  renderMomentsTabToggle();

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

  el.configDirOpenBtn.addEventListener("click", () => void openFolder(el.configDirPathEl.textContent ?? ""));

  el.wipeAllBtn.addEventListener("click", openWipeModal);
  el.wipeCancelBtn.addEventListener("click", hideWipeModal);
  el.wipeConfirmBtn.addEventListener("click", () => void performWipe());
  el.wipeConfirmInput.addEventListener("input", () => {
    el.wipeConfirmBtn.disabled = el.wipeConfirmInput.value.trim().toUpperCase() !== WIPE_CONFIRM_WORD;
  });
}
