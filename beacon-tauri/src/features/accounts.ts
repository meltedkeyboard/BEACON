// Account menu, sign-in (device code flow), offline account add/rename, and the Manage Accounts
// screen. `accounts[0]` is always the account Play/launch uses.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

import { el } from "../dom";
import { accountKey, describeError } from "../helpers";
import { closeAllScreens, showErrorModal } from "../modals";
import { state } from "../state";
import type { Account, DeviceAuthorization } from "../types";
import * as play from "./play";
import * as skins from "./skins";

let accounts: Account[] = [];
let pendingVerificationUri = "";
let offlineModalMode: { kind: "add" } | { kind: "rename"; accountId: string; current: string } | null = null;

export function renderAccount() {
  if (state.currentAccount) {
    el.accountNameEl.textContent = state.currentAccount.username;
    el.accountStatusEl.textContent = "Connected";
    el.accountStatusEl.className = "account__status account__status--connected";
    el.playbarUserEl.textContent = state.currentAccount.username;
  } else {
    el.accountNameEl.textContent = "Sign in";
    el.accountStatusEl.textContent = "Offline mode";
    el.accountStatusEl.className = "account__status";
    el.playbarUserEl.textContent = "Not signed in";
  }
  play.renderPlayButton();
  // Account switches (sign-in, reorder in the account menu) change what the Skins tab should
  // show -- but only bother refetching if it's actually the tab on screen right now.
  if (document.querySelector('[data-tab-panel="skins"]')?.classList.contains("is-active")) {
    void skins.loadSkinsTab();
  }
}

function openAccountMenu() {
  el.accountMenuEl.classList.add("is-open");
}

function closeAccountMenu() {
  el.accountMenuEl.classList.remove("is-open");
}

function renderAccountMenu() {
  el.accountMenuAccountsEl.replaceChildren();
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
    el.accountMenuAccountsEl.appendChild(row);
  });

  const hasMicrosoft = accounts.some((a) => a.type === "Microsoft");
  el.addOfflineMenuItem.disabled = !hasMicrosoft;
  el.manageAddOfflineBtn.disabled = !hasMicrosoft;
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
  el.loginCodeEl.textContent = auth.user_code;
  pendingVerificationUri = auth.verification_uri;
  el.loginModalEl.classList.add("is-open");
}

function hideLoginModal() {
  el.loginModalEl.classList.remove("is-open");
}

// Five different buttons can call this (sidebar, account menu, Accounts screen, the Skins tab's
// sign-in prompt, and Play-while-signed-out) -- without this guard, clicking more than one of
// them before the first device-code flow finishes starts a second, fully independent polling
// loop against Microsoft's token endpoint. Two or three of those running at once is exactly the
// kind of quick-succession hammering that gets rate-limited (429s).
let signInInProgress = false;

export async function startSignIn() {
  if (signInInProgress) return;
  signInInProgress = true;
  el.accountStatusEl.textContent = "Connecting...";
  el.accountStatusEl.className = "account__status";
  try {
    state.currentAccount = await invoke<Account>("login_microsoft_cmd");
    hideLoginModal();
    await loadAccounts();
  } catch (err) {
    console.error(err);
    hideLoginModal();
    el.accountStatusEl.textContent = "Connection failed. Please log in again.";
    el.accountStatusEl.className = "account__status account__status--error";
    showErrorModal(describeError(err));
  } finally {
    signInInProgress = false;
  }
}

// ---------- add / rename offline account ----------

function openOfflineModal(mode: NonNullable<typeof offlineModalMode>) {
  offlineModalMode = mode;
  el.offlineModalEyebrowEl.textContent = mode.kind === "rename" ? "Rename offline account" : "Add offline account";
  el.offlineConfirmBtn.textContent = mode.kind === "rename" ? "Rename" : "Add";
  el.offlineNicknameInput.value = mode.kind === "rename" ? mode.current : "";
  el.offlineNicknameError.hidden = true;
  el.offlineModalEl.classList.add("is-open");
  el.offlineNicknameInput.focus();
}

function hideOfflineModal() {
  el.offlineModalEl.classList.remove("is-open");
  offlineModalMode = null;
}

async function confirmOfflineModal() {
  const mode = offlineModalMode;
  if (!mode) return;
  const nickname = el.offlineNicknameInput.value.trim();
  if (!/^[A-Za-z0-9_]{3,16}$/.test(nickname)) {
    el.offlineNicknameError.textContent = "Nicknames are 3-16 characters: letters, numbers, underscore.";
    el.offlineNicknameError.hidden = false;
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
    el.offlineNicknameError.textContent = describeError(err);
    el.offlineNicknameError.hidden = false;
  }
}

// ---------- manage accounts (fullscreen) ----------

function openAccountsScreen() {
  closeAllScreens();
  renderManageList();
  el.accountsScreenEl.classList.add("is-open");
}

function closeAccountsScreen() {
  el.accountsScreenEl.classList.remove("is-open");
}

function renderManageList() {
  el.manageListEl.replaceChildren();

  if (accounts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "placeholder-text";
    empty.textContent = "No accounts yet.";
    el.manageListEl.appendChild(empty);
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
    el.manageListEl.appendChild(row);
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

export async function loadAccounts() {
  try {
    accounts = await invoke<Account[]>("list_accounts");
    state.currentAccount = accounts[0] ?? null;
  } catch (err) {
    console.error(err);
    accounts = [];
    state.currentAccount = null;
  }
  renderAccount();
  renderAccountMenu();
  renderManageList();
}

export async function init() {
  el.accountButton.addEventListener("click", () => {
    el.accountMenuEl.classList.contains("is-open") ? closeAccountMenu() : openAccountMenu();
  });

  el.accountMenuSigninBtn.addEventListener("click", () => {
    closeAccountMenu();
    void startSignIn();
  });

  el.accountMenuAddOfflineBtn.addEventListener("click", () => {
    closeAccountMenu();
    openOfflineModal({ kind: "add" });
  });

  el.accountMenuManageBtn.addEventListener("click", () => {
    closeAccountMenu();
    openAccountsScreen();
  });

  el.loginOpenBrowserBtn.addEventListener("click", () => {
    if (pendingVerificationUri) void openUrl(pendingVerificationUri);
  });

  el.loginCloseBtn.addEventListener("click", hideLoginModal);

  el.offlineConfirmBtn.addEventListener("click", () => void confirmOfflineModal());
  el.offlineCancelBtn.addEventListener("click", hideOfflineModal);
  el.manageAddOfflineBtn.addEventListener("click", () => openOfflineModal({ kind: "add" }));

  el.accountsNavBtn.addEventListener("click", openAccountsScreen);
  el.accountsBackBtn.addEventListener("click", closeAccountsScreen);

  el.manageAddMicrosoftBtn.addEventListener("click", () => {
    closeAccountsScreen();
    void startSignIn();
  });

  await listen<DeviceAuthorization>("device-code", (event) => showLoginModal(event.payload));
}
