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
use pe_criteria::{
    Count, Criterion, Evaluator, Expectation, NotACount, PathOutcomes, PathView, Plan, Trigger,
    TriggerError, Zone, ZoneError,
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
    /// [`pe_criteria::Route::Nowhere`] for why that is a refusal to guess
    /// rather than a missing feature.
    to_graveyard: Option<String>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct CriterionDef {
    name: Option<String>,
    at_least: Option<f64>,
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
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct ExpectDef {
    name: Option<String>,
    turn: Option<i64>,
    query: Option<String>,
    zone: Option<String>,
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

/// One requirement, with its query resolved to a position in the grouping.
///
/// `zone` is not an `Option`. A clause that left it unresolved would be a
/// clause whose meaning depends on who reads it, and the reader that guesses
/// `hand` is exactly the silent default zones exist to remove. The default is
/// applied once, at parse time, and after that every clause says which zone it
/// counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Clause {
    turn: usize,
    query: usize,
    zone: Zone,
    bounds: Bounds,
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
        let holds_all = |cs: &[Clause]| {
            cs.iter()
                .all(|c| c.bounds.holds(view.count_in(c.turn, c.query, c.zone)))
        };
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

    /// Every clause this criterion holds, wherever it was written. The queries
    /// and zones a file asks about are the union over these, so a branch is no
    /// more hidden from the pre-run analysis than a `require` clause is.
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
    zone: Zone,
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
            .position(|p| p.clauses().any(|c| c.zone == zone))
            .map(|i| self.criteria[i].name.as_str());
        from_criteria.or_else(|| {
            self.probes
                .iter()
                .position(|p| p.zone == zone)
                .map(|i| self.expectations[i].name.as_str())
        })
    }

    /// The deepest turn the file names.
    pub fn horizon(&self) -> u32 {
        self.horizon
    }

    /// Which question first named `query`, so a query that cannot be parsed or
    /// matches nothing can be reported against the thing that asked for it.
    pub fn asked_by(&self, query: &str) -> Option<&str> {
        let idx = self.queries.iter().position(|q| q == query)?;
        let from_criteria = self
            .predicates
            .iter()
            .position(|p| p.clauses().any(|c| c.query == idx))
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
                let seen = view.count_in(probe.turn, probe.query, probe.zone);
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
const SCHEMA: &str = "A criteria file holds [[criterion]] tables (name, at_least, require, \
                      any_of), whose require clauses are (turn, query, zone, min, max) and whose \
                      any_of branches each hold a require of their own, [[expect]] tables (name, \
                      turn, query, zone), and [[effect]] tables (match, look, on, to_graveyard).";

#[derive(Debug, Error)]
pub enum ErrorKind {
    /// The schema is spelled out every time rather than only when a key is
    /// unrecognised. The deserializer says which key it did not know and not
    /// which ones it would have taken, and a key silently dropped is how
    /// `atLeast` turns an assertion into a number that cannot fail.
    #[error("not a criteria file: {0}.\n{SCHEMA}")]
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
        "criterion {name:?}: `at_least = {at_least}` is not a probability. It is the share of \
         hands this must hold in, so 70% is written 0.70"
    )]
    BadThreshold { name: String, at_least: f64 },
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
    facet_toml::from_str(source).map_err(|e| ErrorKind::Malformed(e.kind.to_string()))
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
        let look = def.look.ok_or(ErrorKind::Missing {
            at: at.clone(),
            key: "look",
            why: "so there is nothing for it to examine. Write `look = 1` for \"the top card\"",
        })?;
        let look = u32::try_from(look)
            .ok()
            .filter(|l| (1..=MAX_LOOK).contains(l))
            .ok_or(ErrorKind::BadLook {
                at: at.clone(),
                look,
            })?;
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
            origin: origin.to_string(),
        });
    }
    Ok(EffectLibrary { entries })
}

fn build(source: &str, origin: &str) -> Result<Criteria, ErrorKind> {
    let file = read(source)?;
    if file.criterion.is_empty() && file.expect.is_empty() {
        return Err(ErrorKind::AsksNothing);
    }
    let effects = effects_of(&file, origin)?;

    let mut vocabulary = Vocabulary::default();
    let mut criteria = Vec::with_capacity(file.criterion.len());
    let mut predicates = Vec::with_capacity(file.criterion.len());

    for (i, def) in file.criterion.iter().enumerate() {
        let name = def.name.clone().ok_or(ErrorKind::Unnamed {
            table: "[[criterion]]",
            position: i + 1,
        })?;
        if let Some(at_least) = def.at_least {
            if !(0.0..=1.0).contains(&at_least) {
                return Err(ErrorKind::BadThreshold { name, at_least });
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
        let query = def.query.clone().ok_or(ErrorKind::Missing {
            at: at.clone(),
            key: "query",
            why: "so there is nothing for it to count",
        })?;
        let turn = turn_of(def.turn, &at)?;
        let zone = zone_of(def.zone.as_deref(), &at)?;
        probes.push(Probe {
            turn: turn as usize,
            query: vocabulary.intern(query, turn, zone),
            zone,
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
    })
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
    fn intern(&mut self, query: String, turn: u32, zone: Zone) -> usize {
        self.horizon = self.horizon.max(turn);
        if !self.zones.contains(&zone) {
            self.zones.push(zone);
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
            let query = clause.query.clone().ok_or(ErrorKind::Missing {
                at: at.clone(),
                key: "query",
                why: "so there is nothing for it to count",
            })?;
            let turn = turn_of(clause.turn, &at)?;
            let zone = zone_of(clause.zone.as_deref(), &at)?;
            let bounds = bounds_of(clause, &at, &query)?;
            compiled.push(Clause {
                turn: turn as usize,
                query: self.intern(query, turn, zone),
                zone,
                bounds,
            });
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
