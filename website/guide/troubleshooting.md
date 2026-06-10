# Troubleshooting

## `cargo install` fails: `ld: library not found for -liconv`

**macOS.** This happens when something other than Apple's compiler is first
in `PATH` as `cc` (a Homebrew or Nix GCC — the telltale is `collect2:` in
the error output). rustc links through plain `cc`, and a non-Apple driver
doesn't search the macOS SDK's stub libraries, where `libiconv` has lived
since Big Sur.

Burnrate **0.1.8+** ships a packaged cargo config that pins Apple's
`/usr/bin/cc` as the linker for macOS targets, so plain `cargo install
burnrate` works regardless of `PATH`. On older versions, work around it
with:

```sh
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/cc cargo install burnrate
```

(use `CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER` on Intel Macs).

## Keychain re-prompts on every launch

**macOS.** The keychain binds an "Always Allow" grant to the requesting
app's _code signature_. An unsigned binary (a bare `cargo build` /
`cargo install` build, or an unsigned `.app`) presents a different identity
each build, so the grant never sticks.

Fixes:

- Install a **code-signed** build — the released `.dmg` is signed and
  notarized, so the grant persists.
- Building yourself? Sign it:
  `APPLE_SIGNING_IDENTITY="Developer ID Application: …" ./scripts/package-dmg`
  (run `security find-identity -v -p codesigning` to list identities).
- Or switch the account to **plaintext** secret storage in Preferences to
  skip the keychain entirely.

## Provider CLI "failed to start" in a `.app` install

Apps launched from Finder/Launchpad inherit a minimal `PATH`. Burnrate
searches the common install locations (Homebrew, Nix, Cargo, npm/pnpm/bun
dirs) for `codex` / `claude`; if yours lives elsewhere, point at it:

```sh
BURNRATE_CLAUDE_BIN=/path/to/claude
BURNRATE_CODEX_BIN=/path/to/codex
```

## Claude Code shows a stale or missing subscription

Claude Code usage requires a first-party `claude.ai` OAuth login with an
active subscription. If Burnrate reports a stale, inference-only, or
missing-subscription state, refresh the login:

```sh
claude auth login
```

Or use **Sign in again** on the account in Preferences — for the
auto-detected account it refreshes your real `~/.claude` session.

## Headless diagnostics

The binary ships a hidden debug CLI that exercises the real detection,
config, and fetch code paths without the GUI:

```sh
burnrate debug env       # credential-override env vars + resolved provider CLIs
burnrate debug detect    # run provider auto-detection (read-only)
burnrate debug load      # full startup load: detect + merge + orphan GC
burnrate debug snapshot  # fetch a usage snapshot for every enabled account
```

Each subcommand prints JSON, so it's easy to see exactly what the app would
detect and fetch from your machine.
