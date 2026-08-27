// Skins & capes tab: a real Minecraft Services API-backed 3D preview (skinview3d) for whichever
// account is currently selected for Play. Only meaningful for a Microsoft account -- offline
// accounts get a sign-in prompt instead.

import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { SkinViewer } from "skinview3d";

import { el } from "../dom";
import { accountKey, describeError } from "../helpers";
import { showErrorModal } from "../modals";
import { state } from "../state";
import type { MinecraftProfile } from "../types";

let skinViewer: SkinViewer | null = null;
let selectedSkinVariant: "classic" | "slim" = "classic";
let onSignInRequested: () => void = () => {};

function ensureSkinViewer(): SkinViewer {
  if (!skinViewer) {
    skinViewer = new SkinViewer({
      canvas: el.skinViewerCanvas,
      width: 280,
      height: 380,
    });
    skinViewer.autoRotate = true;
    skinViewer.autoRotateSpeed = 0.6;
  }
  return skinViewer;
}

// Called from `tabs.ts` on every tab switch -- the 3D viewer keeps rendering (and using GPU) even
// while its tab is hidden unless told otherwise.
export function setSkinViewerPaused(paused: boolean) {
  if (skinViewer) skinViewer.renderPaused = paused;
}

function renderSkinVariantButtons() {
  el.skinVariantOptions.forEach((btn) => {
    const selected = btn.dataset.variant === selectedSkinVariant;
    btn.classList.toggle("is-selected", selected);
    btn.setAttribute("aria-checked", String(selected));
  });
}

function renderCapeGrid(profile: MinecraftProfile, accountId: string) {
  el.capeGridEl.replaceChildren();

  const noneCard = document.createElement("button");
  noneCard.type = "button";
  noneCard.className = "cape-card";
  const hasActiveCape = profile.capes.some((c) => c.state === "ACTIVE");
  noneCard.classList.toggle("is-active", !hasActiveCape);
  const noneThumb = document.createElement("span");
  noneThumb.className = "cape-card__thumb";
  const noneName = document.createElement("span");
  noneName.className = "cape-card__name";
  noneName.textContent = "None";
  noneCard.append(noneThumb, noneName);
  noneCard.addEventListener("click", () => void clearCape(accountId));
  el.capeGridEl.appendChild(noneCard);

  for (const cape of profile.capes) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "cape-card";
    card.classList.toggle("is-active", cape.state === "ACTIVE");

    const thumb = document.createElement("span");
    thumb.className = "cape-card__thumb";
    thumb.style.backgroundImage = `url("${cape.url}")`;

    const name = document.createElement("span");
    name.className = "cape-card__name";
    name.textContent = cape.alias;
    name.title = cape.alias;

    card.append(thumb, name);
    card.addEventListener("click", () => void applyCape(accountId, cape.id));
    el.capeGridEl.appendChild(card);
  }
}

export async function loadSkinsTab(forceRefresh = false) {
  if (!state.currentAccount || state.currentAccount.type !== "Microsoft") {
    el.skinsSigninEl.hidden = false;
    el.skinsViewEl.hidden = true;
    return;
  }
  el.skinsSigninEl.hidden = true;
  el.skinsViewEl.hidden = false;

  const accountId = accountKey(state.currentAccount);
  try {
    const profile = await invoke<MinecraftProfile>("get_skin_profile_cmd", { accountId, forceRefresh });
    const activeSkin = profile.skins.find((s) => s.state === "ACTIVE") ?? profile.skins[0];
    const viewer = ensureSkinViewer();
    if (activeSkin) {
      selectedSkinVariant = activeSkin.variant.toLowerCase() === "slim" ? "slim" : "classic";
      await viewer.loadSkin(activeSkin.url, {
        model: selectedSkinVariant === "slim" ? "slim" : "default",
      });
    }
    const activeCape = profile.capes.find((c) => c.state === "ACTIVE");
    if (activeCape) {
      await viewer.loadCape(activeCape.url);
    } else {
      viewer.loadCape(null);
    }
    renderSkinVariantButtons();
    renderCapeGrid(profile, accountId);
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function uploadSkin() {
  if (!state.currentAccount || state.currentAccount.type !== "Microsoft") return;
  const accountId = accountKey(state.currentAccount);
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: "Skin", extensions: ["png"] }] });
    if (!picked || Array.isArray(picked)) return;
    await invoke("upload_skin_cmd", { accountId, filePath: picked, variant: selectedSkinVariant });
    await loadSkinsTab();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function resetSkin() {
  if (!state.currentAccount || state.currentAccount.type !== "Microsoft") return;
  try {
    await invoke("reset_skin_cmd", { accountId: accountKey(state.currentAccount) });
    await loadSkinsTab();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function applyCape(accountId: string, capeId: string) {
  try {
    await invoke("set_cape_cmd", { accountId, capeId });
    await loadSkinsTab();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

async function clearCape(accountId: string) {
  try {
    await invoke("clear_cape_cmd", { accountId });
    await loadSkinsTab();
  } catch (err) {
    console.error(err);
    showErrorModal(describeError(err));
  }
}

export function init(signInRequested: () => void) {
  onSignInRequested = signInRequested;

  el.skinVariantOptions.forEach((btn) => {
    btn.addEventListener("click", () => {
      const variant = btn.dataset.variant;
      if (variant !== "classic" && variant !== "slim") return;
      selectedSkinVariant = variant;
      renderSkinVariantButtons();
    });
  });

  el.skinUploadBtn.addEventListener("click", () => void uploadSkin());
  el.skinResetBtn.addEventListener("click", () => void resetSkin());
  el.skinsSigninBtn.addEventListener("click", () => onSignInRequested());
  el.skinsRefreshBtn.addEventListener("click", () => void loadSkinsTab(true));
}
