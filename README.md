# Burnrate

[![CI](https://github.com/jamesbrink/burnrate/actions/workflows/ci.yml/badge.svg)](https://github.com/jamesbrink/burnrate/actions/workflows/ci.yml)
[![Release](https://github.com/jamesbrink/burnrate/actions/workflows/release.yml/badge.svg)](https://github.com/jamesbrink/burnrate/actions/workflows/release.yml)
[![Docs](https://github.com/jamesbrink/burnrate/actions/workflows/docs.yml/badge.svg)](https://jamesbrink.online/burnrate/)
[![crates.io](https://img.shields.io/crates/v/burnrate.svg)](https://crates.io/crates/burnrate)
[![downloads](https://img.shields.io/crates/d/burnrate.svg)](https://crates.io/crates/burnrate)
[![latest release](https://img.shields.io/github/v/release/jamesbrink/burnrate?sort=semver)](https://github.com/jamesbrink/burnrate/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](https://github.com/jamesbrink/burnrate/releases/latest)

Desktop usage monitor for Claude Code, Codex, GitHub Copilot, OpenRouter, Runpod, and AWS quotas, credits, spend, and subscription limits — plus claudex-powered local usage insights (daily burn, projections, model and project breakdowns). Built with Tauri 2 (Rust + React/TypeScript) and lives in the system tray (the menu bar on macOS).

**Documentation: [jamesbrink.online/burnrate](https://jamesbrink.online/burnrate/)**

## Screenshots

<p align="center">
  <img src="website/public/screenshots/preferences.png" alt="Burnrate Preferences window showing per-account usage across Claude Code, Codex, Runpod, and OpenRouter" width="62%" />
  &nbsp;
  <img src="website/public/screenshots/tray.png" alt="Burnrate menu-bar popover with live quota meters for each account" width="30%" />
</p>

<p align="center"><em>The full Preferences window (left) and the menu-bar popover (right).</em></p>

## Features

- Menu-bar tray summary with a left-click usage popover and right-click actions (Preferences, Refresh, Quit).
- Native translucent (vibrancy) popover on macOS that follows the system light/dark appearance, sizes itself to its content, and dismisses when it loses focus.
- Native Preferences window for account management and manual OpenRouter/Runpod/AWS setup.
- Auto-detects Claude Code, Codex, and GitHub Copilot accounts from local config; OpenRouter and Runpod are added via API key, and AWS uses your existing AWS profile/default credential chain.
- **Multiple Claude Code and Codex accounts**, each signed in from the app via browser OAuth and shown with its email address and usage.
- **Drag to reorder** accounts — reorder the tray usage cards or the Preferences list; the order persists across both windows.
- Claude Code subscription buckets (5-hour, weekly, model-specific) with stale-auth checks via `claude auth status`.
- Codex Pro/Max plan and rate-limit buckets read from the Codex app server.
- **GitHub Copilot premium requests** per month against your plan's allowance — counted locally from Copilot CLI sessions, or exactly via an optional GitHub token and the billing API.
- **Local usage insights** powered by [claudex](https://github.com/utensils/claudex): per-provider daily cost sparklines, today/week/month-to-date spend, a month-end projection, model distribution, and top projects — computed entirely from local CLI session logs.
- Runpod prepaid balance, current spend, burn-rate runway, active resources, and recent Pods/Serverless/storage costs.
- AWS Cost Explorer month-to-date USD spend with optional monthly budgets and configurable service/tag/cost-category buckets such as Bedrock, EC2 compute, and S3.
- Secrets in the OS keyring by default, with an explicit plaintext fallback.
- Hides from the Dock by default; appears only while Preferences is open.
- **Automatic updates (macOS)** with selectable **Stable** and **Nightly** channels: a dismissible banner and tray "Check for Updates…" entry offer a signature-verified one-click "Install & Restart." Choose the channel under Preferences → Updates.

## Install

Download the native bundle for your platform from [GitHub Releases](https://github.com/jamesbrink/burnrate/releases), or install the binary crate (it ships the prebuilt UI):

```sh
cargo install burnrate
```

See the [installation guide](https://jamesbrink.online/burnrate/guide/installation) for Nix, update channels, and signed macOS builds. If `cargo install` fails with `ld: library not found for -liconv` (a non-Apple `cc` first in `PATH`), see [the fix](https://jamesbrink.online/burnrate/guide/troubleshooting#cargo-install-fails-ld-library-not-found-for-liconv).

## Documentation

Setup, provider specifics (including AWS permissions), configuration, and troubleshooting live on the docs site:

- [Getting started](https://jamesbrink.online/burnrate/guide/getting-started) — accounts, browser sign-in, multi-account isolation
- [Configuration](https://jamesbrink.online/burnrate/guide/configuration) — storage paths, secrets, environment variables
- [Providers](https://jamesbrink.online/burnrate/providers/claude-code) — Claude Code, Codex, GitHub Copilot, OpenRouter, Runpod, AWS
- [Local insights](https://jamesbrink.online/burnrate/guide/local-insights) — claudex-backed local usage analytics
- [Troubleshooting](https://jamesbrink.online/burnrate/guide/troubleshooting) — keychain prompts, CLI discovery, stale auth

## Development

```sh
npm install
npm run dev      # tauri dev — launches the desktop app + tray
```

A Nix devshell exposes the full workflow (`nix develop`, then `dev`, `check`, `test`, `fmt`, `build-app`, `build-pure`, `package-dmg`, `docs-dev`). See [AGENTS.md](AGENTS.md) for the architecture map and complete command reference.

The docs site is a VitePress app in `website/` — `docs-dev` starts it with hot reload; pushes to `main` deploy it to GitHub Pages.

## Releases

- `release-plz` manages crate release PRs, version tags, and crates.io publishing.
- Tagging `v*` builds native Tauri bundles (macOS, Linux, Windows) and uploads them with checksums to the GitHub Release, then publishes a signed `latest.json` (the **Stable** auto-update manifest).
- A `nightly` workflow runs after green `CI` on `main`, building a signed macOS pre-release and promoting it to the rolling `nightly` release/manifest (the **Nightly** channel).
- Auto-update signing uses a Tauri minisign keypair: the public key lives in `tauri.conf.json`; the private key + passphrase are the `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repo secrets.

## License

[MIT](LICENSE)
