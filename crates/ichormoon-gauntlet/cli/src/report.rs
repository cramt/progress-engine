//! Turning results into a verdict.

use chip_scryfall::index::TagGap;
use chip_stats::Distribution;
use facet::Facet;
use gauntlet_criteria::{Bound, Criterion, Expectation};
use sha2::{Digest, Sha256};

use crate::library::Library;

#[derive(Facet)]
pub struct CriterionResult {
    pub name: String,
    pub probability: f64,
    pub percent: f64,
    pub at_least: Option<f64>,
    /// Absent rather than null when the file wrote none, so a file that
    /// predates it reports byte for byte what it always did.
    #[facet(skip_serializing_if = Option::is_none)]
    pub at_most: Option<f64>,
    /// Present only for sampled runs: never quote a sampled figure without it.
    #[facet(skip_serializing_if = Option::is_none)]
    pub standard_error: Option<f64>,
    pub pass: bool,
    /// On a failure only, which bound was missed: `"at_least"` or `"at_most"`.
    /// A range is two thresholds, and "FAIL" alone does not say whether the
    /// deck got there too rarely or too often.
    #[facet(skip_serializing_if = Option::is_none)]
    pub missed: Option<&'static str>,
    /// Sampled runs only: the threshold sits inside this figure's error bar, so
    /// `pass` is a fact about this seed as much as about this deck.
    ///
    /// The verdict is still computed the same way, because a comparison that
    /// sometimes declines to have an opinion is harder to act on than one that
    /// is always reproducible. But 79.9% +/- 0.3 against a threshold of 80% has
    /// not really passed or failed, and rounding it one way without saying so
    /// would be a coin toss wearing a verdict's clothes.
    #[facet(skip_serializing_if = Option::is_none)]
    pub inconclusive: Option<bool>,
    /// How this answer was reached, rather than the run.
    ///
    /// A file is not one question, and since #31 it is not one enumeration
    /// either: each class of questions is answered on the narrowest
    /// enumeration that can answer it, so one criterion can be over the ceiling
    /// while the one beside it is enumerated exactly. The top-level `method`
    /// still says what happened to the run as a whole; this says what happened
    /// to this number, which is the one a reader is about to quote.
    pub method: Method,
    /// Where the file declared a mulligan: this criterion's probability had
    /// every first seven been kept instead, from the same engine as
    /// `probability`.
    ///
    /// Beside the verdict rather than instead of it. `pass` is judged on
    /// `probability`, which is the mulligan's number, because that is the
    /// question `at_least` was always asking; this is here because the two
    /// differ by enough that a reader comparing against an older run needs to
    /// see which one moved.
    #[facet(skip_serializing_if = Option::is_none)]
    pub keep_seven: Option<f64>,
    /// `keep_seven` as a percentage, rounded once from the unrounded figure
    /// exactly as `percent` is, so the two never disagree about a last digit
    /// a reader compares across runs.
    #[facet(skip_serializing_if = Option::is_none)]
    pub keep_seven_percent: Option<f64>,
}

/// What an expectation answered: how many, on average, and how that was spread.
///
/// There is no `at_least` and no `pass` here, and their absence is deliberate
/// rather than pending. See `gauntlet_criteria::Expectation` for why an expectation
/// cannot fail a run, and why the assertion that would make sense is a statement
/// about a percentile rather than about this mean.
#[derive(Facet)]
pub struct ExpectationResult {
    pub name: String,
    pub mean: f64,
    /// P(value = k), indexed by k, from zero to the largest value reachable.
    ///
    /// The whole distribution, untruncated, because this is the machine-readable
    /// half. What a human is shown is a window over the same numbers — a
    /// hundred-bucket histogram on a terminal is not a histogram, it is a wall —
    /// and a caller that wants the tail should not have to re-run the tool to
    /// get it.
    pub distribution: Vec<f64>,
    /// Present only for sampled runs, and the standard error of the *mean*
    /// rather than of a proportion: two expectations averaging the same number
    /// have different error bars if one is tightly spread and the other is not.
    #[facet(skip_serializing_if = Option::is_none)]
    pub standard_error: Option<f64>,
    /// How this answer was reached, rather than the run. See
    /// [`CriterionResult::method`].
    pub method: Method,
}

/// How one number was reached: walked exactly, or estimated by sampling.
///
/// Serialised as `"exact"` or `"sampled"`. One answer is only ever one of the
/// two; a run can be both, which is [`RunMethod`].
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum Method {
    Exact,
    Sampled,
}

impl Method {
    /// The method of an answer the run did, or did not, estimate.
    pub fn of(estimated: bool) -> Method {
        if estimated {
            Method::Sampled
        } else {
            Method::Exact
        }
    }
}

/// How a whole run was answered: `"exact"`, `"sampled"`, or `"mixed"`.
///
/// Three states, because a run can now be both. Narrowing means the file is
/// several enumerations, so the question that went over the ceiling no longer
/// takes its neighbours down with it — and a reader has to be able to tell
/// that from a run where everything was sampled.
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum RunMethod {
    Exact,
    Sampled,
    Mixed,
}

impl RunMethod {
    pub fn of(estimated: &Estimated) -> RunMethod {
        match (estimated.any(), estimated.all()) {
            (false, _) => RunMethod::Exact,
            (true, true) => RunMethod::Sampled,
            (true, false) => RunMethod::Mixed,
        }
    }
}

/// How an enumeration reads its turns, serialised as `"cumulative"` or
/// `"per-turn"`.
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Reading {
    /// How many cards had been seen by the turns it names.
    Cumulative,
    /// Every turn up to the last it names, one at a time.
    PerTurn,
}

impl From<gauntlet_criteria::Reading> for Reading {
    fn from(reading: gauntlet_criteria::Reading) -> Self {
        match reading {
            gauntlet_criteria::Reading::Cumulative => Reading::Cumulative,
            gauntlet_criteria::Reading::PerTurn => Reading::PerTurn,
        }
    }
}

/// Why a run sampled, serialised as `"requested"` or `"too_wide"`: the
/// label [`WhySampled`] gives the report, without the width.
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "snake_case")]
pub enum SampledBecause {
    Requested,
    TooWide,
}

/// What the criteria file registered, in registration order within each kind.
///
/// Grouped for the same reason `Scenario` is: they arrive together, they are
/// consumed together, and they index the two halves of [`Answers`] positionally,
/// so a caller that had one without the other could only misuse it.
#[derive(Clone, Copy)]
pub struct Questions<'a> {
    pub criteria: &'a [Criterion],
    pub expectations: &'a [Expectation],
}

/// The numbers an engine produced, before they are given names and thresholds.
///
/// One shape for both engines, so the CLI's call site cannot care which one
/// answered — which is the property that keeps `--simulate` a second
/// implementation rather than a second feature set.
pub struct Answers {
    /// One per criterion. Exact probabilities, or sampled proportions.
    pub probabilities: Vec<f64>,
    /// One per expectation.
    pub distributions: Vec<Distribution>,
    /// Which of them are estimates.
    pub estimated: Estimated,
    /// One per criterion: its keep-your-seven number, where a mulligan was
    /// declared and the engine that answered the criterion reported one.
    pub seven: Vec<Option<f64>>,
}

/// Which answers came from the sampler rather than from the enumeration.
///
/// One flag per question rather than one for the run, because since #31 a run
/// can be both: the file is partitioned into classes, each class gets the
/// narrowest enumeration that answers it, and only a class that is still over
/// the ceiling falls back. A criterion that was enumerated must not be quoted
/// with an error bar it never had, and one that was sampled must never be
/// quoted without one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Estimated {
    pub criteria: Vec<bool>,
    pub expectations: Vec<bool>,
}

impl Estimated {
    /// Nothing estimated: one flag per question, all false.
    pub fn none(plan: gauntlet_criteria::Plan) -> Estimated {
        Estimated {
            criteria: vec![false; plan.criteria],
            expectations: vec![false; plan.expectations],
        }
    }

    pub fn any(&self) -> bool {
        self.criteria.iter().chain(&self.expectations).any(|e| *e)
    }

    pub fn all(&self) -> bool {
        self.criteria.iter().chain(&self.expectations).all(|e| *e)
    }
}

/// How many library cards a query actually matched — and, where the run's
/// line casts a commander from the command zone, that commander too.
///
/// Reported because a query matching nothing is this project's defining failure:
/// it produces a confident 0% rather than an error. Showing the count makes a
/// typo'd category name obvious at a glance.
#[derive(Facet)]
pub struct QueryMatch {
    pub query: String,
    pub cards: u32,
}

/// A zone the criteria file asked about, and whether anything can put a card
/// there.
///
/// Reported for the same reason [`QueryMatch`] is, and against the same
/// failure. A query that matches nothing and a zone nothing routes into both
/// produce a perfectly formatted 0.00%, and in a percentage a zero that means
/// *not modelled* is indistinguishable from a zero that means *never
/// happened*. The run cannot tell the reader which deck they have unless it
/// first tells them which question it was able to ask.
#[derive(Facet)]
pub struct ZoneUse {
    pub zone: &'static str,
    /// False when nothing in this run routes a card into the zone, so every
    /// count in it is zero by construction rather than by measurement.
    pub reachable: bool,
    /// The question that first named it, so the reader knows which line to fix.
    pub asked_by: String,
}

