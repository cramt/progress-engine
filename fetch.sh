#!/usr/bin/env bash
# Fetch the Delver X engine, card database and free-tier model from the origin.
# Everything here is served unauthenticated at mtg.delver.app.
set -euo pipefail
cd "$(dirname "$0")"

ORIGIN="https://mtg.delver.app"
UA='Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/140 Safari/537.36'

get() {
  printf '  %-22s' "$1"
  curl -fsSL -A "$UA" "$ORIGIN/$1" -o "$1"
  printf '%s\n' "$(du -h "$1" | cut -f1)"
}

echo "engine:"
for f in core.js core.wasm; do get "$f"; done

echo "archives:"
for f in data.7z model-alpha.7z model-lambda.7z model-gamma.7z; do get "$f"; done

echo "sidecars:"
for f in data.md5 data.size version.txt model-alpha.md5 model-alpha.size model-lambda.md5 model-lambda.size model-gamma.md5 model-gamma.size; do get "$f"; done

echo "unpacking:"
command -v 7z >/dev/null || { echo "7z not found - run this inside \`nix develop\`" >&2; exit 1; }
7z x -y data.7z >/dev/null
7z x -y model-alpha.7z >/dev/null
7z x -y model-gamma.7z >/dev/null
7z x -y model-lambda.7z >/dev/null
ls -la data.db model-alpha.dat model-gamma.dat model-lambda.dat

echo
echo "version: $(cat version.txt)"
echo "run: cargo run --example query"
