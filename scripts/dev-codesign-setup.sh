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
READY_MARKER="${HOME}/.burnrate-dev-codesign-ready"
P12_PASSWORD="burnrate-dev-local"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This helper is macOS-only." >&2
  exit 0
fi

QUIET=0
if [ "${1:-}" = "--quiet" ]; then
  QUIET=1
fi

unlock_codesign_key() {
  # Re-apply the partition list even for existing identities. Without it,
  # codesign can ask for keychain access on every dev rebuild.
  security set-key-partition-list -S apple-tool:,apple: -s "$KEYCHAIN" >/dev/null 2>&1 || true
}

if security find-identity -v -p codesigning 2>/dev/null | grep -q "$IDENTITY"; then
  if [ -f "$READY_MARKER" ]; then
    if [ "$QUIET" -eq 0 ]; then
      echo "Code-signing identity '$IDENTITY' already present."
    fi
    exit 0
  fi
  unlock_codesign_key
  touch "$READY_MARKER" 2>/dev/null || true
  if [ "$QUIET" -eq 0 ]; then
    echo "Code-signing identity '$IDENTITY' already present and key access is prepared."
  fi
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ "$QUIET" -eq 0 ]; then
  echo "Creating self-signed code-signing certificate '$IDENTITY'…"
fi

if [ "$QUIET" -eq 1 ]; then
  openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -days 3650 \
    -subj "/CN=${IDENTITY}" \
    -addext "basicConstraints=critical,CA:FALSE" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning" >/dev/null 2>&1
else
  openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -days 3650 \
    -subj "/CN=${IDENTITY}" \
    -addext "basicConstraints=critical,CA:FALSE" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning"
fi

PKCS12_LEGACY_FLAG=""
if openssl pkcs12 -help 2>&1 | grep -q -- "-legacy"; then
  PKCS12_LEGACY_FLAG="-legacy"
fi
if [ "$QUIET" -eq 1 ]; then
  openssl pkcs12 $PKCS12_LEGACY_FLAG -export -out "$TMP/identity.p12" \
    -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -passout pass:"$P12_PASSWORD" >/dev/null 2>&1
else
  openssl pkcs12 $PKCS12_LEGACY_FLAG -export -out "$TMP/identity.p12" \
    -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
    -passout pass:"$P12_PASSWORD"
fi

# Import the key and authorise codesign to use it. -T grants codesign access;
# set-key-partition-list is the modern requirement to use the key without a
# prompt (it may ask for your login keychain password once).
security import "$TMP/identity.p12" -k "$KEYCHAIN" -P "$P12_PASSWORD" -T /usr/bin/codesign
unlock_codesign_key
touch "$READY_MARKER" 2>/dev/null || true

if [ "$QUIET" -eq 1 ]; then
  exit 0
fi

echo
echo "Created '$IDENTITY'. Next:"
echo "  1. Run: npm run dev"
echo "  2. If macOS asks once for your login keychain password, allow it."
echo "  3. Rebuilds now keep the grant — no more repeated prompts."
