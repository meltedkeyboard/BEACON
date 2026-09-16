// App-wide keyboard/focus conventions matching a native desktop toolkit (Qt and friends): Escape
// dismisses the topmost overlay, Enter from a text field activates the current dialog's primary
// button, Tab is trapped inside an open modal instead of leaking focus to the shell behind it,
// and arrow keys move focus within tab strips / radio groups instead of requiring repeated Tabs.
// Wired once from `main.ts`, after every feature module's own `init()` has built its DOM.

import { closeContextMenu } from "./contextmenu";
import { hideConfirmModal, hideErrorModal } from "./modals";

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

const ROVING_CONTAINER_SELECTOR = ".tabs, .instance-tabs, [role='radiogroup']";

function isVisible(elm: HTMLElement): boolean {
  return elm.offsetParent !== null;
}

function topOpenModal(): HTMLElement | null {
  const modals = document.querySelectorAll<HTMLElement>(".modal.is-open");
  return modals.length ? modals[modals.length - 1] : null;
}

function topOpenScreen(): HTMLElement | null {
  const screens = document.querySelectorAll<HTMLElement>(".screen-overlay.is-open");
  return screens.length ? screens[screens.length - 1] : null;
}

function openPopover(): HTMLElement | null {
  return document.querySelector<HTMLElement>(
    "#account-menu.is-open, #instance-overflow-menu.is-open, #instance-picker.is-open",
  );
}

function closeModal(modal: HTMLElement) {
  // These two generic modals have dedicated hide functions that also clear pending state; every
  // other modal only ever needs its `is-open` class removed to slide back out.
  if (modal.id === "error-modal") {
    hideErrorModal();
    return;
  }
  if (modal.id === "delete-instance-modal") {
    hideConfirmModal();
    return;
  }
  modal.classList.remove("is-open");
}

function handleEscape() {
  const modal = topOpenModal();
  if (modal) {
    closeModal(modal);
    return;
  }
  if (closeContextMenuIfOpen()) return;
  const popover = openPopover();
  if (popover) {
    popover.classList.remove("is-open");
    return;
  }
  const screen = topOpenScreen();
  screen?.querySelector<HTMLButtonElement>(".accounts-screen__back")?.click();
}

function closeContextMenuIfOpen(): boolean {
  const wasOpen = document.querySelector(".context-menu.is-open") !== null;
  closeContextMenu();
  return wasOpen;
}

function handleEnter(event: KeyboardEvent) {
  const target = event.target as HTMLElement;
  if (target.tagName !== "INPUT") return;
  const modal = topOpenModal();
  if (!modal) return;
  const primary = modal.querySelector<HTMLButtonElement>(".modal__btn--primary:not([disabled])");
  if (!primary) return;
  event.preventDefault();
  primary.click();
}

function handleTabTrap(event: KeyboardEvent) {
  const modal = topOpenModal();
  if (!modal) return;
  const focusables = Array.from(modal.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(isVisible);
  if (focusables.length === 0) return;
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function handleArrowNav(event: KeyboardEvent) {
  const active = document.activeElement as HTMLElement | null;
  if (!active) return;
  const container = active.closest<HTMLElement>(ROVING_CONTAINER_SELECTOR);
  if (!container) return;
  const items = Array.from(container.querySelectorAll<HTMLButtonElement>(":scope > button:not([disabled])"));
  const index = items.indexOf(active as HTMLButtonElement);
  if (index === -1) return;

  let nextIndex: number;
  if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = items.length - 1;
  else if (event.key === "ArrowRight" || event.key === "ArrowDown") nextIndex = (index + 1) % items.length;
  else nextIndex = (index - 1 + items.length) % items.length;

  event.preventDefault();
  items[nextIndex].focus();
  items[nextIndex].click();
}

export function initKeyboard() {
  document.addEventListener("keydown", (event) => {
    switch (event.key) {
      case "Escape":
        handleEscape();
        break;
      case "Enter":
        handleEnter(event);
        break;
      case "Tab":
        handleTabTrap(event);
        break;
      case "ArrowRight":
      case "ArrowLeft":
      case "ArrowUp":
      case "ArrowDown":
      case "Home":
      case "End":
        handleArrowNav(event);
        break;
      default:
        break;
    }
  });
}
