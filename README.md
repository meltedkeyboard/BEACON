# Beacon

Beacon is a from-scratch, open-source Minecraft launcher, written in Rust. It downloads and
verifies game files from Mojang's public version manifest, resolves platform-specific
libraries and natives, and launches the game -- either with an offline profile or with a
real Microsoft account, using the same OAuth2 device code / Xbox Live / XSTS / Minecraft
Services sign-in chain as the official launcher.

This is a personal, hobbyist, **non-commercial** project. It is not a product has no monetization, no analytics, and no telemetry.

## Disclaimer

Beacon is an independent, unofficial project. **It is not approved by or associated with
Mojang or Microsoft.** "Minecraft" is a trademark of Mojang Synergies AB / Microsoft, and
this project's use of the name is limited to describing what Beacon is compatible with, per
[Minecraft's usage guidelines](https://www.minecraft.net/en-us/usage-guidelines). Beacon does
not redistribute any Minecraft game files: all game assets, libraries, and the client jar
are downloaded directly from Mojang's own servers, exactly as the official launcher does.

The person responsible for this project is its author (a single hobbyist developer), not
Mojang or Microsoft. Use of Beacon still requires a legitimate, owned copy of Minecraft, signing in with a Microsoft account is how the launcher verifies that ownership.

## Status

Early stage / work in progress. Currently implemented:

- Version manifest and per-version metadata parsing
- Concurrent, SHA1-verified downloads of the client jar, libraries, natives, and assets
- Offline accounts (gated behind at least one verified Microsoft sign-in, see below)
- Microsoft account sign-in (OAuth2 device code flow, Xbox Live / XSTS / Minecraft Services) with the refresh token stored in the OS credential store, never in plaintext
- A `beacon-tauri` desktop GUI (early, actively changing) alongside the `beacon-cli` command-line interface

Offline accounts exist as a convenience for players who have already proven ownership of the
game through a Microsoft sign-in, they are not available until at least one Microsoft
account has been signed in successfully.

## Workspace layout

- `beacon-core` is the launcher logic as a library: manifest parsing, downloading, account
  and auth handling, JVM launch. No UI code; every public type is serde-serializable, which is
  what lets both `beacon-cli` and `beacon-tauri` sit on top of it directly.
- `beacon-cli` is a thin CLI over `beacon-core`.
- `beacon-tauri` is the desktop GUI (`src-tauri` crate: `beacon-desktop`), a Tauri app wired to
  `beacon-core` -- version picker, accounts, sign-in, install/launch with progress, all still
  early and under active development.

## Building

```
cargo build --workspace
```

Requires a Java runtime on `PATH` (or pass `--java <path>`) to actually launch the game.
