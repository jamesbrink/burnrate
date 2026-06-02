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
# `cargo run`. After running this once: `npm run dev`, click "Always Allow" one
# final time, and the prompts stop.
set -eu

IDENTITY="burnrate-dev"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This helper is macOS-only." >&2
  exit 0
fi

if security find-identity -v -p codesigning 2>/dev/null | grep -q "$IDENTITY"; then
  echo "Code-signing identity '$IDENTITY' already present — nothing to do."
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Creating self-signed code-signing certificate '$IDENTITY'…"
openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -days 3650 \
  -subj "/CN=${IDENTITY}" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"
openssl pkcs12 -export -out "$TMP/identity.p12" \
  -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -passout pass:

# Import the key and authorise codesign to use it. -T grants codesign access;
# set-key-partition-list is the modern requirement to use the key without a
# prompt (it may ask for your login keychain password once).
security import "$TMP/identity.p12" -k "$KEYCHAIN" -P "" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s "$KEYCHAIN" >/dev/null 2>&1 || true

echo
echo "Created '$IDENTITY'. Next:"
echo "  1. Run: npm run dev"
echo "  2. Click 'Always Allow' on the keychain prompt one last time."
echo "  3. Rebuilds now keep the grant — no more repeated prompts."
