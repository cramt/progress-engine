#!/usr/bin/env bash
# Rebuilds oracle-tags.jsonl: Sol Ring's tag mana-rock, the two tags above it
# and one beside it, off Scryfall's Oracle Tags bulk file, their taggings cut
# to the cards in chip-scryfall's scryfall-copy.jsonl. Sol Ring is tagged
# mana-rock alone of the three in a line, so a copy that carries the other two
# on it has expanded a tag to its ancestors.
# Pass the unzipped bulk file; run from this directory.
set -euo pipefail
tags=$1
cards=../../../../reality-chip/scryfall/tests/fixtures/scryfall-copy.jsonl
jq -c --slurpfile ids <(jq -c '.oracle_id // empty' "$cards" | jq -sc 'unique') '
  select(.slug | IN("mana-rock", "mana-producer", "ramp", "adds-multiple-mana"))
  | .taggings |= map(select(.oracle_id | IN($ids[0][])))
' "$tags" > oracle-tags.jsonl
wc -lc oracle-tags.jsonl
