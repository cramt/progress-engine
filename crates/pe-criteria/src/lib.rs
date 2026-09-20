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
//!
//! A run answers two kinds of question off the same walk. A *criterion* asks
//! whether something held, and comes back as a probability. An *expectation*
//! asks how many, and comes back as a mean and the full distribution behind it —
//! P(exactly k) for every k, exact, because enumeration visits each outcome
//! once instead of sampling it.

mod effect;
mod grouping;
pub mod mana;
mod policy;
mod schedule;
mod zone;

pub use effect::{Board, Effect, Route, Trigger, TriggerError};
pub use grouping::{Grouping, GroupingError};
pub use mana::{Cost, CostError, ManaSource, Palette};
pub use policy::LandDropPolicy;
pub use schedule::Schedule;
pub use zone::{Reachable, Zone, ZoneError};

use pe_stats::{Distribution, DistributionBuilder, KahanSum, Probability};

/// One named acceptance criterion, optionally with a threshold it must meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    pub name: String,
    /// The assertion: this criterion must hold at least this often.
    pub at_least: Option<f64>,
}

/// One named quantity to report the mean and the full distribution of.
///
/// **There is no threshold field, and that is a stated gap rather than an
/// oversight.** A criterion's `atLeast` is a threshold on a probability: a
/// number between zero and one, meaning the same thing in every criterion ever
/// written. The same word on an expectation would be a threshold in the units of
/// whatever that expectation counts — "at least 2.5" is lands here and mana
/// there — and nothing in the output would say which reading applied. One
/// keyword with two meanings is exactly the unstated definition this tool exists
/// to eliminate, so rather than ship the trap, expectations are informational
/// and cannot fail a run.
///
/// The assertion people actually want is probably not about the mean at all.
/// "Averages at least 2.5 lands" is satisfied by a deck that floods half the
/// time and is screwed the other half; "has two lands 90% of the time" is a
/// statement about a percentile, and a percentile is a statement about a range,
/// which is [issue #12](https://github.com/cramt/progress-engine/issues/12).
/// Naming that assertion is left until the shape it asserts on exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    pub name: String,
}

/// The largest value an expectation may take.
///
/// Generous for anything that counts cards — a Commander library is 99 — and
/// small enough that one bucket per value is a rounding error of memory.
pub const MAX_COUNT: u32 = 1024;

/// A value an expectation took on one path: a bounded, non-negative integer.
///
/// The bound is what makes the answer a histogram rather than a shrug. A run
/// reports P(value = k) for every k, so every value it accepts needs a bucket of
/// its own, and that needs whole numbers with a ceiling. `count()` returns
/// exactly that and is the only thing a criteria file may look at, so the
/// natural expectation — a count, or a sum of counts — is representable by
/// construction.
///
/// Everything else is refused at the boundary rather than coerced. Rounding 1.5
/// to 2, or bucketing an arbitrary f64 at a width nobody chose, answers a
/// question the reader never asked and leaves no mark in the output saying so,
/// which is this project's defining failure mode in a new costume. An
/// expectation dividing by seven to report a rate is a thing somebody will
/// write, and it gets an error naming itself rather than a plausible histogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Count(u32);

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error(
    "{value} is not a countable value: it must be a whole number from 0 to {MAX_COUNT}.\n\
     An expectation is reported as a distribution with one bucket per value, and there is \
     nowhere to put this one."
)]
pub struct NotACount {
    pub value: f64,
}

impl Count {
    pub fn new(value: u32) -> Result<Self, NotACount> {
        Self::from_f64(f64::from(value))
    }

    /// The boundary conversion: this is where a number arriving from JavaScript
    /// stops being an arbitrary f64.
    pub fn from_f64(value: f64) -> Result<Self, NotACount> {
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > f64::from(MAX_COUNT)
        {
            return Err(NotACount { value });
        }
        Ok(Count(value as u32))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// How many questions of each kind a run answers.
///
/// Two counts rather than one total, because the kinds accumulate differently
/// and are reported differently. Named fields rather than two adjacent `usize`
/// arguments, because swapping those would reinterpret every criterion as an
/// expectation and still typecheck.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    pub criteria: usize,
    pub expectations: usize,
}

/// What every registered question answered on one path.
///
/// Two lists rather than one list of tagged values, so a criterion slot can only
/// hold a bool and an expectation slot can only hold a [`Count`]. Kind confusion
/// is then not something the engine has to detect: it is a state this type
/// cannot hold, and the one place it can go wrong is the JavaScript boundary
/// that builds this — which is where the error belongs, because that is the only
/// place that knows the offender's name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathOutcomes {
    /// One per criterion, in registration order: did it hold on this path?
    pub held: Vec<bool>,
    /// One per expectation, in registration order: what value did it take?
    pub counted: Vec<Count>,
}

