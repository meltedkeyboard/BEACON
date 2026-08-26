import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

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

// Mirrors beacon_core::Account::id() -- the key the config file (and secret store) uses to
// look an account up, so it's what `launch_version_cmd` needs to select a saved account.
function accountKey(account: Account): string {
  return account.type === "Offline" ? `offline:${account.username}` : `microsoft:${account.id}`;
}

function describeError(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

let versions: VersionEntry[] = [];
let selectedVersionId: string | null = null;
let accounts: Account[] = [];
let currentAccount: Account | null = null;
let playStage: "idle" | "installing" | "launching" = "idle";
let installingLabel = "Installing...";
let installProgressPercent = 0;
let pendingVerificationUri = "";
let offlineModalMode: { kind: "add" } | { kind: "rename"; accountId: string; current: string } | null = null;

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
  const playbarVersionEl = document.querySelector<HTMLElement>("#playbar-version")!;
  const playButton = document.querySelector<HTMLButtonElement>("#play-button")!;
  const playLabelEl = document.querySelector<HTMLElement>("#play-label")!;
  const playFillEl = document.querySelector<HTMLElement>("#play-fill")!;
  const versionPickerEl = document.querySelector<HTMLElement>("#version-picker")!;
  const versionPickerTrigger = document.querySelector<HTMLButtonElement>("#version-picker-trigger")!;
  const versionPickerListEl = document.querySelector<HTMLElement>("#version-picker-list")!;
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
  const appEl = document.querySelector<HTMLElement>(".app")!;
  const accountsNavBtn = document.querySelector<HTMLButtonElement>("#accounts-nav")!;
  const accountsBackBtn = document.querySelector<HTMLButtonElement>("#accounts-back")!;
  const manageListEl = document.querySelector<HTMLElement>("#manage-list")!;

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
  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const target = tab.dataset.tab;
      tabs.forEach((t) => t.classList.toggle("is-active", t === tab));
      panels.forEach((panel) => panel.classList.toggle("is-active", panel.dataset.tabPanel === target));
    });
  });

  const navRows = document.querySelectorAll<HTMLButtonElement>(".nav-row[data-nav]");
  navRows.forEach((row) => {
    row.addEventListener("click", () => {
      navRows.forEach((r) => r.classList.toggle("is-selected", r === row));
    });
  });

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
    renderManageList();
    appEl.classList.add("is-managing-accounts");
  }

  function closeAccountsScreen() {
    appEl.classList.remove("is-managing-accounts");
  }

  accountsNavBtn.addEventListener("click", openAccountsScreen);
  accountsBackBtn.addEventListener("click", closeAccountsScreen);

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

  // ---------- versions ----------

  function renderPlaybarVersion() {
    playbarVersionEl.textContent = selectedVersionId ?? "—";
  }

  function openVersionPicker() {
    if (versionPickerTrigger.disabled) return;
    versionPickerEl.classList.add("is-open");
  }

  function closeVersionPicker() {
    versionPickerEl.classList.remove("is-open");
  }

  versionPickerTrigger.addEventListener("click", () => {
    versionPickerEl.classList.contains("is-open") ? closeVersionPicker() : openVersionPicker();
  });

  document.addEventListener("click", (event) => {
    if (!versionPickerEl.contains(event.target as Node)) closeVersionPicker();
    if (!accountMenuEl.contains(event.target as Node)) closeAccountMenu();
  });

  function renderVersions() {
    versionPickerListEl.replaceChildren();
    if (versions.length === 0) {
      const empty = document.createElement("p");
      empty.className = "placeholder-text";
      empty.textContent = "No versions found.";
      versionPickerListEl.appendChild(empty);
      return;
    }

    for (const version of versions) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "nav-row version-row";
      row.classList.toggle("is-selected", version.id === selectedVersionId);

      const idSpan = document.createElement("span");
      idSpan.className = "version-row__id";
      idSpan.textContent = version.id;

      const metaSpan = document.createElement("span");
      metaSpan.className = "version-row__meta";
      metaSpan.textContent = `${version.type} · ${version.releaseTime.slice(0, 10)}`;

      row.append(idSpan, metaSpan);
      row.addEventListener("click", () => {
        selectedVersionId = version.id;
        renderVersions();
        renderPlaybarVersion();
        renderPlayButton();
        closeVersionPicker();
      });
      versionPickerListEl.appendChild(row);
    }
  }

  async function loadVersions() {
    const loading = document.createElement("p");
    loading.className = "placeholder-text";
    loading.textContent = "Loading versions...";
    versionPickerListEl.replaceChildren(loading);

    try {
      versions = await invoke<VersionEntry[]>("list_versions", { snapshots: false });
      if (!selectedVersionId && versions.length > 0) {
        selectedVersionId = versions[0].id;
      }
      renderVersions();
      renderPlaybarVersion();
      renderPlayButton();
    } catch (err) {
      console.error(err);
      const error = document.createElement("p");
      error.className = "placeholder-text";
      error.textContent = `Couldn't load versions: ${describeError(err)}`;
      versionPickerListEl.replaceChildren(error);
    }
  }

  // ---------- play ----------

  function renderPlayButton() {
    versionPickerTrigger.disabled = playStage !== "idle";
    if (versionPickerTrigger.disabled) closeVersionPicker();

    if (playStage === "installing") {
      playButton.disabled = true;
      playLabelEl.textContent = installingLabel;
      playFillEl.style.width = `${installProgressPercent}%`;
      return;
    }
    playFillEl.style.width = "100%";
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
    if (!selectedVersionId) {
      playButton.disabled = true;
      playLabelEl.textContent = "Loading...";
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
    if (!selectedVersionId) return;

    launchErrorEl.hidden = true;
    playStage = "installing";
    installingLabel = "Installing...";
    installProgressPercent = 0;
    renderPlayButton();

    try {
      await invoke("launch_version_cmd", {
        versionId: selectedVersionId,
        account: { type: "saved", accountId: accountKey(currentAccount) },
      });
    } catch (err) {
      console.error(err);
      launchErrorEl.textContent = `Couldn't launch ${selectedVersionId}: ${describeError(err)}`;
      launchErrorEl.hidden = false;
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
    installingLabel = p.files_total > 0 ? `Installing... ${p.files_done}/${p.files_total}` : "Installing...";
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
  await Promise.all([loadAccounts(), loadVersions()]);
}

window.addEventListener("DOMContentLoaded", () => void main());
