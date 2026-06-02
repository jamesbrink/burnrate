# Repository Guidance

> This file is the canonical agent guide. `CLAUDE.md` is a symlink to it — edit
> `AGENTS.md`, never replace the symlink.

## Overview

Burnrate is a macOS-first menu-bar app that monitors remaining quota/credits
across Claude Code, Codex, and OpenRouter. It is a **Tauri 2** app: a Rust
backend (`src/`) plus a React + TypeScript frontend (`src-ui/`), shipped both as
native bundles (GitHub Releases) and as a binary crate (`cargo install
burnrate`).

## Architecture

### Backend (`src/`, Rust, edition 2024)

- `main.rs` — Tauri entrypoint. Declares the `#[tauri::command]` IPC handlers
  (`dashboard`, `list_accounts`, `save_account`, `remove_account`,
  `detect_accounts`, `save_settings`, `refresh_snapshots`,
  `resize_preferences_to_content`, `close_preferences`),
  builds the two windows and macOS menu, installs the tray, and spawns the
  5-minute background refresh loop. Closing the Preferences window is
  intercepted to _hide_ (tray-only), not quit.
- `app_state.rs` — `AppState` is the managed Tauri state: a `Mutex<AppConfig>`
  plus the `ProviderClient`. `dashboard()` fans out one snapshot fetch per
  enabled account concurrently (`tokio::spawn`), preserves order, and rolls the
  results into a `TraySummary`. All persistence flows through here.
- `config.rs` — `AppConfig` (settings + accounts) persisted to `accounts.json`.
  Writes are atomic (temp file + rename) with `0600` file / `0700` dir perms on
  unix; a malformed file is moved aside (`.json.invalid-<nonce>`) and replaced
  with defaults. `views()` strips secrets and exposes only `hasSecret`;
  `merge_detected` reconciles auto-detected accounts. `config_dir()` honors
  `BURNRATE_CONFIG_DIR`.
- `key_store.rs` — secret storage: OS **keyring by default**, **plaintext only
  when explicitly selected**, with migration between modes and an in-process
  read cache. Secrets never live in `accounts.json` under keyring mode.
- `models.rs` — every serde wire type shared with the UI. Structs are
  `camelCase`, enums `kebab-case`. This is the single source of truth that
  `src-ui/types.ts` mirrors by hand — keep them in sync.
- `providers/mod.rs` — `ProviderClient` (HTTP client + 5-minute _success_ cache
  keyed by `provider:id:endpoint:updated_at`), provider dispatch, and the shared
  JSON helpers: pointer-based `number/text/bool_value/datetime` lookups, generic
  usage-bucket and subscription parsing, status thresholds (Warning ≤20% /
  Exhausted ≤5% remaining), endpoint validation (HTTPS-only except localhost),
  and token resolution (keyring → credential file via provider-specific JSON
  pointers). `detect_accounts()` aggregates per-provider detection.
- `providers/{claude,codex,openrouter}.rs` — each implements `fetch()`, and
  claude/codex also implement `detect()`. claude reads `~/.claude` creds +
  macOS Keychain, validates with `claude auth status --json`, and queries the
  Anthropic OAuth usage endpoint (with its own usage cache + error backoff);
  codex detects `CODEX_HOME`/`~/.codex` and talks to the Codex app server over
  stdio JSON (reset timestamps may be seconds or millis — both normalized);
  openrouter hits `/api/v1/credits`.
- `tray.rs` — tray icon/menu (Preferences / Refresh / Quit), left-click toggles
  the cursor-anchored `tray` popover window, `summarize()` reduces snapshots to
  a single status/label, and the macOS activation policy switch (Accessory =
  hidden from Dock, Regular = shown while Preferences is open).

### Frontend (`src-ui/`, React 18 + Vite)

- One bundle serves **both** Tauri windows. `App.tsx` reads `?view=tray` to
  render `TrayPanel` (popover) vs. the full `Preferences` UI, and subscribes to
  backend events.
- `api.ts` is the IPC bridge and the only file that touches Tauri. Every call
  checks `isTauri`; **outside Tauri** (vitest, or `dev:web` in a plain browser)
  it returns mock dashboard/account data so the UI and its tests run without the
  Rust backend. It wraps `invoke()` commands and `listen()` for the
  `burnrate-refresh-requested` / `burnrate-dashboard-updated` /
  `burnrate-settings-updated` events.
- `types.ts` mirrors the Rust wire models; `Preferences.tsx`, `TrayPanel.tsx`,
  `ProviderLogo.tsx`, and `format.ts` are the focused UI pieces.

