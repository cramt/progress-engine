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
mod strategy;
mod zone;

pub use effect::{Board, Delay, Effect, Fetch, Fetched, Route, Trigger, TriggerError};
pub use grouping::{Grouping, GroupingError};
pub use mana::{Cost, CostError, Demand, LandDetail, ManaSource, Palette};
pub use policy::{CastingPolicy, Keep, LandDropPolicy, MulliganPolicy};
pub use schedule::{Policies, Reading, Schedule};
pub use strategy::{
    optimise, run_chosen, Chosen, Conditionals, Continuation, Decision, Objective, Optimised,
    Strategy, Table, MAX_OPTIMISE_PATHS,
};
pub use zone::{Counted, Reachable, Zone, ZoneError};

use chip_stats::{Distribution, DistributionBuilder, KahanSum, Probability};

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

/// Which of a run's questions one enumeration is wide enough to answer.
///
/// A file is not one question, and enumerating it as though it were is what
/// [issue #31](https://github.com/cramt/progress-engine/issues/31) is about: a
/// grouping wide enough for a `can_cast` clause and a path long enough for a
/// cross-turn criterion get charged to every other question in the file. So the
/// caller partitions the questions into classes, builds the cheapest
/// enumeration each class needs, and names that class here.
///
/// The indices are positions in the [`Plan`], and the answers come back
/// parallel to **these lists** rather than to the plan. That is on purpose: an
/// [`Outcomes`] padded out to the plan's length would hold a 0% for every
/// question this enumeration was not wide enough to ask, and a zero that means
/// *not asked* is exactly the confident wrong number this tool exists to
/// prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answering {
    plan: Plan,
    criteria: Vec<usize>,
    expectations: Vec<usize>,
}

impl Answering {
    /// Every question in the plan, which is what an un-narrowed run asks.
    pub fn all(plan: Plan) -> Answering {
        Answering {
            criteria: (0..plan.criteria).collect(),
            expectations: (0..plan.expectations).collect(),
            plan,
        }
    }

    /// A subset of the plan's questions. `None` where an index is out of range
    /// or repeated, because either would file one answer under two names.
    pub fn some(plan: Plan, criteria: Vec<usize>, expectations: Vec<usize>) -> Option<Answering> {
        let sound = |picks: &[usize], total: usize| {
            picks.iter().all(|&i| i < total)
                && picks
                    .iter()
                    .enumerate()
                    .all(|(n, i)| !picks[..n].contains(i))
        };
        (sound(&criteria, plan.criteria) && sound(&expectations, plan.expectations)).then_some(
            Answering {
                plan,
                criteria,
                expectations,
            },
        )
    }

    pub fn plan(&self) -> Plan {
        self.plan
    }

    /// Positions in the plan of the criteria this run answers.
    pub fn criteria(&self) -> &[usize] {
        &self.criteria
    }

    /// Positions in the plan of the expectations this run answers.
    pub fn expectations(&self) -> &[usize] {
        &self.expectations
    }
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
///
/// Parallel to the [`Answering`] the run was given, which for an un-narrowed
/// run is the whole plan in registration order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcomes {
    /// One per criterion answered.
    pub probabilities: Vec<Probability>,
    /// One per expectation answered.
    pub distributions: Vec<Distribution>,
    /// What the declared mulligan did, where the run declared one. Every
    /// number above is then the mulligan's number.
    pub mulligan: Option<Mulliganed>,
}

