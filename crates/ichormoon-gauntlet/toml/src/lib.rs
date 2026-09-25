//! TOML acceptance criteria, evaluated against the exact engine.
//!
//! A criteria file is data. It names questions, and each question is a turn, a
//! Scryfall query and a range of counts — no expressions, no host language,
//! nothing that can run. That is what makes the whole query set and the whole
//! turn horizon readable before the engine starts, which is the property the
//! rest of the tool is built on: a refusal can name every query the file asks
//! for, and the hash in the provenance block is a promise about a file that
//! cannot behave differently the second time it is read.
//!
//! A file registers two kinds of question. A `[[criterion]]` is answered with a
//! probability: `require` is a conjunction of clauses, `any_of` is a
//! disjunction of branches, and a criterion may hold either or both. An
//! `[[expect]]` names one count and is answered with a mean and the
//! distribution behind it. They are separate tables rather than one table with
//! an optional threshold, so a question cannot be read as the other kind — the
//! confusion the JavaScript front end had to detect at runtime is not a state
//! this format can hold.
//!
//! A disjunction is still counts and still data. Each branch is a function of
//! the composition, so their union is one too: a path satisfies some branch or
//! it satisfies none, and the walk adds that path's probability once either
//! way. That is why `any_of` needs no second pass and cannot double-count the
//! hands two routes both cover.
//!
//! A question also names a `zone`, and what it defaults to is the one place
//! this crate is deliberately lenient: silence means `hand`, so every criteria
//! file written before zones existed keeps the numbers it already had. The
//! leniency stops there. Every zone the file *does* name is resolved to a
//! [`Zone`] at parse time, so an unmodelled one is refused by name rather than
//! answered, and the whole zone set is readable before the engine starts for
//! the same reason the query set is.
//!
//! The narrowness is the whole trick, and it is the same one the exact engine
//! needs: a criterion that depends only on counts is a pure function of the
//! composition, so it can be evaluated once per possible composition rather
//! than once per simulated hand.

use facet::Facet;
use gauntlet_criteria::{
    Cost, CostError, Count, Counted, Criterion, Delay, Evaluator, Expectation, Fetched, NotACount,
    Palette, PathOutcomes, PathView, Plan, Trigger, TriggerError, Zone, ZoneError,
};
use thiserror::Error;

/// The effect library that ships with the tool.
///
/// Not a special format and not special machinery: `[[effect]]` tables in the
/// same syntax a brewer writes for the one niche card nobody thought of. It is
/// loaded before the user's file, which is the whole of what "prelude" means
/// here — last-wins does the rest, so overriding an entry needs no override
/// syntax to exist.
pub const STANDARD_LIBRARY: &str = include_str!("standard-effects.toml");

/// What a criteria file may call the standard library in an error or a report.
pub const STANDARD_LIBRARY_ORIGIN: &str = "the standard effect library";

/// The deepest look an effect may declare.
///
/// Not a rule of the game — it is a bound on the work, the same kind as
/// [`MAX_TURN`]. Every card a look examines is a card the enumeration has to
/// turn over on every turn of the run, so a `look = 400` typo would ask for a
/// schedule that draws the library several times over before anything got the
/// chance to refuse it.
pub const MAX_LOOK: u32 = 10;

/// The deepest turn a criteria file may name.
///
/// Not a rule of the game — it is a bound on the work. The run horizon becomes
/// one checkpoint per turn and the library runs out long before this, so a
/// four-billion turn typo would otherwise ask for a four-billion element vector
/// before anything got the chance to refuse it.
pub const MAX_TURN: u32 = 100;

// --- The file as written --------------------------------------------------
//
// Every field is optional except the ones TOML itself has to see, so a missing
// one becomes an error of this crate's own — which knows which criterion it was
// reading and can say so — rather than a deserializer's, which does not.

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct FileDef {
    #[facet(default)]
    criterion: Vec<CriterionDef>,
    #[facet(default)]
    expect: Vec<ExpectDef>,
    #[facet(default)]
    effect: Vec<EffectDef>,
    /// The priority over the one land drop a turn. One table, not a list of
    /// them: a run has one land drop to arbitrate, and two tables would be two
    /// policies over one resource, which is the thing this key exists to stop.
    land_drop: Option<LandDropDef>,
    /// The priority over the turn's mana. One table for the same reason
    /// `land_drop` is one: a run has one pool to arbitrate.
    casting: Option<CastingDef>,
}

/// The declared priority over the land drop, as written.
///
/// The same mechanism as mulligan bottoming
/// ([#7](https://github.com/cramt/progress-engine/issues/7)) and selection
/// routing ([#17](https://github.com/cramt/progress-engine/issues/17)): a list
/// of queries in the order the pilot would take them. Not a fourth policy
/// language, and deliberately not richer than the other two — anything that
/// had to look at the hand rather than at counts would take the engine out of
/// being exact.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct LandDropDef {
    prefer: Option<Vec<String>>,
}

/// The declared priority over the turn's mana, as written.
///
/// One more resource on the mechanism the others already use, and written the
/// same way so that nobody has to learn a second shape for the same idea. What differs is what silence means: an unnamed **land** is still
/// played, and an unnamed **spell** is not cast — because pricing every spell
/// in a deck splits the library by mana cost, and a list that named your whole
/// deck would be an enumeration nobody asked for. The list is the line you are
/// asking about.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct CastingDef {
    prefer: Option<Vec<String>>,
}

/// One entry of the effect library, as written.
///
/// Keyed on a query rather than on a card name, which is the whole reason a
/// library of these is maintainable: keyed by card it would be thousands of
/// entries and stale on every set release, and keyed by query a new printing
/// that surveils is covered the day it exists.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct EffectDef {
    /// `match` in the file; `match` is a keyword here.
    #[facet(rename = "match")]
    matches: Option<String>,
    look: Option<i64>,
    on: Option<String>,
    /// Absent means nothing leaves the top of the library. See
    /// [`gauntlet_criteria::Route::Nowhere`] for why that is a refusal to guess
    /// rather than a missing feature.
    to_graveyard: Option<String>,
    /// The declared priority over what this goes and gets out of the library,
    /// highest first — the same list shape as `[land_drop]` and `[casting]`,
    /// over the resource a tutor contests.
    ///
    /// It is written here rather than in the standard library for the reason
    /// `to_graveyard` is: *which* mana value 1 artifact Trinket Mage fetches is
    /// the question you are asking, not a fact about the card.
    fetch: Option<Vec<String>>,
    /// Where the fetched card is put. Required beside `fetch` and meaningless
    /// without it: a card that left the library has to be somewhere, and a
    /// default would be this tool choosing a zone on your behalf.
    to: Option<String>,
    /// Whole turns between the trigger and the effect. Urza's Saga's third
    /// chapter is `after = 2`: the lore counters it gains after your next two
    /// draw steps. Absent is an effect that happens when it is triggered.
    after: Option<i64>,
    /// Whether the card that set a delayed effect up leaves the battlefield
    /// once it resolves, which a Saga does after its last chapter. Only
    /// meaningful beside `after`, and refused without it.
    sacrifice: Option<bool>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct CriterionDef {
    name: Option<String>,
    at_least: Option<f64>,
    at_most: Option<f64>,
    #[facet(default)]
    require: Vec<ClauseDef>,
    /// An `Option` rather than a defaulted `Vec`, which `require` can afford to
    /// be: a written-out `any_of = []` is a disjunction of no branches and
    /// holds on no hand, and it has to be distinguishable from a criterion that
    /// never mentioned `any_of` at all so it can be refused by name.
    any_of: Option<Vec<BranchDef>>,
}

