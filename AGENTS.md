# Repository Guidance

> This file is the canonical agent guide. `CLAUDE.md` is a symlink to it — edit
> `AGENTS.md`, never replace the symlink.

## Overview

Burnrate is a macOS-first menu-bar app that monitors remaining quota/credits
across Claude Code, Codex, OpenRouter, Runpod, and AWS Cost Explorer spend
(with multiple accounts per provider for Claude Code and Codex). It is a
**Tauri 2** app: a Rust
backend (`src/`) plus a React + TypeScript frontend (`src-ui/`), shipped both as
native bundles (GitHub Releases) and as a binary crate (`cargo install
burnrate`).

## Architecture

### Backend (`src/`, Rust, edition 2024)

- `main.rs` — Tauri entrypoint. Declares the `#[tauri::command]` IPC handlers
  (`dashboard`, `list_accounts`, `save_account`, `remove_account`,
  `detect_accounts`, `reorder_accounts`, `start_account_login`,
  `submit_account_login_code`, `cancel_account_login`, `logout_account`, `save_settings`,
  `refresh_snapshots`, `resize_preferences_to_content`,
  `resize_tray_to_content`, `close_preferences`, `open_preferences`,
  `updater_available`, `check_for_updates`, `install_pending_update`),
  registers `tauri-plugin-updater`,
  builds the two windows and macOS menu, installs the tray, and spawns the
  5-minute background refresh loop. Closing the Preferences window is
  intercepted to _hide_ (tray-only), not quit.
- `updater.rs` — auto-updater IPC over `tauri-plugin-updater`: `check_for_updates`
  (channel-aware: `stable` hits `releases/latest`, `nightly` discovers candidate
  tags via the GitHub Releases API with a static fallback) stashes the pending
  `Update` in the managed `UpdaterState`; `install_pending_update` downloads +
  installs (emitting `burnrate-update-progress`) and restarts. `updater_available`
  gates the UI on a configured pubkey so unsigned dev builds stay quiet. Pure
  parsing/endpoint helpers are unit-tested.
- `app_state.rs` — `AppState` is the managed Tauri state: the `ConfigStore`
  (sqlite), a `Mutex<AppConfig>` (in-memory copy), the `ProviderClient`, and a
  `LoginManager`. `load()` opens the store, merges auto-detected accounts, and
  garbage-collects orphaned managed CLI dirs — but skips GC when the database
  was just created (fresh/restored DB would classify live secondary-account
  credentials as orphans). `dashboard()` fans out one snapshot
  fetch per enabled account concurrently (`tokio::spawn`) in display order, rolls
  the results into a `TraySummary`, and persists any newly discovered account
  emails. Hosts the login orchestration: `start_account_login` either
  re-authenticates an existing account **in place** (`reauth_id` — refreshing its
  real credential location, e.g. the system-default `~/.claude`) or creates an
  isolated, disabled placeholder account + per-account CLI dir for a brand-new
  account, then spawns the sign-in; on completion it reuses/refreshes an existing
  account when the email already matches, else enables the new one, emitting
  `burnrate-login-complete` / `-failed`. `submit_account_login_code` forwards
  the user-pasted browser auth code to the active sign-in (Claude's login
  requires it). `cancel_account_login` only tears down a
  placeholder when it actually canceled an active brand-new sign-in (the
  `LoginManager` is reauth-aware and single-flight). `logout_account` and
  `remove_account` share one teardown that, for managed dirs only (never the
  system default), runs the CLI sign-out, deletes the orphan-prone macOS Keychain
  entry as a fallback, and removes the dir. All persistence flows through here.
- `config.rs` — the **in-memory** `AppConfig` (settings + accounts) and path
  helpers; persistence lives in `storage.rs`. `views()` strips secrets and
  exposes only `hasSecret`, returning
  accounts in `order_index` order; `reorder()` persists a user-defined global
  order (without bumping `updated_at`, which keys the provider cache);
  `merge_detected` reconciles auto-detected accounts. `account_cli_dir()` /
  `is_managed_cli_dir()` / `create_private_dir()` manage the isolated per-account
  CLI homes (`<config_dir>/cli/<provider>/<id>`). `config_dir()` honors
  `BURNRATE_CONFIG_DIR`; `database_path()` / `config_path()` point at
  `burnrate.sqlite` and the legacy `accounts.json`. Legacy JSON loading
  (`load_or_recover_from_path`) moves a malformed file aside
  (`.json.invalid-<nonce>`) instead of overwriting it.
