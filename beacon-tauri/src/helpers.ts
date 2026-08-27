import { convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";

import { showErrorModal } from "./modals";
import type { Account, Instance } from "./types";

// Mirrors beacon_core::Account::id() -- the key the config file (and secret store) uses to
// look an account up, so it's what `launch_instance_cmd` needs to select a saved account.
export function accountKey(account: Account): string {
  return account.type === "Offline" ? `offline:${account.username}` : `microsoft:${account.id}`;
}

export function describeError(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

// `icon_path` can be anywhere on disk (it comes from an open-file dialog with no folder
// restriction), so displaying it needs the `asset:` protocol rather than a plain file:// src --
// the backend allows each icon path individually in the asset protocol scope as it's set
// (`set_instance_icon_cmd`), rather than opening the whole filesystem to it.
export function instanceIconBackground(instance: Instance | null): string {
  return instance?.icon_path ? `url("${convertFileSrc(instance.icon_path)}")` : "";
}

// A flat gray square with nothing in it reads as a broken placeholder, not "no icon set" -- for
// purely decorative icon slots (unlike `instance-detail__icon`, which stays visible because it's
// also the click target for setting one) this just hides the element entirely when there's
// nothing to show.
export function applyDecorativeIcon(el: HTMLElement, instance: Instance | null) {
  const background = instanceIconBackground(instance);
  el.style.backgroundImage = background;
  el.style.display = background ? "" : "none";
}

// Plain `el.textContent = path` wraps a long Windows path at whatever character happens to hit
// the container edge, splitting words like "desktop" into "deskt"/"op". `<wbr>` marks the path's
// separators as the only places allowed to wrap instead, so long paths break at a `\` the same
// way a human would read them -- `.textContent` reads elsewhere are unaffected, since a `<wbr>`
// contributes no text of its own.
export function setPathText(el: HTMLElement, path: string) {
  el.replaceChildren();
  for (const part of path.split(/([\\/])/)) {
    el.append(document.createTextNode(part));
    if (part === "\\" || part === "/") el.append(document.createElement("wbr"));
  }
}

// `openPath` rejects if the folder doesn't exist yet (e.g. nothing installed there yet) or
// the path is otherwise invalid -- surface that instead of swallowing it, so "Open" clicked on
// an empty/fresh setup says why nothing happened instead of looking like it does nothing.
export async function openFolder(path: string) {
  try {
    await openPath(path);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}