/// An effect the library brought to bear on this deck, and what it applied to.
///
/// Reported because an autoloading library changes answers without anybody
/// editing anything, so the first question about a surprising number is what
/// the tool thought the cards do. Last-wins is only honest if the run can say
/// which effect applied to which card, and this is where it says it.
///
/// Only effects that matched at least one card are listed. An entry that
/// matched nothing said nothing about this deck, and a list of every entry in
/// the standard library under every report would bury the two lines that
/// matter.
#[derive(Facet)]
pub struct EffectUse {
    #[facet(rename = "match")]
    pub matches: String,
    pub look: u32,
    pub on: &'static str,
    /// The routing policy, or `null` where none was declared — in which case
    /// every looked-at card stays on top and this effect moves no number.
    #[facet(skip_serializing_if = Option::is_none)]
    pub to_graveyard: Option<String>,
    /// What this goes and gets out of the library, highest priority first, or
    /// absent where it fetches nothing.
    ///
    /// Reported for the same reason `[land_drop]` and `[casting]` are, and
    /// with the same weight: a tutor decides which card left the library, so
    /// every number under it depends on this list. A run that fetched and did
    /// not say what it fetched is the bug this project exists to prevent.
    #[facet(skip_serializing_if = Option::is_none)]
    pub fetch: Option<Vec<String>>,
    /// Where the fetched card is put: `hand` or `battlefield`.
    #[facet(skip_serializing_if = Option::is_none)]
    pub to: Option<&'static str>,
    /// Whole turns between the trigger and the effect, or absent where it
    /// happens when it is triggered. Urza's Saga's third chapter is 2.
    #[facet(skip_serializing_if = Option::is_none)]
    pub after: Option<u32>,
    /// Whether the card that set a delayed effect up leaves the battlefield
    /// when it resolves. Absent beside an effect that does not wait.
    #[facet(skip_serializing_if = Option::is_none)]
    pub sacrifice: Option<bool>,
    /// Which file declared it: the standard library, or the criteria file.
    pub source: String,
    /// The cards it applied to, after the overlap was resolved. A card matched
    /// by a later entry is listed under that entry and not this one.
    pub cards: Vec<String>,
    pub copies: u32,
    /// Whether this effect can move a number. False for an effect that routes
    /// nothing, or that routes to a card this deck does not play.
    pub live: bool,
}

/// A listed card that never enters the library, so was not counted in it.
///
/// Reported for the same reason an empty query is: dropping cards quietly leaves
/// a confident number nobody can question. Someone whose 109-card list comes
/// back as 99 is owed the ten names and the reason.
#[derive(Facet)]
pub struct ExcludedCard {
    pub name: String,
    pub qty: u32,
    pub card_type: &'static str,
}

/// What produced this report, as opposed to what it says.
///
/// A report that carries only the answer cannot explain a change in it. Six
/// separate things move a percentage — the decklist, the criteria file, the card
/// index, an oracle tag, a pod heuristic, the tool itself — and in the output
/// they are indistinguishable: a number moved. Naming the inputs is what lets a
/// later comparison attribute the movement instead of merely observing it, and
/// it is far cheaper to record now than to reconstruct once several external
/// data sources are in play.
///
/// This grows as the tool learns more about what it assumed: mulligan policy and
/// pod composition belong here once they exist. The resolved scenario as it
/// stands — on the play or the draw, which engine, how many trials, which seed —
/// is already reported at the top level of the report.
#[derive(Facet)]
pub struct Provenance {
    pub tool_version: &'static str,
    /// When the card index was built, or `null` when the index never said.
    ///
    /// Null rather than omitted or backfilled with today: an unknown date is a
    /// fact about the run and has to survive into the report as one, whereas a
    /// missing key reads as a tool that forgot to look and a fabricated date
    /// reads as evidence the index was current.
    pub index_updated_at: Option<String>,
    /// SHA-256 of the decklist file exactly as it was read.
    ///
    /// Of the raw bytes, not of the parsed deck: a reordered or re-commented
    /// file is a change somebody made, and a hash that forgave it would hide
    /// precisely the edit a reader is trying to correlate with the number.
    pub deck_sha256: String,
    /// SHA-256 of the criteria file exactly as it was read, for the same reason.
    ///
    /// This covers the user's own `[[effect]]` tables too: they live in the
    /// criteria file, so a routing policy that changed is a criteria file that
    /// changed.
    pub criteria_sha256: String,
    /// SHA-256 of the standard effect library exactly as it shipped.
    ///
    /// An input that moves the numbers and that nobody edited. It autoloads, so
    /// a deck whose percentage changed between two tool versions would
    /// otherwise be indistinguishable from a deck that changed — which is the
    /// ambiguity the other four hashes were added to remove. It is not folded
    /// into `tool_version`: the library is data, it will move on its own
    /// schedule, and the whole point of a hash is that it does not need a
    /// release to be comparable.
    pub effect_library_sha256: String,
}

/// Why a run sampled rather than enumerated.
///
/// The two are not the same event and must not report as one. Asking for
/// sampling is a choice somebody made and can unmake; falling back to it is the
/// tool telling the caller that the question they asked has no exact answer at
/// this ceiling. A caller reading `method: "sampled"` alone cannot tell which
/// happened, and only one of the two is a reason to rewrite the question.
#[derive(Clone, Copy)]
pub enum WhySampled {
    /// `--simulate`: the caller asked for the sampling engine.
    Requested,
    /// Exact enumeration was refused at this width, so the run fell back.
    TooWide { paths: u128, groups: usize },
}

/// The knobs a sampled run has and an exact run has no honest answer for.
///
/// Kept together so none can be reported without the others: trials without a
/// seed does not identify the hands that were dealt, a seed without trials does
/// not reproduce them, and neither says whether anybody chose this engine.
#[derive(Clone, Copy)]
pub struct Sampling {
    pub trials: u32,
    pub seed: u64,
    pub why: WhySampled,
}

/// The width that exact enumeration refused, as the report states it.
///
/// Reported as numbers rather than only inside the warning's prose, because the
/// caller that most needs this is the one that cannot read prose: a builder
/// embedding the engine wants to know how far over the ceiling the question
/// went before it decides whether to narrow it.
///
/// Since #31 this is the width of **the widest class that was refused**, not of
/// the run: a file is enumerated a class at a time, so the questions that fit
/// are answered exactly beside this one and are not described by it. Which
/// questions it cost is in their own `method`, and in the warning.
#[derive(Facet)]
pub struct TooWide {
    /// Compositions the exact engine would have had to walk. A `f64` because
    /// the estimate saturates well past what a JSON reader can hold as an
    /// integer, and an order of magnitude that survives every parser is worth
    /// more here than digits that only some of them keep.
    pub paths: f64,
    pub groups: usize,
    /// The ceiling it was measured against, so `paths` has a scale without the
    /// reader having to know this tool's constants.
    pub ceiling: f64,
}

/// One enumeration this run walked, and which questions it was for.
///
/// Since [#31](https://github.com/cramt/progress-engine/issues/31) a file is
/// several enumerations rather than one, and until now the only one that
/// reached the JSON was the widest class that was *refused*. So a run could
/// not say how it had answered the questions it did answer: the group and
/// composition counts this project quotes about its own narrowings were not
/// reproducible from the output of a run that performed them. This is that
/// fixed, and it is the same argument the provenance block is — every number
/// names its inputs, including the numbers about the numbers.
///
/// `groups` and `compositions` are the width of *this* class, after
/// narrowing, and `compositions` is computed by the same function the ceiling
/// is checked against rather than by a second copy of the arithmetic.
#[derive(Facet)]
pub struct Enumeration {
    /// The criteria this enumeration answered, by name, and then the
    /// expectations. A reader chasing one figure finds it in exactly one of
    /// these lists.
    pub criteria: Vec<String>,
    pub expectations: Vec<String>,
    /// The queries this class can tell apart. Not the file's whole list: a
    /// class keeps the queries it reads plus the ones the walk reads for
    /// itself, and everything else merged.
    pub queries: Vec<String>,
    /// The turns whose counts it reads. Under `"per-turn"` it also walks every
    /// turn up to the last of these, because one land drop a turn is
    /// use-it-or-lose-it and no total can say that.
    pub turns: Vec<usize>,
    /// `"cumulative"` — how many cards had been seen by the turns it names —
    /// or `"per-turn"`.
    pub reading: Reading,
    /// The pip kinds this enumeration told lands apart by, where it priced
    /// mana at all. Absent where it did not, and **empty** where it did and
    /// the costs are all generic: `{2}` is paid by any two lands, so that
    /// enumeration tells a land from a spell and tapped from untapped and
    /// nothing else. Tapped-ness is kept whenever this field is present,
    /// because the land played this turn is the only one that can still be
    /// tapped.
    #[facet(skip_serializing_if = Option::is_none)]
    pub pips: Option<Vec<String>>,
    pub groups: usize,
    /// Compositions this class walks. A `f64` for the same reason
    /// [`TooWide::paths`] is: the count saturates well past what a JSON reader
    /// holds as an integer, and an order of magnitude that survives every
    /// parser is worth more than digits only some of them keep.
    pub compositions: f64,
    /// `"exact"` where this enumeration was walked, `"sampled"` where it was
    /// not — because it went over the ceiling, or because `--simulate` asked
    /// for the other engine. In both of those cases the width beside it is
    /// what this class *would* have cost, which is the number a caller
    /// deciding whether to narrow their question needs.
    pub method: Method,
    /// Where the file declared a mulligan: how many times this enumeration is
    /// dealt, once per hand size from seven down to the floor. The ceiling is
    /// checked against `compositions`, which is one deal; the work is up to
    /// this many of them, and more where a tie in the bottoming list branches
    /// a deal. Absent on a run that keeps every seven, which deals once.
    #[facet(skip_serializing_if = Option::is_none)]
    pub deals: Option<u32>,
}

