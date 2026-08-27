import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl, openPath } from "@tauri-apps/plugin-opener";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { SkinViewer } from "skinview3d";

interface VersionEntry {
  id: string;
  type: string;
  url: string;
  time: string;
  releaseTime: string;
  sha1: string;
  complianceLevel: number;
}

type Account =
  | { type: "Offline"; username: string; uuid: string }
  | { type: "Microsoft"; id: string; username: string; uuid: string };

interface DownloadProgress {
  phase: string;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  current_file: string | null;
}

interface DeviceAuthorization {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message: string;
}

interface Instance {
  id: string;
  name: string;
  version_id: string;
  icon_path: string | null;
  pinned_screenshot: string | null;
  dir: string;
  mods_dir: string;
  saves_dir: string;
  resource_packs_dir: string;
  shader_packs_dir: string;
  screenshots_dir: string;
}

interface WorldInfo {
  name: string;
  datapacks: string[];
}

interface ModInfo {
  name: string;
  enabled: boolean;
}

interface ScreenshotInfo {
  name: string;
  path: string;
}

interface SkinInfo {
  id: string;
  state: string;
  url: string;
  variant: string;
}

interface CapeInfo {
  id: string;
  state: string;
  url: string;
  alias: string;
}

interface MinecraftProfile {
  id: string;
  name: string;
  skins: SkinInfo[];
  capes: CapeInfo[];
}

interface InstancesResponse {
  instances: Instance[];
  selected_id: string | null;
}

interface DirectorySettings {
  game_dir: string;
  instances_dir: string;
  config_dir: string;
  libraries_dir: string;
}

// Mirrors beacon_core::Account::id() -- the key the config file (and secret store) uses to
// look an account up, so it's what `launch_instance_cmd` needs to select a saved account.
function accountKey(account: Account): string {
  return account.type === "Offline" ? `offline:${account.username}` : `microsoft:${account.id}`;
}

