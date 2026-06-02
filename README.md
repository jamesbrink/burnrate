# Burnrate

[![CI](https://github.com/jamesbrink/burnrate/actions/workflows/ci.yml/badge.svg)](https://github.com/jamesbrink/burnrate/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Desktop usage monitor for Claude Code, Codex, and OpenRouter quotas, credits, and subscription limits. Built with Tauri 2 (Rust + React/TypeScript) and lives in the system tray (the menu bar on macOS).

## Features

- Menu-bar tray summary with a left-click usage popover and right-click actions (Preferences, Refresh, Quit).
- Native Preferences window for account management and manual OpenRouter setup.
- Auto-detects Claude Code and Codex accounts from local config; OpenRouter is added via API key.
- Claude Code subscription buckets (5-hour, weekly, model-specific) with stale-auth checks via `claude auth status`.
- Codex Pro/Max plan and rate-limit buckets read from the Codex app server.
- Secrets in the OS keyring by default, with an explicit plaintext fallback.
- Hides from the Dock by default; appears only while Preferences is open.

## Install

Download the native bundle for your platform from [GitHub Releases](https://github.com/jamesbrink/burnrate/releases), or install the binary crate (it ships the prebuilt UI):

```sh
cargo install burnrate
```

## Development

```sh
npm install
npm run dev      # tauri dev — launches the desktop app + tray
```

A Nix devshell exposes the full workflow (`nix develop`, then `dev`, `check`, `test`, `fmt`, `build-app`, `build-pure`). See [AGENTS.md](AGENTS.md) for the architecture map and complete command reference.

## Providers & configuration

Claude Code usage requires a first-party `claude.ai` OAuth login with an active subscription. If Burnrate reports a stale, inference-only, or missing-subscription state, refresh with:

```sh
claude auth login
```

Burnrate refreshes in the background every five minutes and caches successful snapshots for five minutes to avoid tight polling.

Only non-secret account configuration is stored on disk. On macOS the default path is `~/Library/Application Support/burnrate/accounts.json`; set `BURNRATE_CONFIG_DIR` to override. Manual secrets live in the OS keyring unless plaintext storage is explicitly selected for that account.

## Releases

- `release-plz` manages crate release PRs, version tags, and crates.io publishing.
- Tagging `v*` builds native Tauri bundles (macOS, Linux, Windows) and uploads them with checksums to the GitHub Release.

## License

[MIT](LICENSE)