### Cross-cutting invariants

- **`dist/` (built frontend) is committed.** The crate's `include` list bundles
  `dist/**`, so `cargo install burnrate` ships prebuilt assets, and both
  `ci.yml` and
  `release-plz.yml` enforce `git diff --exit-code -- dist`. After any frontend
  change: `npm run build` and commit the updated `dist/`.
- **CSP allowlist.** `tauri.conf.json` restricts `connect-src` to the Anthropic,
  ChatGPT, OpenRouter, and localhost hosts — adding a provider endpoint requires
  editing that CSP.
- **Two release channels.** `release-plz` manages the crate PR → tag →
  crates.io; `release.yml` (on tag `v*`) builds native bundles via `tauri-action`
  and uploads checksums to the GitHub Release.

## Commands

Run inside `nix develop` (or `direnv` auto-activates it); the devshell exposes
short aliases (`dev`, `check`, `test`, `fmt`, `build-app`, `build-pure`,
`package-crate`, `clean`).

```sh
npm install            # one-time: install JS deps
npm run dev            # tauri dev — launches the real desktop app + tray (HMR for UI)
npm run dev:web        # vite only, browser mock mode (no backend), port 5173

./scripts/check        # cargo fmt --check, clippy -D warnings, tsc --noEmit
./scripts/fmt          # cargo fmt + prettier (use `fmt`/treefmt in the devshell)
npm run typecheck      # tsc --noEmit

./scripts/test         # cargo test + vitest run
cargo test <name>      # single Rust test by name substring (e.g. cargo test finds_provider_specific_tokens)
npx vitest run src-ui/api.test.ts        # single UI test file
npx vitest run -t "summary promotes"     # single UI test by name

npm run coverage       # UI + Rust coverage; both gated at 80%
                       # (Rust gate ignores main.rs/app_state.rs/tray.rs — Tauri glue)

./scripts/build-app    # npm run build + cargo build --release
nix build .#burnrate   # pure Nix package build
cargo package --allow-dirty   # verify the crates.io archive (incl. bundled dist/)
nix flake check        # when Nix/devshell wiring changes
```

## Git

- Always work from a proper branch such as `feat/<topic>`, `fix/<topic>`, `chore/<topic>`, or `docs/<topic>`; do not continue directly on `main` unless the user explicitly asks for it.
- For miscellaneous fixes, jump to a short `chore/<topic>` or `fix/<topic>` branch before editing.
- Use detailed conventional commits: `feat(scope): summary`, `fix(scope): summary`, `test(scope): summary`, `chore(scope): summary`, `docs(scope): summary`, `refactor(scope): summary`, and similar.
- Commit messages should be specific enough to explain the behavioral or maintenance outcome without reading the diff.
- Commit in coherent chunks after meaningful, verified progress, unless the user explicitly says not to commit.

## Engineering Practice

- Prefer TDD for new behavior: write or update a focused failing test first, implement the smallest change that makes it pass, then refactor.
- Keep test coverage proportional to risk. Provider parsing, config migration, key storage, tray behavior, and release/package wiring should have direct tests.
- Avoid god files and oversized components. Split code by responsibility before a file becomes hard to scan or test.
- Keep module boundaries clear:
  - provider detection/fetch/parsing belongs under `src/providers/`;
  - persistent account configuration belongs in `src/config.rs`;
  - secret handling belongs in `src/key_store.rs`;
  - tray behavior belongs in `src/tray.rs`;
  - UI API bridging belongs in `src-ui/api.ts`;
  - large React UI pieces should become focused components instead of accumulating in `App.tsx`.
- Prefer existing project patterns over new abstractions. Add an abstraction only when it removes real duplication or clarifies ownership.
- Keep secrets out of tracked files. Store only non-secret account configuration in app data; use keyring storage by default and plaintext only when explicitly selected.
- Keep docs and metadata in sync when behavior changes: `README.md`, `Cargo.toml`, `package.json`, `tauri.conf.json`, `flake.nix`, release workflows, and committed `dist/` assets should describe the same app surface.
- Keep `AGENTS.md` the source of truth for agent guidance; `CLAUDE.md` should remain a symlink to it.

## Verification

- Run the smallest relevant test while iterating.
- Before finishing substantial work, run the appropriate local gate:
  - `./scripts/check`
  - `./scripts/test`
  - `./scripts/build-app`
  - `cargo package --allow-dirty` when crate packaging or bundled assets changed
  - `nix flake check` when Nix/devshell wiring changed
