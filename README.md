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

## Development

```sh
npm install
npm run dev
cargo run
```

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

## Release

`release-plz` opens release PRs, tags versions, and publishes the Rust crate. The GitHub release workflow builds native Tauri artifacts and uploads checksums for the generated bundles.
