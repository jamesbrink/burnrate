#!/usr/bin/env bash
set -euo pipefail

# Build a Tauri updater manifest (`latest.json`) from the macOS universal
# bundle's `.sig` file, emitting it on stdout.
#
# Burnrate ships in-app updates for macOS only, as a single universal
# (aarch64 + x86_64) `.app.tar.gz`. The Tauri updater client looks up a
# `darwin-{arch}` key by the running architecture, so the one universal
# signature is mapped to BOTH `darwin-aarch64` and `darwin-x86_64` (plus the
# legacy `darwin-aarch64-app` / `darwin-x86_64-app` keys older clients use).
#
# Building the manifest centrally (rather than via tauri-action's per-leg
# `includeUpdaterJson`) avoids the parallel-upload race on the multi-platform
# release matrix — the Linux/Windows legs simply don't contribute updater keys.
#
# Required positional arguments:
#   $1 - directory containing the macOS `*.app.tar.gz.sig` file
#   $2 - version string (e.g. `0.1.2` or `0.2.0-dev.12.g1a2b3c4`)
#   $3 - asset URL prefix (e.g.
#        `https://github.com/jamesbrink/burnrate/releases/download/v0.1.2`)

usage() {
  echo "usage: $0 <sig-dir> <version> <url-prefix>" >&2
  exit 64
}

[ "$#" -eq 3 ] || usage
SIG_DIR="$1"
VERSION="$2"
URL_PREFIX="$3"

[ -d "$SIG_DIR" ] || {
  echo "::error::$SIG_DIR is not a directory" >&2
  exit 1
}

# Locate the single universal updater signature. Tauri names it
# `<productName>[_<version>]_universal.app.tar.gz` (or plain
# `Burnrate.app.tar.gz` on older toolchains), so glob rather than hardcode.
shopt -s nullglob
sigs=("$SIG_DIR"/*.app.tar.gz.sig)
shopt -u nullglob

if [ "${#sigs[@]}" -eq 0 ]; then
  echo "::error::no *.app.tar.gz.sig found in $SIG_DIR" >&2
  exit 1
fi
if [ "${#sigs[@]}" -gt 1 ]; then
  echo "::error::expected exactly one *.app.tar.gz.sig in $SIG_DIR, found ${#sigs[@]}: ${sigs[*]}" >&2
  exit 1
fi

sig_path="${sigs[0]}"
asset="$(basename "$sig_path" .sig)"

platforms='{}'
for key in darwin-aarch64 darwin-x86_64 darwin-aarch64-app darwin-x86_64-app; do
  platforms="$(jq \
    --arg key "$key" \
    --rawfile sig "$sig_path" \
    --arg url "$URL_PREFIX/$asset" \
    '. + {($key): {signature: $sig, url: $url}}' \
    <<<"$platforms")"
done

PUB_DATE="$(date -u +%FT%T.000Z)"

jq -n \
  --arg version "$VERSION" \
  --arg pub_date "$PUB_DATE" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: "", pub_date: $pub_date, platforms: $platforms}'
