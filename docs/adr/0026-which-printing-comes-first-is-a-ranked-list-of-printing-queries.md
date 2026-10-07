# Which printing comes first is a ranked list of printing queries in `meldweb.toml`

Meldweb Curator offers a card's printings best first, for walking a finished deck and picking one per card. What "best" means is the user's, so it is declared in the Magic repo ([ADR-0021](0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md)) in a new root file, `meldweb.toml`, and saved like everything else there:

```toml
[printings]
rank = [
  { avoid = "is:digital" },
  { avoid = "-lang:en" },
  { prefer = "t:basic is:fullart" },
  { avoid = "is:textless" },
  { avoid = "is:sourcematerial" },
  { avoid = "is:ub" },
]
```

- **A rule is a Scryfall query over one printing.** `chip-scryfall` learned the printing terms Scryfall has (`is:fullart`, `is:textless`, `is:sourcematerial`, `is:ub`, `lang:`, `frame:`, `border:`, `st:`, `game:` and the rest in `printing.rs`), each reading the field Scryfall reads. Only `parse_printing` accepts them. `parse`, which Gauntlet uses, refuses each one by name, because the index has one record per card and no printing to ask about.
- **The rules are one ordered list.** The first rule that tells two printings apart decides between them, and later rules only break ties. A `prefer` above an `avoid` beats it, and an `avoid` above a `prefer` beats that. The list above puts a Japanese full-art Forest below an English plain one, and a textless full-art Forest below a full-art Forest with text and above a plain one. Printings no rule separates go newest first, as the nearest wording to current oracle text.
- **It only orders, and a pick is the user's.** A printing every rule avoids is still offered, at the bottom, so a card printed only as Universes Beyond still has printings to pick. Each picture is labelled with the rules that moved it.
- **A missing file is the defaults**, which are written out in `meldweb-wasm`'s `preference.rs`. No repo version bump: an older Curator never reads the file. A file the format does not allow, or a rule that does not parse, is named in the grid with its rule number, and the grid falls back to newest first.
- **The ranking runs in Rust.** `meldweb-wasm` gains `chip-scryfall` and evaluates the rules against each printing's Scryfall object. The TypeScript carries the order back and decides nothing. That adds 130 KB to the wasm (37 KB gzipped).

## Considered Options

- **A fixed vocabulary of named styles to avoid** (`avoid = ["source-material", "universes-beyond"]`). Rejected: "full-art *basics* are pretty" has a scope (lands) and a direction (up), and a list of words can say neither without a new word for each sentence. That vocabulary would be a policy language of its own, which [VISION.md](../../VISION.md) is written against. Queries are the mechanism the repo already has.
- **Separate `prefer` and `avoid` lists.** Rejected: with `prefer` ranked first, a digital or foreign full-art Forest beats an English plain one, and putting `avoid` first loses the textless full-art Forest. Only one interleaved order can say both.
- **One query string with `or`.** Supported inside a rule, but not as the whole format: inside one string every avoided printing ties, and the tie goes to the newest, which for most Universes Beyond staples is a comic-panel printing.
- **Per-browser settings in localStorage.** Rejected: it is a taste that should follow the user to every device, and the repo is where Curator keeps what the user declared.
- **Encoding "everyone knows this card" so textless is fine for staples.** Not attempted. `prints<N` would approximate it, but the user's click already says it, and that pick is saved in the deck.
