// Mutated in place (`state.foo = x`), never reassigned -- ES modules don't allow an importer
// to reassign an imported `let`, only to mutate a property of an imported object.

import type { Account, DirectorySettings, Instance } from "./types";

export const state = {
  currentAccount: null as Account | null,
  instances: [] as Instance[],
  selectedInstanceId: null as string | null,
  viewingInstanceId: null as string | null,
  directorySettings: null as DirectorySettings | null,
  showSnapshots: false,
  momentsTabEnabled: false,
  screenshotsBgEnabled: true,
  screenshotsBgBlur: 6,
};

export function currentInstance(): Instance | null {
  return state.instances.find((i) => i.id === state.selectedInstanceId) ?? null;
}
