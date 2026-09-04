// Tab switching (Moments / Installations / Skins / Patch notes) and the cosmetic sidebar-selection
// highlight. Owns `showTab` in its own module (rather than main.ts) specifically so
// `features/play.ts` can call it too (clicking Play with no instance selected jumps to the
// Installations tab) without a circular import between main.ts and play.ts.

import { el } from "./dom";
import * as skins from "./features/skins";

let tabs: NodeListOf<HTMLButtonElement>;
let panels: NodeListOf<HTMLElement>;

export function showTab(target: string) {
  tabs.forEach((t) => t.classList.toggle("is-active", t.dataset.tab === target));
  panels.forEach((panel) => panel.classList.toggle("is-active", panel.dataset.tabPanel === target));
  // The 3D skin viewer keeps rendering (and using GPU) even while its tab is hidden unless told
  // otherwise -- pause it off-tab, resume (and load fresh data) when the tab is actually opened.
  skins.setSkinViewerPaused(target !== "skins");
  if (target === "skins") void skins.loadSkinsTab();
}

export function initTabs() {
  tabs = document.querySelectorAll<HTMLButtonElement>("[data-tab]");
  panels = document.querySelectorAll<HTMLElement>("[data-tab-panel]");

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => showTab(tab.dataset.tab ?? "installations"));
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

  document.addEventListener("click", (event) => {
    if (!el.instancePickerEl.contains(event.target as Node)) el.instancePickerEl.classList.remove("is-open");
    if (!el.accountMenuEl.contains(event.target as Node)) el.accountMenuEl.classList.remove("is-open");
    if (!el.instanceOverflowMenuEl.contains(event.target as Node)) el.instanceOverflowMenuEl.classList.remove("is-open");
  });
}
