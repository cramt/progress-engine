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
//! A file registers two kinds of question. A `[[criterion]]` is a conjunction
//! of clauses and is answered with a probability. An `[[expect]]` names one
//! count and is answered with a mean and the distribution behind it. They are
//! separate tables rather than one table with an optional threshold, so a
//! question cannot be read as the other kind — the confusion the JavaScript
//! front end had to detect at runtime is not a state this format can hold.
//!
//! The narrowness is the whole trick, and it is the same one the exact engine
//! needs: a criterion that depends only on counts is a pure function of the
//! composition, so it can be evaluated once per possible composition rather
//! than once per simulated hand.

use facet::Facet;
use pe_criteria::{
    Count, Criterion, Evaluator, Expectation, NotACount, PathOutcomes, PathView, Plan,
};
use thiserror::Error;

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
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct CriterionDef {
    name: Option<String>,
    at_least: Option<f64>,
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
    min: Option<i64>,
    max: Option<i64>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct ExpectDef {
    name: Option<String>,
    turn: Option<i64>,
    query: Option<String>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Clause {
    checkpoint: usize,
    query: usize,
    bounds: Bounds,
}

/// Where an expectation reads its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    checkpoint: usize,
    query: usize,
}

/// A parsed criteria file, ready to answer against either engine.
#[derive(Debug)]
pub struct Criteria {
    /// Every distinct query the file names, in the order it first names them.
    /// This is the whole set, known before a single hand is enumerated.
    queries: Vec<String>,
    horizon: u32,
    /// Parallel to `clauses`, and to the answers a run hands back: everything
    /// here is matched up by position. Both vectors are filled in one pass in
    /// `parse` and are immutable afterwards.
    criteria: Vec<Criterion>,
    clauses: Vec<Vec<Clause>>,
    expectations: Vec<Expectation>,
    probes: Vec<Probe>,
}

impl Criteria {
    /// Parse a criteria file. `origin` names it, for the errors.
    pub fn parse(source: &str, origin: &str) -> Result<Self, CriteriaError> {
        build(source).map_err(|kind| CriteriaError {
            origin: origin.to_string(),
            kind,
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

    /// The deepest turn the file names.
    pub fn horizon(&self) -> u32 {
        self.horizon
    }

    /// Which question first named `query`, so a query that cannot be parsed or
    /// matches nothing can be reported against the thing that asked for it.
    pub fn asked_by(&self, query: &str) -> Option<&str> {
        let idx = self.queries.iter().position(|q| q == query)?;
        let from_criteria = self
            .clauses
            .iter()
            .position(|cs| cs.iter().any(|c| c.query == idx))
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
        let held = self
            .clauses
            .iter()
            .map(|clauses| {
                clauses
                    .iter()
                    .all(|c| c.bounds.holds(view.count(c.checkpoint, c.query)))
            })
            .collect();
        let counted = self
            .probes
            .iter()
            .zip(&self.expectations)
            .map(|(probe, expectation)| {
                let seen = view.count(probe.checkpoint, probe.query);
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
const SCHEMA: &str = "A criteria file holds [[criterion]] tables (name, at_least, require), \
                      whose require clauses are (turn, query, min, max), and [[expect]] tables \
                      (name, turn, query).";

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
        "criterion {name:?} has no `require` clauses, so there is nothing for it to hold or \
         fail on and it would report 100% of every deck"
    )]
    NoClauses { name: String },
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

fn build(source: &str) -> Result<Criteria, ErrorKind> {
    // `e.kind` rather than `e`: the Display of the whole error appends a debug
    // dump of the target type's reflection data, which is a page of noise in
    // front of the one line that says which key was wrong.
    let file: FileDef =
        facet_toml::from_str(source).map_err(|e| ErrorKind::Malformed(e.kind.to_string()))?;
    if file.criterion.is_empty() && file.expect.is_empty() {
        return Err(ErrorKind::AsksNothing);
    }

    let mut queries: Vec<String> = Vec::new();
    let mut horizon = 0u32;
    let mut criteria = Vec::with_capacity(file.criterion.len());
    let mut clauses = Vec::with_capacity(file.criterion.len());

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
        if def.require.is_empty() {
            return Err(ErrorKind::NoClauses { name });
        }
        let mut compiled = Vec::with_capacity(def.require.len());
        for (j, clause) in def.require.iter().enumerate() {
            let at = format!("criterion {name:?}, clause {}", j + 1);
            let query = clause.query.clone().ok_or(ErrorKind::Missing {
                at: at.clone(),
                key: "query",
                why: "so there is nothing for it to count",
            })?;
            let turn = turn_of(clause.turn, &at)?;
            let bounds = bounds_of(clause, &at, &query)?;
            horizon = horizon.max(turn);
            compiled.push(Clause {
                checkpoint: turn as usize,
                query: intern(&mut queries, query),
                bounds,
            });
        }
        criteria.push(Criterion {
            name,
            at_least: def.at_least,
        });
        clauses.push(compiled);
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
        horizon = horizon.max(turn);
        probes.push(Probe {
            checkpoint: turn as usize,
            query: intern(&mut queries, query),
        });
        expectations.push(Expectation { name });
    }

    Ok(Criteria {
        queries,
        horizon,
        criteria,
        clauses,
        expectations,
        probes,
    })
}

/// Queries are deduplicated across the whole file, so two criteria asking about
/// `t:land` cost one group bit rather than two — and the enumeration the engine
/// has to walk grows with the number of *distinct* queries.
fn intern(queries: &mut Vec<String>, query: String) -> usize {
    match queries.iter().position(|q| *q == query) {
        Some(i) => i,
        None => {
            queries.push(query);
            queries.len() - 1
        }
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
