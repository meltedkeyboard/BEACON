// A single reusable right-click context menu, in the vein of a native (Qt-style) desktop app:
// one absolutely-positioned panel appended to <body> and repositioned per invocation, rather than
// a fresh element built for every list row. Callers describe *what* to show; this module owns
// *where* and *how* it appears, closing on outside click, Escape, or scroll.

export interface ContextMenuItem {
  label: string;
  onClick: () => void;
  danger?: boolean;
  separatorBefore?: boolean;
}

let menuEl: HTMLElement | null = null;

function ensureMenuEl(): HTMLElement {
  if (menuEl) return menuEl;
  menuEl = document.createElement("div");
  menuEl.className = "context-menu";
  document.body.appendChild(menuEl);
  return menuEl;
}

export function closeContextMenu() {
  if (menuEl) menuEl.classList.remove("is-open");
}

export function showContextMenu(x: number, y: number, items: ContextMenuItem[]) {
  const menu = ensureMenuEl();
  menu.replaceChildren();
  for (const item of items) {
    if (item.separatorBefore) {
      const sep = document.createElement("div");
      sep.className = "context-menu__separator";
      menu.appendChild(sep);
    }
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "context-menu__item" + (item.danger ? " context-menu__item--danger" : "");
    btn.textContent = item.label;
    btn.addEventListener("click", () => {
      closeContextMenu();
      item.onClick();
    });
    menu.appendChild(btn);
  }
  menu.classList.add("is-open");

  // Placed first, then nudged back on-screen -- the menu's own size isn't known until it has
  // rendered with its real content.
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;
  const rect = menu.getBoundingClientRect();
  const overflowX = rect.right - window.innerWidth;
  const overflowY = rect.bottom - window.innerHeight;
  if (overflowX > 0) menu.style.left = `${Math.max(0, x - overflowX)}px`;
  if (overflowY > 0) menu.style.top = `${Math.max(0, y - overflowY)}px`;
}

export function initContextMenu() {
  document.addEventListener("click", (event) => {
    if (menuEl && !menuEl.contains(event.target as Node)) closeContextMenu();
  });
  document.addEventListener("contextmenu", (event) => {
    if (menuEl && !menuEl.contains(event.target as Node)) closeContextMenu();
  });
  window.addEventListener("blur", closeContextMenu);
  window.addEventListener("scroll", closeContextMenu, true);
}
