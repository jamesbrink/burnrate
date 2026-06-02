#!/usr/bin/env sh
# Cargo `runner` for the macOS host targets (see .cargo/config.toml).
#
# Before launching the dev binary, re-sign it with the persistent "burnrate-dev"
# identity so its code signature stays stable across rebuilds — that keeps the
# macOS keychain "Always Allow" grant from being invalidated on every recompile.
#
# Safe everywhere: it only touches a binary literally named `burnrate` (test
# binaries have a hash suffix and are skipped), and it does nothing unless the
# one-time identity from scripts/dev-codesign-setup.sh exists — so CI and other
# machines run unaffected.
set -eu

BIN="$1"
shift

if [ "$(basename "$BIN")" = "burnrate" ] \
  && command -v codesign >/dev/null 2>&1 \
  && command -v security >/dev/null 2>&1 \
  && security find-identity -v -p codesigning 2>/dev/null | grep -q "burnrate-dev"; then
  codesign --force --sign "burnrate-dev" "$BIN" >/dev/null 2>&1 || true
fi

exec "$BIN" "$@"
