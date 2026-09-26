# One directory per product, so the shared core can be extracted with one command

Crates live under `crates/{ichormoon-gauntlet,reality-chip,gitaxian-probe}/`, and each package prefix follows its directory. Extracting the shared core is then one `git filter-repo --subdirectory-filter crates/reality-chip`, with history intact (`fa53aaa`). The extraction waits for a second consumer that actually needs it. Gitaxian Probe was expected to be that consumer and needed none of it, because Delver ships its own catalogue.

`chip-scryfall` is known to hold code only Ichormoon Gauntlet uses (query parsing, legality, mana, tags). It is left there until a real consumer shows where the seam goes.

Names follow Scryfall `arttag:progress-engine`. See [NAMES_FOR_FUTURE.md](../../NAMES_FOR_FUTURE.md).
