#!/usr/bin/env bash
# Rebuilds printings.jsonl: every printing of a few cards, as Scryfall's search
# answers, trimmed to the fields chip-scryfall reads. Run from this directory.
set -euo pipefail
fields='{name, lang, layout, type_line, oracle_text, mana_cost, cmc, colors,
  color_identity, produced_mana, keywords, rarity, set, set_type,
  collector_number, released_at, frame, frame_effects, border_color, full_art,
  textless, digital, promo, reprint, oversized, promo_types, games,
  flavor_name, card_faces} | with_entries(select(.value != null))'
for q in '!"Heroic Intervention"' '!"Counterspell"' '!"Forest" (is:fullart or is:textless or s:m21)'; do
  url="https://api.scryfall.com/cards/search?unique=prints&include_extras=true&order=released&q=$(jq -rn --arg q "$q" '$q|@uri')"
  while [ -n "$url" ]; do
    page=$(curl -sf -H 'User-Agent: progress-engine-fixtures/0' -H 'Accept: application/json' "$url")
    jq -c ".data[] | $fields" <<<"$page"
    url=$(jq -r 'if .has_more then .next_page else "" end' <<<"$page")
    sleep 0.2
  done
done > printings.jsonl
