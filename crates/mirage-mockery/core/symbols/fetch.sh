#!/usr/bin/env bash
# Refresh this directory from Scryfall's symbology: every symbol with an SVG,
# named by what goes between its braces with `/` as `_` ({U/P} is U_P.svg).
# Run when a set adds a symbol: `nix shell nixpkgs#curl nixpkgs#jq -c ./fetch.sh`
set -euo pipefail
cd "$(dirname "$0")"
ua=(-H 'User-Agent: progress-engine/mirage-mockery' -H 'Accept: application/json;q=0.9,*/*;q=0.8')
curl -sf "${ua[@]}" https://api.scryfall.com/symbology |
  jq -r '.data[] | select(.svg_uri != null) | [.symbol, .svg_uri] | @tsv' |
  while IFS=$'\t' read -r symbol uri; do
    code=${symbol#\{}; code=${code%\}}
    curl -sf "${ua[@]}" "$uri" -o "${code//\//_}.svg"
    sleep 0.08 # Scryfall asks for 50–100 ms between requests
  done
