# Burnrate

Burnrate is a tray-first desktop app for watching quota, credits, and usage burn rate across local AI developer tools and API accounts.

It is built with Tauri 2, Rust, React, and TypeScript. The primary distribution path is native desktop bundles from GitHub Releases, with a secondary binary-only Rust crate for `cargo install burnrate`.

## Features

- Tray-first usage summary with a compact left-click account and usage popover.
- Right-click tray actions for opening Preferences, refreshing usage, toggling the Dock icon, and quitting.
- Native Preferences window for account management, provider state, and manual OpenRouter setup.
- Claude Code account detection from local Claude configuration and macOS Keychain, with stale-auth checks through `claude auth status --json`.
- Claude Code subscription buckets including 5-hour, weekly, weekly OAuth app, model-specific weekly buckets, and extra usage when available.
- Codex account detection from `CODEX_HOME` or `~/.codex`, including Pro/Max plan and 5-hour/weekly rate-limit buckets when exposed by the Codex app server.
- OpenRouter API key accounts using the `/api/v1/credits` endpoint.
- OS keyring storage for secrets by default, with an explicit plaintext fallback mode.
- macOS Hide Dock setting for menu-bar style use. New installs hide the Dock icon by default.

## Install

Download the native app bundle for your platform from GitHub Releases when releases are available.

Rust users can install the binary crate:

```sh
cargo install burnrate
```

The crate includes the built frontend assets needed to launch the Tauri dashboard after installation.

## Development

Install JavaScript dependencies, then run the desktop app:

```sh
npm install
npm run dev
```

`npm run dev` starts `tauri dev`, which launches the actual desktop app and tray icon. Tauri starts the Vite dev server through `npm run dev:web`; frontend edits hot-reload through Vite HMR, while Rust and Tauri edits restart the desktop process.

The Nix devshell exposes the same workflow as short helper commands:

```sh
nix develop
dev
check
test
fmt
build-app
build-pure
package-crate
clean
```

Burnrate also has a pure Nix package target:

```sh
nix build .#burnrate
```

The flake package metadata is derived from `Cargo.toml`, including the app version, description, homepage, repository release URL, main program, and MIT license mapping.

## Provider Notes

Claude Code usage requires a first-party `claude.ai` OAuth login with a detected subscription. If Burnrate reports a stale, inference-only, third-party, or missing-subscription Claude auth state, refresh Claude Code with:

```sh
claude auth login
```

Burnrate deliberately stores only non-secret account configuration in its app data. Manual account secrets are stored in the OS keyring unless plaintext storage is explicitly selected for that account.

## Verification

Run the standard local gates:

```sh
./scripts/check
./scripts/test
npm run coverage
cargo package --allow-dirty
```

Coverage is expected to stay at or above 80% for the configured UI and Rust coverage gates.

CI runs formatting, clippy, Rust tests, frontend tests, release build smoke checks, Nix checks, crate packaging, and Codecov uploads for Rust and UI coverage.

## Release

Burnrate is prepared for two release channels:

- `release-plz` manages crate release PRs, version tags, and crates.io publishing.
- GitHub Actions build native Tauri bundles, including macOS artifacts, and upload checksums with each GitHub Release.

## License

Burnrate is licensed under the MIT License. See [LICENSE](LICENSE).
