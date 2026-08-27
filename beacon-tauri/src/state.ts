// Shared mutable app state -- only fields more than one feature module actually reads or writes.
// Everything else (e.g. `playStage`, `accounts`, `skinViewer`) stays a local variable inside the
// one feature file that owns it; importing modules only call that file's exported functions.
//
// Mutated in place (`state.foo = x`), never reassigned as a binding -- ES modules don't allow an
// importer to reassign an imported `let`, only to mutate a property of an imported object.

import type { Account, DirectorySettings, Instance } from "./types";

export const state = {
  currentAccount: null as Account | null,
  instances: [] as Instance[],
  selectedInstanceId: null as string | null,
  viewingInstanceId: null as string | null,
  directorySettings: null as DirectorySettings | null,
  // Play-tab-background and "show snapshots" are per-device settings (owned/persisted by
  // `settings.ts`), but `play.ts` and `versions.ts` respectively need to read the current value.
  showSnapshots: false,
  screenshotsBgEnabled: true,
  screenshotsBgBlur: 6,
};

export function currentInstance(): Instance | null {
  return state.instances.find((i) => i.id === state.selectedInstanceId) ?? null;
}
