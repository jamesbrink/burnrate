# Configuration

## Where settings live

Only non-secret account configuration is stored on disk, in a SQLite
database. On macOS the default path is:

```
~/Library/Application Support/burnrate/burnrate.sqlite
```

Set `BURNRATE_CONFIG_DIR` to override the directory. Legacy `accounts.json`
files from older versions are imported once, then renamed to
`accounts.json.migrated-<timestamp>`.

## Secrets

Manual secrets (API keys, tokens) are stored in the **OS keyring** by
default. Plaintext storage in the config database is available per account,
but only when explicitly selected.

::: warning macOS keychain and code signatures
The keychain binds its "Always Allow" grant to the app's code signature.
Signed release builds remember the grant; unsigned local builds re-prompt on
every launch. See
[Troubleshooting](/guide/troubleshooting#keychain-re-prompts-on-every-launch).
:::

## Refresh cadence

Burnrate refreshes accounts every five minutes in the background and caches
successful snapshots for five minutes. AWS Cost Explorer snapshots are cached
for 24 hours instead—even when you click manual refresh—because its data is
delayed and every paginated API request is billable.

## Environment variables

| Variable                             | Purpose                                                      |
| ------------------------------------ | ------------------------------------------------------------ |
| `BURNRATE_CONFIG_DIR`                | Override the config directory.                               |
| `BURNRATE_CLAUDE_BIN` / `CLAUDE_BIN` | Path to the `claude` CLI if it isn't in a standard location. |
| `BURNRATE_CODEX_BIN` / `CODEX_BIN`   | Path to the `codex` CLI if it isn't in a standard location.  |
| `BURNRATE_RUNPOD_REST_URL`           | Override the Runpod REST endpoint (development/proxies).     |
| `BURNRATE_RUNPOD_GRAPHQL_URL`        | Override the Runpod GraphQL endpoint (development/proxies).  |

Burnrate strips inherited credential environment variables
(`CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …) from
the provider CLIs it spawns, so an agent token in your shell can't shadow
the account that's actually signed in.
