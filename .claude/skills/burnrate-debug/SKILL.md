---
name: burnrate-debug
description: Headless debugging of Burnrate's provider auth, detection, and fetch paths via the hidden `burnrate debug` CLI — use when diagnosing detection failures, auth/subscription errors, env pollution, or keychain issues without driving the GUI.
---

# Burnrate Headless Debugging

The `burnrate` binary has a hidden diagnostics mode that runs the real
provider/config code paths without launching the Tauri GUI:

```sh
cargo build                        # or: nix develop -c cargo build
./target/debug/burnrate debug env       # env overrides + resolved provider CLIs
./target/debug/burnrate debug detect    # provider auto-detection (read-only)
./target/debug/burnrate debug load      # full startup load: detect + merge + GC (persists!)
./target/debug/burnrate debug snapshot  # load, then fetch usage for every enabled account
```

All output is JSON on stdout; diagnostics go to stderr. Exit 0 on success.

## Sandboxing

`load` and `snapshot` mutate the config store. To avoid touching the real one
(`~/Library/Application Support/burnrate`), sandbox with:

```sh
BURNRATE_CONFIG_DIR=$(mktemp -d) ./target/debug/burnrate debug load
```

Provider credentials are still the machine-global ones (`~/.claude`,
`~/.codex`, macOS Keychain) — only Burnrate's own account DB is sandboxed.

## Simulating environment pollution

AI-agent terminals (cmux, Claude Code sessions) export credential env vars
(`CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …) into
their surfaces. Spawned provider CLIs must never see these — with such a token
set, `claude auth status --json` reports `loggedIn: true, authMethod:
"oauth_token"` with **no subscriptionType/email**, which breaks sign-in verify,
usage fetch, and detection validation. Burnrate strips
`providers::CREDENTIAL_ENV_OVERRIDES` from every CLI spawn. Verify with:

```sh
CLAUDE_CODE_OAUTH_TOKEN=fake ./target/debug/burnrate debug env
CLAUDE_CODE_OAUTH_TOKEN=fake BURNRATE_CONFIG_DIR=$(mktemp -d) \
  ./target/debug/burnrate debug snapshot   # claude must still show its subscription
```

## Other debugging levers

- `BURNRATE_CLAUDE_BIN` / `BURNRATE_CODEX_BIN` — override CLI binary resolution
  (the app searches PATH then Homebrew/Nix/Cargo/JS-toolchain dirs; an old
  `/opt/homebrew/bin/claude` may not support `auth status --json`).
- App DB lives at `~/Library/Application Support/burnrate/burnrate.sqlite`;
  inspect with `sqlite3 <db> "SELECT id, provider, email, config_dir FROM accounts;"`.
- Managed per-account CLI homes: `~/Library/Application Support/burnrate/cli/<provider>/<id>`.
- Claude Keychain items: `Claude Code-credentials` (system default, never
  Burnrate's to touch) and `Claude Code-credentials-<sha256(dir)[..8]>` per
  managed dir. List: `security dump-keychain | grep "Claude Code"`.
- Keychain prompts in dev: the cargo runner re-signs the dev binary with the
  `burnrate-dev` identity, but only when `~/.burnrate-dev-codesign-authorized`
  exists — `./scripts/dev-codesign-setup.sh` probes and creates it; the
  `--authorize-key` flag is the password-prompting fallback.
- A GUI dev app's exact environment: `ps eww <pid>` of the running
  `target/debug/burnrate` process shows what it inherited from its surface.
