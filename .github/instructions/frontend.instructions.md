---
applyTo: "src-ui/**/*.{ts,tsx}"
---

# Frontend review

React 18 + TypeScript, built with Vite, rendered inside Tauri webviews. Review
changes under `src-ui/` with these priorities.

## Tauri boundary and mock mode

- `api.ts` is the only file allowed to call `invoke()` / `listen()`. UI
  components must go through its exported functions, not Tauri APIs directly.
- Every `api.ts` function branches on `isTauri`. The non-Tauri branch must keep
  returning realistic mock data so the UI and Vitest run without the Rust
  backend. Flag a new IPC call that omits the mock fallback.
- Event names must match the backend emitters: `burnrate-refresh-requested`,
  `burnrate-dashboard-updated`, `burnrate-settings-updated`.

## Types and structure

- `types.ts` mirrors the serde wire types in `src/models.rs`: fields are
  camelCase, string-literal unions use kebab-case (e.g. `"claude-code"`,
  `"not-configured"`). Flag drift from the Rust models.
- Keep components focused. New UI sections should become their own component
  rather than growing `App.tsx`.
- Prefer strict typing; avoid `any` and non-null assertions on values that can be
  null per `types.ts` (`quota`, `subscription`, `limit`, `remaining`, `resetAt`).
- Remember the two-window split: `App.tsx` branches on the `?view=tray` query
  param to render `TrayPanel` vs. the full Preferences UI.

## Build and tests

- The compiled bundle in `dist/` is committed and CI-enforced. A change to
  `src-ui/` must be accompanied by a `dist/` rebuild — flag PRs that touch the UI
  source without it.
- Add or update Vitest tests for new behavior; UI coverage must stay at or above
  80% (branches, functions, lines, statements).