/// What a declared mulligan did to a run.
///
/// The keep-your-seven number travels beside the mulligan's rather than being
/// dropped, because the two differ by enough that switching from one to the
/// other silently would look like the deck changed. Every number names its
/// inputs, and the mulligan is one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mulliganed {
    /// P(the hand kept is `opener - d` cards), for every depth `d` from 0 to
    /// the policy's floor. Sums to 1: the floor is kept whatever it holds.
    pub kept: Vec<Probability>,
    /// One per criterion answered, parallel to [`Outcomes::probabilities`]:
    /// how often it holds had every first seven been kept.
    pub seven: Vec<Probability>,
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

    /// How many cards matching `query_idx` this path has put where `counted`
    /// says, by the end of `turn`.
    ///
    /// Indexed by turn rather than by checkpoint, and that indirection is the
    /// whole point. A turn was one checkpoint until an effect started looking
    /// at the top of the library, after which it is several — and every
    /// criteria file ever written names turns.
    ///
    /// There is no default form of this, on purpose. A `count(turn, query)`
    /// would mean the hand without saying so, which is the unnamed default
    /// zones exist to delete — so every call site names what it is counting,
    /// including the ones that still mean what they always meant.
    ///
    /// Returns 0 for a turn beyond the horizon rather than panicking: a
    /// criterion asking about turn 9 of a 5-turn run should be false, not a
    /// crash.
    pub fn count_at(&self, turn: usize, query_idx: usize, counted: Counted) -> u32 {
        self.board.count_at(turn, query_idx, counted)
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
    /// Refused by the sampler too, in the same words, though the reasoning
    /// here is about enumeration — an empty grouping collects no probability
    /// mass. A question about a deck with no library is a question about
    /// nothing, whichever engine is asked (#37).
    #[error("the library is empty: every card in the list is a commander or outside the deck")]
    EmptyLibrary,
    /// A hand that cannot be dealt enumerates to no paths at all, so every
    /// criterion would collect zero probability mass and report a confident 0%.
    /// Refuse instead — the sampler, which clamps to the library and answers a
    /// different question, refuses the same way.
    #[error("this question draws {draws} cards from a library of {population}")]
    NotEnoughCards { population: u32, draws: u32 },
    /// The same refusal, reached by a library that shrinks. A fetch takes a
    /// card out without drawing it, so a question that draws every card but
    /// one can run out on the games where something was fetched — and the
    /// enumeration dealt nothing on those, and lost their mass, while the
    /// sampler dealt short hands and answered.
    ///
    /// Decided before either engine runs, from what *could* be fetched rather
    /// than what a path did: each copy of a card that fetches takes at most one
    /// card, so the bound is a count. Deciding it per path would make the
    /// sampler refuse only on the hands it happened to deal (#37).
    #[error(
        "this question draws {draws} cards from a library of {population}, and {fetched} more \
         can be fetched out of it without being drawn, so on some games the library runs out \
         before the last draw"
    )]
    LibraryRunsOut {
        population: u32,
        draws: u32,
        fetched: u32,
    },
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