/// The run these numbers describe: which seat, and which engine answered.
///
/// Grouped because they travel together everywhere. A percentage means nothing
/// without the seat it was computed for, and a sampled figure means nothing
/// without the trials and seed that produced it.
#[derive(Clone, Copy)]
pub struct Scenario {
    pub on_the_draw: bool,
    pub sampled: Option<Sampling>,
}

/// What the run was able to ask about, as opposed to what it answered.
///
/// Grouped because they are one thought: a query that matched nothing, a zone
/// nothing routes into and an effect that applied where nobody expected it are
/// three doors to the same failure, and a reader chasing a surprising number
/// reads all three or none of them.
pub struct Breakdown {
    pub queries: Vec<QueryMatch>,
    pub zones: Vec<ZoneUse>,
    pub effects: Vec<EffectUse>,
    /// How this run enumerated, one entry per class of question.
    pub enumerations: Vec<Enumeration>,
    /// Lands whose tapped-ness this run decided for the pilot. Empty unless the
    /// run asked a mana question, because otherwise it decided nothing.
    pub assumed_tapped: Vec<String>,
    /// The priority that decided the land drop, where the file declared one.
    ///
    /// Beside the tapped-ness assumptions rather than anywhere else, because
    /// it is the same kind of fact: something that chose between two lands and
    /// moved every number that depended on which one was played.
    pub land_drop: Option<LandDropUse>,
    /// The priority that decided which spells were cast, where the file
    /// declared one. The same kind of fact again, over the turn's mana.
    pub casting: Option<CastingUse>,
    /// The mulligan that decided which hand every number is of, where the
    /// file declared one.
    pub mulligan: Option<MulliganUse>,
    /// The strategy chosen for the file's objective, where it declared one.
    pub optimised: Option<OptimisedUse>,
}

/// The mulligan chosen for a weighted objective, and what it trades (#63).
///
/// Everything a reader needs to check the choice rather than trust it: the
/// objective as written, the score, the threshold at each depth, how often
/// each hand size is kept, what each objective criterion gets under the
/// strategy beside what it would get if the strategy served it alone, and the
/// whole table the strategy is.
#[derive(Facet)]
pub struct OptimisedUse {
    /// Whether this is the strategy every number in the run is played under.
    /// False where the file declared a rule of its own, which is then the
    /// one played, and this is reported beside it.
    pub played: bool,
    pub objective: Vec<ObjectiveUse>,
    /// The objective's expected score under the chosen strategy.
    pub score: f64,
    /// The same score under the rule the file declared, where it declared
    /// one — the gap between the pilot's strategy and the best available, as
    /// a number.
    #[facet(skip_serializing_if = Option::is_none)]
    pub score_declared: Option<f64>,
    /// The score a hand needs to be kept at each hand size above the floor.
    pub thresholds: Vec<Threshold>,
    pub down_to: u32,
    /// The share of games kept at each hand size, largest first.
    pub kept: Vec<KeptAt>,
    /// Paths walked to price every opener and every way of putting back.
    pub walked: f64,
    /// What a tie between ways of putting back was settled by.
    pub tie_break: &'static str,
    /// The whole strategy: what it does with every opener it can tell apart.
    pub strategy: StrategyTable,
}

/// One criterion of an objective, and what the strategy did to it.
#[derive(Facet)]
pub struct ObjectiveUse {
    pub criterion: String,
    pub weight: f64,
    /// Its chance under the chosen strategy.
    pub probability: f64,
    /// Its chance under the strategy that would serve it alone.
    pub alone: f64,
    /// Its chance under the rule the file declared, where it declared one.
    #[facet(skip_serializing_if = Option::is_none)]
    pub declared: Option<f64>,
}

#[derive(Facet)]
pub struct Threshold {
    pub cards: u32,
    /// A hand of this size is kept when it scores at least this.
    pub keep_at_least: f64,
}

/// A strategy as a lookup table.
#[derive(Facet)]
pub struct StrategyTable {
    /// What each of the strategy's groups is. An opener below is a count per
    /// group, in this order.
    pub groups: Vec<String>,
    pub openers: Vec<OpenerUse>,
}

#[derive(Facet)]
pub struct OpenerUse {
    pub hand: Vec<u32>,
    /// The chance of being dealt it.
    pub share: f64,
    /// One per hand size, largest first.
    pub decisions: Vec<DecisionUse>,
}

#[derive(Facet)]
pub struct DecisionUse {
    pub keep: bool,
    /// What the hand scores, put back the best way.
    pub value: f64,
    /// The ways of putting back that score it, as counts per group, and the
    /// chance each is the one taken.
    pub bottom: Vec<BottomUse>,
}

#[derive(Facet)]
pub struct BottomUse {
    pub cards: Vec<u32>,
    pub share: f64,
}

/// The declared mulligan this run kept its hands by, and what it came to.
///
/// Carried as the rule rather than as a flag, for the reason the land drop's
/// list is: two mulligans over one deck are two different sets of numbers, and
/// a reader has to be able to see which produced the page in front of them.
#[derive(Facet)]
pub struct MulliganUse {
    /// The keep rule, one clause per entry, as the run reads it.
    pub keep: Vec<String>,
    /// What the file wrote, first to go back first.
    pub bottom: Vec<String>,
    /// What this run did about the cards the list does not name.
    pub then: &'static str,
    /// How a tie inside one entry was settled.
    pub tie_break: &'static str,
    /// The smallest hand gone to, kept whatever it holds.
    pub down_to: u32,
    /// The share of games kept at each hand size, largest first. Sums to 1.
    pub kept: Vec<KeptAt>,
    /// `"exact"` or `"sampled"`, for `kept`.
    pub method: Method,
}

/// How often one hand size was the one kept.
#[derive(Facet)]
pub struct KeptAt {
    pub cards: u32,
    pub share: f64,
}

/// The declared priority this run resolved its land drops by.
///
/// Carried as the list rather than as a flag. "Resolved by policy" is not the
/// claim worth reporting — *which* policy is, because two different lists over
/// the same deck are two different numbers, and a reader comparing them has to
/// be able to see which one produced the page in front of them.
#[derive(Facet)]
pub struct LandDropUse {
    /// What the file wrote, highest priority first.
    pub prefer: Vec<String>,
    /// The tier the file did not write: everything else that is a land.
    pub then: &'static str,
    /// How a tie inside one entry was settled, stated rather than buried.
    pub tie_break: &'static str,
}

/// The declared priority this run spent its mana by.
///
/// Reported for the same reason as the land drop, and it carries more weight:
/// *which* spells a line casts decides what is left in hand, so every count in
/// a run that declared one depends on this list. `then` is the field that is
/// not a mirror of the land drop's — silence about a spell means it is not
/// cast, where silence about a land still plays it — and it is stated rather
/// than left for a reader to infer from a number that came out lower than they
/// expected.
#[derive(Facet)]
pub struct CastingUse {
    /// What the file wrote, highest priority first.
    pub prefer: Vec<String>,
    /// What this run did about the spells the list is silent on.
    pub then: &'static str,
    /// How a tie inside one entry was settled, stated rather than buried.
    pub tie_break: &'static str,
    /// The commanders the list names, which it casts from the command zone.
    /// Absent where it names none, which is every line that casts only what
    /// it draws.
    #[facet(skip_serializing_if = Vec::is_empty)]
    pub from_command_zone: Vec<String>,
}

