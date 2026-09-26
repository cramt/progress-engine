# Which of the suspect numbers are wrong? (#68)

Research note for [#68](https://github.com/cramt/progress-engine/issues/68).
It checks each suspect against the code, `decks/index.jsonl`, Scryfall oracle
text and the Comprehensive Rules (CR). Where a size is given, it was
**measured**, not estimated.

## How the numbers here were produced

`nix` was not available, so the CLI was built in a scratch copy of the tree.
That copy had the `[patch.crates-io] deno_core` block and the
`crates/gitaxian-probe` members removed. Nothing in the Gauntlet crates depends
on either, and the tracked sources were otherwise unchanged. That build
reproduces the committed figures exactly: north star 46.64% ± 0.11 on the play
and 53.64% ± 0.11 on the draw, route 1 11.04% / 12.08%, Loam 9.56%, and so on.

Each fix was then approximated without touching the engine:

- **Index variants.** A copy of `decks/index.jsonl` with one card's `produces`
  rewritten. This is what a corrected palette would do.
- **One throwaway engine hack**, behind an env var. In `mana_source`
  (`cli/src/library.rs:571`), a land whose `produces` is empty became
  `ManaSource::Spell`. This stands in for "a land that makes no mana is not a
  mana source". Under the undeclared (most-generous) land-drop reading it gives
  the same `can_cast` answer as a proper fix.
- **Criteria variants.** The north star on its own (`lantern.criteria.toml`
  lines 94–215), with branches split or a `[land_drop]` added.

The north star is sampled, so every comparison below reran it with
`--trials 20000000`, giving a standard error of ±0.01. At that precision the
unmodified file reads **46.73%** on the play and **53.54%** on the draw. The
committed 200k-hand figures are 46.64 ± 0.11 and 53.64 ± 0.11, so each is
within one standard error of it. The **deltas** below are taken against the
20M baseline. The Loam figures and the Lantern route-1 figures are exact
enumerations.

## Verdicts

| # | Suspect | Verdict | Direction on the north star (play / draw) |
|---|---|---|---|
| 1 | Fetchlands make no mana | **Premise wrong. Real bug of a different shape** | They already pay generic. Not paying colour costs up to **−2.4 / −2.5** |
| 1b | *(found)* Maze of Ith pays generic | **Confirmed** | overcount **+0.8 / +0.8** |
| 2a | Castle Doom counts as blue | **Confirmed** | overcount **+0.7 / +0.8** |
| 2b | Spire of Industry colour needs an artifact | **Confirmed** | overcount, at most **+0.7 / +0.8** |
| 2c | Exotic Orchard depends on opponents | **Confirmed**, Loam only | overcount, at most **+0.2** on the Loam cast |
| 3 | Bounce lands count one | **Confirmed as a gap**. Direction is mixed, and it is negligible on current questions | at most ±0.25 on the Loam cast. Not in Lantern |
| 4 | Hindsight in the 12-branch union | **Not a bug in `any_of`.** The hindsight is per clause, and it is documented | at most **+0.8 / +1.25** of clairvoyance, not named in the file |
| 5 | Artificer's Intuition discard unchecked | **Confirmed** (and already disclosed in the file) | overcount **+0.1 to +0.2 / +0.1 to +0.15** |
| 6 | Stale text | **Confirmed**, plus more than the ticket lists | none |
| 7 | *(found)* Urza's Saga still taps after it is sacrificed | **Confirmed** | overcount, at most **+0.13 / +0.10** |

If items 1, 1b, 2a and 2b are all corrected together, the north star moves
**up**, to about 47.2% / 54.2% (+0.5 / +0.7). That assumes the pessimistic
reading of Spire and the optimistic fetch palette described below. The
overcounts and the fetchland undercount roughly cancel. The 75% target is
missed by the same ~28 / ~21 points whichever way they are resolved.

---

## 1. Fetchlands: they are mana sources, for generic only

**The ticket's premise is wrong.** A fetchland with no `produces` is not a land
drop that pays for nothing. It pays **one generic**, exactly like a basic land.
It never pays a coloured pip.

Why:

- `mana_source` makes every card that passes `is_land` into a
  `ManaSource::Land`, whatever its palette (`cli/src/library.rs:571-584`).
  Scalding Tarn therefore becomes `Land { enters_tapped: false, produces:
  EMPTY }`. It carries only the `fetchland` and `tutor` tags, not `tapland`.
- `Board::new` puts **every** land group into the pool, with no filter on the
  palette (`criteria/src/effect.rs:460-470`).
- `Demand::covers` counts every source in the pool toward the total
  (`criteria/src/mana.rs:600-606`). Hall's condition is checked only over the
  pip kinds the cost demands, so an empty-palette land is a valid generic
  payer.
- It has to work this way. `LandDetail::Pips` narrows every palette to the
  pips a cost demands (`mana.rs:193-238`). For `{1}` that is none, so every
  land in the deck has an empty palette. Route 1's enumeration in the JSON
  output says so: `"pips": []`. If an empty palette paid nothing, `{1}` could
  never be paid.

The ticket's other half checks out. With a fetch effect declared, any mana
question beside it is refused (`cli/src/prepare.rs:583-624`, README l.520).

**What the oracle text says.** Scalding Tarn reads "{T}, Pay 1 life,
Sacrifice this land: Search your library for an Island or Mountain card, put it
onto the battlefield". The found land enters **untapped**, because the text
does not say "tapped", so it can be tapped the same turn. Scryfall's
`produced_mana` leaves the fetchland empty (index `produces` absent), and the
index builder copies that field verbatim (`reality-chip/scryfall/src/bulk.rs:261`).

In `lantern.txt`, every fetch can find an **untapped blue** source:

| Fetchland | Untapped blue land it can find |
|---|---|
| Tarn | Island, Volcanic Island |
| Misty Rainforest | Island, Tropical Island |
| Wooded Foothills (Mountain or Forest) | Tropical Island, Volcanic Island |
| Prismatic Vista | Island |

In `loam.txt`, Misty, Foothills and Vista find an untapped Forest.

**Size.** Two measurements bracket the error:

- **Fetches as coloured sources.** Each fetch was given the palette of the
  untapped lands it can find in that deck (Lantern: Tarn UR, Misty GU,
  Foothills RGU, Vista UGR). This is an upper bound: it ignores running out of
  targets, which is rare with 2–4 targets each.

  | Question | Before | After | Change |
  |---|---|---|---|
  | Lantern north star, play | 46.73% | 49.10% | +2.37 |
  | Lantern north star, draw | 53.54% | 56.06% | +2.52 |
  | Lantern route 2 (200k) | 27.15% | 29.23% | |
  | Lantern route 3 (200k) | 4.98% | 6.39% | |
  | Loam cast, exact | 9.5603% | 10.0702% | +0.51 |
  | Loam `{1}{G}` control, exact | 87.81% | 91.91% | +4.1 |
  | Loam turn-3 access, exact | 24.96% | 26.72% | +1.8 |

- **Fetches paying nothing**, which is the ticket's reading. This would be
  **−4.11 / −3.98** on the north star (42.62% / 49.56%). So "fetchlands make
  no mana" would have been a large regression if someone had "fixed" it
  literally.

**Proposed fix.** Give a fetchland the palette of what it can fetch, worked out
where the deck and the card data meet. In `mana_source`, for an
`otag:fetchland` land whose oracle text puts the land onto the battlefield
without "tapped":

- The palette is the union of `produces` over the deck's lands that match the
  searched-for types and are not taplands.
- `enters_tapped` is false.
- For a Terramorphic-style fetch ("onto the battlefield tapped"), the palette
  is that of the basics it finds, with `enters_tapped: true`.

Name the assumption in the run output, the way conditional taplands are named.
That would also let the `ManaBesideAFetchedLand` refusal narrow to the
"tapped" kind. Teach `gauntlet-sim` the same thing. It shares `Board`
(`sim/src/lib.rs:127-131`), so the palette change reaches both engines through
`library.rs`.

## 1b. Maze of Ith pays generic, and makes no mana

This was found while checking item 1, and it is the same mechanism with the
opposite sign. Maze of Ith's only ability is "{T}: Untap target attacking
creature…". It makes no mana, and its index `produces` is absent. The engine
still counts it as a generic payer, for the reasons in item 1.

**Size.** Measured with the engine hack applied on top of the fetch-palette
index, so that only Maze is affected:

| Question | Before | After | Change |
|---|---|---|---|
| North star, play | 49.10% | 48.26% | **−0.84** |
| North star, draw | 56.06% | 55.26% | **−0.80** |
| Route 1, play, exact | 11.044% | 11.031% | |
| Route 1, draw, exact | 12.080% | 12.071% | |
| Route 1 control, play, exact | 99.625% | 99.545% | |

`loam.txt` has no other empty-palette land.

**Proposed fix.** A land that makes no mana must not be in the pool. It cannot
be expressed as a palette, because `{1}` narrows every palette to empty. Add a
bit such as `ManaSource::Land { makes_mana: bool, .. }` that `seen_as`
preserves, and leave such groups out of `pool` in `Board::new`. They still
count as land drops. **Ship it together with fix 1**, or every fetchland
becomes a non-source and the north star drops ~4 points.

## 2. Lands that read as more flexible than they are

The cause is shared. Scryfall's `produced_mana` is every colour a card *could*
produce, with conditions and spending restrictions ignored. `Palette::from_letters`
(`mana.rs:103`) and `mana_source` take it at face value.

### 2a. Castle Doom

Oracle text: "{T}: Add {C}. {T}: Add one mana of any color. Spend this mana
only to cast an artifact spell." The index has `produces: [B,C,G,R,U,W]`.

Under CR 106.6, restricted mana can be spent only as the restriction says. Two
sets of cards are affected:

- **Spells that are not artifacts:** Trinket Mage (a creature), Fabricate (a
  sorcery), both Tezzerets (planeswalkers), Artificer's Intuition (an
  enchantment) and Whir of Invention (an instant). Castle Doom's colour cannot
  pay any of them.
