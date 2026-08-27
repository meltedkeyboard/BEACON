// "Browse mods…" modal on the instance-detail screen's Mods section: search Modrinth (always
// available) or CurseForge (only once the user has pasted their own API key in Settings -- see
// that file's own header comment for why Beacon can't ship a shared key). Checkboxes select
// mods to install; "Review & Install" opens a table showing exactly which version of each (plus,
// Modrinth only, which dependencies it brings in) will be downloaded, changeable via a dropdown,
// before anything actually happens -- the whole point being visibility into what "compatible
// version" the backend picked, instead of a silent one-click auto-install.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import DOMPurify from "dompurify";
import { marked } from "marked";

import { el } from "../dom";
import { describeError } from "../helpers";
import { openConfirmModal } from "../modals";
import { state } from "../state";
import type {
  DownloadProgress,
  Instance,
  ModInstallPreviewEntry,
  ModProvenanceEntry,
  ModSearchResult,
  ModSource,
  ModVersionOption,
} from "../types";
import { refreshInstanceContent } from "./instance-content";

function resultKey(source: ModSource, id: string): string {
  return `${source}:${id}`;
}

let currentInstanceId: string | null = null;
let selectedSource: ModSource = "Modrinth";
let hasCurseForgeKey = false;
let searchToken = 0;
let searchDebounce: ReturnType<typeof setTimeout> | null = null;
let lastResults: ModSearchResult[] = [];
let installedByKey = new Map<string, string>(); // resultKey -> filename
const selected = new Map<string, ModSearchResult>(); // resultKey -> result, survives a re-search
let removing = new Set<string>();

// ---------- source picker ----------

function renderSourceOptions() {
  el.modSourceOptions.forEach((btn) => {
    const source = btn.dataset.source as ModSource;
    const disabled = source === "CurseForge" && !hasCurseForgeKey;
    const isSelected = source === selectedSource;
    btn.classList.toggle("is-selected", isSelected);
    btn.setAttribute("aria-checked", String(isSelected));
    btn.disabled = disabled;
  });
  // Hidden entirely once a key is set (nothing to explain then) -- otherwise CSS shows it only on
  // hover/focus of the button itself (see .mod-source-cf-wrap in styles.css), not always-on.
  el.browseModsHintEl.hidden = hasCurseForgeKey;
}

// ---------- result list ----------

function updateReviewButton() {
  el.browseModsReviewBtn.textContent = `Review & Install (${selected.size})`;
  el.browseModsReviewBtn.disabled = selected.size === 0;
}

