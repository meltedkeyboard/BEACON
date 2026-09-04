// Everything inside the instance-detail screen that lists/mutates one of its content folders:
// Mods, Worlds (+datapacks), Resource Packs, Shader Packs, Screenshots (list/delete/pin). Split
// out of `instances.ts` because this is a distinct concern (folder content CRUD) from instance
// identity CRUD (rename/version/icon/export/delete).

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import { el } from "../dom";
import { describeError, openFolder } from "../helpers";
import { t } from "../i18n";
import { loadInstances } from "./instances";
import { openConfirmModal, showErrorModal } from "../modals";
import { state } from "../state";
import type { ModInfo, ModProvenanceEntry, ModSource, ResourcePackInfo, ScreenshotInfo, WorldInfo } from "../types";

// Text label shown next to a mod/resource pack/shader pack's name for where it came from --
// `undefined` means it was never installed through the content browser (dropped in manually, or
// predates provenance tracking), shown as "Unknown" rather than left blank so it reads as a
// deliberate "we don't know" instead of a missing value.
function renderSourceLabel(source: ModSource | undefined): HTMLElement {
  const label = document.createElement("span");
  label.className = "manage-row__source";
  label.dataset.source = source ?? "Unknown";
  label.textContent = source ?? t("modContent.source.unknown");
  return label;
}

// Shared by Resource Packs (has a real `pack.png`-derived icon, `showIcon: true`) and Shader
// Packs (no equivalent standard convention exists for a preview image, so `showIcon: false` skips
// the icon slot entirely rather than rendering a permanently-blank one).
function renderSimpleContentList(
  container: HTMLElement,
  items: { name: string; icon_data_url: string | null }[],
  emptyText: string,
  showIcon: boolean,
  sourceByFilename: Map<string, ModSource>,
  onDelete: (name: string) => void,
) {
  container.replaceChildren();
  if (items.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = emptyText;
    container.appendChild(empty);
    return;
  }
  for (const item of items) {
    const row = document.createElement("div");
    row.className = showIcon ? "manage-row manage-row--packs" : "manage-row manage-row--packs manage-row--packs-no-icon";

    const info = document.createElement("div");
    info.className = "manage-row__info";
    const nameSpan = document.createElement("span");
    nameSpan.className = "manage-row__name";
    nameSpan.textContent = item.name;
    info.appendChild(nameSpan);

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "manage-row__btn manage-row__btn--danger";
    removeBtn.textContent = t("common.remove");
    removeBtn.addEventListener("click", () => onDelete(item.name));

    if (showIcon) {
      const icon = document.createElement("span");
      icon.className = "manage-row__icon";
      if (item.icon_data_url) icon.style.backgroundImage = `url("${item.icon_data_url}")`;
      row.append(icon, info, renderSourceLabel(sourceByFilename.get(item.name)), removeBtn);
    } else {
      row.append(info, renderSourceLabel(sourceByFilename.get(item.name)), removeBtn);
    }
    container.appendChild(row);
  }
}

