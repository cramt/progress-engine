//! Properties the exact enumerators must satisfy for *any* input.
//!
//! The known-answer tests next door catch the bug someone thought of. These
//! catch the shapes nobody wrote a fixture for: nine groups of one card, a gap
//! vector that draws the library down to nothing, a query that matches every
//! group at once.
//!
//! Every test here runs from a fixed seed. A property test that fails one run in
//! fifty is worse than no test, because it teaches people to re-run CI until it
//! goes green. With the seed pinned, the suite either always passes or always
//! fails, and a failure reproduces by running the same test again — the case
//! sequence does not depend on the clock, the machine or the run count.

use chip_stats::{self as h, KahanSum};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

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

/// How many checkpoint paths a question enumerates: compositions of each gap
/// over the groups, multiplied across gaps. Same formula `gauntlet_criteria` uses to
/// refuse questions that are too wide, kept here so the generators can stay
/// inside a budget the test suite can afford.
fn paths(groups: usize, gaps: &[u32]) -> u128 {
    let bins = (groups as u128).saturating_sub(1);
    gaps.iter()
        .map(|&gap| {
            let n = u128::from(gap) + bins;
            (0..bins).fold(1u128, |acc, i| acc.saturating_mul(n - i) / (i + 1))
        })
        .fold(1u128, |a, b| a.saturating_mul(b))
}

/// Wide enough to cover the interesting shapes, small enough that a hundred
/// cases still run in a fraction of a second.
const PATH_BUDGET: u128 = 8_000;

fn group_sizes() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(1u32..=40, 1..=5)
}

fn groups_and_draws() -> impl Strategy<Value = (Vec<u32>, u32)> {
    group_sizes().prop_flat_map(|groups| {
        let population: u32 = groups.iter().sum();
        (Just(groups), 0u32..=population.min(12))
    })
}

fn groups_and_gaps() -> impl Strategy<Value = (Vec<u32>, Vec<u32>)> {
    (
        group_sizes(),
        0u32..=8,
        prop::collection::vec(0u32..=3, 0..=3),
    )
        .prop_map(|(groups, opening, extras)| {
            let gaps = feasible_gaps(&groups, opening, &extras);
            (groups, gaps)
        })
}

/// Clip a generated gap vector to something the enumerator will actually walk:
/// never more cards than the library holds, never more paths than the budget.
///
/// Shaping rather than filtering, deliberately. A generator that threw away the
/// out-of-range cases would spend most of its cases asserting nothing; this one
/// turns every draw into a question the engine can answer.
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

/// A query is a set of groups: cards matching it may live in several groups at
/// once, which is the case that breaks naive inclusion-exclusion.
fn query_mask(len: usize) -> impl Strategy<Value = Vec<bool>> {
    prop::collection::vec(any::<bool>(), len)
}

fn selected(counts: &[u32], mask: &[bool]) -> u32 {
    counts
        .iter()
        .zip(mask)
        .filter(|(_, keep)| **keep)
        .map(|(n, _)| n)
        .sum()
}