- **Activations, which are not casts:** Dizzy Spell's transmute (CR 702.53)
  and Intuition's own activation.

The one artifact spell in the question is Lantern itself (`{1}`), which any
land pays. So for every branch in the file, Castle Doom is a `{C}` land.

**Size (Doom as `{C}` only):** **−0.73 / −0.79** on the north star. Route 2
falls from 27.15% to 26.77%, and route 3 from 4.98% to 4.62% (200k samples).

### 2b. Spire of Industry

Oracle text: "{T}, Pay 1 life: Add one mana of any color. Activate only if you
control an artifact." The deck has four artifact lands: Darksteel Citadel,
Seat of the Synod, Silverbluff Bridge and Slagwoods Bridge. Its seven mana
rocks are not modelled at all. So Spire's colour is real on some paths and not
on others.

**Size:** with the pessimistic `{C}` reading, **−0.72 / −0.79**. The true
overcount lies between 0 and that. Doom and Spire together, both as `{C}`:
**−1.52 / −1.65**.

### 2c. Exotic Orchard (`loam.txt` only)

Oracle text: "Add one mana of any color that a land an opponent controls could
produce." Under CR 106.7, if no such land exists, no mana is produced at all.
That is always the case on turn 1 on the play. The index has `produces:
[B,G,R,U,W]`.