/// Everything a run answered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcomes {
    /// One per criterion, in registration order.
    pub probabilities: Vec<Probability>,
    /// One per expectation, in registration order.
    pub distributions: Vec<Distribution>,
}

/// What a criterion is allowed to see: counts, never cards.
///
/// A borrow of the [`Board`] the engine just walked rather than of the raw
/// path. The two say the same thing on a run with no live effect and stop
/// saying the same thing the moment one is live, and a criterion must not be
/// able to read past the difference: *how many cards has this path turned
/// over* is not a question anybody asked, and *how many are in my hand* is.
pub struct PathView<'a> {
    board: &'a Board<'a>,
}

impl<'a> PathView<'a> {
    pub fn new(board: &'a Board<'a>) -> Self {
        PathView { board }
    }

    /// Turns this run covers, counting turn 0, the opening hand.
    pub fn turns(&self) -> usize {
        self.board.turns()
    }

    /// How many cards matching `query_idx` are in `zone` at the end of `turn`.
    ///
    /// Indexed by turn rather than by checkpoint, and that indirection is the
    /// whole point. A turn was one checkpoint until an effect started looking
    /// at the top of the library, after which it is several — and every
    /// criteria file ever written names turns.
    ///
    /// There is no zone-less form of this, on purpose. A `count(turn, query)`
    /// would mean the hand without saying so, which is the unnamed default
    /// zones exist to delete — so every call site names the zone it asks
    /// about, including the ones that still mean what they always meant.
    ///
    /// Returns 0 for a turn beyond the horizon rather than panicking: a
    /// criterion asking about turn 9 of a 5-turn run should be false, not a
    /// crash.
    pub fn count_in(&self, turn: usize, query_idx: usize, zone: Zone) -> u32 {
        self.board.count_in(turn, query_idx, zone)
    }

    /// Whether `cost` could have been paid on `turn`.
    ///
    /// A primitive rather than something a criteria file assembles, because it
    /// cannot be assembled: one Hallowed Fountain counts toward `produces:w`
    /// and toward `produces:u`, so a conjunction of independent counts is
    /// satisfied by a hand that cannot pay. See [`crate::mana`].
    pub fn can_cast(&self, turn: usize, cost: &Cost) -> bool {
        self.board.can_cast(turn, cost)
    }
}

/// Evaluates every registered question against one path through the checkpoints.
pub trait Evaluator {
    type Error;

    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum RunError<E> {
    /// Names every query, not just the count.
    ///
    /// It used to name neither, because the queries were learned by running a
    /// JavaScript file and the run was refused part-way through learning them —
    /// so the only honest thing it could report was how far it had got. A
    /// declarative criteria file hands the whole set over before enumeration
    /// starts, so the refusal can say what was actually asked and the reader can
    /// see which query to drop.
    #[error(
        "this question is too wide to answer exactly: {paths} compositions across {groups} groups.\n\
         It asks about {} queries: {}\n\
         Reduce the number of distinct queries, or ask about an earlier turn.",
        .queries.len(),
        .queries.join(", ")
    )]
    TooWide {
        paths: u128,
        groups: usize,
        queries: Vec<String>,
    },
    #[error("the library is empty: every card in the list is a commander or outside the deck")]
    EmptyLibrary,
    /// A hand that cannot be dealt enumerates to no paths at all, so every
    /// criterion would collect zero probability mass and report a confident 0%.
    /// Refuse instead — the sampler, which clamps to the library and answers a
    /// different question, refuses the same way.
    #[error("this question draws {draws} cards from a library of {population}")]
    NotEnoughCards { population: u32, draws: u32 },
    /// The enumeration is supposed to partition every possible draw, so its
    /// path probabilities sum to 1. If they do not, some region of the sample
    /// space was visited twice or not at all, and every criterion's total is
    /// drawn from the wrong denominator.
    #[error(
        "the enumeration summed to {total} instead of 1, so it lost or gained probability mass.\n\
         Every percentage this run would report is wrong by an unknown amount, so it reports none."
    )]
    MassNotOne { total: f64 },
    /// The report pairs answers with names by position, so an evaluator handing
    /// back a different number of them would file every question under its
    /// neighbour's title rather than fail.
    #[error(
        "the criteria answered {held} criteria and {counted} expectations, but this run was \
         set up for {} and {}",
        .plan.criteria, .plan.expectations
    )]
    WrongShape {
        plan: Plan,
        held: usize,
        counted: usize,
    },
    #[error("evaluating criteria: {0}")]
    Evaluator(E),
}

