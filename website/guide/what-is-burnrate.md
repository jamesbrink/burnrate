# What is Burnrate?

Burnrate is a desktop usage monitor for AI provider quotas, credits, spend,
and subscription limits. It lives in the system tray (the menu bar on macOS)
and keeps live meters for every account you add:

- **Claude Code** — subscription buckets (5-hour, weekly, model-specific)
  with stale-auth checks via `claude auth status`.
- **Codex** — Pro/Max plan and rate-limit buckets read from the Codex app
  server.
- **OpenRouter** — remaining prepaid credits.
- **Runpod** — prepaid balance, current burn, runway, and active resources.
- **AWS** — Cost Explorer month-to-date spend with optional budgets and
  per-service buckets.

Built with Tauri 2 — a Rust backend and a React/TypeScript UI — and shipped
as native bundles for macOS, Linux, and Windows, plus a `cargo install`-able
crate.

## How it works

- A left-click on the tray icon opens a translucent popover with usage cards
  for every enabled account; right-click offers Preferences, Refresh, and
  Quit.
- Burnrate refreshes in the background every five minutes and caches
  successful snapshots for five minutes to avoid tight polling.
- Claude Code and Codex accounts are auto-detected from local CLI config.
  Additional accounts sign in from the app via browser OAuth; OpenRouter and
  Runpod are added with an API key, and AWS uses your existing credential
  chain.
- Statuses roll up: the tray icon summarizes all accounts, flagging the one
  closest to exhaustion (warning at ≤20% remaining, exhausted at ≤5%).
- It hides from the Dock by default and appears only while Preferences is
  open.

## Next steps

- [Install Burnrate](/guide/installation)
- [Add your accounts](/guide/getting-started)
- [Provider specifics](/providers/claude-code)