/// One route through a disjunction, as written.
///
/// A table holding a `require` rather than a bare list of clauses, so that
/// `[[criterion.any_of]]` is a section a person can write and the key inside it
/// means what it means everywhere else: all of these clauses, together.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct BranchDef {
    #[facet(default)]
    require: Vec<ClauseDef>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct ClauseDef {
    // Signed, and narrowed later. TOML integers are signed, and a `turn = -1`
    // read straight into a u32 comes back as 4294967295 rather than as a
    // complaint.
    turn: Option<i64>,
    query: Option<String>,
    // Absent means `hand`, which is what a clause without a zone has always
    // meant. Narrowed to a `Zone` in `build`, so an unmodelled zone is refused
    // by name instead of answered.
    zone: Option<String>,
    min: Option<i64>,
    max: Option<i64>,
    /// A mana cost, as printed: `can_cast = "{1}{W}{U}"`.
    ///
    /// A key of its own rather than a query, because it is not a question about
    /// cards. It asks whether the lands this path put into play could have paid
    /// that cost, which is a matching over those lands jointly — and the whole
    /// reason it is a primitive is that a user writing `produces:w` and
    /// `produces:u` as two clauses gets a different, wrong answer.
    can_cast: Option<String>,
    /// A query whose cards this counts **castings of**: `cast = 'name:"Opt"'`.
    ///
    /// Not a zone on `query`, and not the same question as `can_cast`. The
    /// gate asks whether the turn's lands could have paid a cost; this counts
    /// how many times they actually did, out of a pool that a second copy has
    /// to compete for. One Island and six Opt answers `can_cast = "{U}"` yes
    /// and `cast = 'name:"Opt"'` **one**.
    cast: Option<String>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct ExpectDef {
    name: Option<String>,
    turn: Option<i64>,
    query: Option<String>,
    zone: Option<String>,
    /// The same key a clause has, answered as a distribution rather than a
    /// threshold: *how many Opts do you actually cast by turn 5*, with
    /// P(exactly k) behind it. Which is the number HANDS.md hand 1 exists to
    /// pin down, and a mean is a better way to read it than a yes or a no.
    cast: Option<String>,
}

// --- The file as the engine sees it ---------------------------------------

/// How many matching cards a clause will accept.
///
/// Three variants rather than a pair of `Option`s, because two of the four
/// combinations are not questions. A clause with neither bound asks nothing of
/// the query it names and would hold on every hand; a clause with `min` above
/// `max` holds on none. Both are refused at parse time, and neither is
/// representable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bounds {
    AtLeast(u32),
    AtMost(u32),
    Between { min: u32, max: u32 },
}

impl Bounds {
    fn holds(self, count: u32) -> bool {
        match self {
            Bounds::AtLeast(min) => count >= min,
            Bounds::AtMost(max) => count <= max,
            Bounds::Between { min, max } => count >= min && count <= max,
        }
    }
}

/// One requirement of one criterion.
///
/// Two kinds, because a file asks two kinds of thing and only one of them is
/// about cards. Keeping them apart here rather than as one struct with
/// optional halves is what makes `can_cast = "{W}{U}", min = 2` unwritable
/// rather than something the reader has to be warned about: a cost has no
/// bounds and no zone, and there is nowhere in this type to put them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Clause {
    /// How many cards matching a query are in a zone by a turn.
    Count(Tally),
    /// Whether a cost could have been paid by a turn.
    Cast { turn: usize, cost: Cost },
}

/// One counting requirement, with its query resolved to a position in the
/// grouping.
///
/// `counted` is not an `Option`. A clause that left it unresolved would be a
/// clause whose meaning depends on who reads it, and the reader that guesses
/// `hand` is exactly the silent default zones exist to remove. The default is
/// applied once, at parse time, and after that every clause says what it
/// counts: cards in a zone, or spells it paid for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tally {
    turn: usize,
    query: usize,
    counted: Counted,
    bounds: Bounds,
}

impl Clause {
    fn holds(&self, view: &PathView<'_>) -> bool {
        match self {
            Clause::Count(c) => c.bounds.holds(view.count_at(c.turn, c.query, c.counted)),
            Clause::Cast { turn, cost } => view.can_cast(*turn, cost),
        }
    }

    fn counting(&self) -> Option<&Tally> {
        match self {
            Clause::Count(c) => Some(c),
            Clause::Cast { .. } => None,
        }
    }
}

/// What a criterion asks of one path.
///
/// Three variants rather than two possibly-empty lists, for the same reason
/// [`Bounds`] has three: of the four combinations one is not a question. A
/// criterion with neither a conjunction nor a disjunction asks nothing and
/// would report a confident 100%, so it is refused at parse time and is not
/// representable here.
///
/// The disjunction is a list of branches and each branch is a conjunction, one
/// level deep. That is what the routes a deck actually has look like — this
/// turn or that turn, this zone or that zone — and a branch that could itself
/// hold an `any_of` would buy nothing a second branch does not already buy.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Predicate {
    /// `require` alone.
    All(Vec<Clause>),
    /// `any_of` alone: the union of the branches, not their sum.
    Any(Vec<Vec<Clause>>),
    /// Both, which is the useful combination: shared preconditions, then
    /// alternative routes off them.
    AllAndAny {
        all: Vec<Clause>,
        any: Vec<Vec<Clause>>,
    },
}

impl Predicate {
    fn holds(&self, view: &PathView<'_>) -> bool {
        let holds_all = |cs: &[Clause]| cs.iter().all(|c| c.holds(view));
        // `any` short-circuits on the first branch that holds, which is also
        // why overlapping branches cannot be counted twice: this answers
        // whether the path is in the union, and the walk outside adds that
        // path's probability once.
        let holds_any = |bs: &[Vec<Clause>]| bs.iter().any(|b| holds_all(b));
        match self {
            Predicate::All(all) => holds_all(all),
            Predicate::Any(any) => holds_any(any),
            Predicate::AllAndAny { all, any } => holds_all(all) && holds_any(any),
        }
    }

    /// Every counting clause this criterion holds, wherever it was written. The
    /// queries and zones a file asks about are the union over these, so a
    /// branch is no more hidden from the pre-run analysis than a `require`
    /// clause is.
    fn counts(&self) -> impl Iterator<Item = &Tally> {
        self.clauses().filter_map(Clause::counting)
    }

    /// Every clause, counting or casting.
    fn clauses(&self) -> impl Iterator<Item = &Clause> {
        const NO_CLAUSES: &[Clause] = &[];
        const NO_BRANCHES: &[Vec<Clause>] = &[];
        let (all, any): (&[Clause], &[Vec<Clause>]) = match self {
            Predicate::All(all) => (all, NO_BRANCHES),
            Predicate::Any(any) => (NO_CLAUSES, any),
            Predicate::AllAndAny { all, any } => (all, any),
        };
        all.iter().chain(any.iter().flatten())
    }
}

/// Where an expectation reads its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    turn: usize,
    query: usize,
    counted: Counted,
}

/// A parsed criteria file, ready to answer against either engine.
#[derive(Debug)]
pub struct Criteria {
    /// Every distinct query the file names, in the order it first names them.
    /// This is the whole set, known before a single hand is enumerated.
    queries: Vec<String>,
    /// Every distinct zone the file asks about, discovered the same way and for
    /// the same reason: a file that never says `graveyard` must not pay to
    /// track one, and a run cannot warn about a zone it only learns by running.
    zones: Vec<Zone>,
    horizon: u32,
    /// Parallel to `predicates`, and to the answers a run hands back:
    /// everything here is matched up by position. Both vectors are filled in
    /// one pass in `parse` and are immutable afterwards.
    criteria: Vec<Criterion>,
    predicates: Vec<Predicate>,
    expectations: Vec<Expectation>,
    probes: Vec<Probe>,
    effects: EffectLibrary,
    /// The land-drop priority, still as text: which cards a query picks out is
    /// a question about a decklist rather than about this file, exactly as it
    /// is for an effect's `match`.
    land_drop: Vec<String>,
    /// The casting priority, as text and for the same reason. What a card
    /// *costs* is also a question about a decklist, which is why this crate
    /// never sees a spell's mana cost: the caller holding the index prices the
    /// list and refuses what it cannot pay.
    casting: Vec<String>,
}

