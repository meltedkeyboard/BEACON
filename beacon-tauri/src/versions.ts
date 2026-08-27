// Fetched once at startup, rendered on demand inside the create-instance and change-version
// modals (both in `features/instances.ts`).

import { invoke } from "@tauri-apps/api/core";

import { state } from "./state";
import type { VersionEntry } from "./types";

let versions: VersionEntry[] = [];

export async function loadVersions() {
  try {
    versions = await invoke<VersionEntry[]>("list_versions", { snapshots: state.showSnapshots });
  } catch (err) {
    console.error(err);
    versions = [];
  }
}

export function firstVersionId(): string | null {
  return versions[0]?.id ?? null;
}

export function renderVersionOptions(container: HTMLElement, selectedId: string | null, onPick: (versionId: string) => void) {
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
