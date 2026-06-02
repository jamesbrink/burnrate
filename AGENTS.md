# Repository Guidance

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
