// Instance grid (Installations tab), instance picker (playbar dropdown), create/import instance,
// and the instance-detail screen's own chrome: open/close, rename, change version, icon pick/
// clear, open-folder/export/delete. Folder *content* (mods/worlds/packs/screenshots) lives in
// `./instance-content` instead -- a distinct concern from instance identity CRUD.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { el } from "../dom";
import { accountKey, applyDecorativeIcon, describeError, instanceIconBackground, openFolder } from "../helpers";
import { t } from "../i18n";
import { closeAllScreens, openConfirmModal, showErrorModal } from "../modals";
import { currentInstance, state } from "../state";
import type { Instance, InstancesResponse, LaunchStatusEvent } from "../types";
import { firstVersionId, renderVersionOptions } from "../versions";
import * as contentBrowser from "./content-browser";
import { refreshInstanceContent } from "./instance-content";
import * as modLoader from "./mod-loader";
import * as play from "./play";

// ---------- instance picker (playbar) ----------

function renderInstancePickerTrigger() {
  const current = currentInstance();
  el.playbarInstanceNameEl.textContent = current
    ? current.name
    : state.instances.length === 0
      ? t("instances.playbar.noInstances")
      : t("instances.picker.selectInstance");
  el.playbarInstanceVersionEl.textContent = current ? current.version_id : "—";
  applyDecorativeIcon(el.instancePickerIconEl, current);
}

function renderInstancePickerList() {
  el.instancePickerListEl.replaceChildren();
  if (state.instances.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = t("instances.picker.emptyList");
    el.instancePickerListEl.appendChild(empty);
    return;
  }
  for (const instance of state.instances) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "nav-row version-row";
    row.classList.toggle("is-selected", instance.id === state.selectedInstanceId);

    const nameSpan = document.createElement("span");
    nameSpan.className = "version-row__title";
    nameSpan.textContent = instance.name;

    const metaSpan = document.createElement("span");
    metaSpan.className = "version-row__meta";
    metaSpan.textContent = instance.version_id;

    row.append(nameSpan, metaSpan);
    row.addEventListener("click", () => void selectInstance(instance.id));
    el.instancePickerListEl.appendChild(row);
  }
}

function openInstancePicker() {
  if (el.instancePickerTrigger.disabled) return;
  el.instancePickerEl.classList.add("is-open");
}

function closeInstancePicker() {
  el.instancePickerEl.classList.remove("is-open");
}