- `storage.rs` — `ConfigStore`, the sqlite persistence layer (`rusqlite`, WAL,
  foreign keys, busy timeout, `0600`-hardened files). Schema is versioned via
  `PRAGMA user_version` + `run_migrations`; a fresh database does a **one-time
  import** of the legacy `accounts.json` (renamed `.migrated-<nonce>` after) —
  a schema-less/truncated DB file counts as fresh. `created_database()` is how
  `app_state.rs` knows to skip destructive startup reconciliation. Accounts,
  AWS category buckets (own table, `ON DELETE CASCADE`), and settings all live
  here; `plaintext_secret` is a column but is only populated in plaintext mode.
- `key_store.rs` — secret storage: OS **keyring by default**, **plaintext only
  when explicitly selected**, with migration between modes and an in-process
  read cache. Secrets never reach the config database under keyring mode. macOS
  binds a keychain "Always Allow" grant to the app's code signature, so an
  unsigned build re-prompts every launch; a code-signed install
  (`APPLE_SIGNING_IDENTITY` → `package-dmg`, with `entitlements.macos.plist` for
  the hardened runtime) makes the grant persist.
- `models.rs` — every serde wire type shared with the UI. Structs are
  `camelCase`, enums `kebab-case`. This is the single source of truth that
  `src-ui/types.ts` mirrors by hand — keep them in sync.
- `providers/mod.rs` — `ProviderClient` (HTTP client + 5-minute _success_ cache
  keyed by `provider:id:endpoint:updated_at`), provider dispatch, and the shared
  JSON helpers: pointer-based `number/text/bool_value/datetime` lookups, generic
  usage-bucket and subscription parsing, status thresholds (Warning ≤20% /
  Exhausted ≤5% remaining), endpoint validation (HTTPS-only except localhost),
  and token resolution (keyring → credential file via provider-specific JSON
  pointers). `detect_accounts()` aggregates per-provider detection. `resolve_cli`
  / `augmented_path` locate provider CLIs (`codex`, `claude`) across Homebrew,
  Nix, Cargo, and JS-toolchain dirs, because a Finder-launched `.app` inherits
  only a minimal `PATH`; overridable via `BURNRATE_CODEX_BIN`/`CODEX_BIN` and
  `BURNRATE_CLAUDE_BIN`/`CLAUDE_BIN`. Every spawned provider CLI gets
  `CREDENTIAL_ENV_OVERRIDES` (`CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY`,
  `OPENAI_API_KEY`, …) stripped via `strip_credential_env` — an inherited agent
  token (e.g. from a cmux surface) otherwise makes `claude auth status` report
  env auth with no subscription/email, breaking verify, fetch, and detection.
