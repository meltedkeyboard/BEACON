// Play button state machine (install/launch), plus the Play-tab screenshot backdrop that shows
// the currently selected instance's own screenshots behind it.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { el } from "../dom";
import { accountKey, describeError } from "../helpers";
import { showErrorModal } from "../modals";
import { currentInstance, state } from "../state";
import { showTab } from "../tabs";
import type { DownloadProgress, ScreenshotInfo } from "../types";

let playStage: "idle" | "installing" | "launching" = "idle";
let installingLabel = "Installing...";
let installProgressPercent = 0;
// Whether any `install-progress` event so far this launch has actually downloaded a file, as
// opposed to just re-verifying files that were already there -- most launches download nothing at
// all, and the Play button claiming "Installing..." the whole time it's silently re-hashing
// thousands of already-good files is exactly backwards from what's happening.
let installingHasDownloads = false;

// Set via `init()` -- clicking Play while signed out reuses the exact same device-code sign-in
// flow as the account menu's "Sign in" button (owned by `features/accounts.ts`). Taking it as a
// callback instead of importing accounts.ts directly avoids a circular import: accounts.ts needs
// to call `renderPlayButton`/`refreshPlayBackdrop` from *this* module after a sign-in completes.
let onSignInRequested: () => void = () => {};

export function renderPlayButton() {
  el.instancePickerTrigger.disabled = playStage !== "idle";
  if (el.instancePickerTrigger.disabled) el.instancePickerEl.classList.remove("is-open");

  if (playStage === "installing") {
    el.playButton.disabled = true;
    el.playLabelEl.textContent = installingHasDownloads ? "Installing..." : "Checking...";
    el.progressPanelEl.hidden = false;
    el.progressLabelEl.textContent = installingLabel;
    el.progressPercentEl.textContent = `${Math.round(installProgressPercent)}%`;
    el.progressFillEl.style.width = `${installProgressPercent}%`;
    return;
  }
  el.progressPanelEl.hidden = true;
  if (playStage === "launching") {
    el.playButton.disabled = true;
    el.playLabelEl.textContent = "Launching...";
    return;
  }
  if (!state.currentAccount) {
    el.playButton.disabled = false;
    el.playLabelEl.textContent = "Sign In";
    return;
  }
  if (!state.selectedInstanceId) {
    el.playButton.disabled = false;
    el.playLabelEl.textContent = state.instances.length === 0 ? "New instance" : "Select an instance";
    return;
  }
  el.playButton.disabled = false;
  el.playLabelEl.textContent = "Play";
}

async function handlePlayClick() {
  if (playStage !== "idle") return;
  if (!state.currentAccount) {
    onSignInRequested();
    return;
  }
  if (!state.selectedInstanceId) {
    showTab("installations");
    return;
  }

  el.launchErrorEl.hidden = true;
  playStage = "installing";
  installingLabel = "Checking files...";
  installProgressPercent = 0;
  installingHasDownloads = false;
  renderPlayButton();

  try {
    await invoke("launch_instance_cmd", {
      instanceId: state.selectedInstanceId,
      account: { type: "saved", accountId: accountKey(state.currentAccount) },
    });
  } catch (err) {
    console.error(err);
    const message = `Couldn't launch: ${describeError(err)}`;
    // The inline bar is easy to miss (small, below the fold if the window's short) -- a modal
    // guarantees the user actually sees why Play didn't work instead of wondering if it's stuck.
    el.launchErrorEl.textContent = message;
    el.launchErrorEl.hidden = false;
    showErrorModal(message);
  } finally {
    playStage = "idle";
    renderPlayButton();
  }
}

// ---------- screenshot backdrop ----------

let playBackdropScreenshots: ScreenshotInfo[] = [];
let playBackdropIndex = 0;
let playBackdropActiveLayer: "a" | "b" = "a";
let playBackdropTimer: ReturnType<typeof setInterval> | null = null;

