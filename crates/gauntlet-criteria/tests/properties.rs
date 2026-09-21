//! Properties the exact engine must satisfy for *any* question.
//!
//! `engine.rs` next door pins down the questions someone thought to ask. This
//! file generates the ones nobody did: a library of four cards, a query that
//! matches everything, a gap vector that draws the deck to the bottom.
//!
//! Every test here runs from a fixed seed. A property test that fails one run in
//! fifty is worse than no test, because it teaches people to re-run CI until it
//! goes green. With the seed pinned, the suite either always passes or always
//! fails, and a failure reproduces by running the same test again.

use std::convert::Infallible;

use gauntlet_criteria::mana::Pip;
use gauntlet_criteria::{
    Cost, Count, Counted, Evaluator, Grouping, ManaSource, Palette, PathOutcomes, PathView, Plan,
    Schedule, Zone,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};

type Check = Box<dyn FnMut(&PathView<'_>) -> bool>;
type Tally = Box<dyn FnMut(&PathView<'_>) -> u32>;

struct Closures(Vec<Check>);

impl Evaluator for Closures {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, Infallible> {
        Ok(PathOutcomes {
            held: self.0.iter_mut().map(|f| f(view)).collect(),
            counted: Vec::new(),
        })
    }
}

/// The expectation half: closures answering *how many* rather than *whether*.
struct Counters(Vec<Tally>);

impl Evaluator for Counters {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, Infallible> {
        Ok(PathOutcomes {
            held: Vec::new(),
            counted: self
                .0
                .iter_mut()
                .map(|f| Count::new(f(view)).expect("a count of a drawn card is in range"))
                .collect(),
        })
    }
}

fn only_criteria(n: usize) -> Plan {
    Plan {
        criteria: n,
        expectations: 0,
    }
}

fn only_expectations(n: usize) -> Plan {
    Plan {
        criteria: 0,
        expectations: n,
    }
}

/// A runner with the RNG pinned and failure persistence off.
///
/// Persistence is off because the seed already lives in the source: a
/// `proptest-regressions` file would be an untracked second source of truth, and
/// the Nix build sandbox cannot write next to the test anyway.
fn runner(cases: u32) -> TestRunner {
    TestRunner::new_with_rng(
        Config {
            cases,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    )
}

/// Far below the engine's own 5,000,000, because these run a few hundred times.
const PATH_BUDGET: u128 = 8_000;

/// A library bucketed by which queries its cards match, and the gap vector to
/// draw it with. Everything the engine needs to be asked a question.
#[derive(Debug, Clone)]
struct Question {
    grouping: Grouping,
    gaps: Vec<u32>,
    queries: usize,
}

/// Questions the engine can actually answer.
///
/// Shaped rather than filtered: the draws are clipped to the library and the
/// gap vector is truncated until the enumeration fits the budget, so every
/// generated case asserts something instead of being thrown away as a refusal.
fn question() -> impl Strategy<Value = Question> {
    (1usize..=3)
        .prop_flat_map(|queries| {
            (
                Just(queries),
                prop::collection::vec((0u64..(1u64 << queries), 1u32..=40), 1..=4),
                0u32..=8,
                prop::collection::vec(0u32..=3, 0..=3),
            )
        })
        .prop_map(|(queries, cards, opening, extras)| {
            let names = (0..queries).map(|i| format!("q{i}")).collect();
            let grouping = Grouping::build(names, cards).unwrap();
            let gaps = feasible_gaps(grouping.group_sizes(), opening, &extras);
            Question {
                grouping,
                gaps,
                queries,
            }
        })
}

fn feasible_gaps(groups: &[u32], opening: u32, extras: &[u32]) -> Vec<u32> {
    feasible_gaps_within(groups, opening, extras, PATH_BUDGET)
}

fn feasible_gaps_within(groups: &[u32], opening: u32, extras: &[u32], budget: u128) -> Vec<u32> {
    let population: u32 = groups.iter().sum();
    let mut gaps = Vec::with_capacity(extras.len() + 1);
    let mut drawn = 0;
    for gap in std::iter::once(opening).chain(extras.iter().copied()) {
        let gap = gap.min(population - drawn);
        drawn += gap;
        gaps.push(gap);
    }
    // The engine's own count rather than a copy of the formula: a generator
    // budgeting against a second opinion could produce a question the engine
    // then refuses.
    while gaps.len() > 1 && gauntlet_criteria::compositions(groups.len(), &gaps) > budget {
        gaps.pop();
    }
    gaps
}

/// "At least `k` cards matching `query` by `checkpoint`" — the shape of
/// criterion the exact engine is built for.
#[derive(Debug, Clone, Copy)]
struct Threshold {
    checkpoint: usize,
    query: usize,
    k: u32,
}

/// Generated loose, resolved against the question later: the number of
/// checkpoints and queries is not known until the question exists.
fn loose_thresholds() -> impl Strategy<Value = Vec<(u8, u8, u32)>> {
    prop::collection::vec((any::<u8>(), any::<u8>(), 1u32..=3), 1..=4)
}

fn resolve(loose: &[(u8, u8, u32)], q: &Question) -> Vec<Threshold> {
    loose
        .iter()
        .map(|&(checkpoint, query, k)| Threshold {
            checkpoint: usize::from(checkpoint) % q.gaps.len(),
            query: usize::from(query) % q.queries,
            k,
        })
        .collect()
}

fn checks(thresholds: &[Threshold]) -> Closures {
    Closures(
        thresholds
            .iter()
            .copied()
            .map(|t| {
                Box::new(move |v: &PathView<'_>| {
                    v.count_at(t.checkpoint, t.query, Counted::In(Zone::Hand)) >= t.k
                }) as Check
            })
            .collect(),
    )
}

#[test]
fn every_run_accounts_for_all_of_its_probability_mass() {
    // A criterion true on every path collects the whole enumeration, so its
    // answer *is* the total mass. `run` checks this internally against
    // MASS_TOLERANCE; this asserts it over shapes no fixture covers.
    runner(256)
        .run(&question(), |q| {
            let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
            let r = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_criteria(1),
                &mut ev,
            )
            .map(|o| o.probabilities)
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            prop_assert!(
                (r[0].get() - 1.0).abs() < 1e-12,
                "{:?} over gaps {:?} summed to {}",
                q.grouping.group_sizes(),
                q.gaps,
                r[0].get()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_count_threshold_never_gets_less_likely_as_turns_advance() {
    // "At least k by turn t" is a nested family of events, because counts are
    // cumulative and a later checkpoint has seen everything an earlier one did.
    // A decrease is unambiguously an engine bug, not a rounding artefact.
    let cases = (question(), any::<u8>(), 1u32..=3);
    runner(192)
        .run(&cases, |(q, query, k)| {
            let query = usize::from(query) % q.queries;
            let by_turn: Vec<Threshold> = (0..q.gaps.len())
                .map(|checkpoint| Threshold {
                    checkpoint,
                    query,
                    k,
                })
                .collect();
            let mut ev = checks(&by_turn);
            let r = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_criteria(by_turn.len()),
                &mut ev,
            )
            .map(|o| o.probabilities)
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let ps: Vec<f64> = r.iter().map(|p| p.get()).collect();
            for pair in ps.windows(2) {
                prop_assert!(
                    pair[1] >= pair[0] - 1e-12,
                    "{:?} over gaps {:?}, query {query} k={k}: {ps:?} decreased",
                    q.grouping.group_sizes(),
                    q.gaps
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_single_turn_criterion_matches_the_closed_form_at_least() {
    // One query, one checkpoint: the enumeration reduces to a plain
    // hypergeometric, and the two must not drift apart. The engine counts by
    // walking compositions over groups; `at_least` sums a short tail of the
    // pmf. Nothing but the answer is shared between them.
    let cases = (question(), any::<u8>(), 0u32..=3);
    runner(256)
        .run(&cases, |(q, query, k)| {
            let query = usize::from(query) % q.queries;
            let gaps = &q.gaps[..1];
            let mut ev = checks(&[Threshold {
                checkpoint: 0,
                query,
                k,
            }]);
            let enumerated = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(gaps),
                only_criteria(1),
                &mut ev,
            )
            .map(|o| o.probabilities)
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let closed = pe_stats::at_least(
                q.grouping.population(),
                q.grouping.matching_total(query),
                gaps[0],
                k,
            );
            prop_assert!(
                (enumerated[0].get() - closed.get()).abs() < 1e-11,
                "{:?} drawing {}, {} matching, k={k}: {} vs {}",
                q.grouping.group_sizes(),
                gaps[0],
                q.grouping.matching_total(query),
                enumerated[0].get(),
                closed.get()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn criteria_evaluated_together_get_the_same_answers_as_criteria_evaluated_alone() {
    // One run answers every criterion from a single enumeration, accumulating
    // them side by side. Identical paths in an identical order feed identical
    // compensated sums, so "together" and "alone" must agree bit for bit —
    // anything else is one criterion's accumulator leaking into another's.
    let cases = (question(), loose_thresholds());
    runner(128)
        .run(&cases, |(q, loose)| {
            let thresholds = resolve(&loose, &q);
            let mut ev = checks(&thresholds);
            let together = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_criteria(thresholds.len()),
                &mut ev,
            )
            .map(|o| o.probabilities)
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            for (i, t) in thresholds.iter().enumerate() {
                let mut solo = checks(&[*t]);
                let alone = gauntlet_criteria::run(
                    &q.grouping,
                    &Schedule::plain(&q.gaps),
                    only_criteria(1),
                    &mut solo,
                )
                .map(|o| o.probabilities)
                .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
                prop_assert_eq!(
                    together[i].get(),
                    alone[0].get(),
                    "criterion {:?} answered differently in company",
                    t
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn every_expectation_distribution_is_a_distribution() {
    // The same statement as the mass check above, made about the other kind of
    // answer. A histogram that does not sum to 1 has lost or double-counted a
    // region of the sample space, and every bucket in it is then drawn from the
    // wrong denominator -- which no single bucket can reveal on its own.
    let cases = (question(), any::<u8>());
    runner(256)
        .run(&cases, |(q, query)| {
            let query = usize::from(query) % q.queries;
            let last = q.gaps.len() - 1;
            let mut ev = Counters(vec![
                Box::new(move |v: &PathView<'_>| v.count_at(0, query, Counted::In(Zone::Hand))),
                Box::new(move |v: &PathView<'_>| v.count_at(last, query, Counted::In(Zone::Hand))),
            ]);
            let r = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_expectations(2),
                &mut ev,
            )
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            for (i, d) in r.distributions.iter().enumerate() {
                prop_assert!(
                    (d.total() - 1.0).abs() < 1e-12,
                    "{:?} over gaps {:?}, expectation {i} summed to {}",
                    q.grouping.group_sizes(),
                    q.gaps,
                    d.total()
                );
                prop_assert!(
                    d.probabilities().iter().all(|p| *p >= 0.0),
                    "a negative bucket in {:?}",
                    d.probabilities()
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn an_expectation_matches_the_closed_form_mean() {
    // One query at one checkpoint reduces to a plain hypergeometric, whose mean
    // is draws * successes / population in closed form. The engine walks every
    // composition and weights it; `pe_stats::mean` divides three numbers.
    // Nothing but the answer is shared between them.
    let cases = (question(), any::<u8>());
    runner(256)
        .run(&cases, |(q, query)| {
            let query = usize::from(query) % q.queries;
            let gaps = &q.gaps[..1];
            let mut ev = Counters(vec![Box::new(move |v: &PathView<'_>| {
                v.count_at(0, query, Counted::In(Zone::Hand))
            })]);
            let r = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(gaps),
                only_expectations(1),
                &mut ev,
            )
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let closed = pe_stats::mean(
                q.grouping.population(),
                q.grouping.matching_total(query),
                gaps[0],
            );
            let enumerated = r.distributions[0].mean();
            prop_assert!(
                (enumerated - closed).abs() < 1e-11,
                "{:?} drawing {}, {} matching: enumerated {enumerated} vs closed {closed}",
                q.grouping.group_sizes(),
                gaps[0],
                q.grouping.matching_total(query)
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_threshold_is_the_tail_of_the_distribution_it_thresholds() {
    // The two kinds of answer are the same walk accumulated two ways, so asking
    // the same question both ways must not produce two numbers. This is what
    // stops the histogram and the probabilities drifting apart: a criterion
    // "at least k" is by definition the mass at k and above.
    let cases = (question(), any::<u8>(), 1u32..=3);
    runner(192)
        .run(&cases, |(q, query, k)| {
            let query = usize::from(query) % q.queries;
            let last = q.gaps.len() - 1;

            let mut counting = Counters(vec![Box::new(move |v: &PathView<'_>| {
                v.count_at(last, query, Counted::In(Zone::Hand))
            })]);
            let counted = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_expectations(1),
                &mut counting,
            )
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let buckets = counted.distributions[0].probabilities();
            let tail: f64 = buckets.iter().skip(k as usize).sum();

            let mut checking = checks(&[Threshold {
                checkpoint: last,
                query,
                k,
            }]);
            let held = gauntlet_criteria::run(
                &q.grouping,
                &Schedule::plain(&q.gaps),
                only_criteria(1),
                &mut checking,
            )
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;

            prop_assert!(
                (tail - held.probabilities[0].get()).abs() < 1e-12,
                "{:?} over gaps {:?}, query {query} k={k}: tail {tail} vs criterion {}",
                q.grouping.group_sizes(),
                q.gaps,
                held.probabilities[0].get()
            );
            Ok(())
        })
        .unwrap();
}

/// The tolerance a narrowing is held to.
///
/// Not `==`, and the reason is arithmetic rather than a hedge. A narrowed
/// enumeration sums a *different set of terms* to the same total — fewer,
/// larger ones — so the log-gamma round trip inside each term lands in a
/// different place in the last bits. The floor the engine measures for its own
/// mass check is ~1e-13 over a Commander-sized library; anything above this is
/// a narrowing that changed the question, not a rounding difference. Every
/// figure this tool prints is rounded to six decimal places, so a difference
/// this small cannot reach a report at all.
const SAME_ANSWER: f64 = 1e-12;

#[test]
fn coarsening_away_a_query_nobody_reads_leaves_every_answer_alone() {
    // The group axis of #31. A criterion counting `cat:"Ramp"` cannot tell a
    // Plains from an Island, so the groups a `can_cast` clause needs are groups
    // it must not be charged for — and merging them has to be *exactly* free,
    // because a coarser grouping is a marginal of the finer one and a marginal
    // is not an approximation.
    runner(256)
        .run(&(question(), loose_thresholds()), |(q, loose)| {
            let thresholds = resolve(&loose, &q);
            let schedule = Schedule::plain(&q.gaps);
            let plan = only_criteria(thresholds.len());

            let full =
                gauntlet_criteria::run(&q.grouping, &schedule, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;

            // Exactly the queries these criteria read, and nothing else.
            let keep = thresholds
                .iter()
                .fold(0u64, |bits, t| bits | 1u64 << t.query);
            let coarse = q
                .grouping
                .coarsened(keep, gauntlet_criteria::LandDetail::Ignored);
            prop_assert!(
                coarse.group_sizes().len() <= q.grouping.group_sizes().len(),
                "coarsening cannot add groups"
            );
            prop_assert_eq!(
                coarse.population(),
                q.grouping.population(),
                "and cannot lose a card"
            );

            let narrowed =
                gauntlet_criteria::run(&coarse, &schedule, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused narrowed: {e}")))?;

            for (i, (wide, narrow)) in full
                .probabilities
                .iter()
                .zip(&narrowed.probabilities)
                .enumerate()
            {
                prop_assert!(
                    (wide.get() - narrow.get()).abs() < SAME_ANSWER,
                    "criterion {i} over {:?}: {} un-narrowed, {} narrowed",
                    q.grouping.group_sizes(),
                    wide.get(),
                    narrow.get()
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn collapsing_checkpoints_nobody_reads_leaves_every_answer_alone() {
    // The checkpoint axis. "Loam in hand by turn 5" is one multivariate
    // hypergeometric over eleven cards, not a path through five checkpoints —
    // and a criterion that *does* correlate two turns keeps both of them here,
    // because the checkpoints it reads are exactly the ones it names.
    runner(256)
        .run(&(question(), loose_thresholds()), |(q, loose)| {
            let thresholds = resolve(&loose, &q);
            let schedule = Schedule::plain(&q.gaps);
            let plan = only_criteria(thresholds.len());

            let full =
                gauntlet_criteria::run(&q.grouping, &schedule, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;

            let mut observed: Vec<usize> = thresholds.iter().map(|t| t.checkpoint).collect();
            observed.sort_unstable();
            observed.dedup();
            let collapsed = schedule.narrowed(&observed, gauntlet_criteria::Reading::Cumulative);
            prop_assert_eq!(
                collapsed.gaps().iter().sum::<u32>(),
                q.gaps[..=*observed.last().expect("at least one threshold")]
                    .iter()
                    .sum::<u32>(),
                "the same cards are seen by the last turn anybody asked about"
            );

            let narrowed =
                gauntlet_criteria::run(&q.grouping, &collapsed, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused narrowed: {e}")))?;

            for (i, (wide, narrow)) in full
                .probabilities
                .iter()
                .zip(&narrowed.probabilities)
                .enumerate()
            {
                prop_assert!(
                    (wide.get() - narrow.get()).abs() < SAME_ANSWER,
                    "criterion {i} over gaps {:?} observing {:?}: {} un-narrowed, {} narrowed",
                    q.gaps,
                    observed,
                    wide.get(),
                    narrow.get()
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn narrowing_both_axes_at_once_leaves_every_answer_alone() {
    // Neither narrowing is applied on its own in a real run: a class coarsens
    // its grouping *and* collapses its checkpoints, and the two compose. This
    // is the property the CLI actually depends on.
    runner(256)
        .run(&(question(), loose_thresholds()), |(q, loose)| {
            let thresholds = resolve(&loose, &q);
            let schedule = Schedule::plain(&q.gaps);
            let plan = only_criteria(thresholds.len());

            let full =
                gauntlet_criteria::run(&q.grouping, &schedule, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;

            let keep = thresholds
                .iter()
                .fold(0u64, |bits, t| bits | 1u64 << t.query);
            let mut observed: Vec<usize> = thresholds.iter().map(|t| t.checkpoint).collect();
            observed.sort_unstable();
            observed.dedup();

            let narrowed = gauntlet_criteria::run(
                &q.grouping
                    .coarsened(keep, gauntlet_criteria::LandDetail::Ignored),
                &schedule.narrowed(&observed, gauntlet_criteria::Reading::Cumulative),
                plan,
                &mut checks(&thresholds),
            )
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused narrowed: {e}")))?;

            for (i, (wide, narrow)) in full
                .probabilities
                .iter()
                .zip(&narrowed.probabilities)
                .enumerate()
            {
                prop_assert!(
                    (wide.get() - narrow.get()).abs() < SAME_ANSWER,
                    "criterion {i}: {} un-narrowed, {} narrowed",
                    wide.get(),
                    narrow.get()
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn answering_one_question_of_several_answers_it_the_same_way() {
    // The stitching. A class is answered on its own enumeration and its
    // answers are put back by position, so answering a subset has to give the
    // same numbers in the same order as answering everything.
    runner(128)
        .run(&(question(), loose_thresholds()), |(q, loose)| {
            let thresholds = resolve(&loose, &q);
            let schedule = Schedule::plain(&q.gaps);
            let plan = only_criteria(thresholds.len());

            let together =
                gauntlet_criteria::run(&q.grouping, &schedule, plan, &mut checks(&thresholds))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;

            for (i, _) in thresholds.iter().enumerate() {
                let answering = gauntlet_criteria::Answering::some(plan, vec![i], Vec::new())
                    .expect("one criterion of this plan");
                let alone = gauntlet_criteria::run_answering(
                    &q.grouping,
                    &schedule,
                    &answering,
                    &mut checks(&thresholds),
                )
                .map_err(|e| TestCaseError::fail(format!("{q:?} was refused alone: {e}")))?;
                prop_assert_eq!(alone.probabilities.len(), 1);
                prop_assert!(
                    (together.probabilities[i].get() - alone.probabilities[0].get()).abs()
                        < SAME_ANSWER
                );
            }
            Ok(())
        })
        .unwrap();
}

// --- Restricting the palette (#55) ----------------------------------------

/// A looser budget than [`PATH_BUDGET`], because a castability question is
/// only worth asking past turn 0 — the gate is false on the opening hand by
/// construction — and a six-group manabase spends its whole allowance on the
/// opening seven. A few hundred of these still run in seconds.
const MANA_BUDGET: u128 = 50_000;

/// A manabase and a cost to ask of it.
///
/// Generated as *palettes* rather than as cards, because the thing under test
/// is what the grouping does with a land's colours: five profiles that differ
/// in colours nobody demanded are five groups a cost cannot tell apart, and
/// the whole claim is that merging them changes nothing.
#[derive(Debug, Clone)]
struct ManaQuestion {
    grouping: Grouping,
    gaps: Vec<u32>,
    cost: Cost,
}

fn cost_text() -> impl Strategy<Value = String> {
    (
        0u32..=2,
        prop::collection::vec(
            prop::sample::select(vec!['W', 'U', 'B', 'R', 'G', 'C']),
            0..=3,
        ),
    )
        .prop_map(|(generic, pips)| {
            let mut text = String::new();
            if generic > 0 || pips.is_empty() {
                text.push_str(&format!("{{{generic}}}"));
            }
            for pip in pips {
                text.push_str(&format!("{{{pip}}}"));
            }
            text
        })
}

fn mana_question() -> impl Strategy<Value = ManaQuestion> {
    (
        // One entry per kind of land: what it makes, whether it enters tapped,
        // how many copies.
        prop::collection::vec((0u8..(1 << 6), any::<bool>(), 1u32..=8), 1..=5),
        1u32..=30,
        0u32..=7,
        prop::collection::vec(0u32..=2, 1..=3),
        cost_text(),
    )
        .prop_map(|(lands, spells, opening, extras, text)| {
            let mut cards: Vec<(u64, ManaSource, u32)> = lands
                .into_iter()
                .map(|(bits, enters_tapped, qty)| {
                    let produces = Palette::of(
                        Pip::ALL
                            .into_iter()
                            .filter(|p| bits & (1 << (*p as u8)) != 0),
                    );
                    (
                        0b1,
                        ManaSource::Land {
                            enters_tapped,
                            produces,
                        },
                        qty,
                    )
                })
                .collect();
            cards.push((0b0, ManaSource::Spell, spells));
            let grouping = Grouping::with_mana(vec!["t:land".to_string()], cards).unwrap();
            let gaps = feasible_gaps_within(grouping.group_sizes(), opening, &extras, MANA_BUDGET);
            ManaQuestion {
                grouping,
                gaps,
                cost: Cost::parse(&text).expect("a cost built from payable symbols"),
            }
        })
}

fn castable(cost: &Cost, turns: usize) -> Closures {
    Closures(
        (0..turns)
            .map(|turn| {
                let cost = cost.clone();
                Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)) as Check
            })
            .collect(),
    )
}

#[test]
fn a_cost_is_answered_the_same_on_the_colours_it_demands_alone() {
    // The narrowing of #55, as a property over generated manabases. A cost
    // runs Hall's condition over the pips it demands and nothing else, so two
    // lands agreeing on *those* pips and on tapped-ness are one source to it —
    // and an enumeration keyed on the whole palette is finer than the question
    // it is answering. This asserts the coarser one gives the same number, not
    // a close one.
    runner(192)
        .run(&mana_question(), |q| {
            let schedule = Schedule::plain(&q.gaps);
            let turns = q.gaps.len();
            let plan = only_criteria(turns);

            // The un-narrowed grouping itself, not a coarsening of it: the
            // claim is about what the CLI compares, which is the manabase as
            // the library built it against the one the class asked for.
            let whole = &q.grouping;
            let demanded = q.grouping.coarsened(
                u64::MAX,
                gauntlet_criteria::LandDetail::Pips(q.cost.demands()),
            );
            prop_assert!(
                demanded.group_sizes().len() <= whole.group_sizes().len(),
                "restricting a palette cannot add a group"
            );
            prop_assert_eq!(demanded.population(), whole.population());

            let full =
                gauntlet_criteria::run(whole, &schedule, plan, &mut castable(&q.cost, turns))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let narrowed =
                gauntlet_criteria::run(&demanded, &schedule, plan, &mut castable(&q.cost, turns))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused narrowed: {e}")))?;

            for (turn, (wide, narrow)) in full
                .probabilities
                .iter()
                .zip(&narrowed.probabilities)
                .enumerate()
            {
                prop_assert!(
                    (wide.get() - narrow.get()).abs() < SAME_ANSWER,
                    "{} on turn {turn} over {:?}: {} on the whole palette, {} on {:?}",
                    q.cost.as_str(),
                    q.grouping.group_sizes(),
                    wide.get(),
                    narrow.get(),
                    q.cost.demands().symbols(),
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_battlefield_count_beside_a_cost_survives_the_restriction_too() {
    // The composition the CLI actually builds: a class asking a cost *and*
    // counting what is in play keeps its query bits as well, and both answers
    // have to survive the merge. Counting lands in play reads the same drops
    // the gate does, so a restriction that moved one would move the other.
    runner(128)
        .run(&(mana_question(), 1u32..=3), |(q, k)| {
            let schedule = Schedule::plain(&q.gaps);
            let last = q.gaps.len() - 1;
            let plan = only_criteria(2);
            let questions = |cost: Cost| {
                Closures(vec![
                    Box::new(move |v: &PathView<'_>| v.can_cast(last, &cost)) as Check,
                    Box::new(move |v: &PathView<'_>| {
                        v.count_at(last, 0, Counted::In(Zone::Battlefield)) >= k
                    }) as Check,
                ])
            };

            // The un-narrowed grouping itself, not a coarsening of it: the
            // claim is about what the CLI compares, which is the manabase as
            // the library built it against the one the class asked for.
            let whole = &q.grouping;
            let demanded = q.grouping.coarsened(
                u64::MAX,
                gauntlet_criteria::LandDetail::Pips(q.cost.demands()),
            );

            let full =
                gauntlet_criteria::run(whole, &schedule, plan, &mut questions(q.cost.clone()))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let narrowed =
                gauntlet_criteria::run(&demanded, &schedule, plan, &mut questions(q.cost.clone()))
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused narrowed: {e}")))?;
            for (i, (wide, narrow)) in full
                .probabilities
                .iter()
                .zip(&narrowed.probabilities)
                .enumerate()
            {
                prop_assert!(
                    (wide.get() - narrow.get()).abs() < SAME_ANSWER,
                    "question {i}: {} un-narrowed, {} narrowed",
                    wide.get(),
                    narrow.get()
                );
            }
            Ok(())
        })
        .unwrap();
}
