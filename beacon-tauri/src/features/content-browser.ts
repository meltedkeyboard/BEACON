// Opens over the instance-detail screen, not via closeAllScreens; instances.ts's
// closeInstanceDetail closes this too so it doesn't linger with a stale currentInstanceId.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import DOMPurify from "dompurify";
import { marked } from "marked";

import { el } from "../dom";
import { describeError } from "../helpers";
import { t } from "../i18n";
import { openConfirmModal } from "../modals";
import { state } from "../state";
import type {
  ContentKind,
  ContentUpdateView,
  DownloadProgress,
  ModInstallPreviewEntry,
  ModProvenanceEntry,
  ModSearchResult,
  ModSource,
  ModVersionOption,
} from "../types";
import { refreshInstanceContent } from "./instance-content";

function kindLabels(kind: ContentKind): { noun: string; title: string; searchPlaceholder: string; emptyText: string } {
  switch (kind) {
    case "Mod":
      return { noun: t("modContent.noun.mod"), title: t("modContent.title.mod"), searchPlaceholder: t("modContent.placeholder.mod"), emptyText: t("modContent.empty.mod") };
    case "ResourcePack":
      return {
        noun: t("modContent.noun.resourcePack"),
        title: t("modContent.title.resourcePack"),
        searchPlaceholder: t("modContent.placeholder.resourcePack"),
        emptyText: t("modContent.empty.resourcePack"),
      };
    case "ShaderPack":
      return {
        noun: t("modContent.noun.shaderPack"),
        title: t("modContent.title.shaderPack"),
        searchPlaceholder: t("modContent.placeholder.shaderPack"),
        emptyText: t("modContent.empty.shaderPack"),
      };
  }
}

function resultKey(source: ModSource, id: string): string {
  return `${source}:${id}`;
}

let currentInstanceId: string | null = null;
let currentKind: ContentKind = "Mod";
let selectedSource: ModSource = "Modrinth";
let hasCurseForgeKey = false;
let searchToken = 0;
let searchDebounce: ReturnType<typeof setTimeout> | null = null;
let lastResults: ModSearchResult[] = [];
let installedByKey = new Map<string, string>(); // resultKey -> filename
const selected = new Map<string, ModSearchResult>(); // resultKey -> result, survives a re-search
let viewingKey: string | null = null; // resultKey currently shown in the detail pane, for the highlight
let removing = new Set<string>();
let updatingKeys = new Set<string>();
let updatesByKey = new Map<string, ContentUpdateView>(); // resultKey -> available update, empty until "Check for updates" runs
let checkingUpdates = false;

function renderSourceOptions() {
  el.modSourceOptions.forEach((btn) => {
    const source = btn.dataset.source as ModSource;
    const disabled = source === "CurseForge" && !hasCurseForgeKey;
    const isSelected = source === selectedSource;
    btn.classList.toggle("is-selected", isSelected);
    btn.setAttribute("aria-checked", String(isSelected));
    btn.disabled = disabled;
  });
  el.browseModsHintEl.hidden = hasCurseForgeKey;
}

function updateReviewButton() {
  el.browseModsReviewBtn.textContent = t("modContent.installedFmt", { count: selected.size });
  el.browseModsReviewBtn.disabled = selected.size === 0;
}

function renderResultCard(result: ModSearchResult): HTMLElement {
  const key = resultKey(result.source, result.id);
  const row = document.createElement("div");
  row.className = "manage-row manage-row--browse";
  row.dataset.resultKey = key;
  row.classList.toggle("is-viewing", key === viewingKey);

  const installedFilename = installedByKey.get(key);
  if (!installedFilename) {
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.className = "manage-row__checkbox";
    checkbox.checked = selected.has(key);
    checkbox.addEventListener("click", (e) => e.stopPropagation());
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        selected.set(key, result);
        addReviewRow(key, result);
      } else {
        selected.delete(key);
        removeReviewRow(key);
      }
      updateReviewButton();
    });
    row.appendChild(checkbox);
  }

  const icon = document.createElement("span");
  icon.className = "manage-row__icon";
  if (result.icon_url) icon.style.backgroundImage = `url("${result.icon_url}")`;

  const info = document.createElement("div");
  info.className = "manage-row__info";
  const name = document.createElement("span");
  name.className = "manage-row__name";
  name.textContent = result.title;
  const meta = document.createElement("span");
  meta.className = "manage-row__type";
  meta.textContent = `${result.author} · ${result.downloads.toLocaleString()} downloads`;
  const description = document.createElement("span");
  description.className = "manage-row__description";
  description.textContent = result.description;
  description.title = result.description;
  info.append(name, meta, description);

  row.append(icon, info);
  row.addEventListener("click", () => void openDetail(result));

  if (installedFilename) {
    const update = updatesByKey.get(key);
    if (update) {
      const updateBtn = document.createElement("button");
      updateBtn.type = "button";
      updateBtn.className = "manage-row__btn manage-row__btn--primary";
      updateBtn.textContent = updatingKeys.has(key) ? t("modContent.updating") : t("modContent.updateTo", { version: update.latestVersionNumber });
      updateBtn.disabled = updatingKeys.has(key);
      updateBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        void updateInstalledContent(key, update);
      });
      row.appendChild(updateBtn);
    }

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "manage-row__btn manage-row__btn--danger";
    removeBtn.textContent = removing.has(key) ? t("modContent.removing") : t("common.remove");
    removeBtn.disabled = removing.has(key) || updatingKeys.has(key);
    removeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openConfirmModal(
        t("modContent.removeConfirm", { noun: kindLabels(currentKind).noun }),
        t("modContent.removeBody", { filename: installedFilename }),
        () => void removeInstalledContent(key, installedFilename),
      );
    });
    row.appendChild(removeBtn);
  }

  return row;
}

