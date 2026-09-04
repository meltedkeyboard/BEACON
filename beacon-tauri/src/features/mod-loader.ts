// Mod loader install/change/remove for the instance-detail screen's "Version" section. Kept
// separate from `instances.ts` for the same reason `instance-content.ts` is -- a distinct concern
// (this one has its own install-flow state machine) rather than instance identity CRUD.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { el } from "../dom";
import { describeError } from "../helpers";
import { t } from "../i18n";
import { openConfirmModal, showErrorModal } from "../modals";
import { state } from "../state";
import type { DownloadProgress, Instance, LoaderVersionInfo, ModLoaderKind } from "../types";
import { loadInstances } from "./instances";

const LOADER_KINDS: ModLoaderKind[] = ["Fabric", "Forge", "NeoForge", "Quilt"];

let currentInstanceId: string | null = null;
let currentMcVersion: string | null = null;
let selectedKind: ModLoaderKind = "Fabric";
let versions: LoaderVersionInfo[] = [];
let selectedVersion: string | null = null;
let installing = false;

export function renderLoaderRow(instance: Instance) {
  const loader = instance.mod_loader;
  el.instanceLoaderNameEl.textContent = loader
    ? t("instances.loaderNamed", { name: `${loader.kind} ${loader.loader_version}` })
    : t("instances.loaderNone");
  el.instanceLoaderInstallBtn.textContent = loader ? t("instance.loader.changeEllipsis") : t("instance.loader.installEllipsis");
  el.instanceLoaderRemoveBtn.hidden = !loader;
}

function renderKindOptions() {
  el.loaderKindOptions.forEach((btn) => {
    const selected = btn.dataset.kind === selectedKind;
    btn.classList.toggle("is-selected", selected);
    btn.setAttribute("aria-checked", String(selected));
  });
}

function renderLoaderVersions() {
  el.installLoaderVersionsEl.replaceChildren();
  if (versions.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = "No versions found for this Minecraft version.";
    el.installLoaderVersionsEl.appendChild(empty);
    return;
  }
  for (const version of versions) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "nav-row version-row";
    row.classList.toggle("is-selected", version.version === selectedVersion);

    const idSpan = document.createElement("span");
    idSpan.className = "version-row__id";
    idSpan.textContent = version.version;

    const metaSpan = document.createElement("span");
    metaSpan.className = "version-row__meta";
    metaSpan.textContent = version.recommended ? "Recommended" : version.stable ? "Stable" : "Unstable";

    row.append(idSpan, metaSpan);
    row.addEventListener("click", () => {
      selectedVersion = version.version;
      renderLoaderVersions();
    });
    el.installLoaderVersionsEl.appendChild(row);
  }
}

async function loadVersionsForKind() {
  if (!currentMcVersion) return;
  el.installLoaderErrorEl.hidden = true;
  el.installLoaderVersionsEl.replaceChildren();
  try {
    versions = await invoke<LoaderVersionInfo[]>("list_loader_versions_cmd", {
      kind: selectedKind,
      mcVersion: currentMcVersion,
    });
    selectedVersion = versions.find((v) => v.recommended)?.version ?? versions.find((v) => v.stable)?.version ?? versions[0]?.version ?? null;
    renderLoaderVersions();
  } catch (err) {
    console.error(err);
    versions = [];
    selectedVersion = null;
    el.installLoaderErrorEl.textContent = describeError(err);
    el.installLoaderErrorEl.hidden = false;
  }
}

function renderInstallProgress(label: string, percent: number) {
  el.installLoaderProgressEl.hidden = false;
  el.installLoaderProgressLabelEl.textContent = label;
  el.installLoaderProgressPercentEl.textContent = `${Math.round(percent)}%`;
  el.installLoaderProgressFillEl.style.width = `${percent}%`;
}

function openInstallLoaderModal(instance: Instance) {
  currentInstanceId = instance.id;
  currentMcVersion = instance.version_id;
  selectedKind = instance.mod_loader?.kind ?? "Fabric";
  el.installLoaderErrorEl.hidden = true;
  el.installLoaderProgressEl.hidden = true;
  el.installLoaderConfirmBtn.disabled = false;
  el.installLoaderConfirmBtn.textContent = "Install";
  el.installLoaderCancelBtn.disabled = false;
  renderKindOptions();
  void loadVersionsForKind();
  el.installLoaderModalEl.classList.add("is-open");
}

function hideInstallLoaderModal() {
  el.installLoaderModalEl.classList.remove("is-open");
}

async function confirmInstallLoader() {
  if (installing || !currentInstanceId || !selectedVersion) return;
  installing = true;
  el.installLoaderConfirmBtn.disabled = true;
  el.installLoaderConfirmBtn.textContent = "Installing…";
  el.installLoaderCancelBtn.disabled = true;
  el.installLoaderErrorEl.hidden = true;
  renderInstallProgress("Starting…", 0);

  try {
    await invoke("install_loader_cmd", {
      instanceId: currentInstanceId,
      kind: selectedKind,
      loaderVersion: selectedVersion,
    });
    hideInstallLoaderModal();
    await loadInstances();
  } catch (err) {
    console.error(err);
    el.installLoaderErrorEl.textContent = describeError(err);
    el.installLoaderErrorEl.hidden = false;
  } finally {
    installing = false;
    el.installLoaderProgressEl.hidden = true;
    el.installLoaderConfirmBtn.disabled = false;
    el.installLoaderConfirmBtn.textContent = "Install";
    el.installLoaderCancelBtn.disabled = false;
  }
}

async function removeLoader(instanceId: string) {
  try {
    await invoke("remove_loader_cmd", { instanceId });
    await loadInstances();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

export async function init() {
  el.instanceLoaderInstallBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) openInstallLoaderModal(instance);
  });

  el.instanceLoaderRemoveBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (!instance?.mod_loader) return;
    openConfirmModal(
      "Remove mod loader?",
      `This removes ${instance.mod_loader.kind} ${instance.mod_loader.loader_version} from "${instance.name}". Your mods stay in place, but won't load until a loader is installed again.`,
      () => void removeLoader(instance.id),
    );
  });

  el.loaderKindOptions.forEach((btn) => {
    btn.addEventListener("click", () => {
      const kind = btn.dataset.kind;
      if (!kind || !LOADER_KINDS.includes(kind as ModLoaderKind) || kind === selectedKind) return;
      selectedKind = kind as ModLoaderKind;
      renderKindOptions();
      void loadVersionsForKind();
    });
  });

  el.installLoaderConfirmBtn.addEventListener("click", () => void confirmInstallLoader());
  el.installLoaderCancelBtn.addEventListener("click", () => {
    if (!installing) hideInstallLoaderModal();
  });

  await listen<DownloadProgress>("loader-install-progress", (event) => {
    if (!installing) return;
    const p = event.payload;
    const phase = p.phase || "Files";
    const percent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    const label = p.files_total > 0 ? `${phase} (${p.files_done}/${p.files_total})` : phase;
    renderInstallProgress(label, percent);
  });
}
