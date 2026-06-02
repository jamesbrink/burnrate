---
applyTo: "src/**/*.rs"
---

# Rust backend review

Rust 2024 edition, Tauri 2. Review changes under `src/` with these priorities.

## Robustness

- Provider `fetch()` paths must never panic on external input. Map errors to an
  error snapshot (`error_snapshot`) instead. Avoid `.unwrap()`/`.expect()` on
  parsed JSON, HTTP responses, timestamps, or env vars; reserve them for lock
  acquisition and documented invariants.
- Parse provider JSON defensively through the shared helpers in
  `providers/mod.rs` (`number`, `text`, `bool_value`, `datetime`, the bucket and
  subscription parsers). Prefer extending these over ad-hoc per-provider parsing.
- Normalize provider quirks explicitly (e.g. Codex reset timestamps may arrive in
  seconds or milliseconds; reset values can be string or number).

## Security and persistence

- Never log, serialize into `accounts.json`, or embed in an error/snapshot any
  secret. Keyring is the default; plaintext is opt-in per account only.
- Keep config writes atomic (temp file + rename) and preserve `0600` file /
  `0700` dir permissions on Unix. Malformed config must be recovered, not
  silently overwritten.
- Endpoint validation must remain HTTPS-only except for localhost.

## Concurrency and caching

- `dashboard()` fans out per-account fetches concurrently; keep result ordering
  stable and isolate one account's failure from the others.
- Preserve the 5-minute success cache (and Claude's usage cache + error backoff).
  Flag changes that would cause tight polling of provider APIs.

## Tests

- Add or update tests for new parsing, config, or key-store behavior. Use
  `wiremock` for HTTP and `tempfile` for filesystem fixtures, following existing
  `#[cfg(test)]` modules.
- `main.rs`, `app_state.rs`, and `tray.rs` are excluded from the Rust line-coverage
  gate, but other files must keep coverage at or above 80%.