function renderResults(results: ModSearchResult[]) {
  lastResults = results;
  el.browseModsResultsEl.replaceChildren();
  if (results.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = kindLabels(currentKind).emptyText;
    el.browseModsResultsEl.appendChild(empty);
    return;
  }
  for (const result of results) {
    el.browseModsResultsEl.appendChild(renderResultCard(result));
  }
}

async function runSearch() {
  if (!currentInstanceId) return;
  const token = ++searchToken;
  const instanceId = currentInstanceId;
  const kind = currentKind;
  el.browseModsErrorEl.hidden = true;
  try {
    const [results, provenance] = await Promise.all([
      invoke<ModSearchResult[]>("search_content_cmd", {
        instanceId,
        kind,
        source: selectedSource,
        query: el.browseModsQueryInput.value.trim(),
        offset: 0,
      }),
      invoke<ModProvenanceEntry[]>("list_content_provenance_cmd", { instanceId, kind }),
    ]);
    if (token !== searchToken) return; // a newer search/source-switch superseded this one
    installedByKey = new Map(provenance.map((p) => [resultKey(p.source, p.projectId), p.filename]));
    renderResults(results);
  } catch (err) {
    if (token !== searchToken) return;
    console.error(err);
    el.browseModsResultsEl.replaceChildren();
    el.browseModsErrorEl.textContent = describeError(err);
    el.browseModsErrorEl.hidden = false;
  }
}

function scheduleSearch() {
  if (searchDebounce !== null) clearTimeout(searchDebounce);
  searchDebounce = setTimeout(() => void runSearch(), 300);
}

async function removeInstalledContent(key: string, filename: string) {
  if (!currentInstanceId || removing.has(key)) return;
  removing.add(key);
  renderResults(lastResults);
  try {
    await invoke("remove_content_source_cmd", { instanceId: currentInstanceId, kind: currentKind, filename });
    installedByKey.delete(key);
    await refreshInstanceContent(currentInstanceId);
  } catch (err) {
    console.error(err);
    el.browseModsErrorEl.textContent = describeError(err);
    el.browseModsErrorEl.hidden = false;
  } finally {
    removing.delete(key);
    renderResults(lastResults);
  }
}

async function updateInstalledContent(key: string, update: ContentUpdateView) {
  if (!currentInstanceId || updatingKeys.has(key)) return;
  updatingKeys.add(key);
  renderResults(lastResults);
  try {
    await invoke("update_content_cmd", {
      instanceId: currentInstanceId,
      kind: currentKind,
      source: update.source,
      projectId: update.projectId,
      oldFilename: update.filename,
      versionId: update.latestVersionId,
    });
    updatesByKey.delete(key);
    await Promise.all([runSearch(), refreshInstanceContent(currentInstanceId)]);
  } catch (err) {
    console.error(err);
    el.browseModsErrorEl.textContent = describeError(err);
    el.browseModsErrorEl.hidden = false;
  } finally {
    updatingKeys.delete(key);
    renderResults(lastResults);
  }
}

// Opt-in and slower than runSearch (one lookup per installed item), so kept off the keystroke path.
async function checkForUpdates() {
  if (!currentInstanceId || checkingUpdates) return;
  checkingUpdates = true;
  el.browseModsCheckUpdatesBtn.disabled = true;
  el.browseModsCheckUpdatesBtn.textContent = t("modContent.checking");
  el.browseModsErrorEl.hidden = true;
  const instanceId = currentInstanceId;
  const kind = currentKind;
  try {
    const updates = await invoke<ContentUpdateView[]>("check_content_updates_cmd", { instanceId, kind });
    if (instanceId !== currentInstanceId || kind !== currentKind) return; // navigated/switched kind while this was in flight
    updatesByKey = new Map(updates.map((u) => [resultKey(u.source, u.projectId), u]));
    renderResults(lastResults);
  } catch (err) {
    console.error(err);
    el.browseModsErrorEl.textContent = describeError(err);
    el.browseModsErrorEl.hidden = false;
  } finally {
    checkingUpdates = false;
    el.browseModsCheckUpdatesBtn.disabled = false;
    el.browseModsCheckUpdatesBtn.textContent = t("browseContent.checkUpdates");
  }
}

