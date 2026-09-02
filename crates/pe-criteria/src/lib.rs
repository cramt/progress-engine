//! Turning "which cards match which queries" into exact probabilities.
//!
//! The load-bearing idea: a criterion is only allowed to ask *how many* cards
//! matching a query it has drawn, never which ones. That restriction is what
//! keeps the engine exact. Cards matching the same set of queries are then
//! interchangeable, so the tool enumerates compositions over those groups and
//! evaluates each criterion **once per composition** rather than once per
//! simulated hand — which is both exact and far faster than sampling.
//!
//! The escape hatch for criteria that genuinely need individual cards is
//! simulation, and it is a different engine with different guarantees.

mod grouping;

pub use grouping::{Grouping, GroupingError};

use pe_stats::{Path, Probability};

/// One named acceptance criterion, optionally with a threshold it must meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    pub name: String,
    /// The assertion: this criterion must hold at least this often.
    pub at_least: Option<f64>,
}

/// What a criterion is allowed to see: counts, never cards.
pub struct PathView<'a> {
    grouping: &'a Grouping,
    history: Path<'a>,
}

impl<'a> PathView<'a> {
    pub fn new(grouping: &'a Grouping, history: Path<'a>) -> Self {
        PathView { grouping, history }
    }

    pub fn checkpoints(&self) -> usize {
        self.history.len()
    }

    /// How many cards matching `query_idx` have been drawn by `checkpoint`.
    ///
    /// Returns 0 for an out-of-range checkpoint rather than panicking: the
    /// caller is often JavaScript, and a criterion asking about turn 9 of a
    /// 5-turn run should be false, not a crash.
    pub fn count(&self, checkpoint: usize, query_idx: usize) -> u32 {
        let Some(counts) = self.history.get(checkpoint) else {
            return 0;
        };
        self.grouping.count_matching(counts, query_idx)
    }
}

/// Evaluates every criterion against one path through the checkpoints.
pub trait Evaluator {
    type Error;

    /// One bool per criterion, in registration order.
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<Vec<bool>, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum RunError<E> {
    #[error(
        "this question is too wide to answer exactly: {paths} compositions across {groups} groups.\n\
         Reduce the number of distinct queries, or ask about an earlier turn."
    )]
    TooWide { paths: u128, groups: usize },
    #[error("the library is empty: every card in the list is a commander or outside the deck")]
    EmptyLibrary,
    /// A hand that cannot be dealt enumerates to no paths at all, so every
    /// criterion would collect zero probability mass and report a confident 0%.
    /// Refuse instead — the sampler, which clamps to the library and answers a
    /// different question, refuses the same way.
    #[error("this question draws {draws} cards from a library of {population}")]
    NotEnoughCards { population: u32, draws: u32 },
    #[error("evaluating criteria: {0}")]
    Evaluator(E),
}

/// Above this, enumeration stops being instant and starts being a hang.
const MAX_PATHS: u128 = 5_000_000;

/// Estimated number of compositions, used only to refuse impossible questions
/// before spending an hour on them.
fn estimate_paths(groups: usize, gaps: &[u32]) -> u128 {
    // Compositions of `gap` over `groups` bins: C(gap + groups - 1, groups - 1).
    // Saturating rather than bare arithmetic: `run` refuses an empty grouping
    // before reaching here, but underflowing the bin count would spin the fold
    // for u128::MAX iterations rather than failing.
    let bins = (groups as u128).saturating_sub(1);
    gaps.iter()
        .map(|&gap| {
            let n = u128::from(gap) + bins;
            (0..bins).fold(1u128, |acc, i| acc.saturating_mul(n - i) / (i + 1))
        })
        .fold(1u128, |a, b| a.saturating_mul(b))
}

/// Exact probability that each criterion holds.
pub fn run<E>(
    grouping: &Grouping,
    gaps: &[u32],
    criteria: usize,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Vec<Probability>, RunError<E>> {
    let groups = grouping.group_sizes().len();
    if groups == 0 {
        return Err(RunError::EmptyLibrary);
    }
    let population = grouping.population();
    let draws: u32 = gaps.iter().sum();
    if draws > population {
        return Err(RunError::NotEnoughCards { population, draws });
    }
    let paths = estimate_paths(groups, gaps);
    if paths > MAX_PATHS {
        return Err(RunError::TooWide { paths, groups });
    }

    let mut totals = vec![0.0f64; criteria];
    let mut failure = None;

    pe_stats::for_each_checkpoint_path(grouping.group_sizes(), gaps, |history, p| {
        if failure.is_some() {
            return;
        }
        let view = PathView::new(grouping, history);
        match evaluator.evaluate(&view) {
            Ok(hits) => {
                for (total, hit) in totals.iter_mut().zip(hits) {
                    if hit {
                        *total += p;
                    }
                }
            }
            Err(e) => failure = Some(e),
        }
    });

    match failure {
        Some(e) => Err(RunError::Evaluator(e)),
        None => Ok(totals.into_iter().map(Probability::new).collect()),
    }
}