impl OptimisedUse {
    fn human(&self) -> String {
        let mut out = String::new();
        out.push_str(if self.played {
            "note: every number below is of the hand the mulligan chosen for this objective \
             keeps:\n"
        } else {
            "note: the best mulligan for this objective, beside the rule declared above, which \
             the\n      numbers below are still played under:\n"
        });
        let terms: Vec<String> = self
            .objective
            .iter()
            .map(|o| format!("{} × {:?}", trim(o.weight), o.criterion))
            .collect();
        out.push_str(&format!("      {}\n", terms.join("\n    + ")));
        let most: f64 = self.objective.iter().map(|o| o.weight).sum();
        out.push_str(&format!(
            "      Expected score {:.4} of {}{}.\n",
            self.score,
            trim(most),
            match self.score_declared {
                Some(d) => format!(", against {d:.4} under the declared rule"),
                None => String::new(),
            }
        ));
        for t in &self.thresholds {
            out.push_str(&format!(
                "      Keep {} cards scoring at least {:.4}.\n",
                t.cards, t.keep_at_least
            ));
        }
        out.push_str(&format!(
            "      A hand of {} {}.\n      Cards go back the way that scores best. Ties: {}.\n",
            self.down_to,
            gauntlet_criteria::MulliganPolicy::FLOOR,
            self.tie_break
        ));
        let shares: Vec<String> = self
            .kept
            .iter()
            .map(|k| format!("{} cards {:.2}%", k.cards, k.share * 100.0))
            .collect();
        out.push_str(&format!("      Kept at {}.\n", shares.join(", ")));
        let width = self
            .objective
            .iter()
            .map(|o| o.criterion.chars().count())
            .max()
            .unwrap_or(0)
            + 2;
        out.push_str(&format!(
            "      {:width$}  chosen   alone{}\n",
            "",
            if self.played { "" } else { "  declared" }
        ));
        for o in &self.objective {
            out.push_str(&format!(
                "      {:width$}  {:>6.2}%  {:>6.2}%{}\n",
                format!("{:?}", o.criterion),
                o.probability * 100.0,
                o.alone * 100.0,
                match o.declared {
                    Some(d) => format!("  {:>6.2}%", d * 100.0),
                    None => String::new(),
                }
            ));
        }
        out.push_str(
            "      A strategy values only what this run models: a hand whose strength is a card \
             this\n      tool cannot yet play — a cantrip, a cycling land — is undervalued by \
             it.\n",
        );
        out
    }
}

/// A weight as it was probably written: `3`, not `3.0`.
fn trim(weight: f64) -> String {
    if weight.fract() == 0.0 {
        format!("{weight:.0}")
    } else {
        format!("{weight}")
    }
}

/// SHA-256 of some bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Facet)]
pub struct Report {
    pub library_size: u32,
    pub commanders: Vec<String>,
    pub excluded: Vec<ExcludedCard>,
    pub on_the_draw: bool,
    pub method: RunMethod,
    /// Sampled runs only: `"requested"` or `"too_wide"`.
    ///
    /// A string rather than a bool, because there will be more ways to end up
    /// here than there are today and a `fell_back: false` would have to be
    /// reinterpreted rather than extended.
    #[facet(skip_serializing_if = Option::is_none)]
    pub sampled_because: Option<SampledBecause>,
    /// Present exactly when `sampled_because` is `"too_wide"`.
    #[facet(skip_serializing_if = Option::is_none)]
    pub too_wide: Option<TooWide>,
    #[facet(skip_serializing_if = Option::is_none)]
    pub trials: Option<u32>,
    /// Present only for sampled runs, alongside `trials`: a sampled figure is
    /// only reproducible by whoever knows which seed dealt the hands.
    #[facet(skip_serializing_if = Option::is_none)]
    pub seed: Option<u64>,
    pub queries: Vec<QueryMatch>,
    /// Every zone the criteria file asked about, including the `hand` a clause
    /// meant without saying so.
    pub zones: Vec<ZoneUse>,
    /// Every effect that applied to a card in this deck, standard library and
    /// hand-written alike.
    pub effects: Vec<EffectUse>,
    /// How this run enumerated: one entry per class of question, with the
    /// width it cost and whether it was walked or sampled.
    ///
    /// Always present, including on a run that sampled everything: the
    /// partition is a fact about the file rather than about the engine that
    /// answered it, and a caller comparing two runs of the same file needs to
    /// see it either way.
    pub enumerations: Vec<Enumeration>,
    /// Lands this run assumed into play tapped because the card lets the pilot
    /// decide and nobody has.
    ///
    /// Reported for the same reason the effects are: it moves the numbers and
    /// nobody asked for it. A shockland's "you may pay 2 life" is a choice, the
    /// run makes it pessimistically, and a percentage cannot say so on its own.
    /// Empty on every run that asks no mana question.
    #[facet(skip_serializing_if = Vec::is_empty)]
    pub assumed_tapped: Vec<String>,
    /// The priority this run resolved its land drops by, where a file declared
    /// one.
    ///
    /// Absent on a run that declared none, which is not the same fact as an
    /// empty list: it says the drop was not arbitrated at all, because nothing
    /// in that run needed it to be.
    #[facet(skip_serializing_if = Option::is_none)]
    pub land_drop: Option<LandDropUse>,
    /// The priority this run spent its mana by, where a file declared one.
    ///
    /// Absent on a run that declared none, and that absence is the stronger
    /// fact of the two: it says this run cast nothing at all, so every count
    /// below is of a hand nobody ever spent.
    #[facet(skip_serializing_if = Option::is_none)]
    pub casting: Option<CastingUse>,
    /// The mulligan every number here is of, where the file declared one.
    ///
    /// Absent on a run that declared none, and every number in that run keeps
    /// whatever seven it was dealt — which the human half of the report says
    /// in so many words.
    #[facet(skip_serializing_if = Option::is_none)]
    pub mulligan: Option<MulliganUse>,
    /// The strategy chosen for the file's objective, where it declared one.
    #[facet(skip_serializing_if = Option::is_none)]
    pub optimised: Option<OptimisedUse>,
    pub criteria: Vec<CriterionResult>,
    /// Alongside `criteria` rather than merged into it. They answer different
    /// questions in different units, and several things already read `criteria`
    /// by that name and that shape.
    pub expectations: Vec<ExpectationResult>,
    pub asserted: usize,
    pub failed: usize,
    pub ok: bool,
    pub provenance: Provenance,
}

impl Report {
    pub fn build(
        questions: Questions<'_>,
        answers: &Answers,
        scenario: Scenario,
        library: &Library,
        breakdown: Breakdown,
        provenance: Provenance,
    ) -> Self {
        let Breakdown {
            queries,
            zones,
            effects,
            enumerations,
            assumed_tapped,
            land_drop,
            casting,
            mulligan,
            optimised,
        } = breakdown;
        let Scenario {
            on_the_draw,
            sampled,
        } = scenario;
        // Per answer rather than per run: the enumerated ones get no error bar
        // and the sampled ones never go without.
        let results: Vec<CriterionResult> = questions
            .criteria
            .iter()
            .zip(&answers.probabilities)
            .zip(&answers.estimated.criteria)
            .zip(&answers.seven)
            .map(|(((c, &p), &estimated), &seven)| {
                let how = sampled.filter(|_| estimated);
                let missed = c.missed(p);
                CriterionResult {
                    name: c.name.clone(),
                    probability: round(p, 6),
                    percent: round(p * 100.0, 2),
                    standard_error: how
                        .map(|s| round(gauntlet_sim::standard_error(p, s.trials), 6)),
                    at_least: c.at_least,
                    at_most: c.at_most,
                    // A criterion with no threshold is informational; it reports
                    // a number and cannot fail.
                    pass: missed.is_none(),
                    missed: missed.map(Bound::key),
                    // Either end of a range can be the close call, and the
                    // nearer one is the one that decides it.
                    inconclusive: how.and_then(|s| {
                        let threshold = nearest_bound(c.at_least, c.at_most, p)?;
                        let se = gauntlet_sim::standard_error(p, s.trials);
                        Some((p - threshold).abs() <= INCONCLUSIVE_ERRORS * se)
                    }),
                    method: Method::of(estimated),
                    keep_seven: seven.map(|s| round(s, 6)),
                    keep_seven_percent: seven.map(|s| round(s * 100.0, 2)),
                }
            })
            .collect();

        let expected: Vec<ExpectationResult> = questions
            .expectations
            .iter()
            .zip(&answers.distributions)
            .zip(&answers.estimated.expectations)
            .map(|((e, d), &estimated)| ExpectationResult {
                name: e.name.clone(),
                mean: round(d.mean(), 4),
                distribution: d.probabilities().iter().map(|p| round(*p, 6)).collect(),
                standard_error: sampled
                    .filter(|_| estimated)
                    .map(|s| round(gauntlet_sim::mean_standard_error(d, s.trials), 6)),
                method: Method::of(estimated),
            })
            .collect();

        let asserted = questions.criteria.iter().filter(|c| c.asserts()).count();
        let failed = results.iter().filter(|r| !r.pass).count();

        Report {
            library_size: library.size(),
            commanders: library.commander_names(),
            excluded: library
                .excluded
                .iter()
                .map(|e| ExcludedCard {
                    name: e.name.clone(),
                    qty: e.qty,
                    card_type: e.card_type.as_str(),
                })
                .collect(),
            on_the_draw,
            method: RunMethod::of(&answers.estimated),
            sampled_because: sampled.map(|s| match s.why {
                WhySampled::Requested => SampledBecause::Requested,
                WhySampled::TooWide { .. } => SampledBecause::TooWide,
            }),
            too_wide: sampled.and_then(|s| match s.why {
                WhySampled::Requested => None,
                WhySampled::TooWide { paths, groups } => Some(TooWide {
                    paths: paths as f64,
                    groups,
                    ceiling: gauntlet_criteria::MAX_PATHS as f64,
                }),
            }),
            trials: sampled.map(|s| s.trials),
            seed: sampled.map(|s| s.seed),
            queries,
            zones,
            effects,
            enumerations,
            assumed_tapped,
            land_drop,
            casting,
            mulligan,
            optimised,
            criteria: results,
            expectations: expected,
            asserted,
            failed,
            ok: failed == 0,
            provenance,
        }
    }

