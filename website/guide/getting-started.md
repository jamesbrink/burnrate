# Getting Started

## First launch

On first launch Burnrate auto-detects existing **Claude Code** and **Codex**
accounts from their local CLI config (`~/.claude`, `~/.codex` /
`CODEX_HOME`). If you already use either CLI, your account shows up with its
email and usage — nothing to configure.

The tray icon appears in the menu bar; left-click for the usage popover,
right-click for Preferences, Refresh, and Quit.

## Adding accounts

Open **Preferences** (tray right-click, or the gear in the popover header)
and use **Add account**:

- **Sign in with browser** (Claude Code, Codex) — Burnrate shells out to the
  official `claude` / `codex` CLI, opens your browser, and reads back the
  resulting email and usage. For Claude Code, copy the authentication code
  shown in the browser at the end and paste it into the login dialog.
- **Manual token** (Claude Code, Codex) — paste an existing OAuth token
  instead of signing in.
- **API key** (OpenRouter, Runpod) — paste the provider API key.
- **AWS** — pick a credential profile (or leave blank for the default
  chain). See the [AWS provider page](/providers/aws).

### Multiple accounts

Extra Claude Code and Codex accounts are isolated in their own CLI config
dirs under `<config-dir>/cli/<provider>/<id>`, so your first auto-detected
account keeps using the shared terminal session (`~/.claude`, `~/.codex`)
while additional accounts stay separate. Signing in the same email twice
refreshes the existing account instead of creating a duplicate.

### Re-authenticating

**Sign in again** (in an account's edit panel) re-authenticates that account
in place. For the auto-detected account this refreshes your real `~/.claude`
/ `~/.codex` session — it doubles as the fix for a stale or expired default
login.

### Removing accounts

**Sign out** (or removing the account) clears only that managed account —
running the CLI sign-out and deleting its isolated dir. It never touches
your system session.

## Reordering

Drag account cards to reorder them — in the tray popover or the Preferences
list. The order persists and is shared by both windows.
