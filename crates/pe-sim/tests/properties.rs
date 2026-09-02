//! Properties the sampler and the exact engine must satisfy together.
//!
//! `acceptance.rs` next door checks the two agree on the handful of questions
//! someone wrote down. This file generates the questions instead, which is the
//! only way to check the thing that actually matters: that the two engines
//! never disagree, on anything, including refusing.
//!
//! Every test here runs from a fixed seed, in both senses — the proptest RNG
//! that picks the questions and the ChaCha stream that deals the hands. A
//! property test that fails one run in fifty is worse than no test, because it
//! teaches people to re-run CI until it goes green. With both seeds pinned, the
//! suite either always passes or always fails, and a failure reproduces by
//! running the same test again.

use std::convert::Infallible;

use pe_criteria::{Evaluator, Grouping, PathView, RunError};
use pe_sim::{simulate, standard_error};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};

type Check = Box<dyn FnMut(&PathView<'_>) -> bool>;

struct Closures(Vec<Check>);

impl Evaluator for Closures {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<Vec<bool>, Infallible> {
        Ok(self.0.iter_mut().map(|f| f(view)).collect())
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

/// Hands per generated question. Big enough that the standard error is a few
/// tenths of a percentage point, small enough that a few dozen questions still
/// run in about a second.
const TRIALS: u32 = 20_000;

/// How far the sampler may sit from the exact answer, in standard errors of its
/// own estimate.
///
/// Under the null — the sampler is correct and the only difference is sampling
/// noise — the estimate is asymptotically normal, so a two-sided five-sigma
/// comparison fails by chance about 5.7e-7 of the time. This file makes at most
/// 160 such comparisons, so a whole run trips by chance about once in eleven
/// thousand. Four sigma would be 6.3e-5 per comparison and about one run in a
/// hundred: flaky enough to teach people to re-run CI, which is the failure mode
/// worth avoiding more than any bug this could catch.
///
/// It is still tight enough to mean something. The bias that got a hand-rolled
/// PRNG thrown out of this project's predecessor was eight standard errors; a
/// sampler off by that much lands inside five sigma with probability 0.0014 per
/// comparison, so it survives all 160 with probability about 1e-459.
///
/// The normal approximation is the weak part: for a criterion that fires on well
/// under a percent of hands the count is Poisson rather than normal and the
/// upper tail is fatter, so the real false-failure rate for those comparisons is
/// nearer 1e-5 than 5.7e-7. That is still comfortably below flaky, and the seeds
/// are fixed anyway — this arithmetic is about how much room a future change to
/// the RNG has, not about a die being rolled every CI run.
const SIGMA: f64 = 5.0;

/// How many checkpoint paths a question enumerates. The same formula the exact
/// engine uses to refuse questions that are too wide, mirrored here so the
/// generators can sit deliberately on either side of it.
fn paths(groups: usize, gaps: &[u32]) -> u128 {
    let bins = (groups as u128).saturating_sub(1);
    gaps.iter()
        .map(|&gap| {
            let n = u128::from(gap) + bins;
            (0..bins).fold(1u128, |acc, i| acc.saturating_mul(n - i) / (i + 1))
        })
        .fold(1u128, |a, b| a.saturating_mul(b))
}

/// Far below the engine's own 5,000,000: every case here also deals 20,000
/// hands, so the enumeration is not allowed to be the expensive half.
const PATH_BUDGET: u128 = 4_000;

/// A library bucketed by which queries its cards match, and the gap vector to
/// draw it with.
#[derive(Debug, Clone)]
struct Question {
    grouping: Grouping,
    gaps: Vec<u32>,
    queries: usize,
}

/// Questions both engines can answer.
///
/// Shaped rather than filtered: the draws are clipped to the library and the gap
/// vector is truncated until the enumeration fits the budget, so every generated
/// case compares two answers instead of being thrown away as a refusal.
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

/// "At least `k` cards matching `query` by `checkpoint`".
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
                Box::new(move |v: &PathView<'_>| v.count(t.checkpoint, t.query) >= t.k) as Check
            })
            .collect(),
    )
}

#[test]
fn sampling_agrees_with_the_exact_engine_within_five_standard_errors() {
    // The exact engine is the oracle. Wherever both can answer, the sampler is
    // only allowed to differ by sampling noise, and it has to say how much noise
    // that is. This is the automated form of the acceptance test that got two
    // hand-rolled PRNGs thrown out of this project's predecessor.
    let cases = (question(), loose_thresholds(), any::<u64>());
    runner(40)
        .run(&cases, |(q, loose, seed)| {
            let thresholds = resolve(&loose, &q);

            let mut ev = checks(&thresholds);
            let exact = pe_criteria::run(&q.grouping, &q.gaps, thresholds.len(), &mut ev)
                .map_err(|e| TestCaseError::fail(format!("exact engine refused: {e}")))?;

            let mut ev = checks(&thresholds);
            let sampled = simulate(&q.grouping, &q.gaps, TRIALS, seed, &mut ev)
                .map_err(|e| TestCaseError::fail(format!("sampler refused: {e}")))?;

            for (i, t) in thresholds.iter().enumerate() {
                let truth = exact[i].get();
                // A criterion that never fires makes the sampler report a
                // standard error of exactly zero, which would turn this into a
                // demand for bit-equality. The rule of three says zero hits in n
                // trials still admits a true rate up to 3/n, so that is the
                // floor.
                let tolerance =
                    SIGMA * standard_error(sampled[i], TRIALS) + 3.0 / f64::from(TRIALS);
                prop_assert!(
                    (sampled[i] - truth).abs() <= tolerance,
                    "{:?} over gaps {:?}, {t:?}, seed {seed}: sampled {} vs exact {truth}, \
                     off by {:.1} standard errors",
                    q.grouping.group_sizes(),
                    q.gaps,
                    sampled[i],
                    (sampled[i] - truth).abs() / standard_error(sampled[i], TRIALS)
                );
            }
            Ok(())
        })
        .unwrap();
}