function renderWorlds(instanceId: string, worlds: WorldInfo[]) {
  el.worldsListEl.replaceChildren();
  if (worlds.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = t("instance.worlds.empty");
    el.worldsListEl.appendChild(empty);
    return;
  }

  for (const world of worlds) {
    const row = document.createElement("div");
    row.className = "manage-row";

    const icon = document.createElement("span");
    icon.className = "manage-row__icon";
    if (world.icon_data_url) icon.style.backgroundImage = `url("${world.icon_data_url}")`;

    const info = document.createElement("div");
    info.className = "manage-row__info";
    const name = document.createElement("span");
    name.className = "manage-row__name";
    name.textContent = world.name;
    info.appendChild(name);
    if (world.datapacks.length > 0) {
      const count = document.createElement("span");
      count.className = "manage-row__type";
      count.textContent = `${world.datapacks.length} datapack${world.datapacks.length === 1 ? "" : "s"}`;
      info.appendChild(count);
    }

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "manage-row__btn manage-row__btn--danger";
    removeBtn.textContent = t("common.remove");
    removeBtn.addEventListener("click", () => {
      openConfirmModal(t("confirm.deleteWorld.title"), t("confirm.deleteFilePrefix", { name: world.name }), () =>
        void deleteWorld(instanceId, world.name),
      );
    });

    row.append(icon, info, removeBtn);
    el.worldsListEl.appendChild(row);

    for (const datapack of world.datapacks) {
      const dpRow = document.createElement("div");
      dpRow.className = "manage-row manage-row--nested";

      const dpName = document.createElement("span");
      dpName.className = "manage-row__name";
      dpName.textContent = datapack;

      const dpRemove = document.createElement("button");
      dpRemove.type = "button";
      dpRemove.className = "manage-row__btn manage-row__btn--danger";
      dpRemove.textContent = "Remove";
      dpRemove.addEventListener("click", () => void deleteDatapack(instanceId, world.name, datapack));

      dpRow.append(dpName, dpRemove);
      el.worldsListEl.appendChild(dpRow);
    }
  }
}

// ---------- Mods table: Enabled(checkbox) | Icon | Name | Version, click/Ctrl/Shift-select ----------
// File-manager-style multi-select instead of a per-row Remove button -- one selection, one Delete
// button (in the section header), one confirmation for the whole batch or a single mod alike.

let selectedMods = new Set<string>(); // by filename (ModInfo.name)
let lastClickedMod: string | null = null;
let modOrder: string[] = []; // last-rendered order, for Shift+click range selection

function updateModsDeleteButton() {
  el.modsDeleteBtn.disabled = selectedMods.size === 0;
  el.modsDeleteBtn.textContent = selectedMods.size > 0 ? t("instance.mods.deleteFmt", { count: selectedMods.size }) : t("instance.mods.delete");
}

function renderModSelectionHighlight() {
  el.modsListEl.querySelectorAll<HTMLElement>(".manage-row--selectable").forEach((row) => {
    row.classList.toggle("is-selected", selectedMods.has(row.dataset.modName ?? ""));
  });
}

function handleModRowClick(event: MouseEvent, name: string) {
  if (event.shiftKey && lastClickedMod) {
    const from = modOrder.indexOf(lastClickedMod);
    const to = modOrder.indexOf(name);
    if (from !== -1 && to !== -1) {
      const [start, end] = from < to ? [from, to] : [to, from];
      for (let i = start; i <= end; i++) selectedMods.add(modOrder[i]);
    }
  } else if (event.ctrlKey || event.metaKey) {
    if (selectedMods.has(name)) selectedMods.delete(name);
    else selectedMods.add(name);
    lastClickedMod = name;
  } else {
    selectedMods = new Set([name]);
    lastClickedMod = name;
  }
  updateModsDeleteButton();
  renderModSelectionHighlight();
}

function renderMods(instanceId: string, mods: ModInfo[], sourceByFilename: Map<string, ModSource>) {
  el.modsListEl.replaceChildren();
  modOrder = mods.map((m) => m.name);
  // A mod that was selected but no longer appears (deleted, disabled-toggled away by some other
  // path) shouldn't linger in the selection or inflate the Delete button's count.
  selectedMods = new Set([...selectedMods].filter((name) => modOrder.includes(name)));
  updateModsDeleteButton();

  if (mods.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = t("instance.mods.empty");
    el.modsListEl.appendChild(empty);
    return;
  }
  for (const mod of mods) {
    const row = document.createElement("div");
    row.className = "manage-row manage-row--selectable manage-row--mods";
    row.dataset.modName = mod.name;
    row.classList.toggle("is-selected", selectedMods.has(mod.name));

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.className = "manage-row__checkbox";
    checkbox.checked = mod.enabled;
    checkbox.title = mod.enabled ? "Enabled -- click to disable" : "Disabled -- click to enable";
    checkbox.addEventListener("click", (e) => e.stopPropagation());
    checkbox.addEventListener("change", () => void toggleMod(instanceId, mod.name, checkbox.checked));

    const icon = document.createElement("span");
    icon.className = "manage-row__icon";
    if (mod.icon_data_url) icon.style.backgroundImage = `url("${mod.icon_data_url}")`;

    const info = document.createElement("div");
    info.className = "manage-row__info";
    const name = document.createElement("span");
    name.className = "manage-row__name";
    name.textContent = mod.name;
    info.appendChild(name);
    if (mod.version) {
      const version = document.createElement("span");
      version.className = "manage-row__type";
      version.textContent = mod.version;
      info.appendChild(version);
    }
    row.append(checkbox, icon, info, renderSourceLabel(sourceByFilename.get(mod.name)));
    row.addEventListener("click", (e) => handleModRowClick(e, mod.name));
    el.modsListEl.appendChild(row);
  }
}

