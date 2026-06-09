#!/usr/bin/env sh
# One-time setup so local dev builds keep a *stable* code signature.
#
# Why: macOS binds a keychain "Always Allow" grant to an app's code signature.
# An unsigned dev binary (`npm run dev` / `cargo run`) gets a brand-new code
# identity on every recompile, so the keychain re-prompts for access after every
# change — maddening. A signed-with-a-persistent-identity binary keeps the same
# Designated Requirement across rebuilds, so the grant sticks.
#
# This creates a self-signed code-signing certificate named "burnrate-dev" in
# your login keychain. It is for LOCAL signing only (never distributed). Pair it
# with the cargo runner in .cargo/config.toml, which re-signs the binary on each
# `cargo run`. After running this once, rebuilds keep the same keychain grant.
set -eu

IDENTITY="burnrate-dev"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"
READY_MARKER="${HOME}/.burnrate-dev-codesign-authorized"
P12_PASSWORD="burnrate-dev-local"
AUTHORIZE_KEY=0

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This helper is macOS-only." >&2
  exit 0
fi

QUIET=0
for arg in "$@"; do
  case "$arg" in
    --quiet) QUIET=1 ;;
    --authorize-key) AUTHORIZE_KEY=1 ;;
  esac
done

unlock_codesign_key() {
  # This can require the login keychain password. Never run it from `dev`; only
  # when explicitly requested by `scripts/dev-codesign-setup.sh --authorize-key`.
  security set-key-partition-list -S apple-tool:,apple: -s "$KEYCHAIN" >/dev/null 2>&1 || true
}

authorize_via_probe() {
  # The cert import ACLs the private key for /usr/bin/codesign, which usually
  # lets signing work with no further authorization. Probe-sign a scratch
  # binary; when it succeeds, drop the marker that activates the cargo runner.
  # Without this, the runner stays a silent no-op and keychain prompts persist
  # even though setup "succeeded". (May show a one-time keychain Allow dialog.)
  PROBE="$(mktemp)" || return 0
  cp /bin/ls "$PROBE" 2>/dev/null || { rm -f "$PROBE"; return 0; }
  HASH="$(security find-certificate -a -Z -c "$IDENTITY" "$KEYCHAIN" 2>/dev/null | awk '/SHA-1/{print $NF; exit}')"
  if [ -n "$HASH" ] && codesign --force --sign "$HASH" "$PROBE" >/dev/null 2>&1; then
    touch "$READY_MARKER" 2>/dev/null || true
  fi
  rm -f "$PROBE"
}

if security find-certificate -c "$IDENTITY" "$KEYCHAIN" >/dev/null 2>&1; then
  if [ "$AUTHORIZE_KEY" -eq 1 ]; then
    unlock_codesign_key
    touch "$READY_MARKER" 2>/dev/null || true
  elif [ "$QUIET" -eq 0 ] && [ ! -f "$READY_MARKER" ]; then
    authorize_via_probe
  fi
  if [ "$QUIET" -eq 0 ]; then
    if [ -f "$READY_MARKER" ]; then
      echo "Code-signing identity '$IDENTITY' already present and authorized."
    else
      echo "Code-signing identity '$IDENTITY' present but not authorized; run with --authorize-key."
    fi
  fi
  exit 0
fi

if [ "$QUIET" -eq 1 ]; then
  # `dev` must never block on keychain password prompts. If the local identity
  # has not been created yet, skip setup and let the runner no-op.
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ "$QUIET" -eq 0 ]; then
  echo "Creating self-signed code-signing certificate '$IDENTITY'…"
fi

openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -days 3650 \
  -subj "/CN=${IDENTITY}" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"

PKCS12_LEGACY_FLAG=""
if openssl pkcs12 -help 2>&1 | grep -q -- "-legacy"; then
  PKCS12_LEGACY_FLAG="-legacy"
fi
openssl pkcs12 $PKCS12_LEGACY_FLAG -export -out "$TMP/identity.p12" \
  -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
  -passout pass:"$P12_PASSWORD"

# Import the key and restrict private-key access to codesign. The broader
# set-key-partition-list authorization can require a keychain password, so it is
# only attempted when the user explicitly requests --authorize-key.
security import "$TMP/identity.p12" -k "$KEYCHAIN" -P "$P12_PASSWORD" -T /usr/bin/codesign
if [ "$AUTHORIZE_KEY" -eq 1 ]; then
  unlock_codesign_key
  touch "$READY_MARKER" 2>/dev/null || true
else
  authorize_via_probe
fi

echo
echo "Created '$IDENTITY'. Next:"
echo "  1. Run: npm run dev"
echo "  2. dev will never ask for your keychain password."
if [ ! -f "$READY_MARKER" ]; then
  echo "  3. Signing is not yet authorized — run scripts/dev-codesign-setup.sh --authorize-key once."
fi
