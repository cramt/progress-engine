# What modelling replacement draws costs the exact engine

Research for [#69](https://github.com/cramt/progress-engine/issues/69) (part of
[#66](https://github.com/cramt/progress-engine/issues/66)), 2026-09-26. It lays
out options and their costs. It does not pick a design; that is the design
ticket's job.

Sources are the code at `a8acaab`, the repo's docs, issues #57 and #62, the
Comprehensive Rules of 2026-09-25, and the prior art cited in §4. Numbers
marked **measured** come from a `gauntlet` run on the committed decks and index.
Numbers marked **modelled** come from the scratch path counter in the
[appendix](#appendix-the-path-counter), whose baseline reproduces the real
walk's leaf count exactly.

---

## Summary

- **The refusal is about the shape of the schedule, not the population.**
  `chip-stats` takes every gap as a fixed vector before the walk starts. The
  only thing a path may decide is a deterministic *removal*. A draw on a cast
  is a reveal whose size depends on the path, so today it can only be expressed
  as fixed one-card checkpoints paid every turn whether the spell was cast or
  not. That is the representation #57 measured and ADR 0010 refused.
- **Three mechanisms can express this, and they cost very different amounts.**
  1. A deterministic zone move costs no width.
  2. A **path-dependent gap** is a new `chip-stats` word beside *removals*:
     the path decides how many cards the next gap deals.
  3. A **deferred block**: cards that are milled and never read before the
     question can be dealt at the end of the path, because a shuffled library
     is exchangeable.
- **Modelled on the real Loam class** (7 groups, `[casting]` Aftermath Analyst
  then Life from the Loam, turn 5 on the play), against a baseline of 1,174,303
  paths walked (4,120,116 by the ceiling's bound):

  | mechanic | paths walked |
  |---|---|
  | Analyst mill 3, in place, path-dependent gap | 26,468,358 (22.5×) |
  | Analyst as a 1-card cantrip, path-dependent | 3,982,002 (3.4×) |
  | Loam dredge 3 on the draw step, in place | 832,568,578 (709×) |
  | mill, deferred to a Loam/not-Loam tail | 1,450,287 (1.24×) |
  | dredge, deferred to a Loam/not-Loam tail | 975,485 (0.83×) |
  | the fixed-slot representation (#57's) for mill 3 | 1.96e19 by the ceiling's bound |

- **Two of the ticket's premises need correcting.**
  - Malevolent Rumble and Midnight Tilling are not draw-then-discard. They are
    *look 4, at most one permanent to hand, the rest to the yard*: a router
    that consumes every card it reveals.
  - Opt's scry sends the card to the **bottom**, which no route can express
    today.
  - Of the ticket's cards, only dredge replaces a draw in the rules' sense
    (CR 121.6). The glossary's "replacement draw" is broader on purpose
    (`crates/ichormoon-gauntlet/CONTEXT.md`, "Replacement draw").
- **Prior art does not solve this exactly.** Every tool or article found goes
  one of two ways. It is exact hypergeometric for static questions and falls to
  Monte Carlo once selection or sequencing appears. Or it folds cantrips and
  scry into rule-of-thumb fractional sources.

---

## 1. Why it was refused, and what exactly breaks

### 1.1 The stated reason

- **ADR 0010** (`docs/adr/0010-mana-gate-then-budget-replacement-draws-refused.md`):
  "Every card the walk might draw needs its own checkpoint. On both committed
  decks that puts the cheapest line over the ceiling by turn five … The way
  forward is a non-fixed population (#18), not more checkpoints."
- **VISION.md "Mana"** (l.495–508): once Opt draws, *cards seen by turn T*
  "stops being a fixed schedule and becomes path-dependent, which changes the
  shape of the enumeration rather than the state carried along it". It then
  gives the numbers: a turn with *T* mana can cast *T* cantrips, and a
  checkpoint multiplies the width by the group count.
- **HANDS.md hand 5** (l.161–204) is the one open hand: Island, Opt, and an
  Island on top. "an unordered pair cannot say which of two revealed cards the
  draw took". Its table:
  - `{1}` one-drop: 4 groups, exact to turn 8 today, turn 4 with a draw a turn.
  - `{1}{G}{G}`: 6 groups, turn 5 today, turn 2.
  - A two-spell line: 7 groups, turn 5 today, turn 2.

  These reproduce: 7 groups on the play to turn 3 with one extra singleton per
  turn is C(13,6)·7⁵ = 28,840,812.
- **#57** adds the key sentence for this ticket: "a tutor is a deterministic
  removal from a named group, a replacement draw is a random one off the top,
  and both are 'the population moved mid-path'."
- **ADR 0012** shipped the deterministic half and filed the other: "Exiling off
  the top is a random sample, so it branches the path the way a draw does.
  That half is filed, not approximated." Mill is exactly that other half,
  sending cards to a different zone.

### 1.2 What breaks, layer by layer

**`chip-stats` (the walk).**
- `for_each_checkpoint_path(groups, gaps, f)` takes the whole gap vector up
  front (`crates/reality-chip/stats/src/lib.rs:218`). `descend` deals
  `gaps[depth]` from what is left at every depth (l.370–398).
- A path may influence only `Walk::removals`, and removals "are deterministic
  … A removal that *were* a random sample is a draw, and a draw is what `gaps`
  already says" (l.246–258). Nothing lets a path say *how many* the next gap
  deals.
- Each gap is one multivariate hypergeometric, an **unordered** composition. So
  if a gap reveals more cards than the path consumes, the path cannot say
  which ones it consumed.

**`gauntlet-criteria` (schedule and width).**
- `Schedule::build` (`criteria/src/schedule.rs:175–193`) adds `look_slots`
  one-card checkpoints to **every** turn, set to the deepest look of any
  effect.
  - This is sound for a land drop because it fires at most once a turn.
  - A revealed card the turn did not use waits in the Board's `fresh` queue and
    becomes the next draw (`effect.rs` `walk`, l.733–790).
  - That is how "a look that routes nothing is a no-op" holds.
- The price: every turn pays `g^look` whether or not the effect fired.
- `narrowed` refuses to collapse turns when effects are live or a casting line
  exists (`schedule.rs:251`).
- The width check is the static, **uncapped** product
  `compositions(groups, gaps)` against `MAX_PATHS = 5_000_000`
  (`criteria/src/lib.rs:488, 519–533, 632`).
  - For a gap whose size depends on the path there is no closed-form product.
  - The static bound over-counts by about 3.5× even today: measured 4,120,116
    by the bound against 1,174,303 leaves actually walked, for the 7-group
    Loam class below. (Modelled; the baseline count is checked exactly against
    a capped recount of the same gaps.)

**The Board (effect library and casting).**
- `Board::walk` plays whole turns and stops at a turn whose checkpoints the
  prefix does not reach (`effect.rs:755`).
- `cast` (l.915–950) is a greedy loop over the declared line. It can apply a
  `fetch_on_cast` because that removes a card already counted. It cannot stop
  mid-loop to be dealt a new card and then carry on, and a cantrip whose card
  might be cast or played the same turn needs exactly that.
- Routes are `Nowhere`, `Everything` and `Matching(q)` (`effect.rs:212–236`).
  All of them go to the graveyard.
  - No looked-at card can go to **hand** (Rumble, Tilling).
  - None can go to the **bottom** (Opt's scry). The standard library says so
    for scry lands (`toml/src/standard-effects.toml`, scry entry).
- There is no discard: nothing moves hand → graveyard (`zone.rs`, `Hand` doc).
- Commanders are not in the library and nothing casts from the command zone
  (`cli/src/library.rs:175`). That matters for Borborygmos and Fblthp.

**The file boundary.**
- `toml/src/lib.rs:1476–1482` refuses `trigger == Cast && look > 0` as
  `LooksOnCast`, whatever the destination.
- The message (`effect.rs:184–202`) calls every look on a cast a replacement
  draw.

### 1.3 What #62 gets right and what it leaves open

#62 is right that the ambiguity #57 names does not arise for a pure mill.
`Route::Everything` consumes every card it reveals, so an **unordered** block
of N is enough and nothing needs one-card checkpoints. It is also right that
the refusal could sit on the destination rather than on the trigger.

It leaves the width open. The block exists only on the turns the line casts
the Analyst, and the fixed schedule has to reveal the block every turn or not
at all. If the block is revealed but the Analyst was not cast, the unconsumed
unordered block sits on top of the library, and the next draw cannot say which
card it took. That is #57's problem again. So a mill on a cast needs one of:

- a fixed one-card slot per possible milled card per turn (sound, and
  astronomically wide);
- a path-dependent gap; or
- deferral (§2.4).

#62's "measure on `decks/loam.txt` before anything ships" is §3.2 below.

---

## 2. The building blocks

Every mechanic in the ticket is some mix of four representations.

### 2.1 Deterministic zone move: no new width

This fits the model the tutor uses (ADR 0012). Given the path, one answer
exists, so it is a Board-only change: counts move between `live_hand` and
`live_yard`.
- **Width:** no new checkpoints. Only the query bits of the priority that
  chooses what moves are added. Each new query can split groups, as every
  priority entry already does.
- **Sampler:** it shares the `Board` (`sim/src/lib.rs:300`), so agreement comes
  free unless a tie is priced (below).

### 2.2 Fixed one-card look slots: the existing representation

Extending `Schedule::build` to looks on a cast means `g^N` per turn for a look
of N, charged on every turn to the horizon.
- For mill 3 on the 7-group Loam class to turn 5 on the play, the ceiling's
  bound is 4,120,116 · 7¹⁵ ≈ **1.96e19**.
- For one singleton a turn (the #57 shape) it is 6.9e10.

This is the representation ADR 0010 rejected, and nothing here changes that
verdict.

### 2.3 Path-dependent gap: new `chip-stats` vocabulary

**The idea.** The walk asks the caller, after each checkpoint, how many cards
the next gap deals. The caller replays the Board over the prefix, as it
already does for removals (`criteria/src/lib.rs:752–757`).
- This is the random counterpart of *removals*, and it is what #57 and ADR 0010
  mean by "non-fixed population".
- It can stay Magic-free: "the size of the next draw is a function of the path
  so far". Nothing in the signature needs to know what a cantrip is.
- A gap of 0 costs one composition, so paths where nothing fired pay nothing.
- A block that is always consumed whole can be one **unordered** gap: mill N,
  Rumble or Tilling's look 4, draw N.
- One-card gaps are needed only where a later decision on the **same turn**
  reads which card came first: Opt's scry then draw, or a drawn card cast off
  the same turn's mana.

**What else has to change.**
- The Board has to play **part of a turn**. `walk` currently breaks at turn
  boundaries (`effect.rs:755`), and `cast` has to be resumable after a draw.
- The width check needs a new estimator.
  - The worst-case bound is still the fixed-slot product with blocks in place
    of singletons.
  - The real count depends on how many paths fire.
  - A memoised counting pre-pass works: the appendix counts the baseline in
    0.4 s. But it took 111 s in Python for the 832M-leaf dredge case, so it is
    not free at the wide end.
- **Sampler:** it has to ask the Board the same question at each checkpoint
  and deal that many cards. It already replays the Board per checkpoint for
  fetches (`sim/src/lib.rs:258–283`); this adds "how many next" beside
  "what was removed".

### 2.4 Deferred (exchangeable) block

**The idea.** A uniformly shuffled library is exchangeable, so the joint law
of (the cards milled at turn t, everything drawn afterwards) is unchanged if
the milled cards are dealt **at the end** of the path instead of in place.
This holds when:
- (a) whether the mill happens does not depend on which cards it takes; and
- (b) nothing between the mill and the question reads the milled cards.

Condition (a) is always true: the mill fires or not off earlier cards, and
"library ≥ N" is a count. Condition (b) is the real restriction.

**What deferral buys.**
- A mill (or a dredge's mill) becomes a **trailing gap**. Any trailing card the
  path did not use is harmless, because nothing draws after it. That removes
  the "block left on top" problem of §1.3 even with a **fixed** gap vector.
- So deferral needs neither path-dependent gaps nor a partial-turn Board.
- The dredged-or-milled cards can be dealt over a **coarser partition** than
  the class's grouping. The mana split exists only for decisions, and no
  decision reads the tail.
  - On the Loam question the tail needs only Loam/not-Loam: at most 2
    compositions against 84 for 3 cards over 7 groups.
  - Doing that in the walk means `chip-stats` dealing one gap over a
    coarsening of the groups. That is Magic-free, but it is a second new word.
  - Without the coarsening, a deferred block costs the same as the in-place
    one (§3.2): 26,468,358 both ways.

**Where it stops being sound.** Anything that reads the milled cards before
the question breaks condition (b):
- Life from the Loam returning lands;
- Rumble or Tilling choosing a permanent from among them;
- Aftermath Analyst's own sacrifice ability;
- a criterion about the yard at an earlier turn than the mill. This one is
  fixable: keep one tail block per mill turn.

So deferral covers the north star *as asked* ("Loam in the graveyard by
turn 5") and not the Loam engine in general.

**Sampler.** It needs to learn **nothing**: it deals the milled cards in their
true position. That makes the two-engine agreement a direct test of the
exchangeability claim, which fits ADR 0001's intent.

---

## 3. Per mechanic

Oracle text is from `decks/index.jsonl`, dated 2026-09-21.

Rules text is from the Comprehensive Rules of 2026-09-25:
[MagicCompRules 20260925.txt](https://media.wizards.com/2026/downloads/MagicCompRules%2020260925.txt),
linked from [magic.wizards.com/en/rules](https://magic.wizards.com/en/rules).

### 3.1 (a) Discard as a zone move, hand → graveyard

**Rules.** CR 701.9a: "To discard a card, move it from its owner's hand to that
player's graveyard." CR 701.9b: the discarding player chooses by default.

**The cards.** Every discard outlet the ticket names comes *with a draw*, so
discard alone is only half of each card:

| card | text | parts |
|---|---|---|
| Borborygmos and Fblthp (commander, `{2}{G}{U}{R}`) | "Whenever … enters or attacks, draw a card, then you may discard any number of land cards." | draw 1 (§3.4), then discard by choice. Needs casting from the command zone, which does not exist, and the attack trigger needs combat. |
| Cycling lands (Forgotten Cave `Cycling {R}`, Ketria Triome `Cycling {3}`) | CR 702.29a: "[Cost], Discard this card: Draw a card." | An activated ability paid from the pool, then discard, then draw 1. The card is also a land, so it has **two claimants**: the land drop and the pool. |
| Magmatic Insight (`{R}`) | "As an additional cost … discard a land card. Draw two cards." | discard by choice, then an unordered draw 2. |
| Cavalier of Flame (`{2}{R}{R}{R}`) | "discard any number of cards, then draw that many cards." | discard by choice, then a draw whose size is the discard count. |

**Fit.** The discard half is §2.1: a declared priority over queries (ADR 0004)
of the same shape as `[mulligan] bottom`, naming which held cards are
discarded.
- The one known trap is ties. `bottom` settles a tie inside one entry at
  random and **prices** it, because a decklist order does not commute with the
  per-class merging of ADR 0007 (ADR 0004, second bullet).
- A discard priority would inherit that: a tie is a `for_each_composition` over
  the tier, like `Board::bottomings` (`effect.rs:640`). That is a real branch.
  - It is small when a tier is one group.
  - It multiplies width when a tier spans groups the class keeps apart.
- "Any number of land cards" (B&F) is a choice of *how many* as well as which.
  A priority can state it as "everything the list names", as `to_graveyard`
  already does for a look.

**Width.** No checkpoints for the discard itself, only query bits. Every card
above then needs its draw half (§3.4), which is where the cost is.

**Sampler.** Nothing, unless ties are priced. Then it needs the tossed
counterpart, "discard whichever were dealt first", as `bottom_in_order` does
(`effect.rs:679`).

**Policy risk.**
- The cycling land is a card two existing priorities both claim: the land drop
  and the casting line. The land drop was already a two-claimant resource once
  and was settled by declaring a list (VISION.md "Mana", l.521–530). The same
  question returns here.
- Command-zone casting and combat are new model surface that the ticket's list
  does not mention.

### 3.2 (b) Plain mill from the top

**Rules.** CR 701.17a: "puts that many cards from the top of their library
into their graveyard." CR 701.17b: a player can't mill more cards than the
library holds.

**The cards.** On the Loam list only Aftermath Analyst is a pure mill ("When
this creature enters, mill three cards"), as #62's table found. Midnight
Tilling is mill 4 plus a choice (§3.4).

**Fit.** It is already `look = N, to_graveyard = "*"` (`Route::Everything`).
Only the trigger is refused.
- It is an always-consumed block, so it never needs one-card checkpoints.
- It does need either a path-dependent gap (§2.3) or deferral (§2.4), for the
  reason in §1.3.

**Width.** Modelled, on the real class:
- Class: 7 groups, sizes Analyst 1, green tapped lands 6, spells 52, colourless
  untapped lands 19, green untapped lands 11, colourless tapped lands 8,
  Loam 1.
- Setup: `[casting]` Analyst then Loam; turn 5, on the play unless marked.
- The class is **measured** as 7 groups and 4,120,116 by the bound, exact on
  the play. On the draw it is refused at 28,840,812.

| representation | paths walked, play | paths walked, draw |
|---|---|---|
| today, no mill | 1,174,303 | 6,834,914 |
| fixed one-card slots, 3 a turn | bound 1.96e19 | — |
| path-dependent unordered block | 26,468,358 | 161,276,921 |
| deferred tail over all 7 groups | 26,468,358 | 161,276,921 |
| deferred tail over Loam/not-Loam | **1,450,287** | **8,415,012** |
| turn 3, path-dependent block | 599,944 (base 33,219) | — |

**Two readings of this table.**
- The walk leaves are **not probability-weighted**. The Analyst is one card in
  98, but once it is a group of its own, every composition holding it is a
  distinct path. So even a rare trigger multiplies width by the size of its
  block's composition space: 84 for 3 over 7 groups.
- 1.45M leaves is under `MAX_PATHS`, but the ceiling tests the uncapped bound,
  and the uncapped bound of a trailing gap of 3 is 4,120,116 · 4 = 16,480,464
  even over two bins. A shipped deferral would need the ceiling to count real
  leaves, or it would refuse questions it could answer.

**Sampler.**
- Path-dependent gap: it deals the block when the Board says the mill fired.
- Deferred: it needs nothing new.
- Acceptance would be the three-level agreement already in
  `sim/tests/acceptance.rs` (`a_tutor_agrees_with_the_exact_engine` is the
  model to follow).

### 3.3 (c) Dredge

**Rules.** CR 702.52a: "As long as you have at least N cards in your library,
if you would draw a card, you may instead mill N cards and return this card
from your graveyard to your hand." CR 702.52b: a library shorter than N can't
be milled this way. This is the only mechanic in the ticket that replaces a
draw in the rules' sense (CR 121.6, 614.11).

**The cards.** Life from the Loam (Dredge 3) and Shenanigans (Dredge 1).
Dredge replaces **any** draw, the draw step included, whenever the card is in
the yard.

**Fit.** Neither existing model fits.
- The draw step's gap becomes "0 to hand, N to the yard" on some paths and "1
  to hand" on others. The returned card is deterministic, since it is the
  dredger itself.
- The choice to dredge is a new pilot decision.
- A side note: `zone.rs` (Graveyard doc) says "Dredge cares which card is on top
  of the yard". CR 702.52a sets no such condition; it needs the card in the
  yard and N cards in the library. Unordered is enough for dredge.

**Width.** Modelled; always dredge when Loam is in the yard; Loam reaches the
yard by being cast or milled.

| representation | paths walked, play | paths walked, draw |
|---|---|---|
| today (no dredge, no mill) | 1,174,303 | 6,834,914 |
| in place, path-dependent (mill 3 blocks) | 832,568,578 | — |
| turn 3, in place | 987,647 (base 33,219) | — |
| deferred tail over all 7 groups | 24,629,396 | — |
| deferred tail over Loam/not-Loam | **975,485** | **5,362,230** |

- In place, dredge is the most expensive thing in this report. It fires on
  every draw step once Loam is binned, and each firing is an 84-way block that
  also reshapes every later draw.
- Deferred and coarse, it is *cheaper* than today, because a dredged draw step
  reveals nothing in place.
- But deferral is sound only while nothing reads the dredged cards. Loam's own
  effect ("Return up to three target land cards from your graveyard") reads
  them, so the Loam engine as played is not deferrable. Only the "Loam in the
  yard" north star is.

**Sampler.** It must implement the replacement at the draw step: skip the
draw, deal N to the yard, and return the dredger. It asks the Board whether to
dredge before dealing each draw step, so this is path-dependent in the sampler
whichever design the exact engine takes.

**Policy.** "Dredge or draw" is a decision over **a draw**, a resource no
existing priority ranks. Expressing it as a declared priority over queries is
plausible (dredge when the list's entries are in the yard), but it is the
place a sixth policy language could appear. ADR 0004 is explicit that "Adding a
new policy language is the failure this ADR exists to prevent", so it needs
deciding in the design ticket rather than here.

### 3.4 (d) Draw-then-discard, and the "look N, one to hand" cards

The ticket groups three cards that are two different mechanics:

| card | oracle | mechanic |
|---|---|---|
| Frantic Search `{2}{U}` | "Draw two cards, then discard two cards. Untap up to three lands." | draw 2 (unordered block), then discard 2 by priority (§3.1). The untap is a **mana refund** the budget would have to learn. |
| Malevolent Rumble `{1}{G}` | "Reveal the top four cards … You may put a permanent card from among them into your hand. Put the rest into your graveyard." | look 4, route at most 1 to hand by priority, the rest to the yard. All 4 consumed. Also makes a Spawn token (`{C}`), which is mana the gate does not count. |
| Midnight Tilling `{1}{G}` | "Mill four cards, then you may return a permanent card from among them to your hand." | same shape as Rumble: mill 4, then at most 1 back to hand. |

**Rumble and Tilling.**
- These are the ticket's §3 question ("look at N, keep some, bin the rest")
  exactly, and they are the router ADR 0008 and VISION "One mechanism for
  selection" already describe. It is a declared priority over the looked-at
  set, with **hand** as a new destination (today only the yard or the top).
- Because every revealed card is consumed, one **unordered** block of 4 is
  enough; the choice is a function of the block's counts.
- They need a path-dependent gap (§2.3). They cannot be deferred: the choice
  reads the block, and the card that goes to hand changes later decisions.
- Width is the in-place mill row of §3.2, with a block of 4:
  C(10,6) = 210 compositions over 7 groups, against 84.

**Frantic Search** (and Opt, Magmatic Insight, cycling, B&F's ETB, Cavalier).
- These are true draws: a path-dependent gap to hand.
- An unordered block is enough unless a card drawn this turn is **used this
  turn**: cast off the remaining pool, or played as a land while the drop is
  unspent.
  - HANDS.md hand 5 is the case where it is not. The drop is already spent, so
    the drawn Island waits a turn.
  - Frantic Search's untap makes same-turn use likely: three lands come back.
- If same-turn use is modelled, the Board's `cast` loop has to be resumable and
  each draw has to be a separate checkpoint.
- Modelled: the Analyst as a 1-card cantrip, with no same-turn use, walks
  **3,982,002** leaves on the play, 3.4× today and under the ceiling. The
  static one-slot-a-turn shape #57 measured is 6.9e10 by the bound on the same
  class.
- Opt adds a look before the draw, routed to the **bottom** (CR 701.22a,
  "put any number of them on the bottom … and the rest on top"). That needs a
  bottom destination and makes the reveal 1 or 2 depending on the route: one
  singleton, then a second only if the first went to the bottom.

**Sampler.** For every card here it must deal draws and looks when the Board
says they happen. It already routes by the shared `Board`, so the only new
thing is dealing path-dependent counts. Priced discard ties need the
dealt-first rule of §3.1.

### 3.5 Summary table

| mechanic | existing model? | new `chip-stats` word | enumeration effect (Loam class, turn 5, play) | sampler must learn |
|---|---|---|---|---|
| discard | yes: deterministic zone move | none | query bits only, plus a priced-tie branch if ties are priced | dealt-first tie rule if ties are priced |
| plain mill | the look/route exists; the trigger doesn't | path-dependent gap, **or** deferral (plus per-gap coarsening) | 26.5M in place; 1.45M deferred and coarse | in place: deal when the Board says; deferred: nothing |
| dredge | no: replaces a draw step | path-dependent gap, or deferral (plus coarsening) | 833M in place; 0.98M deferred and coarse, but only while nothing reads the yard | the replacement at the draw step |
| draw N (Frantic, Opt, Insight, cycling, B&F ETB) | no | path-dependent gap; one-card checkpoints only for same-turn use | 4.0M for a 1-copy cantrip, no same-turn use | deal when the Board says |
| look N, one to hand (Rumble, Tilling) | router exists; hand as a destination doesn't | path-dependent gap | about the mill row with 210-way blocks | deal when the Board says |
| scry to the bottom (Opt) | no bottom route | path-dependent gap of 1 or 2 | small per firing | deal when the Board says |

---

## 4. Prior art

**Frank Karsten, "How Many Sources Do You Need to Consistently Cast Your
Spells? A 2022 Update"** (ChannelFireball, now
[TCGplayer](https://www.tcgplayer.com/content/article/How-Many-Sources-Do-You-Need-to-Consistently-Cast-Your-Spells-A-2022-Update/dc23a7d2-0a16-4c0b-ad36-586fcca03ad8/),
dated 8/2/2022). This is the field's reference piece.
- It assumes "The only mana sources are lands, and there is no card selection:
  This assumption is made for tractability."
- It computes "in Python, via simulation, for ease of calculation".
- Cantrips are counted "according to the fraction of your deck that can
  produce the right colored source", and scry by rule of thumb: "A cheap
  scry 1 effect in a 60-card deck with 18 black lands … counts as
  approximately 0.2 black sources."
- So selection is folded into the population as a fractional card, not
  modelled.

**mtgoncurve / landlord** ([github.com/mtgoncurve/landlord](https://github.com/mtgoncurve/landlord),
HEAD `18e995a`, 2025-04-06).
- "a Rust library that simulates the mulligan and card draw process".
- `lib/src/simulation.rs` draws with `SmallRng` and has no scry, cantrip or
  selection code (searched).

**teryror's expanded mana-base guide**
([gist](https://gist.github.com/teryror/881d60e08480a56043895d3bbb83c374)),
a port of Karsten's simulation. It is Monte Carlo ("Repeat a million times")
and does not model cantrips, scry or selection.

**Draw-Probability-Calculator**
([savanaben](https://github.com/savanaben/Draw-Probability-Calculator)).
- "applies hypergeometric and monte carlo methods".
- It is exact only for static questions, and multi-turn draws are backlog.

**Quiet Speculation, "Testing the Consistency of Ancient Stirrings"**
([2018](https://www.quietspeculation.com/2018/09/testing-consistency-ancient-stirrings/)).
The canonical "look at five, keep one" card, analysed by 100,000 goldfish
games of simulation rather than a formula.

**The exact tool for a single look.** One look at N, keep one of a category,
ignore the rest is exactly `1 − C(N−K, n)/C(N, n)` with the sample size raised
by N. That is the hypergeometric Karsten introduces in "An Introduction to the
Hypergeometric Distribution for Magic Players" (ChannelFireball, 2018-12-12;
[reprint PDF](https://orkerhulen.dk/onewebmedia/An%20Introduction%20to%20the%20Hypergeometric%20Distribution%20.pdf)).
It is only right for a single category and a single look. It breaks exactly
where this engine needs it: routing by priority across several groups,
several looks a game, and later decisions that read what was binned.

**In this repository** the "look N, keep by priority, bin the rest" block
already has an exact precedent: mulligan bottoming.
- `Board::bottomings` (`effect.rs:640`) prices a tie as one
  `for_each_composition` over the tier. A declared priority over an
  **unordered** block, with the leftover resolved as one multivariate
  hypergeometric, is a mechanism the engine already trusts.
- The sampler's counterpart `bottom_in_order` is the dealt-first rule.
- Rumble and Tilling are the same computation over a block dealt mid-game
  instead of the opener.

**Nothing found computes selection exactly across turns.** Everything
surveyed either simulates or approximates. The exchangeability argument of
§2.4 is standard probability (a uniformly random permutation's law does not
change when positions are swapped), but no MTG tool was found that uses it to
defer milled cards. It is this report's proposal for evaluation, not prior
art.

---

## 5. Questions for the design ticket

1. **Path-dependent gaps, deferral, or both?**
   - Path-dependent gaps cover everything (draws, Rumble, Tilling, dredge in
     place). They cost a partial-turn Board, a resumable `cast`, and a new
     width estimator, and dredge in place is 833M leaves on this class.
   - Deferral is nearly free for "is it in the yard by T". It covers only
     mills and dredges that nothing reads, and needs a per-gap coarsening word
     in `chip-stats` to pay off.
2. **Should the ceiling count real leaves rather than the uncapped product?**
   The product already over-counts 3.5× today, and it over-counts any tail
   block badly.
3. **Which of the new decisions fit ADR 0004 as declared priorities over
   queries?** There are four:
   - which cards to discard;
   - which permanent Rumble or Tilling keeps;
   - scry top or bottom;
   - whether to dredge.

   The last is the one at risk of being a new policy language.
4. **Discard ties: priced like `bottom` (a branch), or settled by an order?**
   An order is what ADR 0004 says fails the narrowing property test.
5. **Out-of-ticket model surface these cards drag in:**
   - casting the commander from the command zone (B&F);
   - activated abilities paid from the pool (cycling, Analyst's sacrifice);
   - mana refunds (Frantic Search);
   - mana tokens (Rumble's Spawn).

---

## Appendix: the path counter

**What it models.** It is a Python model of one real class: the 7 group sizes
the engine used for `[casting] prefer = ['name:"Aftermath Analyst"',
'name:"Life from the Loam"']` on `decks/loam.txt`. The sizes were read from an
instrumented scratch build of `gauntlet-cli` printing `group_sizes()` per
enumeration; no repository file was changed.

**Its board is simplified.**
- The land drop is taken in the order GU > CU > GT > CT.
- A tapped land gives no mana on the turn it enters.
- The two `{1}{G}` spells are cast greedily.
- Nothing drawn off an effect is used the same turn.

**How it was checked.**
- It counts distinct checkpoint paths (capped by group size, as
  `for_each_composition` walks them) and checks that the probability mass is 1.
- Its baseline, 1,174,303 on the play and 6,834,914 on the draw, equals an
  independent capped recount of the real gaps `[7,0,1,1,1,1]` and
  `[7,1,1,1,1,1]`.
- The absolute numbers for the new mechanics are modelled. The ratios are the
  finding.

Usage: `python3 count.py <mode> <turn> <play|draw>`. The modes are `base`,
`mill`, `cantrip`, `dredge`, `defer`, `defer-coarse`, `dredge-defer` and
`dredge-defer-coarse`.

```python
import sys, math
from functools import lru_cache
from math import comb
SIZES=(1,6,52,19,11,8,1)   # Analyst, GT, spell, CU, GU, CT, Loam
A,GT,SP,CU,GU,CT,LOAM=range(7)
G=len(SIZES)

def comps(avail,k):
    out=[]
    def rec(i,left,cur):
        if i==G-1:
            if left<=avail[i]: out.append(tuple(cur+[left]))
            return
        for t in range(min(left,avail[i])+1): rec(i+1,left-t,cur+[t])
    rec(0,k,[]); return out

def pmf(avail,take):
    n=sum(avail);k=sum(take)
    return math.prod(comb(a,t) for a,t in zip(avail,take))/comb(n,k)

def uncapped(gaps,g=G):
    return math.prod(comb(x+g-1,g-1) for x in gaps)

MODE=sys.argv[1]; T=int(sys.argv[2]); DRAW=sys.argv[3]=='draw'
def draw_for(t): return 1 if t>0 and (DRAW or t>1) else 0

@lru_cache(maxsize=None)
def leaves(lib,hand,yard,field,turn,phase):
    """(number of leaf paths, probability mass) below this state"""
    if turn>T:
        pend=field[4] if len(field)>4 else 0
        if pend==0: return (1,1.0)
        k=min(3*pend,sum(lib))
        if MODE in('defer','dredge-defer'):
            return (len(comps(lib,k)),1.0)
        # coarse tail: only Loam vs rest
        return (len([t for t in range(0,min(lib[LOAM],k)+1) if k-t<=sum(lib)-lib[LOAM]]),1.0)
    if phase=='draw':
        n=draw_for(turn)
        if MODE in('dredge-defer','dredge-defer-coarse') and yard[LOAM]>0 and n>0 and sum(lib)-3*(field[4] if len(field)>4 else 0)>=3:
            h=list(hand);h[LOAM]+=1;y=list(yard);y[LOAM]-=1
            f=list(field)+([0] if len(field)==4 else []);f[4]+=1
            return leaves(lib,tuple(h),tuple(y),tuple(f),turn,'main')
        if MODE in('dredge',) and yard[LOAM]>0 and n>0 and sum(lib)>=3:
            tot=0;mass=0.0
            h=list(hand);h[LOAM]+=1;y=list(yard);y[LOAM]-=1
            for take in comps(lib,3):
                p=pmf(lib,take);l2=tuple(a-b for a,b in zip(lib,take));y2=tuple(a+b for a,b in zip(y,take))
                c,m=leaves(l2,tuple(h),y2,field,turn,'main'); tot+=c;mass+=p*m
            return (tot,mass)
        if n==0: return leaves(lib,hand,yard,field,turn,'main')
        tot=0;mass=0.0
        for take in comps(lib,n):
            p=pmf(lib,take);l2=tuple(a-b for a,b in zip(lib,take));h2=tuple(a+b for a,b in zip(hand,take))
            c,m=leaves(l2,h2,yard,field,turn,'main');tot+=c;mass+=p*m
        return (tot,mass)
    h=list(hand);f=list(field)
    newg=newc=0
    if turn>0:
        for grp,slot,tapped in ((GU,0,0),(CU,1,0),(GT,2,1),(CT,3,1)):
            if h[grp]>0:
                h[grp]-=1;f[slot]+=1
                if tapped: newg+= (grp==GT); newc+=(grp==CT)
                break
    gsrc=f[0]+f[2]-newg; csrc=f[1]+f[3]-newc
    y=list(yard); spent=0; gspent=0
    events=[]
    if turn>0:
        for grp in (A,LOAM):
            if h[grp]>0:
                total=gsrc+csrc
                if total-spent>=2 and gsrc-gspent>=1:
                    spent+=2;gspent+=1;h[grp]-=1
                    if grp==LOAM: y[LOAM]+=1
                    else: events.append(grp)
    if A in events and MODE in('defer','defer-coarse','dredge-defer','dredge-defer-coarse'):
        f=f+[0] if len(f)==4 else f
        f[4]+=1
    hand2=tuple(h);field2=tuple(f)
    if A in events and MODE in('mill','dredge','cantrip'):
        k=1 if MODE=='cantrip' else 3
        tot=0;mass=0.0
        for take in comps(lib,min(k,sum(lib))):
            p=pmf(lib,take);l2=tuple(a-b for a,b in zip(lib,take))
            if MODE=='cantrip':
                h3=tuple(a+b for a,b in zip(hand2,take));y3=tuple(y)
            else:
                h3=hand2;y3=tuple(a+b for a,b in zip(y,take))
            c,m=leaves(l2,h3,y3,field2,turn+1,'draw');tot+=c;mass+=p*m
        return (tot,mass)
    return leaves(lib,hand2,tuple(y),field2,turn+1,'draw')

tot=0;mass=0.0
for take in comps(SIZES,7):
    p=pmf(SIZES,take);l2=tuple(a-b for a,b in zip(SIZES,take))
    c,m=leaves(l2,take,(0,)*G,(0,0,0,0),0,'main');tot+=c;mass+=p*m
gaps=[7]+[draw_for(t) for t in range(1,T+1)]
print(f"{MODE} T={T}: leaves={tot:,} mass={mass:.12f} bound={uncapped(gaps):,}")
```

**The measured runs.** These were produced with `gauntlet test` built from a
scratch copy of the tree:
- `decks/loam.criteria.toml`: its widest exact class is 6 groups and 1,026,432
  compositions on the play, 6,158,592 on the draw.
- The Analyst/Loam `[casting]` file: 7 groups, 4,120,116 on the play, and
  refused at 28,840,812 on the draw.