/// Libraries small enough and hands large enough that about half the generated
/// questions cannot be dealt at all. The refusals are what this generator is
/// for, not noise to be discarded.
fn maybe_undealable() -> impl Strategy<Value = (Grouping, Vec<u32>)> {
    (1usize..=2)
        .prop_flat_map(|queries| {
            (
                Just(queries),
                prop::collection::vec((0u64..(1u64 << queries), 1u32..=12), 1..=3),
                prop::collection::vec(0u32..=12, 1..=2),
            )
        })
        .prop_map(|(queries, cards, gaps)| {
            let names = (0..queries).map(|i| format!("q{i}")).collect();
            (Grouping::build(names, cards).unwrap(), gaps)
        })
}

#[test]
fn neither_engine_answers_a_question_the_other_refuses() {
    // The symmetry that was added after the two engines disagreed by a hundred
    // points: the sampler dealt what it could and answered 100%, the exact
    // engine enumerated nothing and answered 0%, and neither complained.
    //
    // Stated without reference to *why* either refuses, on purpose. A future
    // guard added to one engine and not the other fails this even if both
    // messages look reasonable on their own.
    runner(256)
        .run(&maybe_undealable(), |(grouping, gaps)| {
            let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1)]);
            let exact = pe_criteria::run(&grouping, &gaps, 1, &mut ev);
            // One trial: whether the sampler refuses cannot depend on how many
            // hands it was going to deal.
            let sampled = simulate(&grouping, &gaps, 1, 0xC0FFEE, &mut ev);

            prop_assert_eq!(
                exact.is_err(),
                sampled.is_err(),
                "{:?} over gaps {:?}: exact {}, sampler {}",
                grouping.group_sizes(),
                gaps,
                describe(&exact),
                describe(&sampled)
            );
            if let (Err(e), Err(s)) = (exact, sampled) {
                prop_assert_eq!(
                    e.to_string(),
                    s.to_string(),
                    "same question, same refusal, different words"
                );
            }
            Ok(())
        })
        .unwrap();
}

fn describe<T, E: std::fmt::Display>(r: &Result<T, E>) -> String {
    match r {
        Ok(_) => "answered".to_string(),
        Err(e) => format!("refused ({e})"),
    }
}

/// Questions with enough distinct queries that the exact engine cannot
/// enumerate them: six singleton queries plus the leftovers is seven groups, and
/// two gaps of eight already come to nine million paths.
fn too_wide_to_enumerate() -> impl Strategy<Value = (Grouping, Vec<u32>)> {
    (6usize..=9, 60u32..=90)
        .prop_flat_map(|(queries, leftovers)| {
            (
                Just(queries),
                Just(leftovers),
                prop::collection::vec(1u32..=8, queries),
                prop::collection::vec(8u32..=12, 2..=3),
            )
        })
        .prop_map(|(queries, leftovers, sizes, gaps)| {
            let names = (0..queries).map(|i| format!("q{i}")).collect();
            let cards: Vec<(u64, u32)> = sizes
                .iter()
                .enumerate()
                .map(|(i, &n)| (1u64 << i, n))
                .chain(std::iter::once((0, leftovers)))
                .collect();
            (Grouping::build(names, cards).unwrap(), gaps)
        })
}

#[test]
fn a_question_too_wide_to_enumerate_is_still_answerable_by_sampling() {
    // The one asymmetry that is deliberate, asserted so that it stays
    // deliberate. Sampling is the escape hatch for questions enumeration cannot
    // reach, so `TooWide` is the refusal the sampler must *not* copy — if it
    // ever starts to, the escape hatch has quietly closed.
    runner(64)
        .run(&too_wide_to_enumerate(), |(grouping, gaps)| {
            prop_assert!(
                paths(grouping.group_sizes().len(), &gaps) > 5_000_000,
                "generator produced an enumerable question"
            );
            let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1)]);
            let exact = pe_criteria::run(&grouping, &gaps, 1, &mut ev);
            prop_assert!(
                matches!(exact, Err(RunError::TooWide { .. })),
                "{:?} over gaps {:?}: {}",
                grouping.group_sizes(),
                gaps,
                describe(&exact)
            );
            prop_assert!(simulate(&grouping, &gaps, 200, 0xC0FFEE, &mut ev).is_ok());
            Ok(())
        })
        .unwrap();
}

#[test]
fn sampled_counts_never_decrease_as_turns_advance() {
    // The sampler snapshots its cumulative counts at every checkpoint boundary,
    // including gaps of zero, in a loop that is easy to get off by one. This
    // holds per hand rather than in aggregate, so it needs no tolerance: a
    // single trial where a count goes backwards drags the proportion off 1.
    runner(128)
        .run(&(question(), any::<u64>()), |(q, seed)| {
            let checkpoints = q.gaps.len();
            let queries = q.queries;
            let mut ev = Closures(vec![Box::new(move |v: &PathView<'_>| {
                (1..checkpoints)
                    .all(|t| (0..queries).all(|qi| v.count(t, qi) >= v.count(t - 1, qi)))
            })]);
            let held = simulate(&q.grouping, &q.gaps, 2_000, seed, &mut ev)
                .map_err(|e| TestCaseError::fail(format!("sampler refused: {e}")))?;
            prop_assert_eq!(
                held[0],
                1.0,
                "{:?} over gaps {:?}, seed {}: counts went backwards in some hand",
                q.grouping.group_sizes(),
                q.gaps,
                seed
            );
            Ok(())
        })
        .unwrap();
}
