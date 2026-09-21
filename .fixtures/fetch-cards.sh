#!/usr/bin/env bash
# Pull a handful of reference scans from Scryfall and frame each one the way a
# camera would see it: whole card, contrasting background, room around the edges.
set -euo pipefail
cd "$(dirname "$0")"
UA='delver-probe/0.1'
command -v magick >/dev/null || { echo "ImageMagick not found - run this inside \`nix develop\`" >&2; exit 1; }
while IFS='|' read -r slug name set; do
  [ -z "$slug" ] && continue
  url=$(curl -fsSL -H "User-Agent: $UA" \
    "https://api.scryfall.com/cards/named?exact=$(printf %s "$name" | sed 's/ /+/g')&set=$set" \
    | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>console.log(JSON.parse(d).image_uris.large))')
  curl -fsSL -H "User-Agent: $UA" "$url" -o "$slug.jpg"
  magick "$slug.jpg" -resize 55% -background '#2b2b30' \
    -gravity center -extent 1280x960 "$slug-frame.jpg"
  sleep 0.1
done <<'CARDS'
lotus|Black Lotus|lea
counterspell|Counterspell|lea
shock|Shock|m21
llanowar|Llanowar Elves|m19
swords|Swords to Plowshares|lea
thoughtseize|Thoughtseize|ths
CARDS
ls -1 *-frame.jpg
