# Names for future tools

progress-engine is a family. Each tool in it is named after a Magic card, and
this file is where the naming decisions and the unspent candidates live so that
the next one does not start from a blank page.

## The rule

A candidate qualifies if Scryfall's art tagger puts it in the Progress Engine:

```
https://scryfall.com/search?q=arttag%3Aprogress-engine
```

104 cards as of 2026-09-21. The tag is curated by people who looked at the art,
which makes it a better filter than "does this sound Phyrexian" — it killed four
names that sounded right and surfaced two that were better.

It is a search instrument, not a law. The real test is whether the flavour maps
to the function in one sentence you would not be embarrassed to put in a README.
A card from outside the tag can win, but it has to beat everything inside it and
say why in this file. Exactly one has (see Reality Chip).

Widen with `arttag:phyrexian` (1289) or `arttag:new-phyrexia` (587) only after
the narrow tag has come up empty, which it has not yet.

## Taken

| Name | What it is | In tag |
|---|---|---|
| **Ichormoon Gauntlet** | This repo. Criteria in, exact draw probabilities out. Binary is `gauntlet`. | yes |
| **Reality Chip** | The shared core: card data, Scryfall client, decklist parsing, the maths. Today's `pe-*` crates. | no — see below |
| **Gitaxian Probe** | Card scanning. Point a camera at cards, learn what they are. | yes |
| **Experimental Augury** | The Monte Carlo cross-checking oracle, if `gauntlet-sim` is ever extracted. | yes |

A gauntlet is a set of trials you put something through, which is what a
criteria file is. The card's own text is about planeswalkers and proliferate and
maps to nothing, so this name rests on the word rather than the mechanics —
that is allowed, but it is the weaker of the two justifications and worth
knowing when weighing a future candidate that has both.

Gitaxian Probe is free perfect information about hidden state, which is what
scanning a physical pile of cards is. That one rests on the mechanics.

### Why Reality Chip is in here without the tag

The Reality Chip is `{1}{U}` from Kamigawa: Neon Dynasty, and its type line is
Legendary Artifact Creature — Equipment Jellyfish. Nothing Phyrexian about it on
the card, and it is in no Phyrexian art tag, because the art is the Kamigawan
original.

The lore is the argument. Tameshi built it as a device that grants a connection
to planar physics. Tezzeret, working for Jin-Gitaxias — the Progress Tyrant,
whose sphere this family is named after — experimented with it on Kamigawa, and
the research that came out of it is how Jin-Gitaxias compleated Tamiyo, the
first Phyrexian planeswalker. He also built an oil-infused version to accelerate
turning darksteel into blightsteel. The object became Phyrexian; the printed
card never did.

That maps onto the role exactly. The shared core is not a Phyrexian-facing
product, it is the substrate every product attaches to, and the card is an
Equipment with reconfigure whose entire story is being the outside component
Phyrexia appropriated and rebuilt.

It is not the trigger for the invasion — that was Realmbreaker, the corrupted
World Tree. It is upstream of compleating planeswalkers, which was a
precondition. Do not overclaim this in a README.

## Banked

From the tag, with the role each would fit. None of these are commitments.

| Card | Would suit |
|---|---|
| **Phyrexian Ingester** | Bulk data ingestion — the thing that eats Scryfall's dumps |
| **Psychosis Crawler** | A crawler or scraper |
| **Meldweb Curator** | Collection and inventory tracking |
| **Malcator, Purity Overseer** | A linter or validator. There is also a card literally called **Reject Imperfection** |
| **Annex Sentry** / **Malcator's Watcher** | Monitoring, or a file watcher |
| **Unctus, Grand Metatect** | Codegen, schema generation, the thing that builds other things |
| **Tekuthal, Inquiry Dominus** | A query engine |
| **Serum Visions** | Anything whose pitch is seeing ahead rather than passing trials |
| **Mindsplice Apparatus** | — |
| **Glistener Seer** | — |
| **Vivisurgeon's Insight** | — |
| **Transplant Theorist** | — |
| **Font of Progress** | Held back deliberately: too close to the family name to spend on one tool |

### crates.io

Checked 2026-09-21. Free: `ichormoon-gauntlet`, `reality-chip`, `gitaxian-probe`,
`experimental-augury`, `serum-visions`, `meldweb`, `malcator`, `unctus`,
`tekuthal`, `glistener`, `phyrexia`, `metatect`, `ichormoon`.

Taken as bare words, so those need the full card name: `gauntlet`, `ingester`,
`curator`, `seer`, `augury`, `chip`, `praetor`, `quicksilver`, `toxic`.

`gauntlet` being taken on crates.io does not block the **binary** being called
`gauntlet` — a binary name is declared in `[[bin]]` and is not a registry
entry. It would only matter if this crate were published, and it would publish
as `ichormoon-gauntlet`.

## Crate prefixes

`gauntlet-*` is this tool. `pe-*` is the shared family layer — `pe-scryfall`,
`pe-decklist`, `pe-stats` — and the prefix is honest today because those crates
belong to progress-engine rather than to any one product. They become `chip-*`
when Reality Chip is extracted into its own repo, and not before: renaming them
now would claim a split that has not happened.

Two things are queued behind that extraction and neither has been done:

1. `pe-scryfall` is really two crates. The Scryfall client — bulk download, the
   `POST /cards/collection` batching, the `Skipped`/`Anomaly` discipline — is
   shared. The oracle index, the search-syntax parser, `legality.rs`, `mana.rs`
   and `tags.rs` are Ichormoon Gauntlet's and a scanner wants none of them.
2. The index is oracle-level: `Index.cards` is keyed by name with one `set`
   field per card. Card scanning needs printing-level identity —
   `collector_number`, `illustration_id`, `image_uris` — which is a different
   bulk file and roughly fifteen times the rows. Gitaxian Probe cannot reuse
   this index, only the client underneath it.

Do the extraction when there is a second consumer. One consumer is not enough
evidence about where the seam goes.