    pub fn human(&self) -> String {
        let mut out = String::new();
        // First, above everything, because it does not qualify one number: it
        // changes what kind of thing every number below is. A reader who skims
        // the notes and reads the percentages must still have been told, and
        // the only place that survives skimming is the top.
        if let Some(note) = self.estimate_note() {
            out.push_str(&note);
        }
        for q in &self.queries {
            if q.cards == 0 {
                out.push_str(&format!(
                    "note: query {:?} matched no cards in this deck\n",
                    q.query
                ));
            }
        }
        // What the effect library made of this deck. Printed rather than left
        // in the JSON because the library autoloads: nobody asked for these and
        // they move the numbers, so a run that applied one says so unprompted.
        // An entry that matched nothing says nothing and is not mentioned.
        for e in &self.effects {
            let route = match (&e.to_graveyard, e.live) {
                (None, _) if e.fetch.is_some() => String::new(),
                (None, _) => ", everything stays on top".to_string(),
                (Some(q), true) => format!(", {q} to the graveyard"),
                (Some(q), false) => format!(", {q} to the graveyard — which no card here matches"),
            };
            let look = match e.look {
                0 => String::new(),
                n => format!("look {n}, "),
            };
            // A delayed effect says how long it waited and what it cost, in the
            // same parenthesis as the trigger it waited from: a Lantern that
            // arrived on turn five because of a land played on turn three is a
            // number with two turns in it, and the note names both.
            let route = match (e.after, e.sacrifice) {
                (Some(n), sacrifice) => format!(
                    ", {n} turn{} later{}{route}",
                    if n == 1 { "" } else { "s" },
                    if sacrifice == Some(true) {
                        ", then sacrificed"
                    } else {
                        ""
                    }
                ),
                (None, _) => route,
            };
            out.push_str(&format!(
                "note: effect {:?} ({look}on {}{route})\n      applies to {} card{}: {}\n",
                e.matches,
                e.on,
                e.copies,
                if e.copies == 1 { "" } else { "s" },
                e.cards.join(", ")
            ));
            // A tutor names what it went and got, in the order it would take
            // them. Same discipline as the land drop and the casting line
            // below, over the fourth contested resource: the library this run
            // reports is one card smaller because of this list, so the list is
            // an input to every number under it.
            if let (Some(prefer), Some(to)) = (&e.fetch, e.to) {
                out.push_str(&format!(
                    "      and fetches, to your {to}, the first of these the library still \
                     holds:\n"
                ));
                for (i, query) in prefer.iter().enumerate() {
                    out.push_str(&format!("      {}. {query:?}\n", i + 1));
                }
                out.push_str(
                    "      Ties: the card this decklist names first. A tutor that finds none of \
                     them fetches nothing.\n",
                );
            }
        }
        // The same failure as an empty query, arriving by a different door. A
        // criterion asking about a zone no effect in this run routes into is
        // answered with a confident zero that reads exactly like a deck that
        // never gets there. It stays answered — silently refusing to print a
        // number is its own kind of lie — but it does not get to be mistaken
        // for a measurement. It switches off the moment an effect routes a card
        // there or the line names a spell that resolves into it, and not before: a note that never fires is as useless as one
        // that always does.
        for z in &self.zones {
            if z.reachable {
                continue;
            }
            let fix = if z.zone == "battlefield" {
                "This deck holds no land, so nothing can be played."
            } else {
                "Declare `to_graveyard` on an [[effect]] to route one there, or name an\n      \
                 instant or sorcery in [casting]: one the line casts resolves into it."
            };
            out.push_str(&format!(
                "note: nothing routes a card to the {} in this run, so every count in\n      \
                 it is zero by construction rather than by measurement.\n      \
                 Asked by: {:?}\n      {fix}\n",
                z.zone, z.asked_by
            ));
        }
        // Which land this run played, on every turn it played one. A land drop
        // is one resource with two claimants — the effect that looks at cards
        // and the mana that pays for them — and a run that resolved it by a
        // declared priority says so, because a number that depended on that
        // choice and did not name it is the failure this tool exists to
        // prevent. Printed whether or not anything else moved: the list is an
        // input, exactly like the deck and the criteria file.
        if let Some(policy) = &self.land_drop {
            out.push_str(
                "note: the land drop here is decided by the priority this file declared, and \
                 every\n      number below that depends on which land was played depends on \
                 it:\n",
            );
            for (i, query) in policy.prefer.iter().enumerate() {
                out.push_str(&format!("      {}. {query:?}\n", i + 1));
            }
            out.push_str(&format!(
                "      then {}. Ties: {}.\n",
                policy.then, policy.tie_break
            ));
        }
        // Which spells this run spent its mana on. The same argument as the
        // land drop and a louder one: the pool is a budget, so casting the
        // first thing on this list is what makes the second thing on it
        // uncastable — and a card the list never names is one this run did not
        // cast at all, which is a number no reader could reconstruct from the
        // deck and the criteria file alone.
        if let Some(policy) = &self.casting {
            out.push_str(
                "note: the spells cast here are decided by the priority this file declared, and \
                 every\n      number below that depends on what was cast depends on it:\n",
            );
            for (i, query) in policy.prefer.iter().enumerate() {
                out.push_str(&format!("      {}. {query:?}\n", i + 1));
            }
            out.push_str(&format!(
                "      Then {}. Ties: {}.\n",
                policy.then, policy.tie_break
            ));
            // A card cast without being drawn is a claim no reader could
            // reconstruct from the list of queries, so it is named.
            if !policy.from_command_zone.is_empty() {
                out.push_str(&format!(
                    "      Cast from the command zone — never drawn, always there, cast at most \
                     once: {}.\n      At equal cost inside one entry, a card from the library \
                     is cast first.\n",
                    policy.from_command_zone.join(", ")
                ));
            }
        }
        // Which hand every number is of. Above the tapped-ness assumptions and
        // beside the other declared priorities, because it is the same kind of
        // fact and the largest one: a mulligan decides which seven the land
        // drop, the casting line and every count below were played from.
        match &self.mulligan {
            Some(m) => {
                out.push_str(
                    "note: every number below is of the hand the mulligan this file declared \
                     keeps.\n      A hand is kept when it holds ",
                );
                out.push_str(&m.keep.join(", and "));
                out.push_str(".\n      After a mulligan, cards go back in this order:\n");
                for (i, query) in m.bottom.iter().enumerate() {
                    out.push_str(&format!("      {}. {query:?}\n", i + 1));
                }
                out.push_str(&format!(
                    "      then {}.\n      Ties: {}.\n      A hand of {} {}.\n",
                    m.then,
                    m.tie_break,
                    m.down_to,
                    gauntlet_criteria::MulliganPolicy::FLOOR,
                ));
                let shares: Vec<String> = m
                    .kept
                    .iter()
                    .map(|k| format!("{} cards {:.2}%", k.cards, k.share * 100.0))
                    .collect();
                out.push_str(&format!(
                    "      Kept at {}{}.\n      Each criterion also shows, in brackets, its \
                     number had every first seven been kept.\n",
                    shares.join(", "),
                    if m.method == Method::Sampled {
                        ", sampled"
                    } else {
                        ""
                    },
                ));
            }
            None if self.optimised.as_ref().is_some_and(|o| o.played) => {}
            None => out.push_str(
                "note: every number below keeps whatever seven it is dealt: this file declares \
                 no\n      [mulligan], so no hand is ever sent back.\n",
            ),
        }
        if let Some(o) = &self.optimised {
            out.push_str(&o.human());
        }
        // An assumption the tool made on the pilot's behalf, which moves
        // numbers and which nobody wrote down. Printed on every run it touched,
        // and naming the cards rather than the count: "three lands assumed
        // tapped" is not something a reader can check, and "Hallowed Fountain"
        // is.
        if !self.assumed_tapped.is_empty() {
            out.push_str(&format!(
                "note: {} here let the pilot decide whether to enter tapped. This run assumes \
                 they do:\n      {}.\n      \
                 A shockland's 2 life is a decision no criteria file has made yet, so the \
                 pessimistic\n      reading is taken: it makes no mana the turn it arrives. \
                 Every number below that\n      depends on one of these is a floor rather than \
                 a measurement.\n",
                if self.assumed_tapped.len() == 1 {
                    "one land".to_string()
                } else {
                    format!("{} lands", self.assumed_tapped.len())
                },
                self.assumed_tapped.join(", ")
            ));
        }
        // One width across both sections, so the numbers line up down the whole
        // report rather than restarting at the second heading.
        let width = self
            .criteria
            .iter()
            .map(|c| c.name.chars().count())
            .chain(self.expectations.iter().map(|e| e.name.chars().count()))
            .max()
            .unwrap_or(0)
            .max(9);

        for c in &self.criteria {
            let status = match (c.at_least.or(c.at_most), c.pass) {
                (None, _) => "     ",
                (Some(_), true) => "PASS ",
                (Some(_), false) => "FAIL ",
            };
            // A lone `at_least` reads as it always has. The other shapes say
            // which way they point, and a missed range says which end.
            let target = match (c.at_least, c.at_most) {
                (None, None) => String::new(),
                (Some(lo), None) => format!("  (needs {:.1}%)", lo * 100.0),
                (None, Some(hi)) => format!("  (needs at most {:.1}%)", hi * 100.0),
                (Some(lo), Some(hi)) => format!(
                    "  (needs {:.1}% to {:.1}%{})",
                    lo * 100.0,
                    hi * 100.0,
                    match c.missed {
                        Some(k) if k == Bound::AtLeast.key() => ": under it",
                        Some(_) => ": over it",
                        None => "",
                    }
                ),
            };
            // On the line rather than in a footnote. A sampled percentage
            // printed to two decimals looks exactly as certain as an enumerated
            // one, and the error bar is the only thing on the page that says it
            // is not.
            // The keep-your-seven number, labelled, where a mulligan moved it:
            // the verdict is the mulligan's, and a reader comparing with a run
            // from before the mulligan was declared needs to see both.
            let seven = match c.keep_seven_percent {
                Some(s) => format!("  [keep 7: {s:.2}%]"),
                None => String::new(),
            };
            out.push_str(&format!(
                "{status}{:width$}  {:>6.2}%{}{target}{seven}\n",
                c.name,
                c.percent,
                error_bar(c.standard_error)
            ));
        }

        // The blank status column is not decoration: it is the same five
        // characters PASS and FAIL occupy, and it says that an expectation has
        // no verdict to give rather than that its verdict was omitted.
        let indent = 5 + width + 2;
        for e in &self.expectations {
            out.push_str(&format!(
                "     {:width$}  mean {:.2}{}\n",
                e.name,
                e.mean,
                // In the mean's own units rather than in percentage points: an
                // expectation counts cards, and a standard error scaled like a
                // proportion's would be a different quantity under the same
                // symbol.
                match e.standard_error {
                    Some(se) => format!(" ± {}", error_bar_value(se)),
                    None => String::new(),
                }
            ));
            for line in histogram_lines(&e.distribution, indent) {
                out.push_str(&format!("{:indent$}{line}\n", ""));
            }
        }

        // After the verdicts rather than before them, because this note is only
        // readable next to the line it is about: the PASS or FAIL it qualifies
        // has to have been printed first.
        for c in &self.criteria {
            let (Some(true), Some(threshold), Some(se)) = (
                c.inconclusive,
                nearest_bound(c.at_least, c.at_most, c.probability),
                c.standard_error,
            ) else {
                continue;
            };
            out.push_str(&format!(
                "\nnote: {:?} is {:.2}% ± {} against a threshold of\n      \
                 {:.1}%, so the threshold is inside the error bar. {} is this seed's answer\n      \
                 rather than this deck's, and another seed could return the other one.\n      \
                 Widen the margin, raise --trials, or ask a question the exact engine\n      \
                 can answer.\n",
                c.name,
                c.percent,
                error_bar_value(se * 100.0),
                threshold * 100.0,
                if c.pass { "PASS" } else { "FAIL" },
            ));
        }

        // How close the run came to the ceiling, on every run rather than only
        // the one that crosses it: a file that answers exactly on the play and
        // is estimated on the draw should not be the first anyone hears of how
        // wide it was (#52). The widest question walked exactly, and its share.
        if let Some(widest) = self
            .enumerations
            .iter()
            .filter(|e| e.method == Method::Exact)
            .max_by(|a, b| a.compositions.total_cmp(&b.compositions))
        {
            let ceiling = gauntlet_criteria::MAX_PATHS as f64;
            let name = widest
                .criteria
                .first()
                .or(widest.expectations.first())
                .map_or(String::new(), |n| format!(", {n:?}"));
            out.push_str(&format!(
                "\nwidest exact question: {} compositions across {} groups, {} of the {} \
                 ceiling{name}\n",
                thousands(widest.compositions),
                widest.groups,
                share(widest.compositions / ceiling),
                thousands(ceiling),
            ));
        }

        out.push('\n');
        out.push_str(&if self.ok {
            format!(
                "PASS: {} of {} assertions met",
                self.asserted, self.asserted
            )
        } else {
            format!(
                "FAIL: {} of {} assertions missed",
                self.failed, self.asserted
            )
        });
        out
    }