#[test]
fn the_composition_enumeration_sums_to_one_for_any_group_shape() {
    runner(256)
        .run(&groups_and_draws(), |(groups, draws)| {
            let mut mass = KahanSum::new();
            h::for_each_composition(&groups, draws, |_, p| mass.add(p));
            prop_assert!(
                (mass.total() - 1.0).abs() < 1e-12,
                "{groups:?} drawing {draws} summed to {}",
                mass.total()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn the_checkpoint_enumeration_sums_to_one_for_any_gap_vector() {
    runner(256)
        .run(&groups_and_gaps(), |(groups, gaps)| {
            let mut mass = KahanSum::new();
            h::for_each_checkpoint_path(&groups, &gaps, |_, p| mass.add(p));
            prop_assert!(
                (mass.total() - 1.0).abs() < 1e-12,
                "{groups:?} over gaps {gaps:?} summed to {}",
                mass.total()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_composition_marginal_matches_the_closed_form_at_least() {
    // The enumeration and the closed form answer the same question by different
    // routes, so they must not drift apart. Generalises
    // `composition_marginals_match_the_univariate_case` off its one fixture.
    let cases = groups_and_draws().prop_flat_map(|(groups, draws)| {
        let n = groups.len();
        (Just(groups), Just(draws), query_mask(n), 0u32..=3)
    });
    runner(256)
        .run(&cases, |(groups, draws, mask, k)| {
            let population: u32 = groups.iter().sum();
            let successes = selected(&groups, &mask);
            let enumerated = h::probability_that(&groups, draws, |c| selected(c, &mask) >= k);
            let closed = h::at_least(population, successes, draws, k);
            prop_assert!(
                (enumerated.get() - closed.get()).abs() < 1e-11,
                "{groups:?} drawing {draws}, {successes} successes, k={k}: {} vs {}",
                enumerated.get(),
                closed.get()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_monotone_criterion_never_gets_less_likely_as_turns_advance() {
    // Counts are cumulative, so "at least k by checkpoint t" is a nested family
    // of events. A later checkpoint has seen everything an earlier one did, and
    // any decrease is unambiguously an enumeration bug.
    let cases = groups_and_gaps().prop_flat_map(|(groups, gaps)| {
        let n = groups.len();
        (Just(groups), Just(gaps), query_mask(n), 1u32..=3)
    });
    runner(192)
        .run(&cases, |(groups, gaps, mask, k)| {
            // One enumeration, one accumulator per checkpoint: the checkpoints
            // are compared over exactly the same paths rather than over separate
            // walks that happen to agree.
            let mut hits = vec![KahanSum::new(); gaps.len()];
            h::for_each_checkpoint_path(&groups, &gaps, |hist, p| {
                for (t, total) in hits.iter_mut().enumerate() {
                    if selected(&hist[t], &mask) >= k {
                        total.add(p);
                    }
                }
            });
            let by: Vec<f64> = hits.into_iter().map(KahanSum::total).collect();
            for pair in by.windows(2) {
                prop_assert!(
                    pair[1] >= pair[0] - 1e-12,
                    "{groups:?} over gaps {gaps:?}, k={k}: {by:?} decreased"
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn splitting_the_draws_across_checkpoints_leaves_the_final_marginal_unchanged() {
    // Dealing 7 then 1 then 1 must reach the same distribution as dealing 9 at
    // once, or the turn-indexed engine and the single-turn one disagree about
    // the same deck.
    let cases = groups_and_gaps().prop_flat_map(|(groups, gaps)| {
        let n = groups.len();
        (Just(groups), Just(gaps), query_mask(n), 0u32..=3)
    });
    runner(128)
        .run(&cases, |(groups, gaps, mask, k)| {
            let total: u32 = gaps.iter().sum();
            let last = gaps.len() - 1;
            let stepwise =
                h::probability_that_path(&groups, &gaps, |hist| selected(&hist[last], &mask) >= k);
            let at_once = h::probability_that(&groups, total, |c| selected(c, &mask) >= k);
            prop_assert!(
                (stepwise.get() - at_once.get()).abs() < 1e-11,
                "{groups:?} over gaps {gaps:?}, k={k}: {} vs {}",
                stepwise.get(),
                at_once.get()
            );
            Ok(())
        })
        .unwrap();
}

/// A walk whose sized gaps are decided from the path: after any checkpoint
/// that dealt a card of group 0, deal `size` more — but only while the
/// history holds at most `more` checkpoints, so no path deals more than `more`
/// sized gaps and the walk stays small.
struct Sizing {
    size: u32,
    more: usize,
    mass: KahanSum,
}

impl h::Walk for Sizing {
    fn removals(&mut self, _: h::Path<'_>, _: &mut [u32]) {}
    fn gap(&mut self, reached: h::Path<'_>) -> u32 {
        let now = reached[reached.len() - 1][0];
        let before = reached.len().checked_sub(2).map_or(0, |p| reached[p][0]);
        if now > before && reached.len() <= self.more {
            self.size
        } else {
            0
        }
    }
    fn path(&mut self, _: h::Path<'_>, p: f64) {
        self.mass.add(p);
    }
}

#[test]
fn a_sized_walk_sums_to_one_whatever_the_path_asks_for() {
    // Every deal is one hypergeometric over what the path left, however its
    // size was decided, so the paths still partition the sample space.
    //
    // Shaped to leave room for every sized card before the last fixed gap: a
    // fixed gap the library cannot cover loses its paths, which is the
    // caller's feasibility check to prevent and not what this asserts.
    let cases = (groups_and_gaps(), 0u32..=3, 0usize..=2);
    runner(64)
        .run(&cases, |((groups, gaps), size, more)| {
            let spare = groups.iter().sum::<u32>() - gaps.iter().sum::<u32>();
            let size = size.min(spare / (more.max(1) as u32));
            let mut walk = Sizing {
                size,
                more,
                mass: KahanSum::new(),
            };
            h::for_each_checkpoint_path_sized(&groups, &gaps, &mut walk);
            prop_assert!(
                (walk.mass.total() - 1.0).abs() < 1e-12,
                "{groups:?} over gaps {gaps:?}, sized {size} up to {more} more: {}",
                walk.mass.total()
            );
            Ok(())
        })
        .unwrap();
}