/// One effect, validated but not yet resolved against any deck.
///
/// The queries are still text here, because which cards they pick out is a
/// question about a decklist and an index rather than about this file. What
/// *is* settled by now is the shape: a trigger the engine can fire, a look
/// that is a number of cards, and a destination that either names a query or
/// says nothing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectEntry {
    /// Which cards this effect is about.
    pub matches: String,
    pub look: u32,
    pub trigger: Trigger,
    /// The routing policy: which of the looked-at cards go to the graveyard.
    /// `None` is "none of them", and is the default.
    pub to_graveyard: Option<Destination>,
    /// What this goes and gets out of the library, if anything.
    pub fetch: Option<FetchDecl>,
    /// How long it waits after its trigger, if it waits at all.
    pub delay: Option<Delay>,
    /// Which file declared it. Carried so a report can say where a surprising
    /// effect came from, and so the standard library can stay quiet about
    /// matching nothing while a hand-written entry does not.
    pub origin: String,
}

/// What the routing policy sends to the graveyard.
///
/// Two variants rather than a query that happens to match everything, because
/// "all of them" is what mill is and there is no query in Scryfall's language
/// that says it without a reader having to work out that it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// `to_graveyard = "*"`.
    Everything,
    /// `to_graveyard = '<query>'`.
    Matching(String),
}

/// What a criteria file spells `to_graveyard = "*"`.
pub const EVERYTHING: &str = "*";

/// A declared tutor, as written: what it would go and get, in the order it
/// would take them, and where it puts what it finds.
///
/// The queries are still text for the same reason an effect's `match` is:
/// which cards they pick out is a question about a decklist and an index, and
/// this file knows neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchDecl {
    pub prefer: Vec<String>,
    pub to: Fetched,
}

/// Every destination a fetch may name, for the message that lists them.
pub const FETCH_DESTINATIONS: &str = "hand, battlefield";

/// Read a fetch destination from what a criteria file wrote.
fn fetched_of(name: &str) -> Option<Fetched> {
    match name {
        "hand" => Some(Fetched::Hand),
        "battlefield" => Some(Fetched::Battlefield),
        _ => None,
    }
}

/// What a fetch destination is called in a file and in a report.
pub fn fetched_name(to: Fetched) -> &'static str {
    match to {
        Fetched::Hand => "hand",
        Fetched::Battlefield => "battlefield",
    }
}

/// The effects a run has loaded, in load order.
///
/// Order is the whole semantics. Overlap is not an edge case here — query-keyed
/// effects overlap by design, since `t:land otag:surveil` and
/// `name:"Undercity Sewers"` both match the same card — so the rule is
/// **last-wins, per card**: collect every effect matching a card, apply the
/// last declared. Stacking them would make that card look two deep, which is a
/// confidently wrong number of exactly the shape this project exists to
/// prevent, and erroring on overlap would fire precisely when a brewer does the
/// thing the feature is for.
///
/// The standard library loads first, so a user's own entry overrides it without
/// any override syntax existing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectLibrary {
    entries: Vec<EffectEntry>,
}

impl EffectLibrary {
    /// Read the `[[effect]]` tables out of a file that may hold nothing else.
    ///
    /// Separate from [`Criteria::parse`] because the standard library asks no
    /// questions, and a file of pure effects is not a file that forgot to.
    pub fn parse(source: &str, origin: &str) -> Result<Self, CriteriaError> {
        let file: FileDef = read(source).map_err(|kind| CriteriaError {
            origin: origin.to_string(),
            kind,
        })?;
        effects_of(&file, origin).map_err(|kind| CriteriaError {
            origin: origin.to_string(),
            kind,
        })
    }

    /// `self`, then `later` — so `later` wins wherever both match a card.
    pub fn followed_by(mut self, later: EffectLibrary) -> EffectLibrary {
        self.entries.extend(later.entries);
        self
    }

    pub fn entries(&self) -> &[EffectEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Criteria {
    /// Parse a criteria file. `origin` names it, for the errors.
    pub fn parse(source: &str, origin: &str) -> Result<Self, CriteriaError> {
        build(source, origin).map_err(|kind| CriteriaError {
            origin: origin.to_string(),
            kind,
        })
    }

    /// The `[[effect]]` tables this file declared, in the order it declared
    /// them.
    pub fn effects(&self) -> &EffectLibrary {
        &self.effects
    }

    /// The land-drop priority this file declared, highest first, or empty
    /// where it declared none.
    ///
    /// Empty is the older state and not a lesser one: it means nobody has said
    /// which land they would play, which is answerable on its own and refused
    /// the moment two parts of the run need the answer to differ.
    pub fn land_drop(&self) -> &[String] {
        &self.land_drop
    }

    /// The casting priority this file declared, highest first, or empty where
    /// it declared none.
    ///
    /// Empty means this run casts nothing at all, which is what every run did
    /// before the budget existed. It is a real answer rather than a gap: what
    /// you cast out of a turn's mana is a decision, and a tool that picked for
    /// you would be reporting a line nobody chose.
    pub fn casting(&self) -> &[String] {
        &self.casting
    }

    /// The first question here that counts spells this run cast, if any.
    ///
    /// Carried out so the caller can refuse a `cast` clause in a file that
    /// declared no casting priority — the same shape as the land drop's
    /// refusal, and the same remedy: say which spells you would cast.
    pub fn counts_castings(&self) -> Option<&str> {
        let counts_cast = |c: &Clause| matches!(c, Clause::Count(t) if t.counted == Counted::Cast);
        let from_criteria = self
            .predicates
            .iter()
            .position(|p| p.clauses().any(counts_cast))
            .map(|i| self.criteria[i].name.as_str());
        from_criteria.or_else(|| {
            self.probes
                .iter()
                .position(|p| p.counted == Counted::Cast)
                .map(|i| self.expectations[i].name.as_str())
        })
    }

    pub fn criteria(&self) -> &[Criterion] {
        &self.criteria
    }

    pub fn expectations(&self) -> &[Expectation] {
        &self.expectations
    }

    /// Every query the file names, deduplicated, in first-mention order.
    pub fn queries(&self) -> &[String] {
        &self.queries
    }

    /// Every zone the file asks about, deduplicated, in first-mention order.
    ///
    /// Includes the zone a clause meant without saying so, because a question
    /// that defaulted to the hand still asked about the hand.
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// Which question first asked about `zone`, so a zone nothing routes a card
    /// into can be reported against the criterion that will read zero.
    pub fn zone_asked_by(&self, zone: Zone) -> Option<&str> {
        let from_criteria = self
            .predicates
            .iter()
            .position(|p| p.counts().any(|c| c.counted == Counted::In(zone)))
            .map(|i| self.criteria[i].name.as_str());
        from_criteria.or_else(|| {
            self.probes
                .iter()
                .position(|p| p.counted == Counted::In(zone))
                .map(|i| self.expectations[i].name.as_str())
        })
    }

    /// The deepest turn the file names.
    pub fn horizon(&self) -> u32 {
        self.horizon
    }

    /// Every query this file counts on the battlefield, with the question that
    /// asked, in first-mention order.
    ///
    /// Read by the caller that holds the card data, because whether a query is
    /// answerable there is a fact about the cards rather than about the file: a
    /// land arrives on a land drop and anything else has to be cast. This crate
    /// cannot tell them apart and does not guess.
    pub fn battlefield_queries(&self) -> Vec<(&str, &str)> {
        let mut found: Vec<(&str, &str)> = Vec::new();
        for (predicate, criterion) in self.predicates.iter().zip(&self.criteria) {
            for clause in predicate
                .counts()
                .filter(|c| c.counted == Counted::In(Zone::Battlefield))
            {
                let query = self.queries[clause.query].as_str();
                if !found.iter().any(|(q, _)| *q == query) {
                    found.push((query, criterion.name.as_str()));
                }
            }
        }
        for (probe, expectation) in self.probes.iter().zip(&self.expectations) {
            let query = self.queries[probe.query].as_str();
            if probe.counted == Counted::In(Zone::Battlefield)
                && !found.iter().any(|(q, _)| *q == query)
            {
                found.push((query, expectation.name.as_str()));
            }
        }
        found
    }

    /// The first question here that asks whether a cost could be paid, if any.
    ///
    /// Distinct from [`Criteria::mana_question`] because the two need different
    /// things of the card data. What is in play is a land drop count, which any
    /// index can answer; what could be *paid* needs to know what each land
    /// makes and whether it arrives tapped, which an index built before those
    /// fields cannot say — and it is also what widens the enumeration, since
    /// telling a Plains from an Island splits a group no query split.
    /// Counting castings needs it too, and more of it: a budget reads what
    /// each land makes *and* what each spell costs, on every turn, because the
    /// spell it paid for is a spell that left the hand.
    pub fn casts(&self) -> Option<&str> {
        self.predicates
            .iter()
            .position(|p| p.clauses().any(|c| matches!(c, Clause::Cast { .. })))
            .map(|i| self.criteria[i].name.as_str())
            .or_else(|| self.counts_castings())
    }

    /// The first question here that the mana model has to answer, if any.
    ///
    /// Anything asking what is in play or what could be paid for. Carried out
    /// so a run can check, before it enumerates anything, that its card data
    /// can actually answer that kind of question — an index that never fetched
    /// `otag:tapland` would otherwise report every land as untapped and be
    /// confidently, silently optimistic.
    pub fn mana_question(&self) -> Option<&str> {
        let asked = |clause: &Clause| match clause {
            Clause::Cast { .. } => true,
            Clause::Count(c) => matches!(c.counted, Counted::In(Zone::Battlefield) | Counted::Cast),
        };
        let from_criteria = self
            .predicates
            .iter()
            .position(|p| p.clauses().any(asked))
            .map(|i| self.criteria[i].name.as_str());
        from_criteria.or_else(|| {
            self.probes
                .iter()
                .position(|p| matches!(p.counted, Counted::In(Zone::Battlefield) | Counted::Cast))
                .map(|i| self.expectations[i].name.as_str())
        })
    }

    /// Which question first named `query`, so a query that cannot be parsed or
    /// matches nothing can be reported against the thing that asked for it.
    pub fn asked_by(&self, query: &str) -> Option<&str> {
        let idx = self.queries.iter().position(|q| q == query)?;
        let from_criteria = self
            .predicates
            .iter()
            .position(|p| p.counts().any(|c| c.query == idx))
            .map(|i| self.criteria[i].name.as_str());
        from_criteria.or_else(|| {
            self.probes
                .iter()
                .position(|p| p.query == idx)
                .map(|i| self.expectations[i].name.as_str())
        })
    }

    /// The shape of the answer this file produces, for whichever engine is
    /// about to run it.
    pub fn plan(&self) -> Plan {
        Plan {
            criteria: self.criteria.len(),
            expectations: self.expectations.len(),
        }
    }

    /// What each question here reads of an enumeration, in the same order the
    /// answers come back in.
    ///
    /// This is the analysis that [issue
    /// #31](https://github.com/cramt/progress-engine/issues/31) wants and that
    /// nothing could do while criteria were a script: the queries, the turns
    /// and the kind of every clause are readable without running anything, so
    /// a caller can build one enumeration per class of question instead of one
    /// wide enough for all of them at once.
    pub fn reads(&self) -> QuestionReads {
        QuestionReads {
            criteria: self
                .predicates
                .iter()
                .map(|p| Reads::of(p.clauses()))
                .collect(),
            expectations: self
                .probes
                .iter()
                .map(|probe| {
                    let mut reads = Reads::default();
                    reads.count(probe.turn, probe.query, probe.counted);
                    reads
                })
                .collect(),
        }
    }
}

/// What every question in one file reads, question by question.
///
/// Two lists rather than one, for the same reason [`Plan`] is two counts: they
/// index different halves of the answer, and a caller holding them as one
/// sequence could file a criterion's requirement under an expectation's name
/// and still typecheck.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionReads {
    pub criteria: Vec<Reads>,
    pub expectations: Vec<Reads>,
}

/// What one question reads of an enumeration: which queries, which turns, and
/// which of the two things only the mana model can answer.
///
/// The point of writing it down is that a question that reads *less* can be
/// answered by a *cheaper* enumeration, exactly. A criterion counting
/// `cat:"Ramp"` at turn 3 needs a grouping that can tell Ramp from not-Ramp
/// and nothing else, and needs to know how many cards had been seen by turn 3
/// rather than how they arrived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reads {
    queries: u64,
    turns: Vec<usize>,
    demands: Option<Palette>,
    battlefield: bool,
}