    /// What to say, before any number, about numbers that are estimates.
    ///
    /// Two different events, and the difference is the whole of why an
    /// automatic fallback is allowed to exist. `--simulate` is a choice
    /// somebody made, so it gets a note in the same voice as every other note.
    /// Falling back is the tool answering a question nobody could have known
    /// was too wide until it tried, with a number of a different kind from the
    /// one that was asked for — so it gets a heading of its own, in a column
    /// nothing else in this output uses, and it says the word estimate.
    fn estimate_note(&self) -> Option<String> {
        let trials = self.trials?;
        // Keyed off the width rather than off the label beside it, so the loud
        // form cannot be lost to a report that says `too_wide` and forgot to
        // say how wide.
        if let Some(w) = &self.too_wide {
            let estimated = self.estimated_names();
            let total = self.criteria.len() + self.expectations.len();
            // Which questions, when it is not all of them. Since #31 a file is
            // several enumerations, so "this question was too wide" is true of
            // some of them and not others, and a reader quoting one of the
            // exact numbers below has to be able to tell which they have.
            let scope = if estimated.len() == total {
                "Every percentage below is an ESTIMATE, not an exact answer.\n          \
                 The ± beside each one is its standard error, and a difference\n          \
                 smaller than that is not a difference."
                    .to_string()
            } else {
                format!(
                    "{} of the {total} questions here needed that, and {}\n          \
                     quoted with a ±:\n          \
                     {}\n          \
                     Everything else below was enumerated exactly. A difference\n          \
                     smaller than a figure's ± is not a difference.",
                    estimated.len(),
                    if estimated.len() == 1 {
                        "it is the one"
                    } else {
                        "they are the ones"
                    },
                    estimated.join("\n          "),
                )
            };
            return Some(format!(
                "ESTIMATE: a question here was too wide to enumerate exactly: {:.0}\n          \
                 compositions across {} groups, against a ceiling of {:.0}. It was\n          \
                 answered by sampling {trials} hands instead.\n          \
                 {scope}\n          \
                 Pass --exact to refuse a question this wide rather than estimate it.\n",
                w.paths, w.groups, w.ceiling,
            ));
        }
        Some(format!(
            "note: sampled rather than enumerated, because --simulate asked for it. Every\n      \
             percentage below is an estimate from {trials} hands, ± its standard error.\n"
        ))
    }

    /// The questions this run estimated, by name, in the order they are
    /// printed.
    fn estimated_names(&self) -> Vec<&str> {
        self.criteria
            .iter()
            .filter(|c| c.method == Method::Sampled)
            .map(|c| c.name.as_str())
            .chain(
                self.expectations
                    .iter()
                    .filter(|e| e.method == Method::Sampled)
                    .map(|e| e.name.as_str()),
            )
            .collect()
    }
}

/// A sampled figure's error bar, in the same units as the figure.
///
/// Empty for an exact run, which has no error to report and must not be given
/// one: a `± 0.00` reads as a measurement that happened to be precise rather
/// than as a number that was never sampled.
fn error_bar(standard_error: Option<f64>) -> String {
    match standard_error {
        Some(se) => format!(" ± {}", error_bar_value(se * 100.0)),
        None => String::new(),
    }
}

