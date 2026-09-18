// Own module, not main.ts: play.ts calls showTab too, and that would be a circular import.

import { el } from "./dom";
import * as skins from "./features/skins";

let tabs: NodeListOf<HTMLButtonElement>;
let panels: NodeListOf<HTMLElement>;

export function showTab(target: string) {
  tabs.forEach((t) => t.classList.toggle("is-active", t.dataset.tab === target));
  panels.forEach((panel) => panel.classList.toggle("is-active", panel.dataset.tabPanel === target));
  skins.setSkinViewerPaused(target !== "skins"); // avoids the 3D viewer rendering off-tab
  if (target === "skins") skins.renderSkinsTabState();
}

export function initTabs() {
  tabs = document.querySelectorAll<HTMLButtonElement>("[data-tab]");
  panels = document.querySelectorAll<HTMLElement>("[data-tab-panel]");

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => showTab(tab.dataset.tab ?? "installations"));
  });

  const navRows = document.querySelectorAll<HTMLButtonElement>(
    ".nav-row[data-nav]:not(#accounts-nav):not(#settings-nav)",
  );
  navRows.forEach((row) => {
    row.addEventListener("click", () => {
      navRows.forEach((r) => r.classList.toggle("is-selected", r === row));
    });
  });

  document.addEventListener("click", (event) => {
    if (!el.instancePickerEl.contains(event.target as Node)) el.instancePickerEl.classList.remove("is-open");
    if (!el.accountMenuEl.contains(event.target as Node)) el.accountMenuEl.classList.remove("is-open");
    if (!el.instanceOverflowMenuEl.contains(event.target as Node)) el.instanceOverflowMenuEl.classList.remove("is-open");
  });
}