impl Reads {
    fn of<'a>(clauses: impl Iterator<Item = &'a Clause>) -> Reads {
        let mut reads = Reads::default();
        for clause in clauses {
            match clause {
                Clause::Count(c) => reads.count(c.turn, c.query, c.counted),
                Clause::Cast { turn, cost } => {
                    // Joined over every cost in the question, because one
                    // enumeration answers all of them: a criterion asking for
                    // `{1}{U}` on turn 3 and `{B}` on turn 4 can tell a blue
                    // source from a black one from anything else, and nothing
                    // finer than that.
                    let demanded = cost.demands();
                    reads.demands = Some(match reads.demands {
                        Some(already) => already.union(demanded),
                        None => demanded,
                    });
                    reads.at(*turn);
                }
            }
        }
        reads
    }

    fn count(&mut self, turn: usize, query: usize, counted: Counted) {
        self.queries |= 1u64 << query;
        self.battlefield |= counted == Counted::In(Zone::Battlefield);
        self.at(turn);
    }

    fn at(&mut self, turn: usize) {
        if !self.turns.contains(&turn) {
            self.turns.push(turn);
            self.turns.sort_unstable();
        }
    }

    /// The grouping bits this question counts, as a mask.
    pub fn queries(&self) -> u64 {
        self.queries
    }

    /// Every turn it names, ascending. A criterion correlating two turns names
    /// both, which is how it keeps the path enumeration a criterion about one
    /// turn does not need.
    pub fn turns(&self) -> &[usize] {
        &self.turns
    }

    /// What its costs demand, where it asks whether a cost could have been
    /// paid at all — the only thing in the language that can tell a Plains
    /// from an Island.
    ///
    /// `None` and `Some(Palette::EMPTY)` are different questions and the
    /// distinction is the point: nothing here asks about mana, versus
    /// something asks `can_cast = "{2}"`, which reads how many lands are in
    /// play and untapped but cannot tell one colour from another.
    pub fn demands(&self) -> Option<Palette> {
        self.demands
    }

    /// Whether it counts cards on the battlefield, which reads the land drops
    /// turn by turn rather than a total.
    pub fn battlefield(&self) -> bool {
        self.battlefield
    }
}

