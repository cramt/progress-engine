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

use pe_criteria::{Count, Evaluator, Grouping, PathOutcomes, PathView, Plan, Zone};
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

/// How many checkpoint paths a question enumerates. The same formula the engine
/// uses to refuse questions that are too wide, mirrored here so the generators
/// can stay inside a budget the test suite can afford.
fn paths(groups: usize, gaps: &[u32]) -> u128 {
    let bins = (groups as u128).saturating_sub(1);
    gaps.iter()
        .map(|&gap| {
            let n = u128::from(gap) + bins;
            (0..bins).fold(1u128, |acc, i| acc.saturating_mul(n - i) / (i + 1))
        })
        .fold(1u128, |a, b| a.saturating_mul(b))
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
    let population: u32 = groups.iter().sum();
    let mut gaps = Vec::with_capacity(extras.len() + 1);
    let mut drawn = 0;
    for gap in std::iter::once(opening).chain(extras.iter().copied()) {
        let gap = gap.min(population - drawn);
        drawn += gap;
        gaps.push(gap);
    }
    while gaps.len() > 1 && paths(groups.len(), &gaps) > PATH_BUDGET {
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
                    v.count_in(t.checkpoint, t.query, Zone::Hand) >= t.k
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
            let r = pe_criteria::run(&q.grouping, &q.gaps, only_criteria(1), &mut ev)
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
            let r = pe_criteria::run(&q.grouping, &q.gaps, only_criteria(by_turn.len()), &mut ev)
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
            let enumerated = pe_criteria::run(&q.grouping, gaps, only_criteria(1), &mut ev)
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
            let together = pe_criteria::run(
                &q.grouping,
                &q.gaps,
                only_criteria(thresholds.len()),
                &mut ev,
            )
            .map(|o| o.probabilities)
            .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            for (i, t) in thresholds.iter().enumerate() {
                let mut solo = checks(&[*t]);
                let alone = pe_criteria::run(&q.grouping, &q.gaps, only_criteria(1), &mut solo)
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
                Box::new(move |v: &PathView<'_>| v.count_in(0, query, Zone::Hand)),
                Box::new(move |v: &PathView<'_>| v.count_in(last, query, Zone::Hand)),
            ]);
            let r = pe_criteria::run(&q.grouping, &q.gaps, only_expectations(2), &mut ev)
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
                v.count_in(0, query, Zone::Hand)
            })]);
            let r = pe_criteria::run(&q.grouping, gaps, only_expectations(1), &mut ev)
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
                v.count_in(last, query, Zone::Hand)
            })]);
            let counted =
                pe_criteria::run(&q.grouping, &q.gaps, only_expectations(1), &mut counting)
                    .map_err(|e| TestCaseError::fail(format!("{q:?} was refused: {e}")))?;
            let buckets = counted.distributions[0].probabilities();
            let tail: f64 = buckets.iter().skip(k as usize).sum();

            let mut checking = checks(&[Threshold {
                checkpoint: last,
                query,
                k,
            }]);
            let held = pe_criteria::run(&q.grouping, &q.gaps, only_criteria(1), &mut checking)
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
