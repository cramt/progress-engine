#!/usr/bin/env bash
# The splash's art: Font of Progress (ONE), Aaron J. Riley, as Scryfall crops
# it. Kept here rather than linked because the site is cross-origin isolated
# under `require-corp`, which blocks a CSS background from a host that sends no
# Cross-Origin-Resource-Policy, and Scryfall's CDN sends none. index.html
# credits the artist, as Scryfall asks of anyone showing an art crop.
# Run to refresh: `nix shell nixpkgs#curl nixpkgs#jq -c scripts/fetch-splash.sh`
set -euo pipefail
cd "$(dirname "$0")/../public/splash"
ua=(-H 'User-Agent: progress-engine/meldweb-curator' -H 'Accept: application/json;q=0.9,*/*;q=0.8')
uri=$(curl -sf "${ua[@]}" 'https://api.scryfall.com/cards/named?exact=Font%20of%20Progress&set=one' | jq -r .image_uris.art_crop)
curl -sf "${ua[@]}" "$uri" -o font-of-progress.jpg
