# A rules engine for Magic, and which Rust logic language runs in wasm

Question: should progress-engine build a game rules engine beside
Vivisurgeon's Insight, and if part of it is logic programming, which Rust
option still compiles to `wasm32-unknown-unknown`, so it can run in Meldweb's
browser build the way `meldweb-wasm` does?

**Answer.** Arena already runs this architecture. Its Game Rules Engine
(GRE) is C++ plus CLIPS, a forward-chaining rule language, and its Game Rules
Parser, a Python tool, turns a card's rules text into CLIPS rules and gets
"80% or so" of new cards working with no hand work
([Werner, 2023](https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough)).
That is Insight's pipeline, shipped. It raises the one design decision that
matters (*Cards as rules, or abilities as data*). We recommend abilities as
data: Insight's typed nodes become facts, and one fixed set of general rules,
written in **ascent** and compiled with the engine, interprets them. Ascent
builds and runs under wasm, has stratified negation, aggregation and
lattices, and keeps Rust types in its relations. The game itself is a typed
Rust state machine, not Datalog, because Magic is Turing complete. Recompute
characteristics from scratch, gated by a version counter that only
characteristic-changing facts bump. Salsa adds nothing a game needs and
loses in search.

Measured on 2026-10-13, rustc 1.98.0, release builds with LTO, the toys in
`/tmp/logic-wasm*` (not in the repo). Wasm runs are `wasm32-unknown-unknown`
bound by wasm-bindgen 0.2.126 and run in Node 24.19.0. Times are the mean of
20 runs after a warm-up, unless the table says otherwise.

## What Arena does

