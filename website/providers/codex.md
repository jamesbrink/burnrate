# Codex

Burnrate reads Codex **Pro/Max plan** info and **rate-limit buckets**
(5-hour and weekly windows, including Spark buckets where present) directly
from the Codex app server over stdio — the same data the Codex UI shows.

## Requirements

- The `codex` CLI installed and signed in.

Accounts are auto-detected from `CODEX_HOME` / `~/.codex`. The account
email comes from the local `auth.json` identity token.

## Multiple accounts

Additional accounts sign in from **Preferences → Add account → Sign in with
browser**. Burnrate runs `codex login` under an isolated per-account
`CODEX_HOME`, so secondary accounts never disturb your terminal session in
`~/.codex`.

## Notes

- If the `codex` binary lives somewhere unusual, set `BURNRATE_CODEX_BIN`.
- Inherited credential env vars (e.g. `OPENAI_API_KEY`) are stripped from
  spawned CLIs so they can't shadow the signed-in account.