- `providers/{claude,codex,openrouter,runpod,aws}.rs` — each implements
  `fetch()`, and
  claude/codex also implement `detect()`. claude reads `~/.claude` creds +
  macOS Keychain (service name derived per account via
  `keychain_service_name_for(account.cli_config_dir(), …)`), validates with
  `claude auth status --json` (also yields the account **email** — a local
  read that never refreshes tokens), and queries the Anthropic OAuth usage
  endpoint (with its own usage cache + error backoff). Claude access tokens
  live ~8h and refresh tokens are **single-use** (rotation), so claude also
  refreshes the OAuth token itself near expiry — but **only** for accounts in
  Burnrate-managed CLI dirs, where Burnrate is the credential's sole client
  (`refresh_decision`): it POSTs the CLI's public client id to the
  platform.claude.com token endpoint (mimicking the CLI's User-Agent — the
  endpoint rate-limits generic agents) and persists the rotated credential
  back to its source (keychain or file) **before** using it. The
  system-default `~/.claude` is never refreshed — its rotation belongs to the
  user's terminal sessions, and a second writer would sign them out;
  codex detects `CODEX_HOME`/`~/.codex` and talks to the Codex app server over
  stdio JSON (reset timestamps may be seconds or millis — both normalized), and
  reads the account email by decoding the `tokens.id_token` JWT in `auth.json`
  (`base64`, payload claims only — no signature check on local trusted data).
  Each provider threads the account's per-account config dir
  (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`) into its CLI calls so multiple accounts
  stay isolated; openrouter hits `/api/v1/credits`. runpod combines the REST
  API (`https://rest.runpod.io/v1`) and GraphQL (`https://api.runpod.io/graphql`)
  for prepaid balance, burn-rate runway, and active resources — both endpoints
  overridable via `BURNRATE_RUNPOD_REST_URL` / `BURNRATE_RUNPOD_GRAPHQL_URL`.
  aws uses the official AWS SDK (default credential chain, optional named
  profile, region defaulting to `us-east-1` — no static keys stored): STS
  caller identity, then Cost Explorer `GetCostAndUsage` month-to-date
  `UnblendedCost` in USD; an optional monthly budget turns spend into
  remaining/warning/exhausted status, and user-editable category buckets
  (service/tag/cost-category filters, optional group-by) become extra buckets.
- `providers/login.rs` — interactive sign-in. Shells out to `claude auth login`
  / `codex login` under the account's config dir, streams an **allowlist-redacted**
  view of CLI output (surfacing the auth URL, masking token-shaped lines) via the
  `burnrate-login-progress` event, opens the URL with the OS opener, then verifies
  the result and reads the email. Claude's flow additionally needs the browser
  auth code pasted back: progress events carry `needs_code`, and
  `LoginManager::submit_input` writes the code to the CLI's stdin.
  `LoginManager` enforces a single concurrent
  sign-in and supports cancel (task abort + `kill_on_drop`). `run_logout` performs
  the CLI sign-out. No PTY: piped stdio + opening the URL ourselves.
- `tray.rs` — tray icon/menu (Preferences / Refresh / Check for Updates / Quit;
  the updates entry emits `burnrate-check-update-requested`), left-click toggles
  the cursor-anchored `tray` popover window, `summarize()` reduces snapshots to
  a single status/label, and the macOS activation policy switch (Accessory =
  hidden from Dock, Regular = shown while Preferences is open).
  `set_dock_icon_if_unbundled()` sets the Dock icon at runtime (via `objc2`
  `NSApplication.setApplicationIconImage`) so the bare, non-bundled binary still
  shows the Burnrate icon; the `.app` bundle's `icon.icns` is left untouched.

### Frontend (`src-ui/`, React 18 + Vite)

- One bundle serves **both** Tauri windows. `App.tsx` reads `?view=tray` to
  render `TrayPanel` (popover) vs. the full `Preferences` UI, and subscribes to
  backend events.
- `api.ts` is the IPC bridge and the only file that touches Tauri. Every call
  checks `isTauri`; **outside Tauri** (vitest, or `dev:web` in a plain browser)
  it returns mock dashboard/account data so the UI and its tests run without the
  Rust backend — including a **mock login driver** that simulates the sign-in
  event sequence via window `CustomEvent`s. It wraps `invoke()` commands and
  `listen()` for the `burnrate-refresh-requested` / `burnrate-dashboard-updated` /
  `burnrate-settings-updated`, `burnrate-login-progress` / `-complete` /
  `-failed`, and the updater's `burnrate-update-progress` /
  `burnrate-check-update-requested` events. The updater calls are mocked too
  (a `VITE_MOCK_UPDATE` opt-in advertises a fake update for `dev:web`).
- `types.ts` mirrors the Rust wire models; `constants.ts` holds shared provider
  labels/endpoints (kept cycle-free). `Preferences.tsx`, `TrayPanel.tsx`,
  `ProviderLogo.tsx`, and `format.ts` are the focused UI pieces, plus
  `AccountForm.tsx`, `AddAccountMenu.tsx`, `LoginModal.tsx`, the `useLogin.ts`
  hook, `SortableList.tsx` (reusable `@dnd-kit` drag-to-reorder used by both
  surfaces), and the updater pair `useUpdater.ts` (channel-aware poll/check/
  install state machine) + `UpdateBanner.tsx`. The `TrayPanel` header has a
  settings gear that calls `open_preferences`; Preferences hosts the Updates
  section (channel selector + check button) and renders the banner. Multi-account:
  each account shows its email; the Add-account menu offers browser sign-in vs.
  manual token entry for Claude Code / Codex (plus API-key entry for
  OpenRouter/Runpod and profile-based AWS setup). `LoginModal` includes the
  paste-the-auth-code step (`needsCode`) that Claude sign-in requires.

### Docs site (`website/`, VitePress)

- `website/` is a self-contained VitePress site (own `package.json` +
  lockfile — deliberately **not** part of the app's npm tree, so the flake's
  `npmDepsHash` is unaffected). `docs-dev` / `docs-build` devshell helpers run
  it; `.github/workflows/docs.yml` builds and deploys it to GitHub Pages
  (<https://jamesbrink.online/burnrate/>, base `/burnrate/`) on pushes to
  `main` that touch `website/**`. Theme: forced dark, flat ember palette from
  `icons/app-icon.svg`, customized in `website/.vitepress/theme/custom.css`.
  App screenshots live in `website/public/screenshots/` (the README embeds
  them from there too).

### Cross-cutting invariants

- **`dist/` (built frontend) is committed.** The crate's `include` list bundles
  `dist/**`, so `cargo install burnrate` ships prebuilt assets, and both
  `ci.yml` and
  `release-plz.yml` enforce `git diff --exit-code -- dist`. After any frontend
  change: `npm run build` and commit the updated `dist/`.
- **`custom-protocol` is a default Cargo feature.** Tauri only embeds `dist/`
  (instead of loading the `devUrl` dev server) when `custom-protocol` is on. The
  `tauri` CLI enables it for `tauri build`, but plain `cargo build --release` and
  `cargo install burnrate` cannot — so it is on by default, and `tauri dev`
  (`npm run dev`) opts out via `--no-default-features` to keep live reload.
  Removing the default would make every non-`tauri build` binary open blank.
- **CSP allowlist.** `tauri.conf.json` restricts `connect-src` to the Anthropic,
  ChatGPT, OpenRouter, Runpod, and localhost hosts — adding a provider endpoint
  requires editing that CSP. HTTP that runs in Rust (the updater's `reqwest`
  calls and the AWS SDK) is exempt — only webview traffic is governed.
- **App version derives from `Cargo.toml`.** `tauri.conf.json` deliberately omits
  `version` so the bundle/updater version tracks the crate version that
  `release-plz` bumps — keeping the auto-updater's version comparison correct. Do
  not re-add a hardcoded `version` there; the nightly workflow stamps `Cargo.toml`.
- **Updater signing.** `bundle.createUpdaterArtifacts` is on, so every release leg
  signs its updater bundle and the build needs `TAURI_SIGNING_PRIVATE_KEY` +
  `_PASSWORD` on all runners (not just macOS). The public key lives in
  `tauri.conf.json` `plugins.updater.pubkey`; `src/updater.rs` refuses to promise
  updates if it's blank.
- **Packaged `.cargo/config.toml` pins the darwin linker — with limits.**
  rustc links via plain `cc` from PATH; a non-Apple cc (Homebrew/Nix GCC)
  can't resolve the macOS SDK's `-liconv` stub (linked by the `libc` crate on
  Apple targets), so `cargo install` of any Rust binary fails on such Macs.
  The crate ships `.cargo/config.toml` (in the `include` list) pinning
  `linker = "/usr/bin/cc"` for both apple-darwin targets. **Caveat
  (empirically verified):** cargo honors packaged config for `--path`/`--git`
  installs, but plain registry installs read config ONLY from `$CARGO_HOME`
  and the environment — packaged and cwd config are both ignored, so there is
  no crate-side lever; affected users need the pin in their own
  `~/.cargo/config.toml` (documented in the website troubleshooting page).
  The Nix package and devshell override the pin via `CARGO_TARGET_*_LINKER`
  env vars (env beats file). Never add dev-only entries (like the codesign
  `runner`) to that file — it ships to users; the runner lives in devshell
  env instead.
- **IPC command sync.** Adding a `#[tauri::command]` requires touching three
  places in lockstep: register it in `main.rs`, wrap it in `src-ui/api.ts`, and
  add it to the `vi.hoisted` mock in `App.states.test.tsx` (an unmocked export
  throws at render).
- **Two release channels + auto-update.** `release-plz` manages the crate PR →
  tag → crates.io; `release.yml` (on tag `v*`) builds native bundles via
  `tauri-action`, uploads checksums, and the `publish-manifest` job attaches a
  signed `latest.json` (the **Stable** updater manifest). `nightly.yml` runs after
  green `CI` on `main`, builds a signed macOS pre-release into `nightly-staging`,
  and promotes it to the rolling `nightly` release/manifest (the **Nightly**
  channel). The updater manifest is macOS-only; `scripts/build-updater-manifest.sh`
  maps the one universal `.app.tar.gz` signature to both `darwin-*` keys. macOS
  bundles are Developer ID-signed and notarized when the `APPLE_*` repo secrets are
  set (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`); without them the build still
  succeeds unsigned.

## Commands

Run inside `nix develop` (or `direnv` auto-activates it); the devshell exposes
short aliases (`dev`, `check`, `test`, `fmt`, `build-app`, `build-pure`,
`package-dmg`, `package-crate`, `clean`).

```sh
npm install            # one-time: install JS deps
npm run dev            # tauri dev — launches the real desktop app + tray (HMR for UI)
npm run dev:web        # vite only, browser mock mode (no backend), port 5173
./scripts/dev-codesign-setup.sh  # one-time (macOS): stop keychain re-prompts in dev (see below)

./scripts/check        # cargo fmt --check, clippy -D warnings, tsc --noEmit
./scripts/fmt          # cargo fmt + prettier (use `fmt`/treefmt in the devshell)
npm run typecheck      # tsc --noEmit

./scripts/test         # cargo test + vitest run
cargo test <name>      # single Rust test by name substring (e.g. cargo test finds_provider_specific_tokens)
npx vitest run src-ui/api.test.ts        # single UI test file
npx vitest run -t "summary promotes"     # single UI test by name

npm run coverage       # UI + Rust coverage; both gated at 80%
                       # (Rust gate ignores main.rs/app_state.rs/tray.rs/updater.rs/debug.rs — glue)

./target/debug/burnrate debug <env|detect|load|snapshot>
                       # headless diagnostics: real provider/config code paths
                       # without the GUI (see .claude/skills/burnrate-debug)

docs-dev               # VitePress dev server for website/ (hot reload); docs-build for static output

./scripts/build-app    # npm run build + cargo build --release (embeds dist/ via default custom-protocol)
./scripts/package-dmg  # macOS .dmg + .app bundle via `tauri build` (real Dock icon; macOS only)
nix build .#burnrate   # pure Nix package build
cargo package --allow-dirty   # verify the crates.io archive (incl. bundled dist/)
nix flake check        # when Nix/devshell wiring changes
```

### Dev keychain prompts (macOS)

An unsigned dev binary gets a new code identity on every recompile, so the
keychain "Always Allow" grant is invalidated and macOS re-prompts each launch.
`scripts/dev-codesign-setup.sh` creates a persistent self-signed `burnrate-dev`
identity (one-time) and probe-signs to create the
`~/.burnrate-dev-codesign-authorized` marker; the cargo runner
(`scripts/dev-codesign-run.sh`, wired as `CARGO_TARGET_*_RUNNER` env vars by
the **devshell** — deliberately not in `.cargo/config.toml`, which ships in
the crate) then re-signs the
`burnrate` binary on every `cargo run` / `tauri dev` so the signature — and the
grant — stays stable. The runner only signs when that marker exists (if the
probe couldn't authorize the key, run `dev-codesign-setup.sh --authorize-key`
once — it may ask for the login keychain password) and only signs the
`burnrate` binary (not test binaries), so CI (Linux), the Nix build, and
`cargo install` are unaffected, and neither script is in the crate's `include`
list. Outside the devshell the runner simply isn't active.

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
  - the in-memory config model and path helpers belong in `src/config.rs`;
  - sqlite persistence and schema migrations belong in `src/storage.rs`;
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
