# Which tests actually bite? A mutation audit

[Issue #77](https://github.com/cramt/progress-engine/issues/77). `cargo-mutants`
27.1.0 was run over the code that decides numbers: `chip-stats`, the query
half of `chip-scryfall`, `gauntlet-toml`, and `gauntlet-criteria`'s mana code,
zones and the budget/tutor half of the walk. A mutant that **survives** is a
change to the engine that no test noticed. This file lists every survivor,
says whether it could change a reported number, and says which of the
important ones the tests added alongside it now kill.

Nothing here changes engine behaviour. Two survivors turned out to point at
behaviour that looks wrong rather than at a missing test; they are
[reported below](#suspected-bugs-reported-not-fixed) and left alone.

## How it was run

Four CPUs, shared with other sessions' builds for most of the run (load
average 20–35), so wall times below are pessimistic and the timeouts are
partly load.

| Run | Files | Tests run per mutant | Wall time |
|---|---|---|---|
| 1 | `chip-stats` `lib.rs`; `chip-scryfall` `lib.rs`, `parse.rs`, `mana.rs`; `gauntlet-toml` `lib.rs` | the mutated package's own tests | 35 min |
| 3 | `gauntlet-criteria` `mana.rs`, `zone.rs` (report-text functions excluded: `Display`, `as_str`, `letter`, `symbols`) | `gauntlet-criteria --lib --test engine` | 12 min |
| 4 | `gauntlet-criteria` `effect.rs`, only `Board::{cast, can_pay, can_cast, cast_by, played_by, fetch, fetch_on_cast, unrevealed, resolve_pending, declared_drop, land_drop, count_at}` | `gauntlet-criteria --lib --test engine` | 10 min |

```
cargo mutants --jobs 3 --timeout-multiplier 3 -f <files>                       # run 1
cargo mutants --jobs 3 --timeout 90 --test-package gauntlet-criteria \
  --exclude-re "Display|as_str|letter|symbols" -f <files> -- --lib --test engine   # run 3
cargo mutants --jobs 3 --timeout 90 --test-package gauntlet-criteria \
  --re "Board<'a>::(cast|can_pay|…|count_at)\b" -f effect.rs -- --lib --test engine # run 4
```

Run 2 was a first attempt at run 3 that used all of `gauntlet-criteria`'s tests plus
`gauntlet-toml`'s: its baseline was 47 s of tests under that load, which put
the file at well over an hour, and it was stopped after 16 mutants (14 caught,
1 timeout, 1 unviable — none of them survivors below). Runs 3 and 4 therefore
leave out `criteria/tests/properties.rs` (13 s of proptest), `gauntlet-toml`,
`gauntlet-sim` and `gauntlet-cli`. A survivor of runs 3 and 4 is "no *unit or
engine* test notices", not "nothing in the workspace notices": the CLI tests
in particular exercise the budget end to end and probably catch some of
them. One data point: re-testing run 3's first survivors against all of
`gauntlet-criteria` + `gauntlet-toml` + `gauntlet-sim` before it too was
stopped for time, `Palette::makes -> true` still survived.

Finally every survivor was re-tested with `--iterate` against the tree with
the new tests, using the same test selection as its run; the "after" column is
that re-run. Mutation runtime in all, the stopped attempts included, was about 95 minutes.

## Totals

"Missed after" is the survivors re-tested with the new tests.
Timeouts are mutants that turned a loop infinite (`i += 1` into `i *= 1`) and
count as detected; unviable ones did not compile.

| Area | Mutants | Caught | Missed | Timeout | Unviable | Missed after |
|---|---|---|---|---|---|---|
| `chip-stats` | 174 | 136 | 34 | 1 | 3 | 4 (benign) |
| `chip-scryfall` query (`lib.rs`, `parse.rs`, `mana.rs`) | 290 | 213 | 34 | 26 | 17 | 4 (benign) |
| `gauntlet-toml` | 227 | 158 | 47 | 1 | 21 | 11 (benign) |
| `gauntlet-criteria` mana + zones | 155 | 111 | 34 | 3 | 7 | 7 (benign) |
| `gauntlet-criteria` budget/tutor walk | 110 | 88 | 17 | 3 | 2 | 3 (important: `land_drop`) |
| **Total** | **956** | **706** | **166** | **34** | **50** | **29** (26 benign, 3 important) |

Mutants skipped entirely, for time: the rest of `effect.rs` (the look/route
walk, bottoming, `Board::new`, ~80 mutants), `gauntlet-criteria`'s `lib.rs`
(run orchestration, 110), `strategy.rs` (mulligan optimiser, 167),
`grouping.rs` (49), `mulligan.rs` (38), `schedule.rs` (57), `policy.rs` (22),
all of `gauntlet-sim` and `gauntlet-cli`, and `chip-scryfall`'s index, bulk
and legality modules.

## Survivors, by file

**Important** means the mutant could change a number the tool reports, or
turn a refusal into a confident answer. **Benign** says why not. **Killed**
names the test added for it.

### `crates/reality-chip/stats/src/lib.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 470–534 | every mutant of `Distribution::{probabilities, total, mean, sd}` and `DistributionBuilder::{add, build}` (26) | **Important.** Nothing in `chip-stats` built a `Distribution`; the histogram, mean and SD every `[[expect]]` prints go through it. | `a_distribution_is_the_histogram_it_was_built_from` |
| 345 | `>` → `>=`, `>` → `==`, `+` → `-`, `+` → `*` in `for_each_checkpoint_path_removing_after` | **Important.** The over-draw guard: `>=` drops the path that draws every card left; `*` drops walks that are feasible. | `a_resumed_walk_can_draw_every_card_left` |
| 104 | `-` → `+` in `pmf` | Benign: the guard is redundant, `ln_choose` already returns −∞ there. | — |
| 171 | `idx + 1` → `idx * 1` in `walk` | Benign: a looser lower bound; the extra branches reach no leaf. Performance only. | — |
| 412 | `<` → `<=`, `+` → `*` in `descend` | Benign: asks for removals at the last checkpoint too, which nothing reads. Performance only (the comment says a third of the run). | — |

### `crates/reality-chip/scryfall/src/lib.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 132 | `\|=` → `&=` in `Colors::parse` | **Important.** `produces:c` became the empty set, which every card contains. | `produces_c_is_the_colourless_symbol_and_asks_for_it_alongside_a_colour` |
| 165 | `&` → `\|` in `Colors::intersect` | **Important.** Devotion counted every coloured symbol, not the colours asked. | `devotion_counts_only_symbols_of_the_colours_asked_about` |
| 298–303 | delete arm `"c"`, `"r"`, `"s"`, `"m"`, `"b"` in `Rarity::parse` | **Important** (a query key; `r:c` stops parsing). | `every_one_letter_rarity_is_its_rarity` |
| 458, 460, 462 | `c<`, `c>`, `c!=` over a colour set | **Important.** Strict and negated colour comparisons were untested. | `strict_and_negated_colour_comparisons_are_not_their_loose_forms` |
| 492 | `==` → `!=` for `restricted:` | **Important.** | `format_legality_is_asked_by_name` (strengthened, syntax.rs) |
| 502, 504 | `m<` and `m!=` | **Important.** | `strict_and_negated_mana_cost_comparisons_are_not_their_loose_forms` |
| 549, 593 | delete the `Not` arm of `collect_unknown_keywords` / `collect_unknown_tags` | **Important.** `-otag:mill` on an index without that tag would answer instead of refusing — the tagless-index failure CLAUDE.md warns about. | `a_negated_typo_is_still_a_typo` |
| 629 | `\|\|` → `&&` in `has_keyword` | **Important.** `is:partner` missed "Partner with". | `a_partner_keyword_with_more_after_it_is_still_partner` |
| 652–654 | `&&` → `\|\|` in `is:bear` | **Important.** | `a_bear_is_all_three_of_two_mana_a_creature_and_two_by_two` |
| 679 | delete `!` in `is:hybrid` | **Important.** Hybrid and Phyrexian swapped. | `hybrid_and_phyrexian_are_told_apart` |
| 161 | `\|` → `^` in `Colors::union` | Benign: only used to fold the colours of a devotion term's symbols, which must already agree; only `{U}{U/P}`-style spellings could tell. Still survives. | — |

### `crates/reality-chip/scryfall/src/mana.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 59 | guard `is_ascii_alphabetic()` → `true` | **Important.** The space around a split card's ` // ` became a symbol, so `m={W}` missed Wear // Tear. | `each_face_of_a_split_cost_is_read_without_the_separator` |
| 53 | `<` → `<=` | **Important** (panics on a cost that ends in a digit, `m=3`). | `a_bare_generic_cost_is_a_cost_and_not_an_empty_one` |
| 137 | `==` → `!=` in `is_empty` | **Important.** An all-generic cost read as empty and was refused. | same |
| 169 | `==` → `!=` in `normalize` | **Important.** `{G/B}` and `{B/G}` stopped being one symbol (WUBRG order only held for pairs involving white). | `a_hybrid_is_the_same_symbol_in_either_order_for_every_pair` |
| 49 | `end + 1` → `end * 1` | Benign: the loop then steps over the `}` itself. Equivalent. | — |
| 137 | `is_empty -> false` | Benign: only a whitespace-only `m:" "` reaches it, and then answers instead of refusing. | — |

### `crates/reality-chip/scryfall/src/parse.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 295 | delete arm `ColorField::Produces` | **Important.** `produces:wc` dropped the `{C}`. | `produces_c_is_…` |
| 159, 163 | the quoted-value loop | **Important.** `name:"Opt" t:instant` swallowed the next term; an unterminated quote panicked. | `a_quoted_value_ends_at_its_quote_and_tolerates_a_missing_one` |
| 193 | `\|\|` → `&&` in `split_term` | Benign: a term whose key has a non-letter (`a1:x`) becomes an unknown-key refusal instead of a name search. Fails safe. | — |

### `crates/ichormoon-gauntlet/toml/src/lib.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 302 | `<=` → `>` in `Bounds::holds` (`min`+`max`) | **Important.** A range read its upper bound backwards; the only test asserted it was *narrower*, which the mutant still was. | `clauses_are_anded_and_a_range_is_two_sided` (strengthened with the closed form) |
| 2105 | `>` → `>=` in `bounds` | **Important.** `min = max` refused. | `a_range_of_one_value_is_exactly_that_many` |
| 1995 | `>` → `<` in `conjunction` | **Important.** A clause with `query` and `cast` answered one of them. | `a_clause_asking_two_questions_is_refused_rather_than_answering_one` |
| 658, 1760 | `Criteria::casting`, `casting_of` | **Important.** The `[casting]` priority lost or garbled. | `a_file_says_what_it_casts_and_which_questions_count_castings` |
| 676, 685, 775 | `counts_castings`, `casts` | **Important.** Refusal of a `cast` question with no `[casting]`, and loading the mana data. | same |
| 695 | `expectations -> []` | **Important.** | same |
| 747, 754, 755 | `battlefield_queries` | **Important.** The refusal of a battlefield question about a spell. | `battlefield_queries_come_from_expectations_too_once_each` |
| 843–954 | `Criteria::reads`, `Reads::{of, count, at, queries, turns, demands, battlefield}` (18) | **Important.** Sizes each class's enumeration; a question reading less than it asks runs on a grouping too coarse for it. | `what_each_question_reads_is_what_it_names` |
| 570 | `fetched_name` | Benign: report wording. | — |
| 621 | `EffectLibrary::is_empty -> false` | Benign: builds an empty effects narrowing that narrows nothing. | — |
| 964 | `fork -> None` | Benign: the engine falls back to one thread; same numbers. | — |
| 1393, 1579–1580, 1686, 1868 | `locate`, error positions, which refusal wording | Benign: the text and line number of an error, never a number. | — |

### `crates/ichormoon-gauntlet/criteria/src/mana.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 121 | `Palette::makes -> true`, `&` → `\|` | **Important.** A forced source could pay a pip it does not make, so an all-pip cost forced through the land played this turn read as payable. Also survives `properties.rs`, `gauntlet-toml` and `gauntlet-sim`. | `a_forced_source_pays_once_and_only_its_own_pip` |
| 589, 596 | `covers`' spent-source guards | **Important.** The forced land stayed in the pool, or paid every pip of its kind. | same |
| 616 | `&` → `\|`, `<<` → `>>` in the subset loop | **Important.** Hall's condition on proper subsets not checked: one Island and two Plains paid `{U}{U}{W}`. | `two_blue_pips_need_two_blue_sources_however_many_lands_there_are` |
| 664, 670 | `&&` → `\|\|`, `>` → `>=` in `forced` | **Important.** An eligible source satisfied the obligation without paying. | `a_forced_source_pays_once_…` |
| 136 | `\|` → `^` in `Palette::union` | **Important.** `LandDetail::join` of two classes demanding the same pip erased it, merging lands the cost tells apart. | `joining_two_details_keeps_everything_either_kept` |
| 384, 396, 398, 418 | `Cost::parse`: whitespace, multi-digit shorthand, trailing digit, the `MAX_COST` bound | **Important** for the first three (a pasted cost refused, `12U` read as three, a panic); the bound is at the edge of a work limit. | `a_cost_reads_spaces_multi_digit_generic_and_its_bound` |
| 117 | `\|` → `^` in `Palette::of` | Benign: only called on distinct pips. | — |
| 125 | `Palette::is_empty` (3) | Benign: not on the enumeration path. | — |
| 452, 456 | `Cost::total -> 0`, `Cost::is_free` | Benign: the walk calls `Demand::total`/`is_free`, not `Cost`'s; only a unit test reads these. | — |
| 653 | `>` → `<` in `forced` | Benign: equivalent; a cost with generic then takes the pip loop, which can only return true when `covers(None)` would. | — |

### `crates/ichormoon-gauntlet/criteria/src/zone.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 90, 120–123, 158 | `Counted::zone`, `Zone::parse` arms, `Reachable::includes` | **Important** (which zone a clause counts; which zeros are labelled *not modelled*). Probably caught by `gauntlet-toml` and `gauntlet-cli`; not by the engine tests. | `every_zone_reads_back_as_itself`, `only_the_contingent_zones_depend_on_the_run` |

### `crates/ichormoon-gauntlet/criteria/src/effect.rs`

| Line | Mutant | Judgement | Killed by |
|---|---|---|---|
| 940 | `live_hand -= 1` → `+=`/`/=` in `cast` | Timed out rather than survived, both before and after: with a `{0}` spell the budget loop never ends. A cast spell staying castable is **important**, so a test now asks it at a `{U}` cost as well; the re-run still reports a timeout, because the free-tutor test loops first. | `a_spell_that_is_cast_leaves_the_hand_for_good` |
| 1034, 1065 | `removed += 1` → `*=`; `- removed` → `+` | **Important.** A tutor could find the same card twice. | `two_tutors_do_not_find_one_card_twice` |
| 1052 | `live_bottomed -= 1` → `+=`/`/=` | **Important.** A card the mulligan bottomed could be tutored twice. | `a_card_the_mulligan_bottomed_is_found_once_not_twice` |
| 1006, 1007 | `fetch_on_cast` to the battlefield | **Important.** | `a_tutor_to_the_battlefield_puts_its_card_in_play_and_out_of_the_library` |
| 973 (and 967, a timeout) | `resolve_pending`: a delayed fetch to hand | **Important.** No test had a delayed fetch that goes to hand. | `a_delayed_fetch_to_hand_arrives_in_hand_when_it_fires` |
| 1196, 1205 | `count_at` library `- cast_by`, battlefield `+ cast_by` | **Important.** | `a_cast_spell_is_counted_in_play_and_not_in_the_library` |
| 1378, 1381 | `can_pay` line three: which land "arrived", and the pool it adds to | **Important.** A land held since turn 1 could be played twice; a land drawn this turn replaced the pool instead of joining it. | `the_gate_does_not_play_a_land_that_did_not_arrive_this_turn`, `a_land_that_arrives_this_turn_pays_beside_the_ones_already_down` |
| 1114, 1117, 1120 | `land_drop` with no declared priority: which effect's land, whether one is left to play, the deeper-look tie rule | **Important**, not killed here: they need a surveil-style deck with two effects of different depth, and are left as a gap. | — |

## Suspected bugs (reported, not fixed)

Two mutants led to hands where the current code gives an answer that looks
wrong. Neither is changed here; both want a HANDS.md entry deciding them.

1. **A castable permanent held in hand is counted on the battlefield when no
   `[land_drop]` is declared.** `Board::played_by` without a declared priority
   runs the use-it-or-lose-it recurrence over *every* card matching the query
   in hand, as if each were a land. `battlefield_queries` lets a spell through
   when `[casting]` names it (`stranded_matching`), and `count_at` then adds
   `played_by + cast_by`. Observed with the real CLI on
   `cli/tests/fixtures/hand-tutor.txt` and `tutor-index.jsonl`, `[casting]
   prefer = ['name:"Lantern of Insight"']`, turn 1 on the play:

   | question | answer |
   |---|---|
   | Lantern in hand on turn 1 | 0.88% |
   | Lantern cast by turn 1 | 57.45% |
   | Lantern on the battlefield on turn 1 | **58.33%** |

   58.33% is 7/12, the chance of having *seen* it — the uncast copies in hand
   are being counted as in play, which is the "drawn under another name"
   failure `refuse_unmodelled_mana`'s comment says it prevents. With
   `[land_drop]` declared, `played_by` reads the line and the count is right
   (the engine test above declares one for that reason).

2. **A tutor that puts a card onto the battlefield on cast shows it on the next
   turn, not this one.** `Board::walk` snapshots `landed[turn]` and
   `played_at[turn]` before `cast` runs, and after it re-records only the hand
   and the cast counts. So on the turn a cast-triggered fetch puts a card into
   play, that turn's battlefield count misses it and its library count still
   includes it; from the next turn on both are right. In the engine test
   above, on the one deal in seven where the tutor fetches: turn 1 library
   count 1, turn 2 library count 0. The CLI refuses non-land targets for this
   (`FetchNonLandToBattlefield`), but a land target — a Rampant Growth-shaped
   effect — is let through. The sampler has not been checked for the same
   timing.