impl Evaluator for Criteria {
    type Error = EvalError;

    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, EvalError> {
        let held = self.predicates.iter().map(|p| p.holds(view)).collect();
        let counted = self
            .probes
            .iter()
            .zip(&self.expectations)
            .map(|(probe, expectation)| {
                let seen = view.count_at(probe.turn, probe.query, probe.counted);
                Count::new(seen).map_err(|source| EvalError {
                    name: expectation.name.clone(),
                    source,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(PathOutcomes { held, counted })
    }
}

// --- Errors ---------------------------------------------------------------

/// Something wrong with a criteria file, named against the file it was in.
#[derive(Debug, Error)]
#[error("{origin}: {kind}")]
pub struct CriteriaError {
    pub origin: String,
    pub kind: ErrorKind,
}

/// Every key the format has, for the error that lists them.
const SCHEMA: &str = "A criteria file holds [[criterion]] tables (name, at_least, at_most, \
                      require, any_of), whose require clauses are (turn, query, zone, min, max), (turn, \
                      cast, min, max) or (turn, can_cast), and whose any_of branches each hold \
                      a require of their own, \
                      [[expect]] tables (name, turn, query, zone) or (name, turn, cast), [[effect]] \
                      tables (match, on, look, to_graveyard, fetch, to, after, sacrifice), one \
                      [land_drop] table (prefer) \
                      and one [casting] table (prefer).";

#[derive(Debug, Error)]
pub enum ErrorKind {
    /// The schema is spelled out every time rather than only when a key is
    /// unrecognised. The deserializer says which key it did not know and not
    /// which ones it would have taken, and a key silently dropped is how
    /// `atLeast` turns an assertion into a number that cannot fail.
    #[error("not a criteria file, {0}\n{SCHEMA}")]
    Malformed(String),
    #[error(
        "no [[criterion]] and no [[expect]] tables, so this file asks nothing and every \
         answer it could give would be about a question nobody wrote"
    )]
    AsksNothing,
    #[error(
        "{table} number {position} has no `name`, and a report refers to a question by its name"
    )]
    Unnamed {
        table: &'static str,
        position: usize,
    },
    #[error(
        "criterion {name:?} has neither `require` clauses nor `any_of` branches, so there is \
         nothing for it to hold or fail on and it would report 100% of every deck"
    )]
    NoClauses { name: String },
    /// Distinct from [`ErrorKind::NoClauses`] because it fails the other way
    /// round: an empty conjunction holds on every hand, an empty disjunction on
    /// none. Both are confident numbers about a question nobody asked.
    #[error(
        "criterion {name:?} has an empty `any_of`, so there is no route for it to take and it \
         would report a confident 0% of every deck.\n\
         Write a [[criterion.any_of]] branch for each route, or drop the key"
    )]
    EmptyAnyOf { name: String },
    #[error(
        "criterion {name:?}, any_of branch {position} has no `require` clauses, so that branch \
         holds on every hand and the whole criterion would report 100% whatever the other \
         branches say"
    )]
    EmptyBranch { name: String, position: usize },
    #[error("{at}: no `{key}`, {why}")]
    Missing {
        at: String,
        key: &'static str,
        why: &'static str,
    },
    #[error(
        "{at}: names {query:?} but has neither `min` nor `max`, so it asks nothing of it. \
         Write `min = 1` for \"at least one\""
    )]
    NoBounds { at: String, query: String },
    #[error(
        "{at}: `min = {min}` is above `max = {max}`, so no hand can satisfy it and the \
         criterion would report a confident 0%"
    )]
    EmptyRange { at: String, min: u32, max: u32 },
    #[error(
        "{at}: `{key} = {value}` is not a number of cards: it must be a whole number from 0 \
         to {}",
        u32::MAX
    )]
    BadCount {
        at: String,
        key: &'static str,
        value: i64,
    },
    #[error(
        "{at}: `turn = {turn}` is not a turn: turns count from 0, the opening hand, up to \
         {MAX_TURN}"
    )]
    BadTurn { at: String, turn: i64 },
    #[error(
        "criterion {name:?}: `{key} = {value}` is not a probability. It is the share of hands \
         this must hold in, so 70% is written 0.70"
    )]
    BadThreshold {
        name: String,
        key: &'static str,
        value: f64,
    },
    /// A range no probability can sit in, which would fail every deck for a
    /// reason nothing in the report states, the same way `min = 5, max = 2`
    /// would report a confident 0%.
    #[error(
        "criterion {name:?}: `at_least = {at_least}` and `at_most = {at_most}` leave no \
         probability between them, so this would fail on every deck"
    )]
    EmptyThreshold {
        name: String,
        at_least: f64,
        at_most: f64,
    },
    /// A trigger the engine does not fire, refused by name.
    ///
    /// Not a `#[source]`, for the same reason [`ErrorKind::BadZone`] is not:
    /// the reason is the whole error, and burying it a level down would show
    /// `effect "...": bad trigger` to anyone who prints only the outermost
    /// layer.
    #[error("{at}: {trigger}")]
    BadTrigger { at: String, trigger: TriggerError },
    #[error(
        "{at}: `look = {look}` is not a number of cards to examine: it must be a whole number \
         from 1 to {MAX_LOOK}.\n\
         Each one is a card the enumeration turns over on every turn of the run, so the ceiling \
         is a bound on the work rather than a rule of the game."
    )]
    BadLook { at: String, look: i64 },
    /// A zone the engine does not model, refused by name.
    ///
    /// Not a `#[source]`: the reason is the whole error, and burying it one
    /// level down would show `criterion "x", clause 1: bad zone` to anyone who
    /// prints only the outermost layer.
    #[error("{at}: {zone}")]
    BadZone { at: String, zone: ZoneError },
    /// A cost the gate cannot pay, refused by name for the same reason as an
    /// unmodelled zone and printed the same way.
    #[error("{at}: {cost}")]
    BadCost { at: String, cost: CostError },
    /// A policy that arbitrates nothing, refused rather than recorded.
    ///
    /// The report says which runs resolved a land drop by policy, and a table
    /// with no entries would put that line above numbers no declaration
    /// touched — a provenance claim that is not true.
    #[error(
        "{table} declares no `prefer` entries, so it settles nothing and every number below it \
         would be answered as if it were not there.\n\
         Write `prefer = ['<query>', ...]` in the order you would take them, or drop the table"
    )]
    NoPreference { table: &'static str },
    #[error(
        "{table}: `prefer` entry {position} repeats entry {first} ({query:?}), so it can never \
         decide anything: the earlier one already took every card it names.\n\
         A priority list is read in order and the first entry that matches wins"
    )]
    RepeatedPreference {
        table: &'static str,
        query: String,
        first: usize,
        position: usize,
    },
    /// Three keys, one question each, and a clause naming two of them would
    /// have to answer one of them silently.
    #[error(
        "{at}: has both `{keys}`, which are different questions.\n\
         `query` counts cards in a zone, `can_cast` asks whether the lands in play could have \
         paid a cost, and `cast` counts the spells you actually paid for. Write them as \
         separate clauses."
    )]
    TwoQuestions { at: String, keys: String },
    /// A casting is not somewhere a card sits, so there is no zone to name.
    ///
    /// Refused rather than ignored, because `cast = '...', zone = "graveyard"`
    /// reads like it means something — where the spell went afterwards — and
    /// that is a question this engine does not answer at all. A clause quietly
    /// dropping half of what it was asked is the failure this format exists to
    /// prevent.
    #[error(
        "{at}: has both `cast` and `zone`, and a casting is not a zone.\n\
         `cast` counts the spells this run paid for. Where one of them ended up afterwards — \
         the battlefield, the graveyard — is not modelled, so there is no zone to name."
    )]
    CastCountWithZone { at: String },
    #[error(
        "{at}: has both `can_cast` and `{key}`, and `{key}` means nothing to a mana question.\n\
         `can_cast` is a yes or no about one turn — there is no count to bound and no zone to \
         count in."
    )]
    CastWithCounting { at: String, key: &'static str },
    /// A destination with nothing arriving at it. Refused rather than ignored,
    /// because a `to` written beside a `to_graveyard` reads like it says where
    /// the routed cards go, and it does not.
    #[error(
        "{at}: has `to = {to:?}` and no `fetch`, so nothing is going there.\n\
         `to` says where a fetched card is put. Where a *looked-at* card goes is \
         `to_graveyard`."
    )]
    ToWithoutFetch { at: String, to: String },
    #[error(
        "{at}: `to = {to:?}` is not somewhere this tool can put a fetched card. Accepted: {}.\n\
         A card that left the library has to be somewhere a criterion can count it, and a \
         destination this engine cannot model would be a card vanishing.",
        FETCH_DESTINATIONS
    )]
    BadFetchDestination { at: String, to: String },
    /// Rampant Growth, and it is refused rather than approximated.
    ///
    /// A land arriving off a spell is not a land drop, so what it taps for on
    /// the turn it lands is a fact about the card that put it there — tapped
    /// for Rampant Growth, untapped for Nature's Lore — and no tag this index
    /// carries separates them. Both are plausible and one of them is wrong,
    /// which is the confident wrong number in its usual costume.
    #[error(
        "{at}: has `on = \"cast\"` and `to = \"battlefield\"`, which is a land arriving off a \
         spell rather than on a land drop, and that is not modelled.\n\
         Whether such a land enters tapped is a fact about the spell that fetched it and no tag \
         separates the two, so the turn's mana would be either overstated or understated with \
         nothing saying which. `to = \"hand\"` is answerable, and so is `on = \"landdrop\"` with \
         `to = \"battlefield\"`, which is a fetchland."
    )]
    FetchOntoTheBattlefieldFromASpell { at: String },
    #[error(
        "{at}: `after = {after}` is not a number of turns to wait: it must be a whole number \
         from 1 to {MAX_TURN}. An effect that waits no turns is written without `after`."
    )]
    BadAfter { at: String, after: i64 },
    /// What may wait, refused by what it would have needed.
    ///
    /// A delayed **fetch** is a subtraction on a later turn, which the walk
    /// already knows how to take. A delayed **look** would turn over cards on a
    /// turn the schedule cannot know the effect fires on, and a delayed cast
    /// trigger is a card nobody has asked for yet.
    #[error(
        "{at}: has `after`, and {why}.\n\
         A delayed effect is Urza's Saga's third chapter: `on = \"landdrop\"`, a `fetch`, and no \
         `look`, because what waits has to be a card named out of the library rather than an \
         unknown one turned over on a turn the enumeration cannot know in advance."
    )]
    UnmodelledDelay { at: String, why: &'static str },
    #[error(
        "{at}: has `sacrifice = true` and no `after`.\n\
         A land that sacrifices itself the moment it is played to put another onto the \
         battlefield is a fetchland, and is written `to = \"battlefield\"` with no `sacrifice`. \
         `sacrifice` is for a card that stayed in play until a delayed effect resolved."
    )]
    SacrificeWithoutDelay { at: String },
}