async function selectInstance(instanceId: string) {
  closeInstancePicker();
  if (instanceId === state.selectedInstanceId) return;
  try {
    await invoke("select_instance_cmd", { instanceId });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

// ---------- installations tab: instance grid ----------

function renderInstanceGrid() {
  el.instanceGridEl.replaceChildren();
  if (state.instances.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = t("instances.grid.empty");
    el.instanceGridEl.appendChild(empty);
    return;
  }
  for (const instance of state.instances) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "instance-card";

    const icon = document.createElement("span");
    icon.className = "instance-card__icon";
    icon.setAttribute("aria-hidden", "true");
    applyDecorativeIcon(icon, instance);

    const name = document.createElement("span");
    name.className = "instance-card__name";
    name.textContent = instance.name;
    name.title = instance.name;

    const version = document.createElement("span");
    version.className = "instance-card__version";
    version.textContent = instance.version_id;

    card.append(icon, name, version);
    card.addEventListener("click", () => openInstanceDetail(instance.id));
    el.instanceGridEl.appendChild(card);
  }
}

export async function loadInstances() {
  try {
    const response = await invoke<InstancesResponse>("list_instances");
    state.instances = response.instances;
    state.selectedInstanceId = response.selected_id;
  } catch (err) {
    console.error(err);
    state.instances = [];
    state.selectedInstanceId = null;
  }
  renderInstancePickerTrigger();
  renderInstancePickerList();
  renderInstanceGrid();
  play.renderPlayButton();
  if (state.viewingInstanceId) renderInstanceDetail();
  void play.refreshPlayBackdrop();
}

// ---------- create instance ----------

let createInstanceSelectedVersion: string | null = null;

// The name field auto-fills with the picked version ("1.21.4") so creating an instance needs no
// typing at all -- but only until the user actually edits it themselves, tracked here so picking
// a different version afterwards doesn't clobber a name they already chose.
let createInstanceNameIsAuto = true;

function pickCreateInstanceVersion(versionId: string) {
  createInstanceSelectedVersion = versionId;
  renderVersionOptions(el.createInstanceVersionsEl, createInstanceSelectedVersion, pickCreateInstanceVersion);
  if (createInstanceNameIsAuto) el.createInstanceNameInput.value = versionId;
}

function openCreateInstanceModal() {
  createInstanceSelectedVersion = firstVersionId();
  createInstanceNameIsAuto = true;
  el.createInstanceNameInput.value = createInstanceSelectedVersion ?? "";
  el.createInstanceErrorEl.hidden = true;
  renderVersionOptions(el.createInstanceVersionsEl, createInstanceSelectedVersion, pickCreateInstanceVersion);
  el.createInstanceModalEl.classList.add("is-open");
  el.createInstanceNameInput.focus();
}

function hideCreateInstanceModal() {
  el.createInstanceModalEl.classList.remove("is-open");
}

async function confirmCreateInstance() {
  const name = el.createInstanceNameInput.value.trim();
  if (!name) {
    el.createInstanceErrorEl.textContent = "Give the instance a name.";
    el.createInstanceErrorEl.hidden = false;
    return;
  }
  if (!createInstanceSelectedVersion) {
    el.createInstanceErrorEl.textContent = "Pick a version.";
    el.createInstanceErrorEl.hidden = false;
    return;
  }
  try {
    await invoke("create_instance_cmd", { name, versionId: createInstanceSelectedVersion });
    hideCreateInstanceModal();
    await loadInstances();
  } catch (err) {
    console.error(err);
    el.createInstanceErrorEl.textContent = describeError(err);
    el.createInstanceErrorEl.hidden = false;
  }
}

// ---------- import ----------

async function importInstance() {
  try {
    const sourcePath = await openFileDialog({
      multiple: false,
      filters: [{ name: "Beacon instance", extensions: ["zip"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    await invoke("import_instance_cmd", { sourcePath });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

// ---------- instance detail tabs ----------
// A second, independent tab bar scoped inside this one screen -- see styles.css's own comment on
// `.instance-tabs` for why this doesn't reuse `tabs.ts`'s top-level `showTab`/`[data-tab]` handling.

let instanceTabs: NodeListOf<HTMLButtonElement>;
let instanceTabPanels: NodeListOf<HTMLElement>;

function showInstanceTab(target: string) {
  instanceTabs.forEach((t) => t.classList.toggle("is-active", t.dataset.instanceTab === target));
  instanceTabPanels.forEach((panel) => panel.classList.toggle("is-active", panel.dataset.instanceTabPanel === target));
}

// ---------- instance detail: Start/Stop + game log ----------
// Independent from the playbar's own Play button (`features/play.ts`) -- this lets a user launch
// or stop the instance they're currently customizing without leaving the detail screen. Only one
// instance can ever be launching/running at a time (enforced backend-side).
//
// Tracked with its own local mirror (`runningInstanceId` below) rather than reading
// `play.runningInstance()` inside this screen's own `launch-status` handler -- both modules
// listen for the same event independently, and `instances.init()` runs before `play.init()`
// (see `main.ts`), so this screen's callback can fire *before* play.ts has updated its copy for
// the same event. Deriving straight from the event payload avoids that ordering race.
let runningInstanceId: string | null = null;
// True only for the (short) window between clicking Start and the backend's first "launching"
// event -- once that arrives, `runningInstanceId` takes over as the source of truth.
let instanceLaunchStarting = false;

// Buffered so switching away from the Advanced tab and back doesn't lose lines received while it
// wasn't visible -- cleared and re-tagged every time a *new* launch's first log line would
// otherwise get appended after stale lines from a previous run of the same instance.
let gameLogInstanceId: string | null = null;
let gameLogLines: string[] = [];

function renderGameLog() {
  if (gameLogInstanceId !== state.viewingInstanceId || gameLogLines.length === 0) {
    el.advancedLogOutputEl.textContent = "No active session for this instance.";
    return;
  }
  el.advancedLogOutputEl.textContent = gameLogLines.join("\n");
  el.advancedLogOutputEl.scrollTop = el.advancedLogOutputEl.scrollHeight;
}

function renderInstancePlayPauseButton() {
  const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
  if (!instance) return;
  const running = runningInstanceId;
  const btn = el.instancePlayPauseBtn;
  btn.classList.remove("modal__btn--danger", "modal__btn--primary");

  if (running === instance.id) {
    btn.disabled = false;
    btn.textContent = "Stop";
    btn.classList.add("modal__btn--danger");
    return;
  }
  if (running) {
    btn.disabled = true;
    btn.textContent = "Another instance is running";
    return;
  }
  if (instanceLaunchStarting) {
    btn.disabled = true;
    btn.textContent = "Starting…";
    return;
  }
  btn.disabled = false;
  btn.textContent = "Start";
  btn.classList.add("modal__btn--primary");
}

async function handleInstancePlayPauseClick() {
  const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
  if (!instance) return;
  const running = runningInstanceId;

  if (running === instance.id) {
    try {
      await invoke("stop_instance_cmd", { instanceId: instance.id });
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
    return;
  }
  if (running || instanceLaunchStarting) return;

  if (!state.currentAccount) {
    showErrorModal("Sign in to an account first.");
    return;
  }

  el.instanceLaunchErrorEl.hidden = true;
  instanceLaunchStarting = true;
  renderInstancePlayPauseButton();
  try {
    await invoke("launch_instance_cmd", {
      instanceId: instance.id,
      account: { type: "saved", accountId: accountKey(state.currentAccount) },
    });
  } catch (err) {
    console.error(err);
    const message = `Couldn't launch: ${describeError(err)}`;
    el.instanceLaunchErrorEl.textContent = message;
    el.instanceLaunchErrorEl.hidden = false;
    showErrorModal(message);
  } finally {
    instanceLaunchStarting = false;
    renderInstancePlayPauseButton();
  }
}

function initInstanceLaunchControls() {
  el.instancePlayPauseBtn.addEventListener("click", () => void handleInstancePlayPauseClick());
  el.advancedLogClearBtn.addEventListener("click", () => {
    gameLogLines = [];
    renderGameLog();
  });

  void invoke<string | null>("running_instance_cmd").then((id) => {
    runningInstanceId = id;
    if (state.viewingInstanceId) renderInstancePlayPauseButton();
  });

  void listen<LaunchStatusEvent>("launch-status", (event) => {
    const { instanceId, status } = event.payload;
    runningInstanceId = status === "exited" ? null : instanceId;
    if (status === "launching") {
      gameLogInstanceId = instanceId;
      gameLogLines = [];
    }
    if (state.viewingInstanceId === instanceId || (status === "exited" && gameLogInstanceId === instanceId)) {
      renderGameLog();
    }
    if (state.viewingInstanceId) renderInstancePlayPauseButton();
  });

  void listen<string>("game-log", (event) => {
    if (!gameLogInstanceId) return;
    gameLogLines.push(event.payload);
    if (gameLogLines.length > 2000) gameLogLines.shift();
    if (gameLogInstanceId === state.viewingInstanceId) renderGameLog();
  });
}

// ---------- overflow menu (Clear icon / Open .minecraft / Open libraries / Export) ----------

function closeOverflowMenu() {
  el.instanceOverflowMenuEl.classList.remove("is-open");
}

// ---------- instance detail (fullscreen) ----------

export function openInstanceDetail(instanceId: string) {
  closeAllScreens();
  state.viewingInstanceId = instanceId;
  showInstanceTab("overview");
  renderInstanceDetail();
  el.instanceScreenEl.classList.add("is-open");
}

function closeInstanceDetail() {
  el.instanceScreenEl.classList.remove("is-open");
  // Browse mods/resource packs/shader packs opens *over* this screen (not through
  // `closeAllScreens`, so Back on it returns here) rather than being closed by it -- closing this
  // screen has to take that overlay down too, or it'd linger open with a `currentInstanceId` for
  // an instance the user has since navigated away from.
  el.browseContentScreenEl.classList.remove("is-open");
  state.viewingInstanceId = null;
}

function renderInstanceDetail() {
  const instance = state.instances.find((i) => i.id === state.viewingInstanceId) ?? null;
  if (!instance) {
    closeInstanceDetail();
    return;
  }
  el.instanceScreenTitleEl.textContent = instance.name;
  el.instanceDetailNameEl.textContent = instance.name;
  el.instanceDetailVersionEl.textContent = instance.version_id;
  el.instanceVersionNameEl.textContent = t("instances.versionPrefix", { version: instance.version_id });
  el.instanceIconBtn.style.backgroundImage = instanceIconBackground(instance);
  modLoader.renderLoaderRow(instance);
  contentBrowser.renderModsBrowseButton(instance.mod_loader !== null);
  renderInstancePlayPauseButton();
  renderGameLog();
  void refreshInstanceContent(instance.id);
}

// ---------- rename instance ----------

let renameInstanceTargetId: string | null = null;

function openRenameInstanceModal() {
  const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
  if (!instance) return;
  renameInstanceTargetId = instance.id;
  el.renameInstanceInput.value = instance.name;
  el.renameInstanceErrorEl.hidden = true;
  el.renameInstanceModalEl.classList.add("is-open");
  el.renameInstanceInput.focus();
}

function hideRenameInstanceModal() {
  el.renameInstanceModalEl.classList.remove("is-open");
  renameInstanceTargetId = null;
}

async function confirmRenameInstance() {
  if (!renameInstanceTargetId) return;
  const name = el.renameInstanceInput.value.trim();
  if (!name) {
    el.renameInstanceErrorEl.textContent = "Give the instance a name.";
    el.renameInstanceErrorEl.hidden = false;
    return;
  }
  try {
    const updated = await invoke<Instance>("rename_instance_cmd", { instanceId: renameInstanceTargetId, name });
    if (state.viewingInstanceId === renameInstanceTargetId) state.viewingInstanceId = updated.id;
    hideRenameInstanceModal();
    await loadInstances();
  } catch (err) {
    console.error(err);
    el.renameInstanceErrorEl.textContent = describeError(err);
    el.renameInstanceErrorEl.hidden = false;
  }
}

// ---------- change version ----------

function pickChangeVersion(versionId: string) {
  void applyInstanceVersion(versionId);
}

function openChangeVersionModal() {
  const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
  if (!instance) return;
  renderVersionOptions(el.changeVersionVersionsEl, instance.version_id, pickChangeVersion);
  el.changeVersionModalEl.classList.add("is-open");
}

function hideChangeVersionModal() {
  el.changeVersionModalEl.classList.remove("is-open");
}

async function applyInstanceVersion(versionId: string) {
  if (!state.viewingInstanceId) return;
  const instanceId = state.viewingInstanceId;
  hideChangeVersionModal();
  try {
    await invoke("set_instance_version_cmd", { instanceId, versionId });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

// ---------- icon ----------

async function pickInstanceIcon() {
  if (!state.viewingInstanceId) return;
  const instanceId = state.viewingInstanceId;
  try {
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
    });
    if (!picked || Array.isArray(picked)) return;
    await invoke("set_instance_icon_cmd", { instanceId, iconPath: picked });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function clearInstanceIcon() {
  if (!state.viewingInstanceId) return;
  const instanceId = state.viewingInstanceId;
  try {
    await invoke("set_instance_icon_cmd", { instanceId, iconPath: null });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

// ---------- export / delete ----------

async function exportInstance() {
  if (!state.viewingInstanceId) return;
  const instanceId = state.viewingInstanceId;
  const instance = state.instances.find((i) => i.id === instanceId);
  try {
    const destPath = await saveFileDialog({
      defaultPath: `${instance?.name ?? "instance"}.zip`,
      filters: [{ name: "Beacon instance", extensions: ["zip"] }],
    });
    if (!destPath) return;
    await invoke("export_instance_cmd", { instanceId, destPath });
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteInstance(instanceId: string) {
  try {
    await invoke("delete_instance_cmd", { instanceId });
    if (state.viewingInstanceId === instanceId) closeInstanceDetail();
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

export function init() {
  initInstanceLaunchControls();

  instanceTabs = document.querySelectorAll<HTMLButtonElement>("[data-instance-tab]");
  instanceTabPanels = document.querySelectorAll<HTMLElement>("[data-instance-tab-panel]");
  instanceTabs.forEach((tab) => {
    tab.addEventListener("click", () => showInstanceTab(tab.dataset.instanceTab ?? "overview"));
  });

  el.instanceOverflowBtn.addEventListener("click", () => {
    el.instanceOverflowMenuEl.classList.toggle("is-open");
  });

  el.instancePickerTrigger.addEventListener("click", () => {
    el.instancePickerEl.classList.contains("is-open") ? closeInstancePicker() : openInstancePicker();
  });

  el.newInstanceBtn.addEventListener("click", openCreateInstanceModal);
  el.importInstanceBtn.addEventListener("click", () => void importInstance());

  el.createInstanceNameInput.addEventListener("input", () => {
    createInstanceNameIsAuto = false;
  });
  el.createInstanceConfirmBtn.addEventListener("click", () => void confirmCreateInstance());
  el.createInstanceCancelBtn.addEventListener("click", hideCreateInstanceModal);

  el.instanceBackBtn.addEventListener("click", closeInstanceDetail);

  el.instanceRenameBtn.addEventListener("click", openRenameInstanceModal);
  el.renameInstanceConfirmBtn.addEventListener("click", () => void confirmRenameInstance());
  el.renameInstanceCancelBtn.addEventListener("click", hideRenameInstanceModal);

  el.instanceVersionBtn.addEventListener("click", openChangeVersionModal);
  el.changeVersionCancelBtn.addEventListener("click", hideChangeVersionModal);

  el.instanceIconBtn.addEventListener("click", () => void pickInstanceIcon());
  el.instanceIconClearBtn.addEventListener("click", () => {
    closeOverflowMenu();
    void clearInstanceIcon();
  });

  el.instanceOpenFolderBtn.addEventListener("click", () => {
    closeOverflowMenu();
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.dir);
  });
  el.instanceLibrariesOpenBtn.addEventListener("click", () => {
    closeOverflowMenu();
    if (state.directorySettings) void openFolder(state.directorySettings.libraries_dir);
  });

  el.instanceExportBtn.addEventListener("click", () => {
    closeOverflowMenu();
    void exportInstance();
  });
  el.instanceDeleteBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (!instance) return;
    openConfirmModal(t("deleteInstance.title"), t("instances.deleteBody", { name: instance.name }), () => void deleteInstance(instance.id));
  });
}