let detailToken = 0;

function renderResultViewHighlight() {
  el.browseModsResultsEl.querySelectorAll<HTMLElement>(".manage-row--browse").forEach((row) => {
    row.classList.toggle("is-viewing", row.dataset.resultKey === viewingKey);
  });
}

async function openDetail(result: ModSearchResult) {
  viewingKey = resultKey(result.source, result.id);
  renderResultViewHighlight();
  const token = ++detailToken;
  el.modDetailPlaceholderEl.hidden = true;
  el.modDetailContentEl.hidden = false;
  el.modDetailTitleEl.textContent = result.title;
  el.modDetailMetaEl.textContent = `${result.author} · ${result.source}`;
  el.modDetailBodyEl.textContent = "Loading…";
  try {
    const raw = await invoke<string>("get_content_description_cmd", { source: result.source, projectId: result.id });
    if (token !== detailToken) return;
    // Always through DOMPurify below: Markdown can embed raw HTML passthrough too.
    const html = result.source === "Modrinth" ? await marked.parse(raw) : raw;
    el.modDetailBodyEl.innerHTML = DOMPurify.sanitize(html);
  } catch (err) {
    if (token !== detailToken) return;
    console.error(err);
    el.modDetailBodyEl.textContent = describeError(err);
  }
}

function resetDetailPane() {
  detailToken++;
  viewingKey = null;
  el.modDetailPlaceholderEl.hidden = false;
  el.modDetailContentEl.hidden = true;
}

interface ReviewRow {
  result: ModSearchResult;
  select: HTMLSelectElement;
  depsEl: HTMLElement;
  el: HTMLElement;
}

let reviewRows = new Map<string, ReviewRow>(); // resultKey -> row, mirrors `selected`
let reviewInstalling = false;

function renderReviewPlaceholder() {
  el.reviewModsPlaceholderEl.hidden = reviewRows.size > 0;
}

async function loadReviewRowOptions(row: ReviewRow) {
  if (!currentInstanceId) return;
  try {
    const versions = await invoke<ModVersionOption[]>("list_content_versions_cmd", {
      instanceId: currentInstanceId,
      kind: currentKind,
      source: row.result.source,
      projectId: row.result.id,
    });
    row.select.replaceChildren();
    for (const v of versions) {
      const option = document.createElement("option");
      option.value = v.id;
      option.textContent = v.is_stable ? `${v.version_number} (${v.filename})` : `${v.version_number} — unstable (${v.filename})`;
      row.select.appendChild(option);
    }
    // Default to newest stable, not just newest -- the list sorts by date, and an unstable build
    // can be newer than the last stable release.
    const firstStable = versions.find((v) => v.is_stable);
    if (firstStable) row.select.value = firstStable.id;
  } catch (err) {
    console.error(err);
  }
  void loadReviewRowPreview(row);
}

async function loadReviewRowPreview(row: ReviewRow) {
  if (!currentInstanceId) return;
  const versionId = row.select.value || undefined;
  row.depsEl.textContent = t("modContent.checking");
  try {
    const entries = await invoke<ModInstallPreviewEntry[]>("preview_content_install_cmd", {
      instanceId: currentInstanceId,
      kind: currentKind,
      source: row.result.source,
      projectId: row.result.id,
      versionId,
    });
    const deps = entries.filter((e) => e.is_dependency);
    row.depsEl.textContent = deps.length > 0 ? t("modContent.bringsIn", { list: deps.map((d) => `${d.title} ${d.version_number}`).join(", ") }) : "";
  } catch (err) {
    console.error(err);
    row.depsEl.textContent = describeError(err);
  }
}

function addReviewRow(key: string, result: ModSearchResult) {
  if (reviewRows.has(key)) return;

  const row = document.createElement("div");
  row.className = "manage-row";

  const info = document.createElement("div");
  info.className = "manage-row__info";
  const name = document.createElement("span");
  name.className = "manage-row__name";
  name.textContent = `${result.title} (${result.source})`;
  const depsEl = document.createElement("span");
  depsEl.className = "manage-row__description";
  info.append(name, depsEl);

  const select = document.createElement("select");
  select.className = "modal__input";

  row.append(info, select);
  el.reviewModsListEl.appendChild(row);

  const reviewRow: ReviewRow = { result, select, depsEl, el: row };
  reviewRows.set(key, reviewRow);
  renderReviewPlaceholder();
  select.addEventListener("change", () => void loadReviewRowPreview(reviewRow));
  void loadReviewRowOptions(reviewRow);
}