/// A count with nowhere to go in a histogram, named against the expectation
/// that produced it.
#[derive(Debug, Error)]
#[error("expectation {name:?}: {source}")]
pub struct EvalError {
    pub name: String,
    #[source]
    pub source: NotACount,
}

// --- Parsing --------------------------------------------------------------

/// `e.kind` rather than `e`: the Display of the whole error appends a debug
/// dump of the target type's reflection data, which is a page of noise in front
/// of the one line that says which key was wrong.
fn read(source: &str) -> Result<FileDef, ErrorKind> {
    facet_toml::from_str(source).map_err(|e| {
        let what = e.kind.to_string();
        let path = e.path.as_ref().map(ToString::to_string);
        match locate(
            source,
            path.as_deref(),
            &what,
            e.span.map(|s| s.offset as usize),
        ) {
            Some((line, text)) => {
                let mut out = format!("line {line}: {what}.\n    {line} | {}", text.trim_end());
                if let Some(hint) = quoting_hint(text) {
                    out.push_str(&format!("\n{hint}"));
                }
                ErrorKind::Malformed(out)
            }
            None => ErrorKind::Malformed(format!("{what}.")),
        }
    })
}

/// Which line of the file an error from the TOML reader is about, and its text.
///
/// Two sources, trusted for different things (#52). A **syntax** error has no
/// path, and its byte offset lands on the right line. An error about a
/// **key** — unknown, or the wrong type — has a path like `criterion[1]` and an
/// offset that points near it rather than at it, so the table is found by
/// counting its headers and the key by reading that table's lines. A 500-line
/// criteria file with an error and no line number is a bisect.
fn locate<'s>(
    source: &'s str,
    path: Option<&str>,
    what: &str,
    offset: Option<usize>,
) -> Option<(usize, &'s str)> {
    let lines: Vec<&str> = source.lines().collect();
    let Some(path) = path else {
        let offset = offset?.min(source.len());
        let line = source[..offset].matches('\n').count();
        return lines.get(line).map(|text| (line + 1, *text));
    };
    // `criterion[1].at_least` is the second `[[criterion]]`, key `at_least`.
    let (table, rest) = path.split_once('[')?;
    let (index, rest) = rest.split_once(']')?;
    let index: usize = index.parse().ok()?;
    let header = format!("[[{table}]]");
    let start = lines.iter().position_nth(|l| l.trim() == header, index)?;
    // The key: the field an "unknown field" message names, which the path
    // does not reach, or else the path's next name.
    let key = what
        .split_once('`')
        .and_then(|(_, r)| r.split_once('`'))
        .map(|(k, _)| k)
        .or_else(|| {
            rest.strip_prefix('.')
                .and_then(|r| r.split(['.', '[']).next())
        })
        .filter(|k| !k.is_empty());
    // The table runs until the next header that is not one of its own
    // sub-tables: `[[criterion.any_of]]` is still inside the criterion.
    let nested = format!("[[{table}.");
    let found = key.and_then(|key| {
        lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .take_while(|(_, l)| {
                let t = l.trim_start();
                !t.starts_with('[') || t.starts_with(&nested)
            })
            .filter(|(_, l)| {
                // At the start of the line, or inside an inline table such as
                // a `require` clause, where no line starts with it.
                l.split([' ', '{', ','])
                    .zip(l.split([' ', '{', ',']).skip(1).chain([""]))
                    .any(|(word, next)| word == key && (next == "=" || next.starts_with('=')))
                    || l.contains(&format!("{key}="))
            })
            .map(|(i, _)| i)
            .next()
    });
    let at = found.unwrap_or(start);
    Some((at + 1, lines[at]))
}

trait PositionNth<T> {
    fn position_nth(self, pred: impl FnMut(&T) -> bool, n: usize) -> Option<usize>;
}

impl<'a, T: 'a, I: Iterator<Item = &'a T>> PositionNth<T> for I {
    fn position_nth(self, mut pred: impl FnMut(&T) -> bool, n: usize) -> Option<usize> {
        self.enumerate()
            .filter(|(_, x)| pred(x))
            .nth(n)
            .map(|(i, _)| i)
    }
}

/// The mistake anyone writing card names makes: an apostrophe inside a
/// single-quoted TOML string, which ends the string early —
/// `'name:"Artificer's Intuition"'`.
fn quoting_hint(line: &str) -> Option<&'static str> {
    (line.matches('\'').count() >= 3 && !line.contains("'''")).then_some(
        "    A single-quoted string ends at its next ', so a card name with an apostrophe \
         in it ends\n    it early. Use triple quotes for those: '''name:\"Artificer's \
         Intuition\"'''",
    )
}

/// Validate the `[[effect]]` tables of an already-deserialized file.
///
/// Everything checkable without a decklist is checked here: the trigger has to
/// be one the engine fires, the look has to be a number of cards, and the
/// match has to be there at all. What is left for the caller is the half that
/// needs card data — whether the queries parse, and which cards they pick out.
fn effects_of(file: &FileDef, origin: &str) -> Result<EffectLibrary, ErrorKind> {
    let mut entries = Vec::with_capacity(file.effect.len());
    for (i, def) in file.effect.iter().enumerate() {
        let at = format!("[[effect]] number {}", i + 1);
        let matches = def.matches.clone().ok_or(ErrorKind::Missing {
            at: at.clone(),
            key: "match",
            why: "so there are no cards for it to be about",
        })?;
        let at = format!("effect {matches:?}");
        let on = def.on.clone().ok_or(ErrorKind::Missing {
            at: at.clone(),
            key: "on",
            why: "so there is no point in the game for it to happen at",
        })?;
        let trigger = Trigger::parse(&on).map_err(|trigger| ErrorKind::BadTrigger {
            at: at.clone(),
            trigger,
        })?;
        let fetch = fetch_of(def, &at)?;
        // `look` is required unless this effect only fetches, and the two are
        // different things: a look turns over a card nobody has seen, a fetch
        // names one. An entry doing neither would be a checkpoint spent on an
        // effect that cannot move a number.
        let look = match (def.look, &fetch) {
            (Some(look), _) => u32::try_from(look)
                .ok()
                .filter(|l| (1..=MAX_LOOK).contains(l))
                .ok_or(ErrorKind::BadLook {
                    at: at.clone(),
                    look,
                })?,
            (None, Some(_)) => 0,
            (None, None) => {
                return Err(ErrorKind::Missing {
                    at: at.clone(),
                    key: "look",
                    why: "so there is nothing for it to examine. Write `look = 1` for \"the top \
                          card\", or `fetch = ['<query>']` for a card it goes and gets out of \
                          the library",
                })
            }
        };
        // A look on a cast is the replacement draw, and it is the one thing
        // this trigger will not do. A fetch on a cast is a subtraction.
        if trigger == Trigger::Cast && look > 0 {
            return Err(ErrorKind::BadTrigger {
                at: at.clone(),
                trigger: TriggerError::LooksOnCast,
            });
        }
        if let Some(fetch) = &fetch {
            if trigger == Trigger::Cast && fetch.to == Fetched::Battlefield {
                return Err(ErrorKind::FetchOntoTheBattlefieldFromASpell { at: at.clone() });
            }
        }
        let delay = delay_of(def, &at, trigger, look)?;
        entries.push(EffectEntry {
            matches,
            look,
            trigger,
            to_graveyard: def.to_graveyard.as_deref().map(|d| {
                if d == EVERYTHING {
                    Destination::Everything
                } else {
                    Destination::Matching(d.to_string())
                }
            }),
            fetch,
            delay,
            origin: origin.to_string(),
        });
    }
    Ok(EffectLibrary { entries })
}

