# Beacon desktop (Tauri)

Empty-shell Tauri app wired to `beacon-core`. No real UI yet -- this exists so the GUI work can
start immediately once Microsoft-auth approval lands, without also having to bootstrap the Tauri
project from scratch at that point.

`src-tauri` (crate `beacon-desktop`) exposes a first pass of commands over `beacon-core`:
`list_versions`, `list_accounts`, `install_version_cmd` (emits `install-progress` events),
`login_microsoft_cmd` (emits a `device-code` event with the code to show the user), `logout_cmd`,
`launch_version_cmd` (emits `game-log` events with the launched game's stdout/stderr, since
`beacon_core::launch` pipes them instead of inheriting a console). None of this is called from the
frontend yet -- `src/main.ts` is still the stock vanilla-ts template.

## Running

```
npm install
npm run tauri dev
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