/// A standard error printed to enough places to be a number.
///
/// Two decimals suit a percentage and lose an expectation's mean entirely: a
/// mean is counted in cards, where a hundred-thousand-hand error bar really is
/// four thousandths of one, and `± 0.00` reads as a figure that was measured
/// exactly rather than as a small uncertainty. So the precision follows the
/// number instead of the other way round.
fn error_bar_value(standard_error: f64) -> String {
    // An exactly-zero error is a real measurement rather than a small one: a
    // criterion that held in no hand at all has nothing to be uncertain about,
    // and trailing zeroes hunting for a digit that is not there would suggest
    // otherwise.
    if standard_error == 0.0 {
        return "0.00".to_string();
    }
    let places = (2..=6)
        .find(|&p| standard_error >= 0.5 * 10f64.powi(-p))
        .unwrap_or(6) as usize;
    format!("{standard_error:.places$}")
}

/// How many standard errors around a threshold count as too close to call.
///
/// Two, so the flag fires roughly when a 95% interval straddles the threshold.
/// One would call a third of genuine passes inconclusive and train the reader
/// to ignore the note; three would stay quiet while the verdict flips from seed
/// to seed.
const INCONCLUSIVE_ERRORS: f64 = 2.0;

/// Of a criterion's bounds, the one `p` sits closest to: the one a sampling
/// error could carry it across.
fn nearest_bound(at_least: Option<f64>, at_most: Option<f64>, p: f64) -> Option<f64> {
    at_least
        .into_iter()
        .chain(at_most)
        .min_by(|a, b| (p - a).abs().total_cmp(&(p - b).abs()))
}

/// What to tell a human about the cards that never reach the library.
///
/// Printed before the run rather than folded into `human()`, because a list that
/// is *all* stickers fails with "the library is empty" and the reader still
/// needs to know where their cards went.
/// A warning that the index predates fields the queries read.
///
/// Louder than a bare note because the failure it prevents is silent: a
/// `produces:` or `pow:` term against an index built before those fields
/// existed matches nothing and reports a confident 0%, which reads exactly like
/// a deck that genuinely has none. Telling the two apart is what this tool is
/// for, so it will not answer that question without saying which one it might
/// be looking at.
pub fn stale_index_note(library: &Library) -> Option<String> {
    library.index_is_stale.then(|| {
        "note: this index was built before some of the fields queries now read.\n\
         Terms such as produces:, c:, pow: and f: will match nothing rather than \
         fail, which reads\n      the same as a deck that has none. Rebuild it \
         with: gauntlet sync"
            .to_string()
    })
}