**Size:** read as a colourless-in-effect land (empty palette, so generic only),
the Loam figures move as follows. The true overcount is smaller, since in a pod
of four someone usually has a green source by turn 3.

| Loam question (exact) | Before | After |
|---|---|---|
| Loam cast | 9.5603% | 9.3442% |
| `{1}{G}` control | 87.81% | 86.04% |
| Turn-3 access | 24.96% | 24.25% |

### Proposed fix for 2a–2c

Treat these the way HANDS.md hand 8 treats shocklands. Read the
**unconditional** palette, which is `{C}` for Doom and Spire and nothing for
Orchard. Detect a conditional palette from the oracle text: "Spend this mana
only", "Activate only if", or "could produce". List every card the run made
that assumption about. Making Spire declarable, or pricing it by the artifact
lands actually in play, is a later refinement.

Command Tower (Loam) is also listed with `[B,G,R,U,W]`, but Borborygmos and
Fblthp is Temur and every cost asked is inside the colour identity. **Not a
bug for current questions.**

## 3. Bounce lands (Izzet Boilerworks, Simic Growth Chamber)

Both are in `loam.txt` only. The oracle text reads "This land enters tapped.
When this land enters, return a land you control to its owner's hand. {T}: Add
{U}{R}." The model treats each as one tapped dual (`tapland`, `produces:
[R,U]` or `[G,U]`).

What really happens, worked through:

- **With another land already down,** you tap that land in response to the
  trigger, which is still one mana that turn. Then you replay it with the next
  land drop.
- **With plenty of lands,** the lands-in-play count lags by one for a turn, and
  the karoo's second mana makes up for it. The model and reality agree on
  2-mana costs.
- **With few lands,** a karoo gives one mana more than the model from the turn
  after (undercount). It only matters for costs of 3 or more.
- **As the first land,** the karoo must return itself (CR 603 trigger, "a land
  you control"), so it never sticks. The model counts it as a source from the
  next turn (overcount). The generous line avoids this whenever another land
  is held.

**Verdict.** The gap is real. Its direction depends on the cost, and for the
two `{1}{G}` questions it asks today it is close to zero.

**Bound:** removing both karoos from the pool entirely moves the Loam cast only
from 9.48% to 9.23% (−0.25), so the actual error is a fraction of that.

**Proposed fix:** at minimum, name karoos in the run output as an assumption.
Properly, it needs "adds two" plus "costs a land drop", which is a
`ManaSource` change and should be answered by a HANDS.md hand first.

## 4. Hindsight in the Lantern union

**The `any_of` union does not overstate anything.** On each path, a branch
holds when *some* line satisfies it, and the criterion holds when some branch
does. "Some branch has some line" is exactly "some line satisfies some
branch". Every branch is a complete line on its own, so letting branch A
assume the Saga went down on turn 1 while branch B assumes an untapped blue
source did is not a contradiction. Each path is counted once. The union is
therefore exact for the question it states, given each branch.

**The hindsight lives inside each clause.** It is documented behaviour:

- With no `[land_drop]`, `played_by` (`effect.rs:1221-1266`) and `can_cast` /
  `can_pay` (`effect.rs:1275-1390`) each take "the most generous" reading.
  That means any set of drawn lands, played in the best order, with the whole
  path in view. In other words, a pilot who knows what they will draw.
- The file's "WHAT IS STILL NOT MODELLED" list (l.40-60) does not mention it.
- None of the twelve branches mixes a Saga battlefield clause with a
  `can_cast`, which is the case `played_by`'s doc warns about.
- The multi-turn branches, `can_cast` on turn 4 and then `{1}` on turn 5, are
  jointly realisable. Lands only accumulate, and all of them are untapped by
  the next turn.

**Size, as an upper bound.** The north star was rerun with a declared
`[land_drop]` (no Saga effect, 5M trials) under four fixed priorities:

| Priority | Play | Draw |
|---|---|---|
| Saga, then `t:land` | 44.56 | 50.49 |
| Taplands first | 45.00 | 51.03 |
| Untapped blue first | 44.24 | 50.09 |
| Saga, then taplands, then the rest | **45.93** | **52.29** |
| Undeclared (clairvoyant) | 46.73 | 53.54 |

A fixed priority is weaker than an adaptive human, so clairvoyance is worth
**at most 0.8 / 1.25 points**.

**Proposed fix:** no code change. Add the clairvoyant land sequencing to the
file's not-modelled list, since it is the one entry there that pushes the
number **up**. Optionally, quote the best-declared-priority figure beside the
union.

## 5. Artificer's Intuition's discard

Oracle text: "{U}, Discard an artifact card: Search your library for an
artifact card with mana value 1 or less…". The discard is a cost, and under
CR 118.3 a cost cannot be paid without the resources. The file already says it
is not checked (`lantern.criteria.toml` l.87-88 and l.160-163).

**Size:** the Dizzy/Intuition branches were split, and the Intuition branches
given an extra clause requiring an artifact card in hand. All runs used 20M
trials.

| Intuition branches | Play | Draw |
|---|---|---|
| Split, no check | 46.73 | 53.55 |
| Plus `t:artifact` (artifact lands are legal discards) | 46.64 | 53.47 |
| Plus `t:artifact -t:land` | 46.54 | 53.41 |

The overcount is **0.09–0.19 on the play** and **0.08–0.14 on the draw**. The
right answer sits between the two rows: an artifact land in hand is a legal
discard, but only if it was not also needed as a land drop.

**Proposed fix:** split the branch as above with the `t:artifact` clause, and
keep the comment about the land-drop conflict.

## 6. Stale text

| File | Line | Says | Should say |
|---|---|---|---|
| `VISION.md` | 44 | "whichever of **three** routes", with a three-row table | four routes: add the battlefield tutors, as the file has them |
| `VISION.md` | 61 | "the three routes are writable in one criterion" | four |
| `VISION.md` | 115 | "nine of its **ten** branches price a cost" | nine of its **twelve** (three Saga branches price nothing) |
| `VISION.md` | 183 | "Lantern's three routes … all three are priced" | four |
| `VISION.md` | 755 | "nine of whose ten branches" | twelve |
| `VISION.md` | 757 | "The other **twelve** questions … between 10 and 122,880 compositions" | see below |
| `README.md` | 142-143 | "nine of whose ten branches", "the other twelve questions" | same fixes |
| `lantern-route-b.criteria.toml` | 4 | "VISION.md lists three routes" | four |
| `lantern-route-b.criteria.toml` | 11 | "`lantern.criteria.toml` counts cards SEEN and says so" | the reverse: it now prices mana with `can_cast` and says "Cards paid for, not cards seen" (`lantern.criteria.toml` l.11-17). Route B's real difference is that it *spends* the pool with `[casting]` and a cast-triggered fetch, where the north star only *gates* it |
| `lantern-route-b.criteria.toml` | 38, 53 | "The other two routes", "one route of three" | the other three routes; one of four |

On VISION l.757, the run's `enumerations` block has 17 questions, 4 of them
sampled. So there are **thirteen** other questions, at between 8 and 30,720
compositions on the play and 8 and 122,880 on the draw. `lantern.criteria.toml`
l.574 already says "thirteen".

## 7. Urza's Saga keeps tapping after it is sacrificed

This one was also found, not listed in the ticket. Without a `[land_drop]` no
effect is live for the Saga. It is a land with `produces: [C]`, so the
generous reading keeps it in the pool on turns after chapter III has
sacrificed it (CR 714.4). The chapter III turn itself is honest: you can tap
it in response to the trigger, and under CR 500.4 the mana lasts the rest of
that main phase.

**Bound:** removing the Saga from the pool entirely moves the north star
**−0.13 / −0.10**. The overcount is the subset of that where the Saga had to
be played by turn 3 to make the land drops. It is negligible.

**Fix:** document it with item 4. Do not declare the Saga effect in the north
star, since that breaks the route-4 library clauses, as the route-c file
explains.

## Sources

- **Code:**
  - `crates/ichormoon-gauntlet/criteria/src/mana.rs` (`Palette::from_letters`
    103, `LandDetail` 193-238, `Demand::covers` 580-632)
  - `crates/ichormoon-gauntlet/criteria/src/effect.rs` (pool 460-470, drops
    803-866, `played_by` 1221-1266, `can_cast` / `can_pay` 1268-1390)
  - `crates/ichormoon-gauntlet/cli/src/library.rs` (`is_land` 536,
    `mana_source` 571-584)
  - `crates/ichormoon-gauntlet/cli/src/prepare.rs` 583-624
  - `crates/reality-chip/scryfall/src/bulk.rs` 261
- **Card data:** `decks/index.jsonl` entries for every land in both decks
  (oracle text and `produces` quoted above). Scryfall API card object,
  `produced_mana`: "Colors of mana that this card could produce".
- **Comprehensive Rules:** 106.6 (restricted mana), 106.7 ("could produce"),
  118.3 (costs need resources), 305.2 (one land a turn), 500.4 (mana empties
  between steps and phases), 702.53 (Transmute), 714.3–714.4 (Saga lore
  counters and sacrifice).