From two first-party dev blogs and Decked Out EDH's interview with Arena's
Game Director ([video](https://www.youtube.com/watch?v=PeI0TTUMWXU); the
engine part runs from 37:20 to 52:00).

- **The GRE is C++ and CLIPS, and it knows no cards.** It knows priority,
  phases, casting and the like. It writes out the actions or choices that
  are pending, then "naps". Card-specific CLIPS rules edit that list,
  removing, adding or swapping entries, and the GRE carries out what is left
  without knowing which rules touched it
  ([Werner](https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough)).
  In the interview, CLIPS is "some of the heart of how the rules engine ends
  up processing Magic game states" (38:56), one of "eight to 10 languages"
  running on Arena (38:05).
- **Cards are compiled into rules.** The Game Rules Parser, in Python, turns
  English rules text into CLIPS. About 80% of new cards work automatically;
  the rest need the parser extended. Living Breakthrough needed four new
  generated rules and a change to the client protocol
  ([Werner](https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough)).
- **Tests are rules questions with one right answer.** They are scripted,
  text-only games run without a client
  ([Werner](https://magic.wizards.com/en/news/mtg-arena/on-whiteboards-naps-and-living-breakthrough)),
  over 3,000 of them by 2023
  ([Adams, 2023](https://magic.wizards.com/en/news/mtg-arena/why-i-decided-not-to-do-emrakul-and-how-we-shipped-it-anyway)).
  Rules "are actually pretty easy to regression test" (48:31). Correctness
  is combinatorial, with interactions among three, four or five cards
  (44:55).
- **Breakage happens far from the change.** The Ninja's Kunai bug turned a
  change into a week of turn-two wins. Arena has since built tools that show
  whether a change touched "weird reaches of the world" it had no reason to
  (between 44:55 and 48:21). That is the idea behind Insight's `snapshot/`.
- **Each match is its own GRE process.** A match controller starts one per
  game and handles 100 to 200 players (43:59).
- **Autotap is pathfinding,** with A* over graphs of billions to trillions
  of nodes (39:33).
- **Mindslaver effects touched every action in the game.** Emrakul, the
  Promised End was Arena's costliest single card, larger than either earlier
  remastered set
  ([Adams](https://magic.wizards.com/en/news/mtg-arena/why-i-decided-not-to-do-emrakul-and-how-we-shipped-it-anyway)).

CLIPS itself was developed at NASA's Johnson Space Center from 1985 to 1996.
It is written in C and has been public domain since 1996
([clipsrules.net](https://www.clipsrules.net/)). Its rules act "like
WHENEVER-THEN statements": the inference engine keeps track of every rule
whose conditions hold, fires one, and repeats until none apply, and its
matching is a Rete network
([Basic Programming Guide 6.4.2](https://www.clipsrules.net/documentation/v642/bpg642.pdf), §2.5 and §9).

**Arena's 80% is not Insight's 85.6%.** Arena's number is new cards that
*play correctly* on top of years of engine rules. Insight's 85.6% of
abilities is a typed tree that nothing executes yet. They measure different
things, so neither says one is ahead of the other.

## Cards as rules, or abilities as data

This is the central decision, and it chooses the engine.

**(a) Cards compiled into rules, Arena's way.** Each card's text becomes
rules. That needs a logic engine that takes rules at run time: CLIPS,
Scryer Prolog, egglog, or ascent-interpreter. Rules are strings or untyped
terms, so the AST's types end at the boundary.

**(b) Abilities as data.** Insight's nodes become facts, and one fixed set of
general rules interprets them. For example, a modifier node in layer 7c
applies its +N/+N to every object its filter matches. The rules are known
when rustc runs, so compile-time engines (ascent, crepe) work, and relations
hold the AST's own enums.

The toys below are already (b). A layer-4 effect is the fact
`l4(id, timestamp, filter, adds)`. The same three rules (dependency, order,
application) handle any number of such effects, whatever card they came
from:

```rust
dep(b, a) <-- l4(b, _, filt, _), l4(a, _, _, adds), if a != b && filt == adds;   // CR 613.8a
before(a, b) <-- dep(b, a);
before(a, b) <-- l4(a, ta, _, _), l4(b, tb, _, _), if ta < tb, !dep(a, b), !dep(b, a);
pos(e, n) <-- l4(e, _, _, _), agg n = count() in before(_, e);
has_type(s + 1, o, *adds) <-- pos(e, s), l4(e, _, filt, adds), has_type(s, o, filt);
```

We recommend (b). It fits Insight's typed tree, it keeps
*Make Invalid States Unrepresentable*, and it puts the cost of a new kind
of node in one general rule rather than in every card that uses it. That is
the same bet Insight made with its grammar. Arena's cost shows up as the
20% of cards that need the parser extended. Ours will show up as nodes that
need a new general rule.

## The toy

CR 613 in miniature, the same in every candidate. Objects 0..n start 2/2;
even ids are creatures, odd ids artifacts. In layer 4, A (timestamp 2) makes
each artifact a creature in addition, and B (timestamp 1) makes each creature
a Zombie in addition. B depends on A, because applying A changes what B
applies to
([CR 613.8a](https://magic.wizards.com/en/rules)), so A applies first despite
its later timestamp (CR 613.8b), and every object ends a Zombie creature.
Timestamp order alone (CR 613.7) would leave the odd ids without Zombie. In
layer 7b, setter k makes object 7k mod n base 0/1, and the latest one wins.
In layer 7c, m anthems each give every creature +1/+1. The answer for
n=1000, m=100, s=98 is `creatures=1000 zombies=1000 sum_p=101804
sum_t=101902`. Every engine below that ran gave exactly that, and so did a
plain Rust baseline.

## The candidates

| Option | Rules written as | Typed Rust values in relations | Negation / aggregation / lattices | Rules at run time | Last release | wasm32-unknown-unknown |
|---|---|---|---|---|---|---|
| [ascent](https://github.com/s-arash/ascent) 0.8.1 | `ascent!` proc macro | yes, any `Clone + Eq + Hash` | stratified / yes / yes | no | 2026-08-29 | **builds and runs**. It needs `default-features = false`, because the default `par` feature pulls in rayon. It has a `wasm-bindgen` feature and uses `web-time` for `Instant` ([Cargo.toml](https://github.com/s-arash/ascent/blob/master/ascent/Cargo.toml), [wasm-tests/](https://github.com/s-arash/ascent/tree/master/wasm-tests)) |
| [ascent-interpreter](https://crates.io/crates/ascent-interpreter) 0.1.5 | Ascent text, parsed at run time | no, a `Value` enum | stratified / yes / yes, plus retraction | **yes** | 2026-03-25 | **builds and runs**. Its JIT is x86-64 assembly, so wasm always interprets |
| [crepe](https://github.com/ekzhang/crepe) 0.2.0 | `crepe!` proc macro | relations must be `Copy` ([lib.rs](https://github.com/ekzhang/crepe/blob/master/src/lib.rs)) | stratified / **no** / no | no | 2025-12-14 | **builds and runs** |
| [datafrog](https://github.com/rust-lang/datafrog) 2.0.1 | joins written by hand in Rust (Polonius's engine) | yes | by hand | no | 2026-10-12 | **builds and runs**, with no dependencies |
| [egglog](https://github.com/egraphs-good/egglog) 3.0.0 | its own s-expression language | no | e-graphs and equality saturation, not layered Datalog | yes | 2026-08-19 | builds only with getrandom's `wasm_js` backend. Our run **panicked** under Node (`unreachable`); we didn't find out why. Upstream ships a [browser demo](https://egraphs-good.github.io/egglog/) |
| [scryer-prolog](https://github.com/mthom/scryer-prolog) 0.10.0 | Prolog, backward chaining | no | Prolog | yes | 2025-09-27 | builds only with `--no-default-features`, clang for `ring`'s C, and getrandom `wasm_js`. Our run hit "memory access out of bounds" under Node. Its README calls wasm support "basic" ([README](https://github.com/mthom/scryer-prolog#building-webassembly)) |
| CLIPS via [clips-bindings](https://github.com/exlee/clips-bindings) 0.1.2 | CLIPS | no | CLIPS | yes | 2026-10-05 | **fails**. Its build script panics with "CLIPS build is not configured for target OS unknown". It builds natively against CLIPS 6.4.2 source (nixpkgs `clips.src`) |
| [differential-dataflow](https://github.com/TimelyDataflow/differential-dataflow) 0.25.1 + timely 0.31 | dataflow operators in Rust | yes | iterate, reduce, retraction | no | 2026-07-15 | builds; a run was in progress at write-up and was **not measured** |
| [DDlog](https://github.com/vmware-archive/differential-datalog) | own language, compiled to Rust | yes | yes, incremental | no | archived 2023-07 | **dead**: the repository is archived |
| [Nemo](https://github.com/knowsys/nemo) | own Datalog dialect | no | stratified / yes / existential rules | yes | active (pushed 2026-10-12) | has a `nemo-wasm` crate and a [web app](https://tools.iccl.inf.tu-dresden.de/nemo/); **not measured** |
| [salsa](https://github.com/salsa-rs/salsa) 0.28.5 | memoised Rust functions, not Datalog | yes | n/a | n/a | 2026-09-24 | **builds and runs** (measured below) |
| Rete crates ([rete](https://crates.io/crates/rete) 0.0.1, 2020; [greynet](https://crates.io/crates/greynet) 0.11; [phronesis-rhai](https://crates.io/crates/phronesis-rhai)) | various | — | — | yes | — | **not measured**. None has the maintenance or adoption of the options above |

## Measurements

### The layers toy, once per run

| Engine | native n=1000 | native n=10000 | wasm n=1000 | wasm n=10000 | wasm-bindgen output | after `wasm-opt -Oz` | gzipped |
|---|---|---|---|---|---|---|---|
| plain Rust (the floor) | 0.004 ms | 0.021 ms | 0.019 ms | 0.103 ms | 24.9 kB | 17.7 kB | 8.1 kB |
| **ascent** | 1.04 ms | 5.70 ms | 1.40 ms | 5.27 ms | 161 kB | 114 kB | 34.2 kB |
| crepe | 1.22 ms | 13.97 ms | 4.00 ms | 18.26 ms | 139 kB | 118 kB | 30.7 kB |
| datafrog | 0.049 ms | 0.439 ms | 0.271 ms | 0.883 ms | 79.6 kB | 63.9 kB | 25.3 kB |
| ascent-interpreter | 12.9 ms | 111 ms | 10.4 ms | 71.9 ms | 1.32 MB | — | 376 kB (unoptimised) |
| scryer-prolog | 319 ms | 1,174 ms | failed | failed | — | — | — |

The ascent-interpreter and Scryer times are the mean of 5 runs and include
parsing. Scryer's also include building the machine and consulting the
facts. egglog never ran the toy; on a 1,000-edge transitive closure it took
883 ms natively.

Datafrog is fastest because the strata, the order and the counts are Rust
written by hand between its joins. Measured as a language it is "no
language": every rule is a join someone writes. Crepe can't count, so its
layer-4 order and anthem count come from Rust between two programs. Ascent
expresses the whole toy as rules.

### Incremental evaluation

The same ascent program grown into a game state: 300 objects and 30 effects
(two layer-4 effects, 14 targeted setters, 14 anthems), with +1/+1 counters
in layer 7c (CR 613.4c) and a tapped flag, which is not a characteristic
(CR 109.3). One from-scratch run costs 0.34–0.51 ms natively and 0.80 ms in
wasm.

Three changes. **Tap** an unrelated permanent: nothing about
characteristics moves. **Counter**: one +1/+1 counter, a local change.
**Humility**: a setter that makes every creature 1/1, a cascade.

| After | Salsa: `characteristics` re-ran | `creature_count` | `board_label` (reads only the count) | `attackers` (reads tapped) | Version-gated: layers re-ran |
|---|---|---|---|---|---|
| Tap | 0 | 0 | 0 | 1 | 0 |
| Counter | 1 | 1 | **0**: the count came out the same, so Salsa backdated it | 1 | 1 |
| Humility | 1 | 1 | 0 | 1 | 1 |

Salsa's field-level tracking and early cutoff work as documented. But for
one game, a version counter that only characteristic-changing facts bump
gives the same skips (no run on a tap) with no framework. A recompute costs
under a millisecond, and that is cheaper than a human ever clicks.

**Search is different.** Search here means AI, autotap and Gauntlet-style
sampling: 10,000 branches off one state, each with one change and then a
read of the characteristics.

| Strategy | native | wasm |
|---|---|---|
| `imbl` persistent state (O(1) fork) + from scratch | 0.45 ms per branch | 0.47 ms |
| `imbl` fork + version-gated (a tap reuses the parent's result) | **0.27 ms** | **0.31 ms** |
| one Salsa db, apply / read / undo / read | 0.69 ms | 1.25 ms |

Salsa can't fork. Setting an input "blocks until all other workers with
access to this storage have completed", and clones share that storage
([storage.rs, `cancel_others`](https://github.com/salsa-rs/salsa/blob/master/src/storage.rs)).
So the only way to search is to walk one database forward and back. An undo
is a new revision, even when it restores equal values, so it re-runs the
layers too. That is why Salsa is the slowest row. The `im` crate is
archived ([bodil/im-rs](https://github.com/bodil/im-rs)), so `imbl` 7.0.2
is its maintained fork.

Incremental evaluation mostly matters for search, not for playing a game.
At about 0.3 ms a branch, 10,000 branches take 3 s. A search engine will
want that cheaper, and the next lever is a cheaper fixpoint, not a smarter
cache (see the open questions).

## The plan

1. **A separate product.** It is a rules engine, not Insight (a spike) and
   not Gauntlet. It is **Chancellor of the Spires**
   (`crates/chancellor-of-the-spires/`); NAMES_FOR_FUTURE.md says why. Norn's
   Decree and Solve the Equation were the other candidates, and both rested
   on the word alone.
2. **Rust. The game is a typed state machine:** priority, the stack,
   choices, hidden information, randomness. Datalog can't be the whole
   engine, because Magic is Turing complete. Any Turing machine can be
   encoded as a game where every move is forced, so even predicting the
   winner is undecidable
   ([Churchill, Biderman and Herrick, 2019](https://arxiv.org/abs/1904.09828)).
   No fixpoint language without unbounded state can express that.
3. **Ascent for what is true now:** characteristics through the layers
   (CR 613), dependency (CR 613.8), and legality. This is approach (b): fixed
   general rules over Insight's nodes as facts.
4. **Insight's typed AST goes in directly,** with no serialisation and no
   second language.
5. **The first slice is the 571 pinned Modern-metagame cards**
   (`crates/vivisurgeons-insight/golden/modern-meta.txt`). The point is to
   feed back on the AST's shape before months of grammar work fix it in
   place.
6. **Tests are rules questions with one right answer,** like HANDS.md and
   Arena's scripted games.
7. **A whole-corpus diff on every change,** like Insight's `snapshot/` and
   Arena's "weird reaches" tooling: record each test position's derived
   characteristics, and fail on any that moved without being approved.
8. **Two modes over the same pure rules:**
   - **Playing a game and the UI:** recompute from scratch, gated by a
     version counter on characteristic-changing facts.
   - **Search:** persistent `imbl` states, forked per branch, each recomputed
     from scratch or reusing the parent's result when the gate allows.

   Salsa is not needed for either.
9. **A pinned Comprehensive Rules version to build against.** The repo is
   public and the CR is Wizards' text, so the pin follows
   `crates/gitaxian-probe/assets/pin.json`: commit the URL, the effective
   date and the sha256, fetch the text into `~/.cache` by hash, and never
   commit the text. The current version is
   <https://media.wizards.com/2026/downloads/MagicCompRules%2020260925.txt>,
   effective September 25, 2026, sha256
   `8d860e451f20f38865b725b42d82feb714c725373dd8f3b32b8652b3eeb070ca`, with
   3,166 numbered rules ([rules page](https://magic.wizards.com/en/rules)).
   Insight already cites 177 distinct rule numbers in its source, README and
   SEMANTICS.md. The pin has two uses:
   - a test that every "CR nnn.n" citation resolves in the pinned version;
   - when Wizards publishes a new CR, a rule-by-rule diff against the pin,
     intersected with the cited rules. That short list goes to an agent as
     a worklist of the nodes and SEMANTICS.md calls to recheck, and then the
     pin is bumped.

   It is the same principle as the snapshot: pin what you build against,
   and diff when it moves.

## Open questions

- **The cost of a fixpoint per branch.** Ascent's 0.3–0.5 ms at game scale
  is mostly per-run overhead. The plain Rust floor is about 100 times
  faster. Search may want an ascent program that keeps its relations
  between runs (re-running with monotone additions), or `ascent_run!`, or
  hand-written datafrog for the hottest strata. None of this was measured.
- **Retraction.** Compiled ascent only adds facts. A permanent leaving play
  is a retraction, so the plan recomputes. ascent-interpreter and
  differential-dataflow retract. DD's wasm run was not finished.
- **Choices and replacement effects inside the fixpoint.** CR 616 lets the
  affected player choose the order of replacement effects, and a choice is
  the state machine's job, not Datalog's. Where the fixpoint stops and a
  choice resumes is undesigned.
- **Rules at run time.** Approach (b) assumes every node kind gets a general
  rule at compile time. If some cards need rules of their own, as Arena's
  20% do, that is either a new general rule, which is a rebuild, or a
  runtime engine beside ascent. ascent-interpreter runs the same language,
  about 10 times slower, and runs in wasm.
- **Not measured:** the egglog and Scryer wasm failures (why they failed),
  differential-dataflow in wasm, Nemo, CLIPS compiled with emscripten (a
  different target from `wasm32-unknown-unknown`), and the Rete crates.

## Recommendation

**Ascent**, compiled, with abilities as data: Insight's nodes as facts under
one fixed set of general rules. It is the only option that builds and runs
in `wasm32-unknown-unknown`, writes the whole layers toy as rules (counts,
negation and the CR 613.8 order included), and keeps the AST's Rust types,
at 34 kB gzipped and 1.4 ms for 1,000 objects in Node. Recompute from
scratch behind a version gate for play, and fork persistent `imbl` states
for search. Salsa saved nothing a gate didn't, and it lost search by about
2.5 times because it can't fork. Keep ascent-interpreter as the fallback if
some cards really need runtime rules, since it is the same language and also
runs in wasm.