async function deleteSelectedMods(instanceId: string, names: string[]) {
  try {
    for (const name of names) {
      await invoke("delete_mod_cmd", { instanceId, name });
    }
    selectedMods.clear();
    updateModsDeleteButton();
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

function renderScreenshotGrid(instanceId: string, screenshots: ScreenshotInfo[], pinnedName: string | null) {
  el.screenshotsGridEl.replaceChildren();
  if (screenshots.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = t("instance.screenshots.empty");
    el.screenshotsGridEl.appendChild(empty);
    return;
  }
  for (const screenshot of screenshots) {
    const card = document.createElement("div");
    card.className = "screenshot-card";
    card.style.backgroundImage = `url("${convertFileSrc(screenshot.path)}")`;
    const isPinned = screenshot.name === pinnedName;
    card.classList.toggle("is-pinned", isPinned);

    const pinBtn = document.createElement("button");
    pinBtn.type = "button";
    pinBtn.className = "screenshot-card__btn screenshot-card__pin";
    pinBtn.textContent = "★";
    pinBtn.title = isPinned ? t("common.unpin") : t("common.pinAsMomentsBg");
    pinBtn.addEventListener("click", () => void togglePinScreenshot(instanceId, isPinned ? null : screenshot.name));

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "screenshot-card__btn screenshot-card__remove";
    removeBtn.textContent = "×";
    removeBtn.title = t("common.remove");
    removeBtn.addEventListener("click", () => void deleteScreenshotAction(instanceId, screenshot.name));

    card.append(pinBtn, removeBtn);
    el.screenshotsGridEl.appendChild(card);
  }
}

async function togglePinScreenshot(instanceId: string, name: string | null) {
  try {
    await invoke("set_pinned_screenshot_cmd", { instanceId, name });
    // Pinning changes `Instance.pinned_screenshot`, which lives in the shared `state.instances`
    // array that only `instances.ts`'s `loadInstances()` refreshes.
    await loadInstances();
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteScreenshotAction(instanceId: string, name: string) {
  try {
    await invoke("delete_screenshot_cmd", { instanceId, name });
    await loadInstances();
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

export async function refreshInstanceContent(instanceId: string) {
  try {
    const [mods, worlds, resourcePacks, shaderPacks, screenshots, modProvenance, resourcePackProvenance, shaderPackProvenance] =
      await Promise.all([
        invoke<ModInfo[]>("list_mods_cmd", { instanceId }),
        invoke<WorldInfo[]>("list_worlds_cmd", { instanceId }),
        invoke<ResourcePackInfo[]>("list_resource_packs_cmd", { instanceId }),
        invoke<string[]>("list_shader_packs_cmd", { instanceId }),
        invoke<ScreenshotInfo[]>("list_screenshots_cmd", { instanceId }),
        invoke<ModProvenanceEntry[]>("list_content_provenance_cmd", { instanceId, kind: "Mod" }),
        invoke<ModProvenanceEntry[]>("list_content_provenance_cmd", { instanceId, kind: "ResourcePack" }),
        invoke<ModProvenanceEntry[]>("list_content_provenance_cmd", { instanceId, kind: "ShaderPack" }),
      ]);
    if (state.viewingInstanceId !== instanceId) return; // navigated away while this was in flight
    const modSourceByFilename = new Map(modProvenance.map((p) => [p.filename, p.source]));
    const resourcePackSourceByFilename = new Map(resourcePackProvenance.map((p) => [p.filename, p.source]));
    const shaderPackSourceByFilename = new Map(shaderPackProvenance.map((p) => [p.filename, p.source]));
    renderMods(instanceId, mods, modSourceByFilename);
    renderWorlds(instanceId, worlds);
    renderSimpleContentList(
      el.resourcePacksListEl,
      resourcePacks,
      t("instance.resourcePacks.empty"),
      true,
      resourcePackSourceByFilename,
      (fileName) => {
        openConfirmModal(t("confirm.deleteResourcePack.title"), t("confirm.deleteFilePrefix", { name: fileName }), () =>
          void deleteResourcePack(instanceId, fileName),
        );
      },
    );
    renderSimpleContentList(
      el.shaderPacksListEl,
      shaderPacks.map((name) => ({ name, icon_data_url: null })),
      t("instance.shaderPacks.empty"),
      false,
      shaderPackSourceByFilename,
      (fileName) => {
        openConfirmModal(t("confirm.deleteShaderPack.title"), t("confirm.deleteFilePrefix", { name: fileName }), () =>
          void deleteShaderPack(instanceId, fileName),
        );
      },
    );
    const pinnedName = state.instances.find((i) => i.id === instanceId)?.pinned_screenshot ?? null;
    renderScreenshotGrid(instanceId, screenshots, pinnedName);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function toggleMod(instanceId: string, name: string, enable: boolean) {
  try {
    await invoke("toggle_mod_cmd", { instanceId, name, enable });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function addMods() {
  if (!state.viewingInstanceId) return;
  const instanceId = state.viewingInstanceId;
  try {
    const picked = await openFileDialog({ multiple: true, filters: [{ name: "Mods", extensions: ["jar"] }] });
    if (!picked || !Array.isArray(picked) || picked.length === 0) return;
    await invoke("add_mods_cmd", { instanceId, sourcePaths: picked });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteWorld(instanceId: string, worldName: string) {
  try {
    await invoke("delete_world_cmd", { instanceId, worldName });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteDatapack(instanceId: string, worldName: string, datapackName: string) {
  try {
    await invoke("delete_datapack_cmd", { instanceId, worldName, datapackName });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteResourcePack(instanceId: string, fileName: string) {
  try {
    await invoke("delete_resource_pack_cmd", { instanceId, fileName });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function deleteShaderPack(instanceId: string, fileName: string) {
  try {
    await invoke("delete_shader_pack_cmd", { instanceId, fileName });
    await refreshInstanceContent(instanceId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

export function init() {
  el.modsAddBtn.addEventListener("click", () => void addMods());
  el.modsDeleteBtn.addEventListener("click", () => {
    if (!state.viewingInstanceId || selectedMods.size === 0) return;
    const instanceId = state.viewingInstanceId;
    const names = Array.from(selectedMods);
    const message =
      names.length === 1
        ? t("confirm.deleteModsBody.single", { name: names[0] })
        : t("confirm.deleteModsBody.multi", { count: names.length, names: names.join(", ") });
    openConfirmModal(t("confirm.deleteMods.title"), message, () => void deleteSelectedMods(instanceId, names));
  });

  el.modsOpenFolderBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.mods_dir);
  });
  el.worldsOpenFolderBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.saves_dir);
  });
  el.resourcePacksOpenFolderBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.resource_packs_dir);
  });
  el.shaderPacksOpenFolderBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.shader_packs_dir);
  });
  el.screenshotsOpenFolderBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance) void openFolder(instance.screenshots_dir);
  });
}
