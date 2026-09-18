import { convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";

import { showErrorModal } from "./modals";
import type { Account, Instance } from "./types";

export function accountKey(account: Account): string {
  return account.type === "Offline" ? `offline:${account.username}` : `microsoft:${account.id}`;
}

export function describeError(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

export function instanceIconBackground(instance: Instance | null): string {
  return instance?.icon_path ? `url("${convertFileSrc(instance.icon_path)}")` : "";
}

export function applyDecorativeIcon(el: HTMLElement, instance: Instance | null) {
  const background = instanceIconBackground(instance);
  el.style.backgroundImage = background;
  el.style.display = background ? "" : "none";
}

// <wbr> after each separator lets long paths wrap at a \ instead of mid-word.
export function setPathText(el: HTMLElement, path: string) {
  el.replaceChildren();
  for (const part of path.split(/([\\/])/)) {
    el.append(document.createTextNode(part));
    if (part === "\\" || part === "/") el.append(document.createElement("wbr"));
  }
}

export async function openFolder(path: string) {
  try {
    await openPath(path);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}