/// Validate the `after` and `sacrifice` keys of one `[[effect]]` table.
///
/// Refused in the order a reader would fix them: a wait that is not a number
/// of turns, then a wait on something that cannot wait, then a sacrifice with
/// nothing to wait for. A wait with nothing at the end of it never gets here:
/// an effect that neither looks nor fetches is refused before this is asked.
fn delay_of(
    def: &EffectDef,
    at: &str,
    trigger: Trigger,
    look: u32,
) -> Result<Option<Delay>, ErrorKind> {
    let Some(after) = def.after else {
        return match def.sacrifice {
            Some(true) => Err(ErrorKind::SacrificeWithoutDelay { at: at.to_string() }),
            _ => Ok(None),
        };
    };
    let turns = u32::try_from(after)
        .ok()
        .filter(|t| (1..=MAX_TURN).contains(t))
        .ok_or(ErrorKind::BadAfter {
            at: at.to_string(),
            after,
        })?;
    let why = if trigger != Trigger::LandDrop {
        Some("fires on a cast, and only a land that stays in play has anything to wait with")
    } else if look > 0 {
        Some("a `look`, which is refused on a later turn")
    } else {
        None
    };
    if let Some(why) = why {
        return Err(ErrorKind::UnmodelledDelay {
            at: at.to_string(),
            why,
        });
    }
    Ok(Some(Delay {
        turns,
        sacrifice: def.sacrifice.unwrap_or(false),
    }))
}

/// Validate the `fetch` and `to` keys of one `[[effect]]` table.
///
/// They stand or fall together: a priority with nowhere to put what it finds
/// and a destination with nothing arriving at it are each half a declaration,
/// and half a declaration is where a default nobody stated gets invented.
fn fetch_of(def: &EffectDef, at: &str) -> Result<Option<FetchDecl>, ErrorKind> {
    let table = "an `[[effect]]` `fetch`";
    match (&def.fetch, &def.to) {
        (None, None) => Ok(None),
        (None, Some(to)) => Err(ErrorKind::ToWithoutFetch {
            at: at.to_string(),
            to: to.clone(),
        }),
        (Some(prefer), to) => {
            if prefer.is_empty() {
                return Err(ErrorKind::NoPreference { table });
            }
            for (i, query) in prefer.iter().enumerate() {
                if let Some(first) = prefer[..i].iter().position(|q| q == query) {
                    return Err(ErrorKind::RepeatedPreference {
                        table,
                        query: query.clone(),
                        first: first + 1,
                        position: i + 1,
                    });
                }
            }
            let to = to.as_ref().ok_or(ErrorKind::Missing {
                at: at.to_string(),
                key: "to",
                why: "so a card it found would have left the library with nowhere to be. \
                      Write `to = \"hand\"` for a tutor, `to = \"battlefield\"` for a fetchland",
            })?;
            let to = fetched_of(to).ok_or_else(|| ErrorKind::BadFetchDestination {
                at: at.to_string(),
                to: to.clone(),
            })?;
            Ok(Some(FetchDecl {
                prefer: prefer.clone(),
                to,
            }))
        }
    }
}

fn build(source: &str, origin: &str) -> Result<Criteria, ErrorKind> {
    let file = read(source)?;
    if file.criterion.is_empty() && file.expect.is_empty() {
        return Err(ErrorKind::AsksNothing);
    }
    let effects = effects_of(&file, origin)?;
    let land_drop = land_drop_of(&file)?;
    let casting = casting_of(&file)?;

    let mut vocabulary = Vocabulary::default();
    let mut criteria = Vec::with_capacity(file.criterion.len());
    let mut predicates = Vec::with_capacity(file.criterion.len());

    for (i, def) in file.criterion.iter().enumerate() {
        let name = def.name.clone().ok_or(ErrorKind::Unnamed {
            table: "[[criterion]]",
            position: i + 1,
        })?;
        for (key, bound) in [("at_least", def.at_least), ("at_most", def.at_most)] {
            if let Some(value) = bound.filter(|t| !(0.0..=1.0).contains(t)) {
                return Err(ErrorKind::BadThreshold { name, key, value });
            }
        }
        if let (Some(at_least), Some(at_most)) = (def.at_least, def.at_most) {
            if at_least > at_most {
                return Err(ErrorKind::EmptyThreshold {
                    name,
                    at_least,
                    at_most,
                });
            }
        }
        // `require` first, so a file that predates `any_of` interns its queries
        // in exactly the order it always did.
        let all = vocabulary.conjunction(&def.require, |j| {
            format!("criterion {name:?}, clause {}", j + 1)
        })?;
        let any = match &def.any_of {
            None => Vec::new(),
            Some(branches) if branches.is_empty() => return Err(ErrorKind::EmptyAnyOf { name }),
            Some(branches) => {
                let mut compiled = Vec::with_capacity(branches.len());
                for (k, branch) in branches.iter().enumerate() {
                    if branch.require.is_empty() {
                        return Err(ErrorKind::EmptyBranch {
                            name,
                            position: k + 1,
                        });
                    }
                    compiled.push(vocabulary.conjunction(&branch.require, |j| {
                        format!(
                            "criterion {name:?}, any_of branch {}, clause {}",
                            k + 1,
                            j + 1
                        )
                    })?);
                }
                compiled
            }
        };
        let predicate = match (all.is_empty(), any.is_empty()) {
            (true, true) => return Err(ErrorKind::NoClauses { name }),
            (false, true) => Predicate::All(all),
            (true, false) => Predicate::Any(any),
            (false, false) => Predicate::AllAndAny { all, any },
        };
        criteria.push(Criterion {
            name,
            at_least: def.at_least,
            at_most: def.at_most,
        });
        predicates.push(predicate);
    }

    let mut expectations = Vec::with_capacity(file.expect.len());
    let mut probes = Vec::with_capacity(file.expect.len());
    for (i, def) in file.expect.iter().enumerate() {
        let name = def.name.clone().ok_or(ErrorKind::Unnamed {
            table: "[[expect]]",
            position: i + 1,
        })?;
        let at = format!("expectation {name:?}");
        if def.query.is_some() && def.cast.is_some() {
            return Err(ErrorKind::TwoQuestions {
                at,
                keys: "query and cast".to_string(),
            });
        }
        let (query, counted) = match &def.cast {
            Some(query) => {
                if def.zone.is_some() {
                    return Err(ErrorKind::CastCountWithZone { at });
                }
                (query.clone(), Counted::Cast)
            }
            None => (
                def.query.clone().ok_or(ErrorKind::Missing {
                    at: at.clone(),
                    key: "query",
                    why: "so there is nothing for it to count. Write `cast` for how many of \
                          them you cast",
                })?,
                Counted::In(zone_of(def.zone.as_deref(), &at)?),
            ),
        };
        let turn = turn_of(def.turn, &at)?;
        probes.push(Probe {
            turn: turn as usize,
            query: vocabulary.intern(query, turn, counted),
            counted,
        });
        expectations.push(Expectation { name });
    }

    let Vocabulary {
        queries,
        zones,
        horizon,
    } = vocabulary;
    Ok(Criteria {
        queries,
        zones,
        horizon,
        criteria,
        predicates,
        expectations,
        probes,
        effects,
        land_drop,
        casting,
    })
}

