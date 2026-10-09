#!/usr/bin/env bash
# Rebuilds scryfall-copy.jsonl: whole card objects off Scryfall's Default Cards
# bulk file, a few of every shape the copy splits and joins again (copy.rs).
# Pass the unzipped bulk file; run from this directory.
set -euo pipefail
bulk=$1
jq -cs '
  def shape:
    if (.name | IN("Sol Ring", "Sol Ring // Sol Ring", "Lightning Bolt",
      "Island", "Delver of Secrets // Insectile Aberration", "Fire // Ice",
      "Cut // Ribbons", "Bruna, the Fading Light",
      "Invasion of Zendikar // Awakened Skyclave", "Bonecrusher Giant // Stomp",
      "Erayo, Soratami Ascendant // Erayo'"'"'s Essence",
      "Valki, God of Lies // Tibalt, Cosmic Impostor", "Treasure"))
    then "name:\(.name)"
    elif ((.image_uris // .card_faces[0].image_uris) == null) then "no picture"
    elif .lang != "en" then "lang:\(.lang)"
    elif (.collector_number | test("-")) then "dashed number"
    elif .layout != "normal" then "layout:\(.layout)"
    elif (.finishes | index("etched")) != null then "etched"
    else empty end;
  map({shape: shape, card: .}) | group_by(.shape) | map(.[0:4][].card)[]
' "$bulk" |
  # Links to elsewhere on Scryfall, which the copy neither reads nor rebuilds.
  jq -c 'del(.all_parts, .related_uris, .purchase_uris)' > scryfall-copy.jsonl
wc -lc scryfall-copy.jsonl