/// What to tell a human about effects this index cannot evaluate.
///
/// Separate from the "matched no cards" note beside it, because they are
/// different failures: that one is a fact about the deck, this one is a fact
/// about the index, and only the second one is why `"effects": []` is not the
/// answer it looks like. The no-tags case is reported once for all the entries
/// it silences rather than once each — it is one fact about one file.
pub fn tag_blind_notes(resolved: &crate::effects::Resolved, library: &Library) -> Vec<String> {
    let mut notes = Vec::new();
    let blinded: Vec<&str> = resolved
        .tag_blind
        .iter()
        .filter(|b| matches!(b.gap, TagGap::NoneFetched(_)))
        .map(|b| b.matches.as_str())
        .collect();
    if !blinded.is_empty() {
        notes.push(format!(
            "note: this index carries no oracle tags, so {} effect library {}\n      \
             cannot match any card here: {}.\n      \
             The effect library is keyed on otag:, so this run models no looks and no \
             routing\n      at all — any zone they would have fed is empty by construction.\n      \
             Fetch the tags with: gauntlet sync   (--from cannot: they come from the \
             search API)",
            blinded.len(),
            if blinded.len() == 1 {
                "entry"
            } else {
                "entries"
            },
            blinded
                .iter()
                .map(|m| format!("{m:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for blind in &resolved.tag_blind {
        if let TagGap::NotCarried(tags) = &blind.gap {
            notes.push(format!(
                "note: effect {:?} names {}, which this index does not carry,\n      \
                 so it matched nothing here rather than nothing being there. \
                 This index carries: {}.",
                blind.matches,
                tags.iter()
                    .map(|t| format!("otag:{t}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                library.index_tags.carried().join(", ")
            ));
        }
    }
    notes
}

/// The resolved effect library, in the shape the report prints.
pub fn effects_applied(resolved: &crate::effects::Resolved) -> Vec<EffectUse> {
    resolved
        .applied
        .iter()
        .map(|a| EffectUse {
            matches: a.matches.clone(),
            look: a.look,
            on: a.on,
            to_graveyard: a.to_graveyard.clone(),
            fetch: a.fetch.as_ref().map(|(prefer, _)| prefer.clone()),
            to: a.fetch.as_ref().map(|(_, to)| *to),
            after: a.delay.map(|d| d.turns),
            sacrifice: a.delay.map(|d| d.sacrifice),
            source: a.origin.clone(),
            cards: a.cards.clone(),
            copies: a.copies,
            live: a.live,
        })
        .collect()
}

pub fn exclusion_note(library: &Library) -> Option<String> {
    if library.excluded.is_empty() {
        return None;
    }
    let total: u32 = library.excluded.iter().map(|e| e.qty).sum();
    let list: Vec<String> = library
        .excluded
        .iter()
        .map(|e| format!("{}x {} ({})", e.qty, e.name, e.card_type))
        .collect();
    Some(format!(
        "note: {total} card{} never in the library and not counted: {}",
        if total == 1 { "" } else { "s" },
        list.join(", ")
    ))
}

/// The cards counted from a token category because each shares its name with
/// a real card.
pub fn token_named_note(library: &Library) -> Option<String> {
    if library.token_named.is_empty() {
        return None;
    }
    Some(format!(
        "note: counted from a token category, because each shares its name with a real card: \
         {}.\n      The index holds cards and not tokens, so these are the cards. If the \
         list meant the\n      tokens, mark the category `{{noDeck}}`.",
        library.token_named.join(", ")
    ))
}

/// Everything worth saying about the library before a question is read, in
/// the order it is printed.
pub fn library_notes(library: &Library) -> Vec<String> {
    [
        stale_index_note(library),
        exclusion_note(library),
        token_named_note(library),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A whole number with thousands separators, as the docs quote widths.
fn thousands(n: f64) -> String {
    let digits = format!("{:.0}", n);
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A share of the ceiling, to the precision that tells a question at 0.1%
/// from one at 99%.
fn share(fraction: f64) -> String {
    match fraction * 100.0 {
        p if p < 0.01 => "under 0.01%".to_string(),
        p if p < 1.0 => format!("{p:.2}%"),
        p => format!("{p:.0}%"),
    }
}

/// How many buckets of a distribution the human output will print.
///
/// A distribution can have as many buckets as the deck has cards, and printing
/// ninety of them is not a histogram, it is a wall of noise that nobody reads
/// and that hides the shape it was meant to show. Twelve is about what fits on
/// two lines of a terminal at a name width that is still readable.
const HISTOGRAM_BUCKETS: usize = 12;

/// Total line width the wrapped histogram may reach, indent included.
const HISTOGRAM_WIDTH: usize = 78;

/// A bucket below this prints as `0.0%`, spending a column to say nothing.
const NEGLIGIBLE: f64 = 0.0005;

/// The stretch of a distribution worth showing: the contiguous window of at most
/// [`HISTOGRAM_BUCKETS`] holding the most mass, with any ends that would print as
/// `0.0%` dropped.
///
/// Contiguous rather than "the twelve likeliest buckets", because a histogram
/// with holes punched in it reads as missing data. Windowed rather than
/// truncated at either end, because the interesting region of "lands by turn
/// twelve" is nowhere near zero.
fn histogram_window(p: &[f64]) -> Option<(usize, usize)> {
    if p.is_empty() {
        return None;
    }
    let width = HISTOGRAM_BUCKETS.min(p.len());
    let mut best = (0usize, p[..width].iter().sum::<f64>());
    let mut mass = best.1;
    for lo in 1..=p.len() - width {
        mass += p[lo + width - 1] - p[lo - 1];
        if mass > best.1 {
            best = (lo, mass);
        }
    }
    let (mut lo, mut hi) = (best.0, best.0 + width - 1);
    while hi > lo && p[hi] < NEGLIGIBLE {
        hi -= 1;
    }
    while lo < hi && p[lo] < NEGLIGIBLE {
        lo += 1;
    }
    Some((lo, hi))
}

/// The histogram as lines of `value: percent` cells, wrapped to the terminal.
///
/// Whatever the window leaves out is stated as a total rather than dropped. The
/// full distribution is in the JSON, so the human summary can afford to be a
/// summary — but a summary that quietly loses four percent of the mass is the
/// confidently wrong number this tool exists to prevent, in miniature.
fn histogram_lines(p: &[f64], indent: usize) -> Vec<String> {
    let Some((lo, hi)) = histogram_window(p) else {
        return Vec::new();
    };
    let cells: Vec<String> = (lo..=hi)
        .map(|k| format!("{k}: {:.1}%", p[k] * 100.0))
        .collect();
    let cell_width = cells.iter().map(|c| c.chars().count()).max().unwrap_or(0);
    let available = HISTOGRAM_WIDTH.saturating_sub(indent).max(cell_width);
    let per_line = ((available + 2) / (cell_width + 2)).max(1);

    let mut lines: Vec<String> = cells
        .chunks(per_line)
        .map(|chunk| {
            chunk
                .iter()
                .map(|c| format!("{c:cell_width$}"))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_string()
        })
        .collect();

    let shown: f64 = p[lo..=hi].iter().sum();
    let outside = p.iter().sum::<f64>() - shown;
    if outside >= NEGLIGIBLE {
        if let Some(last) = lines.last_mut() {
            last.push_str(&format!("  (+{:.1}% outside)", outside * 100.0));
        }
    }
    lines
}

pub fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criterion(
        name: &str,
        p: f64,
        at_least: Option<f64>,
        at_most: Option<f64>,
    ) -> CriterionResult {
        let low = at_least.is_some_and(|lo| p < lo);
        let high = at_most.is_some_and(|hi| p > hi);
        CriterionResult {
            name: name.to_string(),
            probability: p,
            percent: round(p * 100.0, 2),
            at_least,
            at_most,
            standard_error: None,
            pass: !low && !high,
            missed: if low {
                Some(Bound::AtLeast.key())
            } else if high {
                Some(Bound::AtMost.key())
            } else {
                None
            },
            inconclusive: None,
            method: Method::Exact,
            keep_seven: None,
            keep_seven_percent: None,
        }
    }

    fn enumeration(criteria: &[&str], compositions: f64, method: Method) -> Enumeration {
        Enumeration {
            criteria: criteria.iter().map(|c| c.to_string()).collect(),
            expectations: Vec::new(),
            queries: vec!["t:land".to_string()],
            turns: vec![3],
            reading: Reading::Cumulative,
            pips: None,
            groups: 2,
            compositions,
            method,
            deals: None,
        }
    }

    /// A small exact run: one question met, one missed, one expectation.
    fn exact_report() -> Report {
        let criteria = vec![
            criterion("two lands by turn 2", 0.875, Some(0.8), None),
            criterion("flood by turn 5", 0.5, Some(0.1), Some(0.3)),
        ];
        let failed = criteria.iter().filter(|c| !c.pass).count();
        Report {
            library_size: 60,
            commanders: Vec::new(),
            excluded: Vec::new(),
            on_the_draw: false,
            method: RunMethod::Exact,
            sampled_because: None,
            too_wide: None,
            trials: None,
            seed: None,
            queries: vec![
                QueryMatch {
                    query: "t:land".to_string(),
                    cards: 24,
                },
                QueryMatch {
                    query: "name:Rmap".to_string(),
                    cards: 0,
                },
            ],
            zones: Vec::new(),
            effects: Vec::new(),
            enumerations: vec![
                enumeration(&["two lands by turn 2"], 1234.0, Method::Exact),
                enumeration(&["flood by turn 5"], 56789.0, Method::Exact),
            ],
            assumed_tapped: Vec::new(),
            land_drop: None,
            casting: None,
            mulligan: None,
            optimised: None,
            criteria,
            expectations: vec![ExpectationResult {
                name: "lands in opener".to_string(),
                mean: 2.5,
                distribution: vec![0.25, 0.25, 0.5],
                standard_error: None,
                method: Method::Exact,
            }],
            asserted: 2,
            failed,
            ok: failed == 0,
            provenance: Provenance {
                tool_version: "0.0.0",
                index_updated_at: None,
                deck_sha256: String::new(),
                criteria_sha256: String::new(),
                effect_library_sha256: String::new(),
            },
        }
    }

    #[test]
    fn an_exact_run_reads_as_verdicts_under_its_notes() {
        assert_eq!(
            exact_report().human(),
            "note: query \"name:Rmap\" matched no cards in this deck\n\
             note: every number below keeps whatever seven it is dealt: this file declares no\n      \
             [mulligan], so no hand is ever sent back.\n\
             PASS two lands by turn 2   87.50%  (needs 80.0%)\n\
             FAIL flood by turn 5       50.00%  (needs 10.0% to 30.0%: over it)\n     \
             lands in opener      mean 2.50\n                          \
             0: 25.0%  1: 25.0%  2: 50.0%\n\
             \n\
             widest exact question: 56,789 compositions across 2 groups, 1% of the 5,000,000 \
             ceiling, \"flood by turn 5\"\n\
             \n\
             FAIL: 1 of 2 assertions missed"
        );
    }

    /// The same run with its second question over the ceiling and sampled,
    /// under a declared mulligan whose shares the sampler reported.
    fn mixed_report() -> Report {
        let mut report = exact_report();
        report.method = RunMethod::Mixed;
        report.sampled_because = Some(SampledBecause::TooWide);
        report.too_wide = Some(TooWide {
            paths: 9e9,
            groups: 17,
            ceiling: 5e6,
        });
        report.trials = Some(1000);
        report.seed = Some(7);
        report.queries.pop();
        report.criteria[1].method = Method::Sampled;
        report.criteria[1].standard_error = Some(0.0158);
        report.enumerations[1].method = Method::Sampled;
        report.mulligan = Some(MulliganUse {
            keep: vec!["2 to 5 of \"t:land\"".to_string()],
            bottom: vec!["t:land".to_string()],
            then: "anything else",
            tie_break: "the first listed",
            down_to: 5,
            kept: vec![
                KeptAt {
                    cards: 7,
                    share: 0.9,
                },
                KeptAt {
                    cards: 6,
                    share: 0.1,
                },
            ],
            method: Method::Sampled,
        });
        report
    }

    #[test]
    fn a_mixed_run_names_the_estimates_and_walks_only_the_exact_ones() {
        let human = mixed_report().human();
        assert!(
            human.starts_with(
                "ESTIMATE: a question here was too wide to enumerate exactly: 9000000000\n          \
                 compositions across 17 groups, against a ceiling of 5000000. It was\n          \
                 answered by sampling 1000 hands instead.\n          \
                 1 of the 3 questions here needed that, and it is the one\n          \
                 quoted with a ±:\n          \
                 flood by turn 5\n"
            ),
            "{human}"
        );
        assert!(
            human.contains("Kept at 7 cards 90.00%, 6 cards 10.00%, sampled.\n"),
            "{human}"
        );
        assert!(
            human.contains("FAIL flood by turn 5       50.00% ± 1.58  (needs"),
            "{human}"
        );
        // The widest *exact* question: the sampled one is wider and is not it.
        assert!(
            human.contains("widest exact question: 1,234 compositions across 2 groups"),
            "{human}"
        );
    }

    #[test]
    fn the_method_enums_serialise_as_the_strings_the_json_always_carried() {
        let json = facet_json::to_string(&mixed_report()).expect("serialises");
        for needle in [
            r#""method":"mixed""#,
            r#""sampled_because":"too_wide""#,
            r#""reading":"cumulative""#,
            r#""method":"sampled""#,
            r#""method":"exact""#,
        ] {
            assert!(json.contains(needle), "{needle} in {json}");
        }
        let mut per_turn = enumeration(&["x"], 1.0, Method::Exact);
        per_turn.reading = gauntlet_criteria::Reading::PerTurn.into();
        let json = facet_json::to_string(&per_turn).expect("serialises");
        assert!(json.contains(r#""reading":"per-turn""#), "{json}");
    }

    #[test]
    fn a_run_is_mixed_only_when_it_is_both() {
        let estimated = |criteria: Vec<bool>| Estimated {
            criteria,
            expectations: Vec::new(),
        };
        assert_eq!(
            RunMethod::of(&estimated(vec![false, false])),
            RunMethod::Exact
        );
        assert_eq!(
            RunMethod::of(&estimated(vec![true, true])),
            RunMethod::Sampled
        );
        assert_eq!(
            RunMethod::of(&estimated(vec![true, false])),
            RunMethod::Mixed
        );
    }

    #[test]
    fn widths_and_shares_print_the_way_the_docs_quote_them() {
        assert_eq!(thousands(5_000_000.0), "5,000,000");
        assert_eq!(thousands(999.0), "999");
        assert_eq!(share(0.00001), "under 0.01%");
        assert_eq!(share(0.005), "0.50%");
        assert_eq!(share(0.42), "42%");
    }

    #[test]
    fn an_error_bar_keeps_enough_places_to_be_a_number() {
        assert_eq!(error_bar_value(0.0), "0.00");
        assert_eq!(error_bar_value(1.234), "1.23");
        assert_eq!(error_bar_value(0.0042), "0.004");
        assert_eq!(error_bar(None), "");
    }

    #[test]
    fn a_histogram_states_the_mass_it_leaves_out() {
        // Twenty buckets, most of the mass at the top: the window slides up
        // and says what fell below it rather than dropping it.
        let mut p = vec![0.004; 20];
        p[19] = 1.0 - 0.004 * 19.0;
        let lines = histogram_lines(&p, 0);
        let last = lines.last().expect("some lines");
        assert!(last.ends_with("(+3.2% outside)"), "{lines:?}");
        assert!(lines[0].starts_with("8: 0.4%"), "{lines:?}");
    }
}