function stopPlayBackdropTimer() {
  if (playBackdropTimer !== null) {
    clearInterval(playBackdropTimer);
    playBackdropTimer = null;
  }
}

// Crossfades to `screenshot` by loading it into whichever of the two layers is currently
// hidden, then swapping which one is `.is-active` -- avoids the flash a single layer would show
// if its `background-image` changed while still visible.
function showBackdropImage(screenshot: ScreenshotInfo) {
  const nextLayer = playBackdropActiveLayer === "a" ? el.heroBackdropBEl : el.heroBackdropAEl;
  const currentLayer = playBackdropActiveLayer === "a" ? el.heroBackdropAEl : el.heroBackdropBEl;
  nextLayer.style.backgroundImage = `url("${convertFileSrc(screenshot.path)}")`;
  nextLayer.classList.add("is-active");
  currentLayer.classList.remove("is-active");
  playBackdropActiveLayer = playBackdropActiveLayer === "a" ? "b" : "a";
}

const PLAY_BACKDROP_ROTATE_MS = 9000;

export async function refreshPlayBackdrop() {
  stopPlayBackdropTimer();
  const instance = currentInstance();

  if (!state.screenshotsBgEnabled || !instance) {
    el.heroEl.classList.remove("has-backdrop");
    playBackdropScreenshots = [];
    return;
  }

  let screenshots: ScreenshotInfo[];
  try {
    screenshots = await invoke<ScreenshotInfo[]>("list_screenshots_cmd", { instanceId: instance.id });
  } catch (err) {
    // The backdrop is decorative -- a failure here shouldn't interrupt anything the user is
    // doing, just fall back to the placeholder like "no screenshots" would.
    console.error(err);
    screenshots = [];
  }
  // The selected instance (or the toggle) may have changed while the request was in flight --
  // don't let a stale response clobber whatever `refreshPlayBackdrop` ran after this one.
  if (currentInstance()?.id !== instance.id || !state.screenshotsBgEnabled) return;

  playBackdropScreenshots = screenshots;

  const pinned = instance.pinned_screenshot
    ? screenshots.find((s) => s.name === instance.pinned_screenshot)
    : undefined;

  if (screenshots.length === 0) {
    el.heroEl.classList.remove("has-backdrop");
    return;
  }

  el.heroEl.classList.add("has-backdrop");
  if (pinned) {
    showBackdropImage(pinned);
    return;
  }

  playBackdropIndex = 0;
  showBackdropImage(screenshots[0]);
  if (screenshots.length > 1) {
    playBackdropTimer = setInterval(() => {
      playBackdropIndex = (playBackdropIndex + 1) % playBackdropScreenshots.length;
      showBackdropImage(playBackdropScreenshots[playBackdropIndex]);
    }, PLAY_BACKDROP_ROTATE_MS);
  }
}

export async function init(signInRequested: () => void) {
  onSignInRequested = signInRequested;

  el.playButton.addEventListener("click", () => void handlePlayClick());

  await listen<DownloadProgress>("install-progress", (event) => {
    if (playStage !== "installing") return;
    const p = event.payload;
    const phase = p.phase || "Files";
    // Every launch re-verifies every file's SHA1 even when nothing actually needs downloading --
    // that's still real work (and still worth a progress bar for a big instance), but calling it
    // "Downloading" when `downloaded_done` never leaves 0 is just wrong: nothing was downloaded,
    // Beacon was only checking what's already on disk.
    if (p.downloaded_done > 0) installingHasDownloads = true;
    const verb = installingHasDownloads ? "Downloading" : "Checking";
    installingLabel = p.files_total > 0 ? `${verb} ${phase} (${p.files_done}/${p.files_total})` : `${verb} ${phase}`;
    installProgressPercent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    renderPlayButton();
  });

  await listen<string>("launch-status", (event) => {
    if (event.payload === "launching" && playStage === "installing") {
      playStage = "launching";
      renderPlayButton();
    }
  });
}