/// Validate the `[land_drop]` table of an already-deserialized file.
///
/// Everything checkable without a decklist, which is less than it is for an
/// effect: a preference is one query and a query means nothing until there are
/// cards. What is checkable is that the table says something — an empty list
/// is a policy that decides nothing, and a repeated entry is one that can never
/// be reached, and both would read as a declared priority in the report while
/// arbitrating no drop at all.
fn land_drop_of(file: &FileDef) -> Result<Vec<String>, ErrorKind> {
    preference_of(file.land_drop.as_ref().map(|d| &d.prefer), "[land_drop]")
}

/// The same validation for `[casting]`, which is the same shape of table over
/// a different resource.
fn casting_of(file: &FileDef) -> Result<Vec<String>, ErrorKind> {
    preference_of(file.casting.as_ref().map(|d| &d.prefer), "[casting]")
}

fn preference_of(
    prefer: Option<&Option<Vec<String>>>,
    table: &'static str,
) -> Result<Vec<String>, ErrorKind> {
    let Some(prefer) = prefer else {
        return Ok(Vec::new());
    };
    let prefer = prefer.clone().unwrap_or_default();
    if prefer.is_empty() {
        return Err(ErrorKind::NoPreference { table });
    }
    for (i, query) in prefer.iter().enumerate() {
        if let Some(first) = prefer[..i].iter().position(|q| q == query) {
            return Err(ErrorKind::RepeatedPreference {
                table,
                query: query.clone(),
                first: first + 1,
                position: i + 1,
            });
        }
    }
    Ok(prefer)
}

/// What the whole file asks about, accumulated as it is read.
///
/// One of these per file rather than one per criterion, because these three are
/// facts about the file: the enumeration is built from the queries, the zone
/// notes from the zones, and the run horizon from the deepest turn. A clause
/// inside an `any_of` branch contributes to all three exactly as a `require`
/// clause does, which is what keeps a disjunction from widening the
/// enumeration beyond the union of the queries its branches name.
#[derive(Default)]
struct Vocabulary {
    queries: Vec<String>,
    zones: Vec<Zone>,
    horizon: u32,
}

impl Vocabulary {
    /// Queries are deduplicated across the whole file, so two criteria asking
    /// about `t:land` cost one group bit rather than two — and the enumeration
    /// the engine has to walk grows with the number of *distinct* queries.
    ///
    /// Zones are collected for the same reason: so a run knows the whole set
    /// before it starts, and so a file that never mentions the graveyard never
    /// has to be told anything about one.
    /// A turn this file asks about, whatever it asks there.
    ///
    /// A `can_cast` clause names no query, and a run whose horizon stopped at
    /// the deepest *counting* clause would walk fewer turns than the file asked
    /// about and answer the mana question against a board that never got there.
    fn reach(&mut self, turn: u32) {
        self.horizon = self.horizon.max(turn);
    }

    fn intern(&mut self, query: String, turn: u32, counted: Counted) -> usize {
        self.reach(turn);
        // A casting is not a zone, so it adds nothing to the zone list: the
        // reachability note that list feeds is about a card arriving somewhere
        // nothing routes it to, and "how many did you cast" has its own
        // answer for that — the run says whether it declared a priority at
        // all.
        if let Some(zone) = counted.zone() {
            if !self.zones.contains(&zone) {
                self.zones.push(zone);
            }
        }
        match self.queries.iter().position(|q| *q == query) {
            Some(i) => i,
            None => {
                self.queries.push(query);
                self.queries.len() - 1
            }
        }
    }

    /// One list of clauses, all of which must hold. `at` names the position for
    /// the errors, and is a closure because a clause of a `require` and a
    /// clause of an `any_of` branch are in different places by the same rules.
    fn conjunction(
        &mut self,
        defs: &[ClauseDef],
        at: impl Fn(usize) -> String,
    ) -> Result<Vec<Clause>, ErrorKind> {
        let mut compiled = Vec::with_capacity(defs.len());
        for (j, clause) in defs.iter().enumerate() {
            let at = at(j);
            // The kinds are told apart by which key is present, and a clause
            // holding two of them is refused rather than resolved in some
            // order: they are different questions, and a clause that asked two
            // would have to answer one of them silently.
            let named: Vec<&'static str> = [
                ("query", clause.query.is_some()),
                ("can_cast", clause.can_cast.is_some()),
                ("cast", clause.cast.is_some()),
            ]
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(key, _)| key)
            .collect();
            if named.len() > 1 {
                return Err(ErrorKind::TwoQuestions {
                    at,
                    keys: named.join(" and "),
                });
            }
            if let Some(cost) = &clause.can_cast {
                for (key, present) in [
                    ("min", clause.min.is_some()),
                    ("max", clause.max.is_some()),
                    ("zone", clause.zone.is_some()),
                ] {
                    if present {
                        return Err(ErrorKind::CastWithCounting { at, key });
                    }
                }
                let turn = turn_of(clause.turn, &at)?;
                self.reach(turn);
                compiled.push(Clause::Cast {
                    turn: turn as usize,
                    cost: Cost::parse(cost).map_err(|cost| ErrorKind::BadCost { at, cost })?,
                });
                continue;
            }
            // A casting is not in a zone, so there is no zone to name. Refused
            // rather than ignored: `cast = '...', zone = "battlefield"` looks
            // like it means something, and a clause quietly dropping half of
            // what it was asked is the failure this format exists to prevent.
            let (query, counted) = match &clause.cast {
                Some(query) => {
                    if clause.zone.is_some() {
                        return Err(ErrorKind::CastCountWithZone { at });
                    }
                    (query.clone(), Counted::Cast)
                }
                None => (
                    clause.query.clone().ok_or(ErrorKind::Missing {
                        at: at.clone(),
                        key: "query",
                        why: "so there is nothing for it to count. Write `can_cast` for whether \
                              a cost was payable, or `cast` for how many you cast",
                    })?,
                    Counted::In(zone_of(clause.zone.as_deref(), &at)?),
                ),
            };
            let turn = turn_of(clause.turn, &at)?;
            let bounds = bounds_of(clause, &at, &query)?;
            compiled.push(Clause::Count(Tally {
                turn: turn as usize,
                query: self.intern(query, turn, counted),
                counted,
                bounds,
            }));
        }
        Ok(compiled)
    }
}

/// Silence means the hand.
///
/// The one place this format guesses, and it guesses what every criteria file
/// written before zones existed already meant — so no existing number moves. A
/// zone that *is* written is resolved here and nowhere else, which is what
/// keeps `battlefield` a refusal rather than an approximation.
fn zone_of(zone: Option<&str>, at: &str) -> Result<Zone, ErrorKind> {
    match zone {
        None => Ok(Zone::DEFAULT),
        Some(name) => Zone::parse(name).map_err(|zone| ErrorKind::BadZone {
            at: at.to_string(),
            zone,
        }),
    }
}

fn turn_of(turn: Option<i64>, at: &str) -> Result<u32, ErrorKind> {
    let turn = turn.ok_or(ErrorKind::Missing {
        at: at.to_string(),
        key: "turn",
        why: "so there is no point in the game to count at",
    })?;
    u32::try_from(turn)
        .ok()
        .filter(|t| *t <= MAX_TURN)
        .ok_or(ErrorKind::BadTurn {
            at: at.to_string(),
            turn,
        })
}

fn bounds_of(clause: &ClauseDef, at: &str, query: &str) -> Result<Bounds, ErrorKind> {
    let count = |key: &'static str, value: i64| -> Result<u32, ErrorKind> {
        u32::try_from(value).map_err(|_| ErrorKind::BadCount {
            at: at.to_string(),
            key,
            value,
        })
    };
    match (clause.min, clause.max) {
        (None, None) => Err(ErrorKind::NoBounds {
            at: at.to_string(),
            query: query.to_string(),
        }),
        (Some(min), None) => Ok(Bounds::AtLeast(count("min", min)?)),
        (None, Some(max)) => Ok(Bounds::AtMost(count("max", max)?)),
        (Some(min), Some(max)) => {
            let (min, max) = (count("min", min)?, count("max", max)?);
            if min > max {
                return Err(ErrorKind::EmptyRange {
                    at: at.to_string(),
                    min,
                    max,
                });
            }
            Ok(Bounds::Between { min, max })
        }
    }
}