/// How many compositions an enumeration over `groups` groups and `gaps` draws
/// would walk. Used to refuse impossible questions before spending an hour on
/// them, and to report how far over the line a refused one went.
///
/// Public because that second reading is now a number a run states about
/// itself rather than only a reason it gave for refusing. Since
/// [#31](https://github.com/cramt/progress-engine/issues/31) a file is several
/// enumerations, so "how wide was this" has one answer per class — and a
/// second copy of this arithmetic in the caller could drift from the one the
/// ceiling is actually checked against, which would be a reported width no
/// refusal agrees with.
pub fn compositions(groups: usize, gaps: &[u32]) -> u128 {
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

/// The refusals that are facts about the whole run rather than about any one
/// question: an empty library, and a hand bigger than it.
///
/// Worth calling before a file is narrowed, because narrowing cannot make
/// either of these answerable and it can hide them. A class asking only about
/// turn 2 draws eight cards whatever the horizon says, so it would happily
/// answer against a library of forty that the file's own turn 60 could never
/// be dealt from — turning a refusal into a number by asking a smaller
/// question than the file did.
pub fn feasible<E>(grouping: &Grouping, schedule: &Schedule) -> Result<(), RunError<E>> {
    if grouping.group_sizes().is_empty() {
        return Err(RunError::EmptyLibrary);
    }
    let population = grouping.population();
    let draws: u32 = schedule.gaps().iter().sum();
    if draws > population {
        return Err(RunError::NotEnoughCards { population, draws });
    }
    // A fetch can only starve a draw that comes after it, and nothing fetches
    // before turn 1: a question that deals everything in the opening hand
    // cannot run out however much is fetched afterwards.
    let later: u32 = schedule.gaps().iter().skip(1).sum();
    let fetched = fetchable(grouping, schedule);
    if later > 0 && draws + fetched > population {
        return Err(RunError::LibraryRunsOut {
            population,
            draws,
            fetched,
        });
    }
    Ok(())
}

/// The most cards this run can take out of the library without drawing them:
/// one per copy of a card whose effect fetches. A land is played once, a spell
/// cast once, a chapter resolves once.
fn fetchable(grouping: &Grouping, schedule: &Schedule) -> u32 {
    let effects = schedule.effects();
    grouping
        .group_masks()
        .iter()
        .zip(grouping.group_sizes())
        .filter(|(mask, _)| {
            // Last-wins, as the board reads it: the bits are disjoint, so the
            // effect a group carries is the one whose bit it has.
            effects
                .iter()
                .rposition(|e| *mask & (1u64 << e.matched_by) != 0)
                .is_some_and(|e| effects[e].fetch.is_some())
        })
        .map(|(_, &size)| size)
        .sum()
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
    run_answering(grouping, schedule, &Answering::all(plan), evaluator)
}

/// [`run`], for one class of a file's questions against the cheapest
/// enumeration that class needs.
///
/// The evaluator still answers the whole file on every path — it is data, it
/// cannot be asked half a question, and a second entry point into it would be a
/// second reading of the same criteria file. What this does is keep only the
/// answers `answering` names, which are the only ones this enumeration is wide
/// enough for: a query outside the class's grouping counts zero here, and a
/// turn outside its schedule holds a stale total. Those are not answers and are
/// not reported as any.
pub fn run_answering<E>(
    grouping: &Grouping,
    schedule: &Schedule,
    answering: &Answering,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Outcomes, RunError<E>> {
    // A chosen strategy reads its openers on a grouping this class may not
    // tell apart, so it cannot be played from here: `run_chosen` is handed the
    // run's whole grouping and joins the two.
    assert!(
        schedule.chosen().is_none(),
        "a chosen mulligan strategy is played by run_chosen"
    );
    let plan = answering.plan();
    let gaps = schedule.gaps();
    let groups = grouping.group_sizes().len();
    feasible(grouping, schedule)?;
    let paths = compositions(groups, gaps);
    if paths > MAX_PATHS {
        return Err(RunError::TooWide {
            paths,
            groups,
            queries: grouping.queries().to_vec(),
        });
    }
    if schedule.mulligan().is_some() {
        return run_mulligan(grouping, schedule, answering, evaluator);
    }

    let mut totals = vec![KahanSum::new(); answering.criteria().len()];
    let mut histograms = vec![DistributionBuilder::new(); answering.expectations().len()];
    let mut mass = KahanSum::new();
    let mut failure = None;
    let mut wrong_shape = None;

    let mut board = Board::new(grouping, schedule);
    // A run with no tutor in it does not pay for one. The prefix replay in
    // `Walking::removals` is cheap but it is not free, and every number this
    // repository already reports comes off the walk that does not do it.
    let fetches = board.fetches();
    let mut walking = Walking {
        board: &mut board,
        evaluator,
        plan,
        criteria: answering.criteria(),
        expectations: answering.expectations(),
        totals: &mut totals,
        histograms: &mut histograms,
        seven: None,
        weight: 1.0,
        counts: true,
        mass_weight: 1.0,
        mass: &mut mass,
        failure: &mut failure,
        wrong_shape: &mut wrong_shape,
    };
    if fetches {
        chip_stats::for_each_checkpoint_path_removing(grouping.group_sizes(), gaps, &mut walking);
    } else {
        chip_stats::for_each_checkpoint_path(grouping.group_sizes(), gaps, |history, p| {
            chip_stats::Walk::path(&mut walking, history, p)
        });
    }

    settle(failure, wrong_shape, plan, &mass)?;
    Ok(Outcomes {
        probabilities: totals
            .into_iter()
            .map(|t| Probability::new(t.total()))
            .collect(),
        distributions: histograms
            .into_iter()
            .map(DistributionBuilder::build)
            .collect(),
        mulligan: None,
    })
}

/// [`run_answering`] under a declared mulligan: one enumeration per depth,
/// weighted by the chance of reaching it.
///
/// Under the London mulligan every redraw is a fresh deal of the whole
/// library, so depth `d` is the ordinary enumeration with `d` cards put back
/// and a hand kept only if the rule says so — and reaching depth `d` at all is
/// the product of having thrown back every hand before it. Nothing here is
/// sampled, and nothing is new arithmetic: each term is a walk this engine
/// already knew how to do.
///
/// The walk is split at the opener, because that is where the mulligan
/// branches. What goes back is decided from the opener's counts, and where a
/// tie inside one bottoming entry makes that a coin toss, each side of the coin
/// is its own branch with its own weight — and each branch then deals the same
/// later draws out of the same library, but plays them from a different hand.
/// A tutor on turn 2 can depend on which card went back, so the branch has to
/// come before the rest of the path is dealt rather than after.
///
/// A hand the rule throws back at depth `d > 0` is not walked past its opener,
/// because nothing about its later turns is asked. At depth 0 every hand is
/// walked, because the keep-your-seven number beside the mulligan's is exactly
/// the depth-0 walk with the keep rule ignored.
fn run_mulligan<E>(
    grouping: &Grouping,
    schedule: &Schedule,
    answering: &Answering,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Outcomes, RunError<E>> {
    let plan = answering.plan();
    let gaps = schedule.gaps();
    let sizes = grouping.group_sizes();
    let policy = schedule
        .mulligan()
        .expect("only called for a run that declared a mulligan");
    // The opener is the first checkpoint, which a mulligan run always keeps as
    // its own: `Schedule::narrowed` observes turn 0 whenever there is a
    // mulligan to decide there.
    let (opener, later) = gaps
        .split_first()
        .expect("a schedule has at least the opening hand");
    let deepest = policy.deepest(*opener);

    let mut totals = vec![KahanSum::new(); answering.criteria().len()];
    let mut seven = vec![KahanSum::new(); answering.criteria().len()];
    let mut histograms = vec![DistributionBuilder::new(); answering.expectations().len()];
    let mut failure = None;
    let mut wrong_shape = None;
    let mut kept: Vec<Probability> = Vec::with_capacity(deepest as usize + 1);
    let mut reach = 1.0;

    let mut board = Board::new(grouping, schedule);
    let fetches = board.fetches();
    let mut options: Vec<(Vec<u32>, f64)> = Vec::new();
    let mut hand = vec![0u32; sizes.len()];
    for depth in 0..=deepest {
        let floor = depth == deepest;
        let mut mass = KahanSum::new();
        let mut keeps = KahanSum::new();
        chip_stats::for_each_composition(sizes, *opener, |first, p_first| {
            options.clear();
            board.bottomings(first, depth, |back, q| options.push((back.to_vec(), q)));
            for (back, q) in &options {
                let weight = p_first * q;
                for ((h, f), b) in hand.iter_mut().zip(first).zip(back) {
                    *h = f - b;
                }
                let kept_here = floor || board.keeps(&hand);
                if kept_here {
                    keeps.add(weight);
                }
                if !kept_here && depth > 0 {
                    // Thrown back, so no later turn of it is asked about. Its
                    // continuations sum to one by construction, so its whole
                    // weight is accounted for without dealing them.
                    mass.add(weight);
                    continue;
                }
                board.bottom(back);
                let mut walking = Walking {
                    board: &mut board,
                    evaluator: &mut *evaluator,
                    plan,
                    criteria: answering.criteria(),
                    expectations: answering.expectations(),
                    totals: &mut totals,
                    histograms: &mut histograms,
                    seven: (depth == 0).then_some(&mut seven[..]),
                    weight: reach * weight,
                    counts: kept_here,
                    // The mass is kept per depth and unweighted by the reach,
                    // so each depth's enumeration is checked on its own terms.
                    mass_weight: weight,
                    mass: &mut mass,
                    failure: &mut failure,
                    wrong_shape: &mut wrong_shape,
                };
                if fetches {
                    chip_stats::for_each_checkpoint_path_removing_after(
                        sizes,
                        first,
                        later,
                        &mut walking,
                    );
                } else {
                    chip_stats::for_each_checkpoint_path_after(sizes, first, later, |h, p| {
                        chip_stats::Walk::path(&mut walking, h, p)
                    });
                }
            }
        });
        settle(failure.take(), wrong_shape.take(), plan, &mass)?;
        let keeps = keeps.total();
        kept.push(Probability::new(reach * keeps));
        reach *= 1.0 - keeps;
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
        mulligan: Some(Mulliganed {
            kept,
            seven: seven
                .into_iter()
                .map(|t| Probability::new(t.total()))
                .collect(),
        }),
    })
}

/// The refusals a walk can only reach by walking: an evaluator that failed or
/// answered the wrong number of questions, and a mass that did not sum to one.
pub(crate) fn settle<E>(
    failure: Option<E>,
    wrong_shape: Option<(usize, usize)>,
    plan: Plan,
    mass: &KahanSum,
) -> Result<(), RunError<E>> {
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
    Ok(())
}

/// One structure answering both halves of the walk, because they are the same
/// fact asked twice: what this path has taken out of the library, and what it
/// came to. Two closures could not share the board that knows.
pub(crate) struct Walking<'b, 'g, V, E> {
    pub(crate) board: &'b mut Board<'g>,
    pub(crate) evaluator: &'b mut V,
    pub(crate) plan: Plan,
    pub(crate) criteria: &'b [usize],
    pub(crate) expectations: &'b [usize],
    pub(crate) totals: &'b mut [KahanSum],
    pub(crate) histograms: &'b mut [DistributionBuilder],
    /// Where the keep-your-seven number collects, on the walk that has one.
    pub(crate) seven: Option<&'b mut [KahanSum]>,
    /// What every path's probability is multiplied by before it is added to
    /// an answer: 1 for a plain run, and for a mulligan the chance of having
    /// reached this depth and dealt this opener and put these cards back.
    pub(crate) weight: f64,
    /// Whether this path's answers count at all. False for a hand the
    /// mulligan throws back, which is walked only for the number beside it.
    pub(crate) counts: bool,
    /// What a path's probability is multiplied by before it is added to the
    /// mass. The mass is a check on one enumeration, so it is weighted by what
    /// that enumeration dealt and not by the chance of reaching it.
    pub(crate) mass_weight: f64,
    pub(crate) mass: &'b mut KahanSum,
    pub(crate) failure: &'b mut Option<E>,
    pub(crate) wrong_shape: &'b mut Option<(usize, usize)>,
}

impl<V: Evaluator<Error = E>, E> chip_stats::Walk for Walking<'_, '_, V, E> {
    fn removals(&mut self, reached: chip_stats::Path<'_>, out: &mut [u32]) {
        // The same walk that produces the answers, replayed over the prefix.
        // Not a second reading of what a tutor does: a fetch decided here and
        // a fetch decided at the leaf are one line of code, so they cannot
        // drift.
        self.board.walk(reached);
        out.copy_from_slice(self.board.removed());
    }
    fn path(&mut self, reached: chip_stats::Path<'_>, p: f64) {
        self.mass.add(self.mass_weight * p);
        if self.failure.is_some() || self.wrong_shape.is_some() {
            return;
        }
        // Rebuilt in place per path rather than per criterion: where a card
        // ended up is a fact about the path, and computing it once is what
        // stops two criteria from disagreeing about the same surveil.
        self.board.walk(reached);
        let view = PathView::new(self.board);
        match self.evaluator.evaluate(&view) {
            Ok(outcomes) => {
                if outcomes.held.len() != self.plan.criteria
                    || outcomes.counted.len() != self.plan.expectations
                {
                    *self.wrong_shape = Some((outcomes.held.len(), outcomes.counted.len()));
                    return;
                }
                if let Some(seven) = self.seven.as_deref_mut() {
                    // The seven's own probability, with no mulligan weight on
                    // it: it is the number had this hand been kept.
                    let p_seven = self.mass_weight * p;
                    for (total, &i) in seven.iter_mut().zip(self.criteria) {
                        if outcomes.held[i] {
                            total.add(p_seven);
                        }
                    }
                }
                if !self.counts {
                    return;
                }
                let p = self.weight * p;
                for (total, &i) in self.totals.iter_mut().zip(self.criteria) {
                    if outcomes.held[i] {
                        total.add(p);
                    }
                }
                for (histogram, &i) in self.histograms.iter_mut().zip(self.expectations) {
                    histogram.add(outcomes.counted[i].get(), p);
                }
            }
            Err(e) => *self.failure = Some(e),
        }
    }
}
