import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { el } from "../dom";
import { accountKey, describeError } from "../helpers";
import { t } from "../i18n";
import { showErrorModal } from "../modals";
import { currentInstance, state } from "../state";
import { showTab } from "../tabs";
import type { DownloadProgress, LaunchStatusEvent, ScreenshotInfo } from "../types";

let playStage: "idle" | "installing" | "launching" | "running" = "idle";
let runningInstanceId: string | null = null; // not necessarily state.selectedInstanceId

export function runningInstance(): string | null {
  return runningInstanceId;
}
let installingLabel = "";
let installProgressPercent = 0;
let installingHasDownloads = false;

// Taken as a callback instead of importing accounts.ts to avoid a circular import.
let onSignInRequested: () => void = () => {};

export function renderPlayButton() {
  el.instancePickerTrigger.disabled = playStage !== "idle";
  if (el.instancePickerTrigger.disabled) el.instancePickerEl.classList.remove("is-open");

  if (playStage === "installing") {
    el.playButton.disabled = true;
    el.playLabelEl.textContent = installingHasDownloads ? t("play.installing") : t("play.checking");
    el.progressPanelEl.hidden = false;
    el.progressLabelEl.textContent = installingLabel;
    el.progressPercentEl.textContent = `${Math.round(installProgressPercent)}%`;
    el.progressFillEl.style.width = `${installProgressPercent}%`;
    return;
  }
  el.progressPanelEl.hidden = true;
  if (playStage === "launching") {
    el.playButton.disabled = true;
    el.playLabelEl.textContent = t("play.launching");
    return;
  }
  if (playStage === "running") {
    el.playButton.disabled = true;
    el.playLabelEl.textContent = t("play.running");
    return;
  }
  if (!state.currentAccount) {
    el.playButton.disabled = false;
    el.playLabelEl.textContent = t("play.signIn");
    return;
  }
  if (!state.selectedInstanceId) {
    el.playButton.disabled = false;
    el.playLabelEl.textContent = state.instances.length === 0 ? t("play.newInstance") : t("play.selectInstance");
    return;
  }
  el.playButton.disabled = false;
  el.playLabelEl.textContent = t("play.play");
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
  installingLabel = t("play.checkingFiles");
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
    const message = t("play.launchFailedPrefix", { message: describeError(err) });
    el.launchErrorEl.textContent = message;
    el.launchErrorEl.hidden = false;
    showErrorModal(message);
    playStage = "idle";
    renderPlayButton();
  }
}

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
    console.error(err);
    screenshots = [];
  }
  // Guards against a stale response clobbering a newer refreshPlayBackdrop() call.
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
    // Every launch re-verifies file SHA1s even with nothing to download; don't label it
    // "Downloading" unless something actually was.
    if (p.downloaded_done > 0) installingHasDownloads = true;
    const verb = installingHasDownloads ? t("play.downloading") : t("play.checkingVerb");
    installingLabel = p.files_total > 0 ? `${verb} ${phase} (${p.files_done}/${p.files_total})` : `${verb} ${phase}`;
    installProgressPercent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    renderPlayButton();
  });

  await listen<LaunchStatusEvent>("launch-status", (event) => {
    const { instanceId, status } = event.payload;
    playStage = status === "exited" ? "idle" : status;
    runningInstanceId = status === "exited" ? null : instanceId;
    renderPlayButton();
  });
}