function describeError(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

// `icon_path` can be anywhere on disk (it comes from an open-file dialog with no folder
// restriction), so displaying it needs the `asset:` protocol rather than a plain file:// src --
// the backend allows each icon path individually in the asset protocol scope as it's set
// (`set_instance_icon_cmd`), rather than opening the whole filesystem to it.
function instanceIconBackground(instance: Instance | null): string {
  return instance?.icon_path ? `url("${convertFileSrc(instance.icon_path)}")` : "";
}

// A flat gray square with nothing in it reads as a broken placeholder, not "no icon set" -- for
// purely decorative icon slots (unlike `instance-detail__icon`, which stays visible because it's
// also the click target for setting one) this just hides the element entirely when there's
// nothing to show.
function applyDecorativeIcon(el: HTMLElement, instance: Instance | null) {
  const background = instanceIconBackground(instance);
  el.style.backgroundImage = background;
  el.style.display = background ? "" : "none";
}

// Plain `el.textContent = path` wraps a long Windows path at whatever character happens to hit
// the container edge, splitting words like "desktop" into "deskt"/"op". `<wbr>` marks the path's
// separators as the only places allowed to wrap instead, so long paths break at a `\` the same
// way a human would read them -- `.textContent` reads elsewhere are unaffected, since a `<wbr>`
// contributes no text of its own.
function setPathText(el: HTMLElement, path: string) {
  el.replaceChildren();
  for (const part of path.split(/([\\/])/)) {
    el.append(document.createTextNode(part));
    if (part === "\\" || part === "/") el.append(document.createElement("wbr"));
  }
}

let versions: VersionEntry[] = [];
let directorySettings: DirectorySettings | null = null;
let accounts: Account[] = [];
let currentAccount: Account | null = null;
let instances: Instance[] = [];
let selectedInstanceId: string | null = null;
let playStage: "idle" | "installing" | "launching" = "idle";
let installingLabel = "Installing...";
let installProgressPercent = 0;
let pendingVerificationUri = "";
let offlineModalMode: { kind: "add" } | { kind: "rename"; accountId: string; current: string } | null = null;
let viewingInstanceId: string | null = null;
let renameInstanceTargetId: string | null = null;
let createInstanceSelectedVersion: string | null = null;
let pendingConfirmAction: (() => void) | null = null;
let playBackdropScreenshots: ScreenshotInfo[] = [];
let playBackdropIndex = 0;
let playBackdropActiveLayer: "a" | "b" = "a";
let playBackdropTimer: ReturnType<typeof setInterval> | null = null;
let skinViewer: SkinViewer | null = null;
let selectedSkinVariant: "classic" | "slim" = "classic";

async function main() {
  const appWindow = getCurrentWindow();

  const accountMenuEl = document.querySelector<HTMLElement>("#account-menu")!;
  const accountButton = document.querySelector<HTMLButtonElement>("#account-button")!;
  const accountNameEl = document.querySelector<HTMLElement>("#account-name")!;
  const accountStatusEl = document.querySelector<HTMLElement>("#account-status")!;
  const accountMenuAccountsEl = document.querySelector<HTMLElement>("#account-menu-accounts")!;
  const addOfflineMenuItem = document.querySelector<HTMLButtonElement>("#account-menu-add-offline")!;
  const manageAddOfflineBtn = document.querySelector<HTMLButtonElement>("#manage-add-offline")!;
  const playbarUserEl = document.querySelector<HTMLElement>("#playbar-user")!;
  const playButton = document.querySelector<HTMLButtonElement>("#play-button")!;
  const playLabelEl = document.querySelector<HTMLElement>("#play-label")!;
  const progressPanelEl = document.querySelector<HTMLElement>("#progress-panel")!;
  const progressLabelEl = document.querySelector<HTMLElement>("#progress-label")!;
  const progressPercentEl = document.querySelector<HTMLElement>("#progress-percent")!;
  const progressFillEl = document.querySelector<HTMLElement>("#progress-fill")!;
  const launchErrorEl = document.querySelector<HTMLElement>("#launch-error")!;
  const loginModalEl = document.querySelector<HTMLElement>("#login-modal")!;
  const loginCodeEl = document.querySelector<HTMLElement>("#login-code")!;
  const errorModalEl = document.querySelector<HTMLElement>("#error-modal")!;
  const errorMessageEl = document.querySelector<HTMLElement>("#error-message")!;
  const offlineModalEl = document.querySelector<HTMLElement>("#offline-modal")!;
  const offlineModalEyebrowEl = document.querySelector<HTMLElement>("#offline-modal-eyebrow")!;
  const offlineNicknameInput = document.querySelector<HTMLInputElement>("#offline-nickname-input")!;
  const offlineNicknameError = document.querySelector<HTMLElement>("#offline-nickname-error")!;
  const offlineConfirmBtn = document.querySelector<HTMLButtonElement>("#offline-confirm")!;
  const accountsScreenEl = document.querySelector<HTMLElement>("#accounts-screen")!;
  const settingsScreenEl = document.querySelector<HTMLElement>("#settings-screen")!;
  const instanceScreenEl = document.querySelector<HTMLElement>("#instance-screen")!;
  const accountsNavBtn = document.querySelector<HTMLButtonElement>("#accounts-nav")!;
  const accountsBackBtn = document.querySelector<HTMLButtonElement>("#accounts-back")!;
  const manageListEl = document.querySelector<HTMLElement>("#manage-list")!;
  const settingsNavBtn = document.querySelector<HTMLButtonElement>("#settings-nav")!;
  const settingsBackBtn = document.querySelector<HTMLButtonElement>("#settings-back")!;
  const snapshotsToggle = document.querySelector<HTMLButtonElement>("#snapshots-toggle")!;
  const snapshotsToggleLabel = document.querySelector<HTMLElement>("#snapshots-toggle-label")!;
  const screenshotsBgToggle = document.querySelector<HTMLButtonElement>("#screenshots-bg-toggle")!;
  const screenshotsBgToggleLabel = document.querySelector<HTMLElement>("#screenshots-bg-toggle-label")!;
  const screenshotsBgBlurInput = document.querySelector<HTMLInputElement>("#screenshots-bg-blur")!;
  const gameDirPathEl = document.querySelector<HTMLElement>("#game-dir-path")!;
  const gameDirOpenBtn = document.querySelector<HTMLButtonElement>("#game-dir-open")!;
  const gameDirBrowseBtn = document.querySelector<HTMLButtonElement>("#game-dir-browse")!;
  const instancesDirPathEl = document.querySelector<HTMLElement>("#instances-dir-path")!;
  const instancesDirOpenBtn = document.querySelector<HTMLButtonElement>("#instances-dir-open")!;
  const instancesDirBrowseBtn = document.querySelector<HTMLButtonElement>("#instances-dir-browse")!;
  const configDirPathEl = document.querySelector<HTMLElement>("#config-dir-path")!;
  const configDirOpenBtn = document.querySelector<HTMLButtonElement>("#config-dir-open")!;
  const themeOptions = document.querySelectorAll<HTMLButtonElement>(".theme-option");
  const skinsSigninEl = document.querySelector<HTMLElement>("#skins-signin")!;
  const skinsSigninBtn = document.querySelector<HTMLButtonElement>("#skins-signin-btn")!;
  const skinsViewEl = document.querySelector<HTMLElement>("#skins-view")!;
  const skinViewerCanvas = document.querySelector<HTMLCanvasElement>("#skin-viewer-canvas")!;
  const skinVariantOptions = document.querySelectorAll<HTMLButtonElement>(".skin-variant__option");
  const skinUploadBtn = document.querySelector<HTMLButtonElement>("#skin-upload-btn")!;
  const skinResetBtn = document.querySelector<HTMLButtonElement>("#skin-reset-btn")!;
  const capeGridEl = document.querySelector<HTMLElement>("#cape-grid")!;
  const wipeAllBtn = document.querySelector<HTMLButtonElement>("#wipe-all-btn")!;
  const wipeModalEl = document.querySelector<HTMLElement>("#wipe-modal")!;
  const wipeConfirmInput = document.querySelector<HTMLInputElement>("#wipe-confirm-input")!;
  const wipeConfirmBtn = document.querySelector<HTMLButtonElement>("#wipe-confirm-btn")!;
  const wipeCancelBtn = document.querySelector<HTMLButtonElement>("#wipe-cancel-btn")!;

  const heroEl = document.querySelector<HTMLElement>("#hero")!;
  const heroBackdropAEl = document.querySelector<HTMLElement>("#hero-backdrop-a")!;
  const heroBackdropBEl = document.querySelector<HTMLElement>("#hero-backdrop-b")!;

  const instancePickerEl = document.querySelector<HTMLElement>("#instance-picker")!;
  const instancePickerTrigger = document.querySelector<HTMLButtonElement>("#instance-picker-trigger")!;
  const instancePickerIconEl = document.querySelector<HTMLElement>("#instance-picker-icon")!;
  const instancePickerListEl = document.querySelector<HTMLElement>("#instance-picker-list")!;
  const playbarInstanceNameEl = document.querySelector<HTMLElement>("#playbar-instance-name")!;
  const playbarInstanceVersionEl = document.querySelector<HTMLElement>("#playbar-instance-version")!;

  const instanceGridEl = document.querySelector<HTMLElement>("#instance-grid")!;
  const newInstanceBtn = document.querySelector<HTMLButtonElement>("#new-instance-btn")!;
  const importInstanceBtn = document.querySelector<HTMLButtonElement>("#import-instance-btn")!;

  const instanceBackBtn = document.querySelector<HTMLButtonElement>("#instance-back")!;
  const instanceScreenTitleEl = document.querySelector<HTMLElement>("#instance-screen-title")!;
  const instanceIconBtn = document.querySelector<HTMLButtonElement>("#instance-icon-btn")!;
  const instanceDetailNameEl = document.querySelector<HTMLElement>("#instance-detail-name")!;
  const instanceDetailVersionEl = document.querySelector<HTMLElement>("#instance-detail-version")!;
  const instanceVersionNameEl = document.querySelector<HTMLElement>("#instance-version-name")!;
  const instanceRenameBtn = document.querySelector<HTMLButtonElement>("#instance-rename-btn")!;
  const instanceVersionBtn = document.querySelector<HTMLButtonElement>("#instance-version-btn")!;
  const instanceIconClearBtn = document.querySelector<HTMLButtonElement>("#instance-icon-clear-btn")!;
  const instanceOpenFolderBtn = document.querySelector<HTMLButtonElement>("#instance-open-folder-btn")!;
  const instanceLibrariesOpenBtn = document.querySelector<HTMLButtonElement>("#instance-libraries-open-btn")!;
  const instanceExportBtn = document.querySelector<HTMLButtonElement>("#instance-export-btn")!;
  const instanceDeleteBtn = document.querySelector<HTMLButtonElement>("#instance-delete-btn")!;
  const modsListEl = document.querySelector<HTMLElement>("#mods-list")!;
  const modsAddBtn = document.querySelector<HTMLButtonElement>("#mods-add-btn")!;
  const modsOpenFolderBtn = document.querySelector<HTMLButtonElement>("#mods-open-folder-btn")!;
  const worldsListEl = document.querySelector<HTMLElement>("#worlds-list")!;
  const worldsOpenFolderBtn = document.querySelector<HTMLButtonElement>("#worlds-open-folder-btn")!;
  const resourcePacksListEl = document.querySelector<HTMLElement>("#resource-packs-list")!;
  const resourcePacksOpenFolderBtn = document.querySelector<HTMLButtonElement>("#resource-packs-open-folder-btn")!;
  const shaderPacksListEl = document.querySelector<HTMLElement>("#shader-packs-list")!;
  const shaderPacksOpenFolderBtn = document.querySelector<HTMLButtonElement>("#shader-packs-open-folder-btn")!;
  const screenshotsGridEl = document.querySelector<HTMLElement>("#screenshots-grid")!;
  const screenshotsOpenFolderBtn = document.querySelector<HTMLButtonElement>("#screenshots-open-folder-btn")!;

  const createInstanceModalEl = document.querySelector<HTMLElement>("#create-instance-modal")!;
  const createInstanceNameInput = document.querySelector<HTMLInputElement>("#create-instance-name")!;
  const createInstanceVersionsEl = document.querySelector<HTMLElement>("#create-instance-versions")!;
  const createInstanceErrorEl = document.querySelector<HTMLElement>("#create-instance-error")!;
  const createInstanceConfirmBtn = document.querySelector<HTMLButtonElement>("#create-instance-confirm")!;

  const changeVersionModalEl = document.querySelector<HTMLElement>("#change-version-modal")!;
  const changeVersionVersionsEl = document.querySelector<HTMLElement>("#change-version-versions")!;

  const renameInstanceModalEl = document.querySelector<HTMLElement>("#rename-instance-modal")!;
  const renameInstanceInput = document.querySelector<HTMLInputElement>("#rename-instance-input")!;
  const renameInstanceErrorEl = document.querySelector<HTMLElement>("#rename-instance-error")!;
  const renameInstanceConfirmBtn = document.querySelector<HTMLButtonElement>("#rename-instance-confirm")!;

  const confirmModalEl = document.querySelector<HTMLElement>("#delete-instance-modal")!;
  const confirmEyebrowEl = document.querySelector<HTMLElement>("#delete-instance-eyebrow")!;
  const confirmMessageEl = document.querySelector<HTMLElement>("#delete-instance-message")!;
  const confirmActionBtn = document.querySelector<HTMLButtonElement>("#delete-instance-confirm")!;

  // ---------- window controls ----------

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

  // ---------- tabs & sidebar selection (cosmetic only) ----------

  const tabs = document.querySelectorAll<HTMLButtonElement>("[data-tab]");
  const panels = document.querySelectorAll<HTMLElement>("[data-tab-panel]");

  function showTab(target: string) {
    tabs.forEach((t) => t.classList.toggle("is-active", t.dataset.tab === target));
    panels.forEach((panel) => panel.classList.toggle("is-active", panel.dataset.tabPanel === target));
    // The 3D skin viewer keeps rendering (and using GPU) even while its tab is hidden unless told
    // otherwise -- pause it off-tab, resume (and load fresh data) when the tab is actually opened.
    if (skinViewer) skinViewer.renderPaused = target !== "skins";
    if (target === "skins") void loadSkinsTab();
  }

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => showTab(tab.dataset.tab ?? "play"));
  });

  // Accounts/Settings are one-off navigations (they open a fullscreen screen), not a
  // persistent choice like the game entry above them -- excluded from the selection toggle so
  // they don't pick up the beacon-beam highlight on click.
  const navRows = document.querySelectorAll<HTMLButtonElement>(
    ".nav-row[data-nav]:not(#accounts-nav):not(#settings-nav)",
  );
  navRows.forEach((row) => {
    row.addEventListener("click", () => {
      navRows.forEach((r) => r.classList.toggle("is-selected", r === row));
    });
  });

  // ---------- fullscreen screens (accounts / settings / instance detail share one at a time) ----------

  function closeAllScreens() {
    accountsScreenEl.classList.remove("is-open");
    settingsScreenEl.classList.remove("is-open");
    instanceScreenEl.classList.remove("is-open");
  }

  // ---------- generic confirm modal (delete instance, delete world) ----------

  function openConfirmModal(eyebrow: string, message: string, action: () => void) {
    confirmEyebrowEl.textContent = eyebrow;
    confirmMessageEl.textContent = message;
    pendingConfirmAction = action;
    confirmModalEl.classList.add("is-open");
  }

  function hideConfirmModal() {
    confirmModalEl.classList.remove("is-open");
    pendingConfirmAction = null;
  }

  confirmActionBtn.addEventListener("click", () => {
    const action = pendingConfirmAction;
    hideConfirmModal();
    action?.();
  });
  document.querySelector<HTMLButtonElement>("#delete-instance-cancel")!.addEventListener("click", hideConfirmModal);

  // ---------- account / sign-in ----------

  function renderAccount() {
    if (currentAccount) {
      accountNameEl.textContent = currentAccount.username;
      accountStatusEl.textContent = "Connected";
      accountStatusEl.className = "account__status account__status--connected";
      playbarUserEl.textContent = currentAccount.username;
    } else {
      accountNameEl.textContent = "Sign in";
      accountStatusEl.textContent = "Offline mode";
      accountStatusEl.className = "account__status";
      playbarUserEl.textContent = "Not signed in";
    }
    renderPlayButton();
    // Account switches (sign-in, reorder in the account menu) change what the Skins tab should
    // show -- but only bother refetching if it's actually the tab on screen right now.
    if (document.querySelector('[data-tab-panel="skins"]')?.classList.contains("is-active")) {
      void loadSkinsTab();
    }
  }

  function openAccountMenu() {
    accountMenuEl.classList.add("is-open");
  }

  function closeAccountMenu() {
    accountMenuEl.classList.remove("is-open");
  }

  function renderAccountMenu() {
    accountMenuAccountsEl.replaceChildren();
    accounts.forEach((account, index) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "nav-row account-row";
      row.classList.toggle("is-selected", index === 0);

      const nameSpan = document.createElement("span");
      nameSpan.className = "account-row__name";
      nameSpan.textContent = account.username;

      const typeSpan = document.createElement("span");
      typeSpan.className = "account-row__type";
      typeSpan.textContent = account.type === "Microsoft" ? "Microsoft" : "Offline";

      row.append(nameSpan, typeSpan);
      row.addEventListener("click", () => void selectAccount(account));
      accountMenuAccountsEl.appendChild(row);
    });

    const hasMicrosoft = accounts.some((a) => a.type === "Microsoft");
    addOfflineMenuItem.disabled = !hasMicrosoft;
    manageAddOfflineBtn.disabled = !hasMicrosoft;
  }

  async function selectAccount(account: Account) {
    closeAccountMenu();
    if (accounts[0] && accountKey(accounts[0]) === accountKey(account)) return;
    try {
      await invoke("select_account_cmd", { accountId: accountKey(account) });
      await loadAccounts();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  function showLoginModal(auth: DeviceAuthorization) {
    loginCodeEl.textContent = auth.user_code;
    pendingVerificationUri = auth.verification_uri;
    loginModalEl.classList.add("is-open");
  }

  function hideLoginModal() {
    loginModalEl.classList.remove("is-open");
  }

  function showErrorModal(message: string) {
    errorMessageEl.textContent = message;
    errorModalEl.classList.add("is-open");
  }

  function hideErrorModal() {
    errorModalEl.classList.remove("is-open");
  }

  async function startSignIn() {
    accountStatusEl.textContent = "Connecting...";
    accountStatusEl.className = "account__status";
    try {
      currentAccount = await invoke<Account>("login_microsoft_cmd");
      hideLoginModal();
      await loadAccounts();
    } catch (err) {
      console.error(err);
      hideLoginModal();
      accountStatusEl.textContent = "Connection failed. Please log in again.";
      accountStatusEl.className = "account__status account__status--error";
      showErrorModal(describeError(err));
    }
  }

  // ---------- skins & capes ----------

  function ensureSkinViewer(): SkinViewer {
    if (!skinViewer) {
      skinViewer = new SkinViewer({
        canvas: skinViewerCanvas,
        width: 280,
        height: 380,
      });
      skinViewer.autoRotate = true;
      skinViewer.autoRotateSpeed = 0.6;
    }
    return skinViewer;
  }

  function renderSkinVariantButtons() {
    skinVariantOptions.forEach((btn) => {
      const selected = btn.dataset.variant === selectedSkinVariant;
      btn.classList.toggle("is-selected", selected);
      btn.setAttribute("aria-checked", String(selected));
    });
  }

  skinVariantOptions.forEach((btn) => {
    btn.addEventListener("click", () => {
      const variant = btn.dataset.variant;
      if (variant !== "classic" && variant !== "slim") return;
      selectedSkinVariant = variant;
      renderSkinVariantButtons();
    });
  });

  function renderCapeGrid(profile: MinecraftProfile, accountId: string) {
    capeGridEl.replaceChildren();

    const noneCard = document.createElement("button");
    noneCard.type = "button";
    noneCard.className = "cape-card";
    const hasActiveCape = profile.capes.some((c) => c.state === "ACTIVE");
    noneCard.classList.toggle("is-active", !hasActiveCape);
    const noneThumb = document.createElement("span");
    noneThumb.className = "cape-card__thumb";
    const noneName = document.createElement("span");
    noneName.className = "cape-card__name";
    noneName.textContent = "None";
    noneCard.append(noneThumb, noneName);
    noneCard.addEventListener("click", () => void clearCape(accountId));
    capeGridEl.appendChild(noneCard);

    for (const cape of profile.capes) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "cape-card";
      card.classList.toggle("is-active", cape.state === "ACTIVE");

      const thumb = document.createElement("span");
      thumb.className = "cape-card__thumb";
      thumb.style.backgroundImage = `url("${cape.url}")`;

      const name = document.createElement("span");
      name.className = "cape-card__name";
      name.textContent = cape.alias;
      name.title = cape.alias;

      card.append(thumb, name);
      card.addEventListener("click", () => void applyCape(accountId, cape.id));
      capeGridEl.appendChild(card);
    }
  }

  async function loadSkinsTab() {
    if (!currentAccount || currentAccount.type !== "Microsoft") {
      skinsSigninEl.hidden = false;
      skinsViewEl.hidden = true;
      return;
    }
    skinsSigninEl.hidden = true;
    skinsViewEl.hidden = false;

    const accountId = accountKey(currentAccount);
    try {
      const profile = await invoke<MinecraftProfile>("get_skin_profile_cmd", { accountId });
      const activeSkin = profile.skins.find((s) => s.state === "ACTIVE") ?? profile.skins[0];
      const viewer = ensureSkinViewer();
      if (activeSkin) {
        selectedSkinVariant = activeSkin.variant.toLowerCase() === "slim" ? "slim" : "classic";
        await viewer.loadSkin(activeSkin.url, {
          model: selectedSkinVariant === "slim" ? "slim" : "default",
        });
      }
      const activeCape = profile.capes.find((c) => c.state === "ACTIVE");
      if (activeCape) {
        await viewer.loadCape(activeCape.url);
      } else {
        viewer.loadCape(null);
      }
      renderSkinVariantButtons();
      renderCapeGrid(profile, accountId);
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function uploadSkin() {
    if (!currentAccount || currentAccount.type !== "Microsoft") return;
    const accountId = accountKey(currentAccount);
    try {
      const picked = await openFileDialog({ multiple: false, filters: [{ name: "Skin", extensions: ["png"] }] });
      if (!picked || Array.isArray(picked)) return;
      await invoke("upload_skin_cmd", { accountId, filePath: picked, variant: selectedSkinVariant });
      await loadSkinsTab();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function resetSkin() {
    if (!currentAccount || currentAccount.type !== "Microsoft") return;
    try {
      await invoke("reset_skin_cmd", { accountId: accountKey(currentAccount) });
      await loadSkinsTab();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function applyCape(accountId: string, capeId: string) {
    try {
      await invoke("set_cape_cmd", { accountId, capeId });
      await loadSkinsTab();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function clearCape(accountId: string) {
    try {
      await invoke("clear_cape_cmd", { accountId });
      await loadSkinsTab();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  skinUploadBtn.addEventListener("click", () => void uploadSkin());
  skinResetBtn.addEventListener("click", () => void resetSkin());
  skinsSigninBtn.addEventListener("click", () => void startSignIn());

  accountButton.addEventListener("click", () => {
    accountMenuEl.classList.contains("is-open") ? closeAccountMenu() : openAccountMenu();
  });

  document.querySelector<HTMLButtonElement>("#account-menu-signin")!.addEventListener("click", () => {
    closeAccountMenu();
    void startSignIn();
  });

  document.querySelector<HTMLButtonElement>("#account-menu-add-offline")!.addEventListener("click", () => {
    closeAccountMenu();
    openOfflineModal({ kind: "add" });
  });

  document.querySelector<HTMLButtonElement>("#account-menu-manage")!.addEventListener("click", () => {
    closeAccountMenu();
    openAccountsScreen();
  });

  document.querySelector<HTMLButtonElement>("#login-open-browser")!.addEventListener("click", () => {
    if (pendingVerificationUri) void openUrl(pendingVerificationUri);
  });

  document.querySelector<HTMLButtonElement>("#login-close")!.addEventListener("click", hideLoginModal);
  document.querySelector<HTMLButtonElement>("#error-close")!.addEventListener("click", hideErrorModal);

  // ---------- add / rename offline account ----------

  function openOfflineModal(mode: NonNullable<typeof offlineModalMode>) {
    offlineModalMode = mode;
    offlineModalEyebrowEl.textContent = mode.kind === "rename" ? "Rename offline account" : "Add offline account";
    offlineConfirmBtn.textContent = mode.kind === "rename" ? "Rename" : "Add";
    offlineNicknameInput.value = mode.kind === "rename" ? mode.current : "";
    offlineNicknameError.hidden = true;
    offlineModalEl.classList.add("is-open");
    offlineNicknameInput.focus();
  }

  function hideOfflineModal() {
    offlineModalEl.classList.remove("is-open");
    offlineModalMode = null;
  }

  async function confirmOfflineModal() {
    const mode = offlineModalMode;
    if (!mode) return;
    const nickname = offlineNicknameInput.value.trim();
    if (!/^[A-Za-z0-9_]{3,16}$/.test(nickname)) {
      offlineNicknameError.textContent = "Nicknames are 3-16 characters: letters, numbers, underscore.";
      offlineNicknameError.hidden = false;
      return;
    }

    try {
      if (mode.kind === "rename") {
        await invoke("rename_offline_account_cmd", { accountId: mode.accountId, nickname });
      } else {
        await invoke("add_offline_account_cmd", { nickname });
      }
      hideOfflineModal();
      await loadAccounts();
    } catch (err) {
      console.error(err);
      offlineNicknameError.textContent = describeError(err);
      offlineNicknameError.hidden = false;
    }
  }

  offlineConfirmBtn.addEventListener("click", () => void confirmOfflineModal());
  document.querySelector<HTMLButtonElement>("#offline-cancel")!.addEventListener("click", hideOfflineModal);
  manageAddOfflineBtn.addEventListener("click", () => openOfflineModal({ kind: "add" }));

  // ---------- manage accounts (fullscreen) ----------

  function openAccountsScreen() {
    closeAllScreens();
    renderManageList();
    accountsScreenEl.classList.add("is-open");
  }

  function closeAccountsScreen() {
    accountsScreenEl.classList.remove("is-open");
  }

  accountsNavBtn.addEventListener("click", openAccountsScreen);
  accountsBackBtn.addEventListener("click", closeAccountsScreen);

  // ---------- settings (fullscreen) ----------

  function openSettingsScreen() {
    closeAllScreens();
    settingsScreenEl.classList.add("is-open");
  }

  function closeSettingsScreen() {
    settingsScreenEl.classList.remove("is-open");
  }

  settingsNavBtn.addEventListener("click", openSettingsScreen);
  settingsBackBtn.addEventListener("click", closeSettingsScreen);

  // ---------- theme ----------

  const THEMES = ["beacon", "amber", "light", "amber-light"] as const;
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
    themeOptions.forEach((option) => {
      const selected = option.dataset.theme === currentTheme;
      option.classList.toggle("is-selected", selected);
      option.setAttribute("aria-checked", String(selected));
    });
  }

  themeOptions.forEach((option) => {
    option.addEventListener("click", () => {
      const theme = option.dataset.theme;
      if (!theme || !isTheme(theme) || theme === currentTheme) return;
      currentTheme = theme;
      writeTheme(currentTheme);
      applyTheme();
    });
  });

  applyTheme();

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

  let showSnapshots = readShowSnapshots();

  function renderSnapshotsToggle() {
    snapshotsToggle.classList.toggle("is-on", showSnapshots);
    snapshotsToggle.setAttribute("aria-checked", String(showSnapshots));
    snapshotsToggleLabel.textContent = showSnapshots ? "On" : "Off";
  }

  snapshotsToggle.addEventListener("click", () => {
    showSnapshots = !showSnapshots;
    writeShowSnapshots(showSnapshots);
    renderSnapshotsToggle();
    void loadVersions();
  });

  renderSnapshotsToggle();

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

  let screenshotsBgEnabled = readScreenshotsBgEnabled();
  let screenshotsBgBlur = readScreenshotsBgBlur();

  function renderScreenshotsBgSettings() {
    screenshotsBgToggle.classList.toggle("is-on", screenshotsBgEnabled);
    screenshotsBgToggle.setAttribute("aria-checked", String(screenshotsBgEnabled));
    screenshotsBgToggleLabel.textContent = screenshotsBgEnabled ? "On" : "Off";
    screenshotsBgBlurInput.disabled = !screenshotsBgEnabled;
    screenshotsBgBlurInput.value = String(screenshotsBgBlur);
    document.documentElement.style.setProperty("--screenshot-blur", `${screenshotsBgBlur}px`);
  }

  screenshotsBgToggle.addEventListener("click", () => {
    screenshotsBgEnabled = !screenshotsBgEnabled;
    writeScreenshotsBgEnabled(screenshotsBgEnabled);
    renderScreenshotsBgSettings();
    void refreshPlayBackdrop();
  });

  screenshotsBgBlurInput.addEventListener("input", () => {
    screenshotsBgBlur = Number(screenshotsBgBlurInput.value);
    writeScreenshotsBgBlur(screenshotsBgBlur);
    renderScreenshotsBgSettings();
  });

  renderScreenshotsBgSettings();

  // ---------- directory settings ----------

  let directoriesBusy = false;

  function renderDirectoriesBusyState() {
    gameDirBrowseBtn.disabled = directoriesBusy;
    gameDirOpenBtn.disabled = directoriesBusy;
    instancesDirBrowseBtn.disabled = directoriesBusy;
    instancesDirOpenBtn.disabled = directoriesBusy;
  }

  async function loadDirectorySettings() {
    try {
      const settings = await invoke<DirectorySettings>("get_directory_settings");
      directorySettings = settings;
      setPathText(gameDirPathEl, settings.game_dir);
      setPathText(instancesDirPathEl, settings.instances_dir);
      setPathText(configDirPathEl, settings.config_dir);
    } catch (err) {
      console.error(err);
      gameDirPathEl.textContent = "Unknown";
      instancesDirPathEl.textContent = "Unknown";
      configDirPathEl.textContent = "Unknown";
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

  // `openPath` rejects if the folder doesn't exist yet (e.g. nothing installed there yet) or
  // the path is otherwise invalid -- surface that instead of swallowing it, so "Open" clicked on
  // an empty/fresh setup says why nothing happened instead of looking like it does nothing.
  async function openFolder(path: string) {
    try {
      await openPath(path);
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  gameDirOpenBtn.addEventListener("click", () => void openFolder(gameDirPathEl.textContent ?? ""));
  gameDirBrowseBtn.addEventListener("click", () =>
    void relocateDirectory("set_game_dir_cmd", gameDirPathEl, gameDirBrowseBtn, gameDirPathEl.textContent ?? ""),
  );

  instancesDirOpenBtn.addEventListener("click", () => void openFolder(instancesDirPathEl.textContent ?? ""));
  instancesDirBrowseBtn.addEventListener("click", () =>
    void relocateDirectory(
      "set_instances_dir_cmd",
      instancesDirPathEl,
      instancesDirBrowseBtn,
      instancesDirPathEl.textContent ?? "",
    ),
  );

  // Read-only -- see `DirectorySettings.config_dir` on the Rust side for why this one has no
  // Browse button.
  configDirOpenBtn.addEventListener("click", () => void openFolder(configDirPathEl.textContent ?? ""));

  // ---------- wipe all data ----------
  //
  // Deliberately not reusing the generic single-click `openConfirmModal` used for deleting one
  // instance/world -- this deletes every account, instance and setting at once, so it's gated
  // behind typing a confirmation word rather than a single click.

  const WIPE_CONFIRM_WORD = "WIPE";

  function openWipeModal() {
    wipeConfirmInput.value = "";
    wipeConfirmBtn.disabled = true;
    wipeConfirmBtn.textContent = "Wipe everything";
    wipeCancelBtn.disabled = false;
    wipeModalEl.classList.add("is-open");
    wipeConfirmInput.focus();
  }

  function hideWipeModal() {
    wipeModalEl.classList.remove("is-open");
  }

  wipeConfirmInput.addEventListener("input", () => {
    wipeConfirmBtn.disabled = wipeConfirmInput.value.trim().toUpperCase() !== WIPE_CONFIRM_WORD;
  });

  async function performWipe() {
    if (wipeConfirmInput.value.trim().toUpperCase() !== WIPE_CONFIRM_WORD) return;
    wipeConfirmBtn.disabled = true;
    wipeCancelBtn.disabled = true;
    wipeConfirmBtn.textContent = "Wiping...";
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

  wipeAllBtn.addEventListener("click", openWipeModal);
  wipeCancelBtn.addEventListener("click", hideWipeModal);
  wipeConfirmBtn.addEventListener("click", () => void performWipe());

  function renderManageList() {
    manageListEl.replaceChildren();

    if (accounts.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No accounts yet.";
      manageListEl.appendChild(empty);
      return;
    }

    accounts.forEach((account, index) => {
      const row = document.createElement("div");
      row.className = "manage-row";

      const info = document.createElement("div");
      info.className = "manage-row__info";
      const name = document.createElement("span");
      name.className = "manage-row__name";
      name.textContent = account.username;
      const type = document.createElement("span");
      type.className = "manage-row__type";
      type.textContent = account.type === "Microsoft" ? "Microsoft" : "Offline";
      info.append(name, type);
      if (index === 0) {
        const def = document.createElement("span");
        def.className = "manage-row__default";
        def.textContent = "Default";
        info.appendChild(def);
      }

      const actions = document.createElement("div");
      actions.className = "manage-row__actions";

      const upBtn = document.createElement("button");
      upBtn.type = "button";
      upBtn.className = "manage-row__btn";
      upBtn.textContent = "▲";
      upBtn.setAttribute("aria-label", "Move up");
      upBtn.disabled = index === 0;
      upBtn.addEventListener("click", () => void moveAccount(account, "up"));

      const downBtn = document.createElement("button");
      downBtn.type = "button";
      downBtn.className = "manage-row__btn";
      downBtn.textContent = "▼";
      downBtn.setAttribute("aria-label", "Move down");
      downBtn.disabled = index === accounts.length - 1;
      downBtn.addEventListener("click", () => void moveAccount(account, "down"));

      actions.append(upBtn, downBtn);

      if (account.type === "Offline") {
        const renameBtn = document.createElement("button");
        renameBtn.type = "button";
        renameBtn.className = "manage-row__btn";
        renameBtn.textContent = "Rename";
        renameBtn.addEventListener("click", () => {
          openOfflineModal({ kind: "rename", accountId: accountKey(account), current: account.username });
        });
        actions.appendChild(renameBtn);
      }

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "manage-row__btn manage-row__btn--danger";
      removeBtn.textContent = "Remove";
      removeBtn.addEventListener("click", () => void removeAccount(account));
      actions.appendChild(removeBtn);

      row.append(info, actions);
      manageListEl.appendChild(row);
    });
  }

  async function moveAccount(account: Account, direction: "up" | "down") {
    try {
      await invoke("move_account_cmd", { accountId: accountKey(account), direction });
      await loadAccounts();
      renderManageList();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function removeAccount(account: Account) {
    try {
      await invoke("logout_cmd", { accountId: accountKey(account) });
      await loadAccounts();
      renderManageList();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  document.querySelector<HTMLButtonElement>("#manage-add-microsoft")!.addEventListener("click", () => {
    closeAccountsScreen();
    void startSignIn();
  });

  async function loadAccounts() {
    try {
      accounts = await invoke<Account[]>("list_accounts");
      currentAccount = accounts[0] ?? null;
    } catch (err) {
      console.error(err);
      accounts = [];
      currentAccount = null;
    }
    renderAccount();
    renderAccountMenu();
    renderManageList();
  }

  // ---------- versions (fetched once, rendered on demand inside the create/change-version modals) ----------

  function renderVersionOptions(container: HTMLElement, selectedId: string | null, onPick: (versionId: string) => void) {
    container.replaceChildren();
    if (versions.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No versions found.";
      container.appendChild(empty);
      return;
    }

    for (const version of versions) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "nav-row version-row";
      row.classList.toggle("is-selected", version.id === selectedId);

      const idSpan = document.createElement("span");
      idSpan.className = "version-row__id";
      idSpan.textContent = version.id;

      const metaSpan = document.createElement("span");
      metaSpan.className = "version-row__meta";
      metaSpan.textContent = `${version.type} · ${version.releaseTime.slice(0, 10)}`;

      row.append(idSpan, metaSpan);
      row.addEventListener("click", () => onPick(version.id));
      container.appendChild(row);
    }
  }

  async function loadVersions() {
    try {
      versions = await invoke<VersionEntry[]>("list_versions", { snapshots: showSnapshots });
    } catch (err) {
      console.error(err);
      versions = [];
    }
  }

  // ---------- instance picker (playbar) ----------

  function currentInstance(): Instance | null {
    return instances.find((i) => i.id === selectedInstanceId) ?? null;
  }

  function renderInstancePickerTrigger() {
    const current = currentInstance();
    playbarInstanceNameEl.textContent = current ? current.name : instances.length === 0 ? "No instances" : "Select instance";
    playbarInstanceVersionEl.textContent = current ? current.version_id : "—";
    applyDecorativeIcon(instancePickerIconEl, current);
  }

  function renderInstancePickerList() {
    instancePickerListEl.replaceChildren();
    if (instances.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No instances yet.";
      instancePickerListEl.appendChild(empty);
      return;
    }
    for (const instance of instances) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "nav-row version-row";
      row.classList.toggle("is-selected", instance.id === selectedInstanceId);

      const nameSpan = document.createElement("span");
      nameSpan.className = "version-row__title";
      nameSpan.textContent = instance.name;

      const metaSpan = document.createElement("span");
      metaSpan.className = "version-row__meta";
      metaSpan.textContent = instance.version_id;

      row.append(nameSpan, metaSpan);
      row.addEventListener("click", () => void selectInstance(instance.id));
      instancePickerListEl.appendChild(row);
    }
  }

  function openInstancePicker() {
    if (instancePickerTrigger.disabled) return;
    instancePickerEl.classList.add("is-open");
  }

  function closeInstancePicker() {
    instancePickerEl.classList.remove("is-open");
  }

  instancePickerTrigger.addEventListener("click", () => {
    instancePickerEl.classList.contains("is-open") ? closeInstancePicker() : openInstancePicker();
  });

  async function selectInstance(instanceId: string) {
    closeInstancePicker();
    if (instanceId === selectedInstanceId) return;
    try {
      await invoke("select_instance_cmd", { instanceId });
      await loadInstances();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  document.addEventListener("click", (event) => {
    if (!instancePickerEl.contains(event.target as Node)) closeInstancePicker();
    if (!accountMenuEl.contains(event.target as Node)) closeAccountMenu();
  });

  // ---------- installations tab: instance grid ----------

  function renderInstanceGrid() {
    instanceGridEl.replaceChildren();
    if (instances.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No instances yet. Create one to get started.";
      instanceGridEl.appendChild(empty);
      return;
    }
    for (const instance of instances) {
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
      instanceGridEl.appendChild(card);
    }
  }

  async function loadInstances() {
    try {
      const response = await invoke<InstancesResponse>("list_instances");
      instances = response.instances;
      selectedInstanceId = response.selected_id;
    } catch (err) {
      console.error(err);
      instances = [];
      selectedInstanceId = null;
    }
    renderInstancePickerTrigger();
    renderInstancePickerList();
    renderInstanceGrid();
    renderPlayButton();
    if (viewingInstanceId) renderInstanceDetail();
    void refreshPlayBackdrop();
  }

  // ---------- Play tab screenshot backdrop ----------

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
    const nextLayer = playBackdropActiveLayer === "a" ? heroBackdropBEl : heroBackdropAEl;
    const currentLayer = playBackdropActiveLayer === "a" ? heroBackdropAEl : heroBackdropBEl;
    nextLayer.style.backgroundImage = `url("${convertFileSrc(screenshot.path)}")`;
    nextLayer.classList.add("is-active");
    currentLayer.classList.remove("is-active");
    playBackdropActiveLayer = playBackdropActiveLayer === "a" ? "b" : "a";
  }

  const PLAY_BACKDROP_ROTATE_MS = 9000;

  async function refreshPlayBackdrop() {
    stopPlayBackdropTimer();
    const instance = currentInstance();

    if (!screenshotsBgEnabled || !instance) {
      heroEl.classList.remove("has-backdrop");
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
    if (currentInstance()?.id !== instance.id || !screenshotsBgEnabled) return;

    playBackdropScreenshots = screenshots;

    const pinned = instance.pinned_screenshot
      ? screenshots.find((s) => s.name === instance.pinned_screenshot)
      : undefined;

    if (screenshots.length === 0) {
      heroEl.classList.remove("has-backdrop");
      return;
    }

    heroEl.classList.add("has-backdrop");
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

  newInstanceBtn.addEventListener("click", openCreateInstanceModal);
  importInstanceBtn.addEventListener("click", () => void importInstance());

  // ---------- create instance ----------

  // The name field auto-fills with the picked version ("1.21.4") so creating an instance needs no
  // typing at all -- but only until the user actually edits it themselves, tracked here so picking
  // a different version afterwards doesn't clobber a name they already chose.
  let createInstanceNameIsAuto = true;

  function pickCreateInstanceVersion(versionId: string) {
    createInstanceSelectedVersion = versionId;
    renderVersionOptions(createInstanceVersionsEl, createInstanceSelectedVersion, pickCreateInstanceVersion);
    if (createInstanceNameIsAuto) createInstanceNameInput.value = versionId;
  }

  createInstanceNameInput.addEventListener("input", () => {
    createInstanceNameIsAuto = false;
  });

  function openCreateInstanceModal() {
    createInstanceSelectedVersion = versions[0]?.id ?? null;
    createInstanceNameIsAuto = true;
    createInstanceNameInput.value = createInstanceSelectedVersion ?? "";
    createInstanceErrorEl.hidden = true;
    renderVersionOptions(createInstanceVersionsEl, createInstanceSelectedVersion, pickCreateInstanceVersion);
    createInstanceModalEl.classList.add("is-open");
    createInstanceNameInput.focus();
  }

  function hideCreateInstanceModal() {
    createInstanceModalEl.classList.remove("is-open");
  }

  async function confirmCreateInstance() {
    const name = createInstanceNameInput.value.trim();
    if (!name) {
      createInstanceErrorEl.textContent = "Give the instance a name.";
      createInstanceErrorEl.hidden = false;
      return;
    }
    if (!createInstanceSelectedVersion) {
      createInstanceErrorEl.textContent = "Pick a version.";
      createInstanceErrorEl.hidden = false;
      return;
    }
    try {
      await invoke("create_instance_cmd", { name, versionId: createInstanceSelectedVersion });
      hideCreateInstanceModal();
      await loadInstances();
    } catch (err) {
      console.error(err);
      createInstanceErrorEl.textContent = describeError(err);
      createInstanceErrorEl.hidden = false;
    }
  }

  createInstanceConfirmBtn.addEventListener("click", () => void confirmCreateInstance());
  document.querySelector<HTMLButtonElement>("#create-instance-cancel")!.addEventListener("click", hideCreateInstanceModal);

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

  // ---------- instance detail (fullscreen) ----------

  function openInstanceDetail(instanceId: string) {
    closeAllScreens();
    viewingInstanceId = instanceId;
    renderInstanceDetail();
    instanceScreenEl.classList.add("is-open");
  }

  function closeInstanceDetail() {
    instanceScreenEl.classList.remove("is-open");
    viewingInstanceId = null;
  }

  instanceBackBtn.addEventListener("click", closeInstanceDetail);

  function renderInstanceDetail() {
    const instance = instances.find((i) => i.id === viewingInstanceId) ?? null;
    if (!instance) {
      closeInstanceDetail();
      return;
    }
    instanceScreenTitleEl.textContent = instance.name;
    instanceDetailNameEl.textContent = instance.name;
    instanceDetailVersionEl.textContent = instance.version_id;
    instanceVersionNameEl.textContent = `Minecraft ${instance.version_id}`;
    instanceIconBtn.style.backgroundImage = instanceIconBackground(instance);
    void refreshInstanceContent(instance.id);
  }

  function renderSimpleContentList(
    container: HTMLElement,
    names: string[],
    emptyText: string,
    onDelete: (name: string) => void,
  ) {
    container.replaceChildren();
    if (names.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = emptyText;
      container.appendChild(empty);
      return;
    }
    for (const name of names) {
      const row = document.createElement("div");
      row.className = "manage-row";

      const nameSpan = document.createElement("span");
      nameSpan.className = "manage-row__name";
      nameSpan.textContent = name;

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "manage-row__btn manage-row__btn--danger";
      removeBtn.textContent = "Remove";
      removeBtn.addEventListener("click", () => onDelete(name));

      row.append(nameSpan, removeBtn);
      container.appendChild(row);
    }
  }

  function renderWorlds(instanceId: string, worlds: WorldInfo[]) {
    worldsListEl.replaceChildren();
    if (worlds.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No worlds yet.";
      worldsListEl.appendChild(empty);
      return;
    }

    for (const world of worlds) {
      const row = document.createElement("div");
      row.className = "manage-row";

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
      removeBtn.textContent = "Remove";
      removeBtn.addEventListener("click", () => {
        openConfirmModal(
          "Delete world?",
          `This permanently deletes "${world.name}". This can't be undone.`,
          () => void deleteWorld(instanceId, world.name),
        );
      });

      row.append(info, removeBtn);
      worldsListEl.appendChild(row);

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
        worldsListEl.appendChild(dpRow);
      }
    }
  }

  function renderMods(instanceId: string, mods: ModInfo[]) {
    modsListEl.replaceChildren();
    if (mods.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No mods yet.";
      modsListEl.appendChild(empty);
      return;
    }
    for (const mod of mods) {
      const row = document.createElement("div");
      row.className = "manage-row";

      const info = document.createElement("div");
      info.className = "manage-row__info";
      const name = document.createElement("span");
      name.className = "manage-row__name";
      name.textContent = mod.name;
      info.appendChild(name);
      if (!mod.enabled) {
        const status = document.createElement("span");
        status.className = "manage-row__type";
        status.textContent = "Disabled";
        info.appendChild(status);
      }

      const actions = document.createElement("div");
      actions.className = "manage-row__actions";

      const toggleBtn = document.createElement("button");
      toggleBtn.type = "button";
      toggleBtn.className = "manage-row__btn";
      toggleBtn.textContent = mod.enabled ? "Disable" : "Enable";
      toggleBtn.addEventListener("click", () => void toggleMod(instanceId, mod.name, !mod.enabled));

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "manage-row__btn manage-row__btn--danger";
      removeBtn.textContent = "Remove";
      removeBtn.addEventListener("click", () => void deleteMod(instanceId, mod.name));

      actions.append(toggleBtn, removeBtn);
      row.append(info, actions);
      modsListEl.appendChild(row);
    }
  }

  function renderScreenshotGrid(instanceId: string, screenshots: ScreenshotInfo[], pinnedName: string | null) {
    screenshotsGridEl.replaceChildren();
    if (screenshots.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No screenshots yet.";
      screenshotsGridEl.appendChild(empty);
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
      pinBtn.title = isPinned ? "Unpin" : "Pin as Play tab background";
      pinBtn.addEventListener("click", () => void togglePinScreenshot(instanceId, isPinned ? null : screenshot.name));

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "screenshot-card__btn screenshot-card__remove";
      removeBtn.textContent = "×";
      removeBtn.title = "Remove";
      removeBtn.addEventListener("click", () => void deleteScreenshotAction(instanceId, screenshot.name));

      card.append(pinBtn, removeBtn);
      screenshotsGridEl.appendChild(card);
    }
  }

  async function togglePinScreenshot(instanceId: string, name: string | null) {
    try {
      await invoke("set_pinned_screenshot_cmd", { instanceId, name });
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

  screenshotsOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.screenshots_dir);
  });

  async function refreshInstanceContent(instanceId: string) {
    try {
      const [mods, worlds, resourcePacks, shaderPacks, screenshots] = await Promise.all([
        invoke<ModInfo[]>("list_mods_cmd", { instanceId }),
        invoke<WorldInfo[]>("list_worlds_cmd", { instanceId }),
        invoke<string[]>("list_resource_packs_cmd", { instanceId }),
        invoke<string[]>("list_shader_packs_cmd", { instanceId }),
        invoke<ScreenshotInfo[]>("list_screenshots_cmd", { instanceId }),
      ]);
      if (viewingInstanceId !== instanceId) return; // navigated away while this was in flight
      renderMods(instanceId, mods);
      renderWorlds(instanceId, worlds);
      renderSimpleContentList(resourcePacksListEl, resourcePacks, "No resource packs yet.", (fileName) =>
        void deleteResourcePack(instanceId, fileName),
      );
      renderSimpleContentList(shaderPacksListEl, shaderPacks, "No shader packs yet.", (fileName) =>
        void deleteShaderPack(instanceId, fileName),
      );
      const pinnedName = instances.find((i) => i.id === instanceId)?.pinned_screenshot ?? null;
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

  async function deleteMod(instanceId: string, name: string) {
    try {
      await invoke("delete_mod_cmd", { instanceId, name });
      await refreshInstanceContent(instanceId);
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  async function addMods() {
    if (!viewingInstanceId) return;
    const instanceId = viewingInstanceId;
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

  modsAddBtn.addEventListener("click", () => void addMods());

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

  // ---------- rename instance ----------

  function openRenameInstanceModal() {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (!instance) return;
    renameInstanceTargetId = instance.id;
    renameInstanceInput.value = instance.name;
    renameInstanceErrorEl.hidden = true;
    renameInstanceModalEl.classList.add("is-open");
    renameInstanceInput.focus();
  }

  function hideRenameInstanceModal() {
    renameInstanceModalEl.classList.remove("is-open");
    renameInstanceTargetId = null;
  }

  async function confirmRenameInstance() {
    if (!renameInstanceTargetId) return;
    const name = renameInstanceInput.value.trim();
    if (!name) {
      renameInstanceErrorEl.textContent = "Give the instance a name.";
      renameInstanceErrorEl.hidden = false;
      return;
    }
    try {
      const updated = await invoke<Instance>("rename_instance_cmd", { instanceId: renameInstanceTargetId, name });
      if (viewingInstanceId === renameInstanceTargetId) viewingInstanceId = updated.id;
      hideRenameInstanceModal();
      await loadInstances();
    } catch (err) {
      console.error(err);
      renameInstanceErrorEl.textContent = describeError(err);
      renameInstanceErrorEl.hidden = false;
    }
  }

  instanceRenameBtn.addEventListener("click", openRenameInstanceModal);
  renameInstanceConfirmBtn.addEventListener("click", () => void confirmRenameInstance());
  document.querySelector<HTMLButtonElement>("#rename-instance-cancel")!.addEventListener("click", hideRenameInstanceModal);

  // ---------- change version ----------

  function pickChangeVersion(versionId: string) {
    void applyInstanceVersion(versionId);
  }

  function openChangeVersionModal() {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (!instance) return;
    renderVersionOptions(changeVersionVersionsEl, instance.version_id, pickChangeVersion);
    changeVersionModalEl.classList.add("is-open");
  }

  function hideChangeVersionModal() {
    changeVersionModalEl.classList.remove("is-open");
  }

  async function applyInstanceVersion(versionId: string) {
    if (!viewingInstanceId) return;
    const instanceId = viewingInstanceId;
    hideChangeVersionModal();
    try {
      await invoke("set_instance_version_cmd", { instanceId, versionId });
      await loadInstances();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  instanceVersionBtn.addEventListener("click", openChangeVersionModal);
  document.querySelector<HTMLButtonElement>("#change-version-cancel")!.addEventListener("click", hideChangeVersionModal);

  // ---------- icon ----------

  async function pickInstanceIcon() {
    if (!viewingInstanceId) return;
    const instanceId = viewingInstanceId;
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
    if (!viewingInstanceId) return;
    const instanceId = viewingInstanceId;
    try {
      await invoke("set_instance_icon_cmd", { instanceId, iconPath: null });
      await loadInstances();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  instanceIconBtn.addEventListener("click", () => void pickInstanceIcon());
  instanceIconClearBtn.addEventListener("click", () => void clearInstanceIcon());

  // ---------- open folder / export / delete ----------

  instanceOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.dir);
  });

  instanceLibrariesOpenBtn.addEventListener("click", () => {
    if (directorySettings) void openFolder(directorySettings.libraries_dir);
  });

  modsOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.mods_dir);
  });

  worldsOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.saves_dir);
  });

  resourcePacksOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.resource_packs_dir);
  });

  shaderPacksOpenFolderBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (instance) void openFolder(instance.shader_packs_dir);
  });

  async function exportInstance() {
    if (!viewingInstanceId) return;
    const instanceId = viewingInstanceId;
    const instance = instances.find((i) => i.id === instanceId);
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

  instanceExportBtn.addEventListener("click", () => void exportInstance());

  async function deleteInstance(instanceId: string) {
    try {
      await invoke("delete_instance_cmd", { instanceId });
      if (viewingInstanceId === instanceId) closeInstanceDetail();
      await loadInstances();
    } catch (err) {
      console.error(err);
      showErrorModal(describeError(err));
    }
  }

  instanceDeleteBtn.addEventListener("click", () => {
    const instance = instances.find((i) => i.id === viewingInstanceId);
    if (!instance) return;
    openConfirmModal(
      "Delete instance?",
      `This permanently deletes "${instance.name}" -- its worlds and everything else in its folder. This can't be undone.`,
      () => void deleteInstance(instance.id),
    );
  });

  // ---------- play ----------

  function renderPlayButton() {
    instancePickerTrigger.disabled = playStage !== "idle";
    if (instancePickerTrigger.disabled) closeInstancePicker();

    if (playStage === "installing") {
      playButton.disabled = true;
      playLabelEl.textContent = "Installing...";
      progressPanelEl.hidden = false;
      progressLabelEl.textContent = installingLabel;
      progressPercentEl.textContent = `${Math.round(installProgressPercent)}%`;
      progressFillEl.style.width = `${installProgressPercent}%`;
      return;
    }
    progressPanelEl.hidden = true;
    if (playStage === "launching") {
      playButton.disabled = true;
      playLabelEl.textContent = "Launching...";
      return;
    }
    if (!currentAccount) {
      playButton.disabled = false;
      playLabelEl.textContent = "Sign In";
      return;
    }
    if (!selectedInstanceId) {
      playButton.disabled = false;
      playLabelEl.textContent = instances.length === 0 ? "New instance" : "Select an instance";
      return;
    }
    playButton.disabled = false;
    playLabelEl.textContent = "Play";
  }

  async function handlePlayClick() {
    if (playStage !== "idle") return;
    if (!currentAccount) {
      void startSignIn();
      return;
    }
    if (!selectedInstanceId) {
      showTab("installations");
      return;
    }

    launchErrorEl.hidden = true;
    playStage = "installing";
    installingLabel = "Preparing...";
    installProgressPercent = 0;
    renderPlayButton();

    try {
      await invoke("launch_instance_cmd", {
        instanceId: selectedInstanceId,
        account: { type: "saved", accountId: accountKey(currentAccount) },
      });
    } catch (err) {
      console.error(err);
      const message = `Couldn't launch: ${describeError(err)}`;
      // The inline bar is easy to miss (small, below the fold if the window's short) -- a modal
      // guarantees the user actually sees why Play didn't work instead of wondering if it's stuck.
      launchErrorEl.textContent = message;
      launchErrorEl.hidden = false;
      showErrorModal(message);
    } finally {
      playStage = "idle";
      renderPlayButton();
    }
  }

  playButton.addEventListener("click", () => void handlePlayClick());

  // ---------- backend events ----------

  await listen<DeviceAuthorization>("device-code", (event) => showLoginModal(event.payload));

  await listen<DownloadProgress>("install-progress", (event) => {
    if (playStage !== "installing") return;
    const p = event.payload;
    const phase = p.phase || "Files";
    installingLabel = p.files_total > 0 ? `Downloading ${phase} (${p.files_done}/${p.files_total})` : `Downloading ${phase}`;
    installProgressPercent = p.files_total > 0 ? Math.min(100, (p.files_done / p.files_total) * 100) : 0;
    renderPlayButton();
  });

  await listen<string>("launch-status", (event) => {
    if (event.payload === "launching" && playStage === "installing") {
      playStage = "launching";
      renderPlayButton();
    }
  });

  renderAccount();
  renderPlayButton();
  await Promise.all([loadAccounts(), loadInstances(), loadVersions(), loadDirectorySettings()]);
}

window.addEventListener("DOMContentLoaded", () => void main());