function removeReviewRow(key: string) {
  const row = reviewRows.get(key);
  if (!row) return;
  row.el.remove();
  reviewRows.delete(key);
  renderReviewPlaceholder();
}

function clearReviewRows() {
  el.reviewModsListEl.replaceChildren();
  reviewRows = new Map();
  renderReviewPlaceholder();
}

function renderReviewProgress(label: string, percent: number) {
  el.reviewModsProgressEl.hidden = false;
  el.reviewModsProgressLabelEl.textContent = label;
  el.reviewModsProgressPercentEl.textContent = `${Math.round(percent)}%`;
  el.reviewModsProgressFillEl.style.width = `${percent}%`;
}

async function confirmReview() {
  if (reviewInstalling || !currentInstanceId || reviewRows.size === 0) return;
  reviewInstalling = true;
  el.browseModsReviewBtn.disabled = true;
  el.browseModsReviewBtn.textContent = "Installing…";
  el.reviewModsErrorEl.hidden = true;
  renderReviewProgress(t("modContent.startingInstall"), 0);

  const selections = Array.from(reviewRows.values()).map((row) => ({
    source: row.result.source,
    projectId: row.result.id,
    versionId: row.select.value || null,
  }));

  try {
    await invoke("install_selected_content_cmd", { instanceId: currentInstanceId, kind: currentKind, selections });
    selected.clear();
    clearReviewRows();
    await Promise.all([runSearch(), refreshInstanceContent(currentInstanceId)]);
  } catch (err) {
    console.error(err);
    el.reviewModsErrorEl.textContent = describeError(err);
    el.reviewModsErrorEl.hidden = false;
  } finally {
    reviewInstalling = false;
    el.reviewModsProgressEl.hidden = true;
    updateReviewButton();
  }
}

export function renderModsBrowseButton(hasLoader: boolean) {
  el.modsBrowseBtn.disabled = !hasLoader;
  el.modsBrowseBtn.title = hasLoader ? "" : t("instance.mods.needLoader");
}

async function openBrowseContentModal(instanceId: string, kind: ContentKind) {
  currentInstanceId = instanceId;
  currentKind = kind;
  selectedSource = "Modrinth";
  const labels = kindLabels(kind);
  el.browseModsEyebrowEl.textContent = labels.title;
  el.browseModsQueryInput.placeholder = labels.searchPlaceholder;
  el.browseModsQueryInput.value = "";
  el.browseModsErrorEl.hidden = true;
  el.browseModsResultsEl.replaceChildren();
  selected.clear();
  updatesByKey = new Map();
  clearReviewRows();
  updateReviewButton();
  resetDetailPane();
  try {
    hasCurseForgeKey = await invoke<boolean>("has_curseforge_api_key_cmd");
  } catch {
    hasCurseForgeKey = false;
  }
  renderSourceOptions();
  el.browseContentScreenEl.classList.add("is-open");
  void runSearch();
}

export function closeBrowseContentScreen() {
  el.browseContentScreenEl.classList.remove("is-open");
}

export function init() {
  el.modsBrowseBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance?.mod_loader) void openBrowseContentModal(instance.id, "Mod");
  });
  el.resourcePacksBrowseBtn.addEventListener("click", () => {
    if (state.viewingInstanceId) void openBrowseContentModal(state.viewingInstanceId, "ResourcePack");
  });
  el.shaderPacksBrowseBtn.addEventListener("click", () => {
    if (state.viewingInstanceId) void openBrowseContentModal(state.viewingInstanceId, "ShaderPack");
  });

  el.modSourceOptions.forEach((btn) => {
    btn.addEventListener("click", () => {
      const source = btn.dataset.source as ModSource;
      if (btn.disabled || source === selectedSource) return;
      selectedSource = source;
      renderSourceOptions();
      resetDetailPane();
      void runSearch();
    });
  });

  el.browseModsQueryInput.addEventListener("input", scheduleSearch);
  el.browseModsBackBtn.addEventListener("click", closeBrowseContentScreen);
  el.browseModsReviewBtn.addEventListener("click", () => void confirmReview());
  el.browseModsCheckUpdatesBtn.addEventListener("click", () => void checkForUpdates());

  void listen<DownloadProgress>("content-install-progress", (event) => {
    if (!reviewInstalling) return;
    const p = event.payload;
    const phase = p.phase || "Files";
    const percent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    const label = p.files_total > 0 ? `${phase} (${p.files_done}/${p.files_total})` : phase;
    renderReviewProgress(label, percent);
  });
}
