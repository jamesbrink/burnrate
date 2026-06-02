# Copilot review instructions

Burnrate is a Tauri 2 desktop app that monitors quota/credits across Claude
Code, Codex, and OpenRouter. The backend is Rust (`src/`); the frontend is
React + TypeScript (`src-ui/`). See `AGENTS.md` for the full architecture map.

When reviewing a pull request, prioritize the following.

## Secrets and storage

- Flag any secret (API key, OAuth token, credential) written to a tracked file,
  log, error message, or snapshot. Secrets belong in the OS keyring (default) or,
  only when explicitly selected, an account's plaintext field.
- `accounts.json` must contain non-secret config only. `AccountView` must never
  expose a raw secret — it reports `hasSecret` instead.
- Config writes must stay atomic (temp file + rename) and keep `0600` file /
  `0700` dir permissions on Unix.

## Network and providers

- Provider HTTP calls must time out and map failures to an error snapshot rather
  than panicking. Reserve `.expect()`/`.unwrap()` for locks and genuine
  invariants, never for parsed JSON or network responses.
- Endpoint overrides must stay HTTPS-only except for localhost. New outbound
  hosts also require a matching `connect-src` entry in the `tauri.conf.json` CSP
  — flag a new host that is not allow-listed.
- Keep the 5-minute success cache semantics intact; do not introduce tight
  polling loops.

## Boundaries and conventions

- Respect module ownership: provider fetch/parse in `src/providers/`, persistent
  config in `src/config.rs`, secrets in `src/key_store.rs`, tray in `src/tray.rs`,
  IPC bridging in `src-ui/api.ts`. Flag logic that leaks across these.
- `src-ui/types.ts` mirrors the serde wire types in `src/models.rs` (structs
  camelCase, enums kebab-case). Flag a change to one that is not reflected in the
  other.
- The built frontend in `dist/` is committed and CI-enforced
  (`git diff --exit-code -- dist`). Flag a PR that changes `src-ui/` without a
  corresponding `dist/` rebuild.
- Keep version and description in sync across `Cargo.toml`, `package.json`, and
  `tauri.conf.json` when any of them changes.

## Tests and style

- Behavioral changes should add or update focused tests. Provider parsing,
  config migration, key storage, and tray summarization are the high-risk areas.
- Use Conventional Commit style (`feat(scope):`, `fix(scope):`, `docs(scope):`).
- Prefer small, direct changes over new abstractions.