function renderResultCard(result: ModSearchResult) {
  const key = resultKey(result.source, result.id);
  const row = document.createElement("div");
  row.className = "manage-row";

  const installedFilename = installedByKey.get(key);
  if (!installedFilename) {
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.className = "manage-row__checkbox";
    checkbox.checked = selected.has(key);
    checkbox.addEventListener("click", (e) => e.stopPropagation());
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selected.set(key, result);
      else selected.delete(key);
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
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "manage-row__btn manage-row__btn--danger";
    removeBtn.textContent = removing.has(key) ? "Removing…" : "Remove";
    removeBtn.disabled = removing.has(key);
    removeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openConfirmModal(
        "Remove mod?",
        `This permanently deletes "${installedFilename}". This can't be undone.`,
        () => void removeInstalledMod(key, installedFilename),
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
    empty.textContent = "No mods found.";
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
  el.browseModsErrorEl.hidden = true;
  try {
    const [results, provenance] = await Promise.all([
      invoke<ModSearchResult[]>("search_mods_cmd", {
        instanceId,
        source: selectedSource,
        query: el.browseModsQueryInput.value.trim(),
        offset: 0,
      }),
      invoke<ModProvenanceEntry[]>("list_mod_provenance_cmd", { instanceId }),
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

async function removeInstalledMod(key: string, filename: string) {
  if (!currentInstanceId || removing.has(key)) return;
  removing.add(key);
  renderResults(lastResults);
  try {
    await invoke("remove_mod_source_cmd", { instanceId: currentInstanceId, filename });
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

// ---------- detail pane ----------

let detailToken = 0;

async function openDetail(result: ModSearchResult) {
  const token = ++detailToken;
  el.modDetailPlaceholderEl.hidden = true;
  el.modDetailContentEl.hidden = false;
  el.modDetailTitleEl.textContent = result.title;
  el.modDetailMetaEl.textContent = `${result.author} · ${result.source}`;
  el.modDetailBodyEl.textContent = "Loading…";
  try {
    const raw = await invoke<string>("get_mod_description_cmd", { source: result.source, projectId: result.id });
    if (token !== detailToken) return;
    // Modrinth's `body` is Markdown; CurseForge's is already HTML. Either way it's third-party
    // rich text from a remote source, so it always goes through DOMPurify before touching
    // innerHTML -- Markdown itself can embed raw HTML passthrough, so sanitizing only the
    // CurseForge branch wouldn't be enough.
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
  el.modDetailPlaceholderEl.hidden = false;
  el.modDetailContentEl.hidden = true;
}

// ---------- review & install ----------

interface ReviewRow {
  result: ModSearchResult;
  select: HTMLSelectElement;
  depsEl: HTMLElement;
}

let reviewRows: ReviewRow[] = [];
let reviewInstalling = false;

async function loadReviewRowOptions(row: ReviewRow) {
  if (!currentInstanceId) return;
  try {
    const versions = await invoke<ModVersionOption[]>("list_mod_versions_cmd", {
      instanceId: currentInstanceId,
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
    // The list is newest-first regardless of stability (an alpha published yesterday still sorts
    // ahead of last month's release) -- default the dropdown to the newest *stable* build instead
    // of leaving the browser's own "select the first option" behavior pick whatever's newest by
    // date. The user can still explicitly choose an unstable build from the dropdown themselves.
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
  row.depsEl.textContent = "Checking…";
  try {
    const entries = await invoke<ModInstallPreviewEntry[]>("preview_mod_install_cmd", {
      instanceId: currentInstanceId,
      source: row.result.source,
      projectId: row.result.id,
      versionId,
    });
    const deps = entries.filter((e) => e.is_dependency);
    row.depsEl.textContent = deps.length > 0 ? `Brings in: ${deps.map((d) => `${d.title} ${d.version_number}`).join(", ")}` : "";
  } catch (err) {
    console.error(err);
    row.depsEl.textContent = describeError(err);
  }
}

function renderReviewRow(result: ModSearchResult): ReviewRow {
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

  const reviewRow: ReviewRow = { result, select, depsEl };
  select.addEventListener("change", () => void loadReviewRowPreview(reviewRow));
  return reviewRow;
}

function openReviewModal() {
  el.reviewModsListEl.replaceChildren();
  el.reviewModsErrorEl.hidden = true;
  el.reviewModsProgressEl.hidden = true;
  el.reviewModsConfirmBtn.disabled = false;
  el.reviewModsConfirmBtn.textContent = "Install";
  el.reviewModsCancelBtn.disabled = false;

  reviewRows = Array.from(selected.values()).map(renderReviewRow);
  for (const row of reviewRows) void loadReviewRowOptions(row);

  el.reviewModsModalEl.classList.add("is-open");
}

function hideReviewModal() {
  el.reviewModsModalEl.classList.remove("is-open");
}

function renderReviewProgress(label: string, percent: number) {
  el.reviewModsProgressEl.hidden = false;
  el.reviewModsProgressLabelEl.textContent = label;
  el.reviewModsProgressPercentEl.textContent = `${Math.round(percent)}%`;
  el.reviewModsProgressFillEl.style.width = `${percent}%`;
}

async function confirmReview() {
  if (reviewInstalling || !currentInstanceId || reviewRows.length === 0) return;
  reviewInstalling = true;
  el.reviewModsConfirmBtn.disabled = true;
  el.reviewModsConfirmBtn.textContent = "Installing…";
  el.reviewModsCancelBtn.disabled = true;
  el.reviewModsErrorEl.hidden = true;
  renderReviewProgress("Starting…", 0);

  const selections = reviewRows.map((row) => ({
    source: row.result.source,
    projectId: row.result.id,
    versionId: row.select.value || null,
  }));

  try {
    await invoke("install_selected_mods_cmd", { instanceId: currentInstanceId, selections });
    selected.clear();
    updateReviewButton();
    hideReviewModal();
    await Promise.all([runSearch(), refreshInstanceContent(currentInstanceId)]);
  } catch (err) {
    console.error(err);
    el.reviewModsErrorEl.textContent = describeError(err);
    el.reviewModsErrorEl.hidden = false;
  } finally {
    reviewInstalling = false;
    el.reviewModsProgressEl.hidden = true;
    el.reviewModsConfirmBtn.disabled = false;
    el.reviewModsConfirmBtn.textContent = "Install";
    el.reviewModsCancelBtn.disabled = false;
  }
}

// ---------- modal open/close ----------

export function renderBrowseButton(instance: Instance) {
  const hasLoader = instance.mod_loader !== null;
  el.modsBrowseBtn.disabled = !hasLoader;
  el.modsBrowseBtn.title = hasLoader ? "" : "Install a mod loader first";
}

async function openBrowseModsModal(instanceId: string) {
  currentInstanceId = instanceId;
  selectedSource = "Modrinth";
  el.browseModsQueryInput.value = "";
  el.browseModsErrorEl.hidden = true;
  el.browseModsResultsEl.replaceChildren();
  selected.clear();
  updateReviewButton();
  resetDetailPane();
  try {
    hasCurseForgeKey = await invoke<boolean>("has_curseforge_api_key_cmd");
  } catch {
    hasCurseForgeKey = false;
  }
  renderSourceOptions();
  el.browseModsModalEl.classList.add("is-open");
  void runSearch();
}

function closeBrowseModsModal() {
  el.browseModsModalEl.classList.remove("is-open");
}

export function init() {
  el.modsBrowseBtn.addEventListener("click", () => {
    const instance = state.instances.find((i) => i.id === state.viewingInstanceId);
    if (instance?.mod_loader) void openBrowseModsModal(instance.id);
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
  el.browseModsCloseBtn.addEventListener("click", closeBrowseModsModal);
  el.browseModsReviewBtn.addEventListener("click", openReviewModal);

  el.reviewModsConfirmBtn.addEventListener("click", () => void confirmReview());
  el.reviewModsCancelBtn.addEventListener("click", () => {
    if (!reviewInstalling) hideReviewModal();
  });

  void listen<DownloadProgress>("mod-install-progress", (event) => {
    if (!reviewInstalling) return;
    const p = event.payload;
    const phase = p.phase || "Files";
    const percent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    const label = p.files_total > 0 ? `${phase} (${p.files_done}/${p.files_total})` : phase;
    renderReviewProgress(label, percent);
  });
}
