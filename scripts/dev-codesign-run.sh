#!/usr/bin/env sh
# Cargo `runner` for the macOS host targets (see .cargo/config.toml).
#
# Before launching the dev binary, re-sign it with the persistent "burnrate-dev"
# identity so its code signature stays stable across rebuilds — that keeps the
# macOS keychain "Always Allow" grant from being invalidated on every recompile.
#
# Safe everywhere: it only touches a binary literally named `burnrate` (test
# binaries have a hash suffix and are skipped), and it does nothing unless the
# local identity has been explicitly authorized. This keeps `dev` from ever
# blocking on a keychain password prompt.
set -eu

BIN="$1"
shift

READY_MARKER="${HOME}/.burnrate-dev-codesign-authorized"

if [ "$(basename "$BIN")" = "burnrate" ] \
  && [ -f "$READY_MARKER" ] \
  && command -v codesign >/dev/null 2>&1 \
  && command -v security >/dev/null 2>&1; then
  IDENTITY_HASH="$(security find-certificate -a -Z -c burnrate-dev "${HOME}/Library/Keychains/login.keychain-db" 2>/dev/null | awk '/SHA-1/{print $NF; exit}')"
  if [ -n "$IDENTITY_HASH" ]; then
    codesign --force --sign "$IDENTITY_HASH" "$BIN" >/dev/null 2>&1 || true
  fi
fi

exec "$BIN" "$@"
