# Claude Code

Burnrate reads the same subscription windows the provider enforces: the
**5-hour** bucket, the **weekly** bucket, and model-specific buckets (e.g.
weekly Sonnet), each with remaining percentage and reset time.

## Requirements

- The `claude` CLI installed and signed in with a first-party `claude.ai`
  OAuth login.
- An active subscription — API-key (inference-only) auth has no
  subscription quota to report.

Accounts are auto-detected from `~/.claude`. Burnrate validates auth with
`claude auth status`, which also yields the account email shown in the UI.

## Multiple accounts

Additional accounts sign in from **Preferences → Add account → Sign in with
browser**. Burnrate runs `claude auth login` in an isolated per-account
config dir (`CLAUDE_CONFIG_DIR`), opens your browser, and asks you to paste
the authentication code back into the login dialog. Your default terminal
session in `~/.claude` is never touched by secondary accounts.

## Stale auth

If the login goes stale, Burnrate surfaces it on the account card. Refresh
with **Sign in again** in Preferences, or `claude auth login` in a terminal
for the default account.

## Notes

- If the `claude` binary lives somewhere unusual, set
  `BURNRATE_CLAUDE_BIN`.
- Inherited credential env vars (`CLAUDE_CODE_OAUTH_TOKEN`,
  `ANTHROPIC_API_KEY`) are stripped from spawned CLIs so they can't shadow
  the signed-in account.
