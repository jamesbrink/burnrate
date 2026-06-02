# Burnrate

Burnrate is a tray-first desktop quota monitor for Claude Code, Codex, and OpenRouter. It ships as a Tauri 2 desktop app and as a binary-only Rust crate for `cargo install burnrate`.

## Install

The primary install path is the native app bundle from GitHub Releases. Rust users can also install the bundled binary crate:

```sh
cargo install burnrate
```

The crates.io package includes the built `dist/` frontend assets so the installed binary can launch the dashboard without a separate JavaScript build.

## Providers

- Claude Code: detects `CLAUDE_CONFIG_DIR` or `~/.claude` and reads OAuth-style local credentials when available.
- Codex: detects `CODEX_HOME` or `~/.codex` and reads local auth material when available.
- OpenRouter: supports manual API-key accounts and fetches `/api/v1/credits`.

Secrets are stored in the OS keyring by default. Plaintext storage is available only when explicitly selected for an account.

## Tray

Burnrate starts tray-first. Left-click the tray icon to open a compact account and usage summary. Right-click the tray icon for actions such as opening the full app, refreshing usage, or quitting.

The dashboard includes a simple Hide Dock setting for macOS users who want Burnrate to behave like a pure menu-bar utility.

## Development

```sh
npm install
npm run dev
```

`npm run dev` and the devshell `dev` helper launch the Tauri app, including the
tray icon. Tauri starts the Vite dashboard server through `npm run dev:web`.
Frontend edits hot-reload through Vite HMR. Rust/Tauri edits are watched by
`tauri dev` and restart the desktop process.

Inside `nix develop`, Burnrate prints a helper menu and exposes the helper commands on `PATH`:

```sh
dev
build-app
check
test
fmt
clean
package-crate
```

Coverage gates target at least 80%:

```sh
npm run coverage
```

## Release

`release-plz` opens release PRs, tags versions, and publishes the Rust crate. The GitHub release workflow builds native Tauri artifacts and uploads checksums for the generated bundles.