/// Above this, enumeration stops being instant and starts being a hang.
///
/// Public because a refusal at this ceiling is no longer the end of the story:
/// a caller that falls back to sampling has to be able to tell a reader how far
/// over the line the question went, and a second copy of the number in the CLI
/// would be a constant that could drift from the one that actually refused.
pub const MAX_PATHS: u128 = 5_000_000;

/// How far the total probability mass may sit from 1 before the run is a bug
/// rather than arithmetic.
///
/// Measured with `KahanSum` against enumerations whose exact total is 1: 1.2e-13
/// over 57 compositions, 4.5e-14 over 4.3M, 6.0e-14 over 14M checkpoint paths,
/// 2.7e-14 over 160M. The error does not grow with the term count — compensated
/// summation leaves only the log-gamma round trip inside each term, and since
/// every term is positive that error cannot compound either. So the floor is
/// ~1e-13 for a Commander-sized library, and 1e-9 leaves four orders of headroom
/// for larger populations, where log-gamma works with bigger magnitudes.
///
/// It is deliberately not tighter than the causes it exists to catch. The
/// smallest single path in the widest enumeration `MAX_PATHS` allows is ~7e-11,
/// so this will not notice one lone path going missing. It will notice a
/// miscounted group, a wrong gap vector, or an early return dropping a branch,
/// which is what actually goes wrong.
const MASS_TOLERANCE: f64 = 1e-9;

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

/// Exact probability that each criterion holds, and the exact distribution of
/// each expectation.
///
/// Both fall out of one walk over one enumeration. Adding `p` where a criterion
/// held and adding `p` to bucket `value` where an expectation counted is the
/// same loop over the same terms, which is why the distribution costs nothing —
/// and why it is exact rather than estimated: enumeration visits every outcome
/// once, so P(exactly k) is a sum and not a sample.
pub fn run<E>(
    grouping: &Grouping,
    schedule: &Schedule,
    plan: Plan,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Outcomes, RunError<E>> {
    let gaps = schedule.gaps();
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
        return Err(RunError::TooWide {
            paths,
            groups,
            queries: grouping.queries().to_vec(),
        });
    }

    let mut totals = vec![KahanSum::new(); plan.criteria];
    let mut histograms = vec![DistributionBuilder::new(); plan.expectations];
    let mut mass = KahanSum::new();
    let mut failure = None;
    let mut wrong_shape = None;

    let mut board = Board::new(grouping, schedule);
    pe_stats::for_each_checkpoint_path(grouping.group_sizes(), gaps, |history, p| {
        mass.add(p);
        if failure.is_some() || wrong_shape.is_some() {
            return;
        }
        // Rebuilt in place per path rather than per criterion: where a card
        // ended up is a fact about the path, and computing it once is what
        // stops two criteria from disagreeing about the same surveil.
        board.walk(history);
        let view = PathView::new(&board);
        match evaluator.evaluate(&view) {
            Ok(outcomes) => {
                if outcomes.held.len() != plan.criteria
                    || outcomes.counted.len() != plan.expectations
                {
                    wrong_shape = Some((outcomes.held.len(), outcomes.counted.len()));
                    return;
                }
                for (total, hit) in totals.iter_mut().zip(outcomes.held) {
                    if hit {
                        total.add(p);
                    }
                }
                for (histogram, value) in histograms.iter_mut().zip(outcomes.counted) {
                    histogram.add(value.get(), p);
                }
            }
            Err(e) => failure = Some(e),
        }
    });

    if let Some(e) = failure {
        return Err(RunError::Evaluator(e));
    }
    if let Some((held, counted)) = wrong_shape {
        return Err(RunError::WrongShape {
            plan,
            held,
            counted,
        });
    }
    // Free, because the enumeration that produced the answers already produced
    // every term of this sum.
    let total = mass.total();
    if (total - 1.0).abs() > MASS_TOLERANCE {
        return Err(RunError::MassNotOne { total });
    }
    Ok(Outcomes {
        probabilities: totals
            .into_iter()
            .map(|t| Probability::new(t.total()))
            .collect(),
        distributions: histograms
            .into_iter()
            .map(DistributionBuilder::build)
            .collect(),
    })
}
