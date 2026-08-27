// Generic overlays used from nearly every feature module: the plain error modal, the "are you
// sure?" confirm modal (delete world/instance), and closing whichever fullscreen screen
// (Accounts/Settings/Instance detail) happens to be open before opening a different one.

import { el } from "./dom";

export function showErrorModal(message: string) {
  el.errorMessageEl.textContent = message;
  el.errorModalEl.classList.add("is-open");
}

export function hideErrorModal() {
  el.errorModalEl.classList.remove("is-open");
}

let pendingConfirmAction: (() => void) | null = null;

export function openConfirmModal(eyebrow: string, message: string, action: () => void) {
  el.confirmEyebrowEl.textContent = eyebrow;
  el.confirmMessageEl.textContent = message;
  pendingConfirmAction = action;
  el.confirmModalEl.classList.add("is-open");
}

export function hideConfirmModal() {
  el.confirmModalEl.classList.remove("is-open");
  pendingConfirmAction = null;
}

export function closeAllScreens() {
  el.accountsScreenEl.classList.remove("is-open");
  el.settingsScreenEl.classList.remove("is-open");
  el.instanceScreenEl.classList.remove("is-open");
}

export function initModals() {
  el.confirmActionBtn.addEventListener("click", () => {
    const action = pendingConfirmAction;
    hideConfirmModal();
    action?.();
  });
  el.confirmCancelBtn.addEventListener("click", hideConfirmModal);
  el.errorCloseBtn.addEventListener("click", hideErrorModal);
}
