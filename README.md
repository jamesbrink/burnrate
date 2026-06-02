# Burnrate

Burnrate is a desktop usage monitor for Claude Code, Codex, and OpenRouter quotas, credits, and subscription limits.

It is built with Tauri 2, Rust, React, and TypeScript. The primary distribution path is native desktop bundles from GitHub Releases, with a secondary binary-only Rust crate for `cargo install burnrate`.

## Features

- Compact tray usage summary with a left-click account and usage popover.
- Right-click tray actions for opening Preferences, refreshing usage, toggling the Dock icon, and quitting.
- Native Preferences window for account management, provider state, and manual OpenRouter setup.
- Claude Code account detection from local Claude configuration and macOS Keychain, with stale-auth checks through `claude auth status --json`.
- Claude Code subscription buckets including 5-hour, weekly, weekly OAuth app, model-specific weekly buckets, and extra usage when available.
- Codex account detection from `CODEX_HOME` or `~/.codex`, including Pro/Max plan, 5-hour/weekly rate-limit buckets, and additional model-family buckets such as Spark when exposed by the Codex app server.
- OpenRouter API key accounts using the `/api/v1/credits` endpoint.
- OS keyring storage for secrets by default, with an explicit plaintext fallback mode.
- macOS menu-bar style behavior: Burnrate hides from the Dock by default, shows in the Dock while Preferences is open, then returns to tray-only when Preferences closes.

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

Burnrate refreshes provider usage in the background every five minutes. Manual refreshes, opening the tray popover, and opening Preferences can request fresh data too, but successful provider snapshots are cached for five minutes by default so Claude Code, Codex, and OpenRouter avoid tight polling loops.

Codex rate limits are read from the Codex app server. Burnrate displays the primary Codex 5-hour and weekly buckets plus any additional limit groups returned by `rateLimitsByLimitId`; for example, Spark subscriptions can expose `Spark 5-hour` and `Spark Weekly` buckets. Codex reset timestamps may be returned as either Unix seconds or milliseconds, and Burnrate normalizes both forms before rendering reset times.

Burnrate deliberately stores only non-secret account configuration in its app data. On macOS, the default path is:

```text
~/Library/Application Support/burnrate/accounts.json
```

Set `BURNRATE_CONFIG_DIR` to override the directory; Burnrate will then use `$BURNRATE_CONFIG_DIR/accounts.json`. Manual account secrets are stored in the OS keyring unless plaintext storage is explicitly selected for that account. Plaintext fallback is opt-in and should be treated as local-cleartext storage, especially on non-Unix platforms where Burnrate cannot apply Unix-style `0600` file permissions.

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
