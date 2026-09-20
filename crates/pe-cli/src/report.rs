//! Turning results into a verdict.

use facet::Facet;
use pe_criteria::{Criterion, Expectation};
use pe_scryfall::index::TagGap;
use pe_stats::Distribution;
use sha2::{Digest, Sha256};

use crate::library::Library;

#[derive(Facet)]
pub struct CriterionResult {
    pub name: String,
    pub probability: f64,
    pub percent: f64,
    pub at_least: Option<f64>,
    /// Present only for sampled runs: never quote a sampled figure without it.
    #[facet(skip_serializing_if = Option::is_none)]
    pub standard_error: Option<f64>,
    pub pass: bool,
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
    /// `"exact"` or `"sampled"`, for this answer rather than for the run.
    ///
    /// A file is not one question, and since #31 it is not one enumeration
    /// either: each class of questions is answered on the narrowest
    /// enumeration that can answer it, so one criterion can be over the ceiling
    /// while the one beside it is enumerated exactly. The top-level `method`
    /// still says what happened to the run as a whole; this says what happened
    /// to this number, which is the one a reader is about to quote.
    pub method: &'static str,
}

/// What an expectation answered: how many, on average, and how that was spread.
///
/// There is no `at_least` and no `pass` here, and their absence is deliberate
/// rather than pending. See `pe_criteria::Expectation` for why an expectation
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
    /// `"exact"` or `"sampled"`, for this answer rather than for the run. See
    /// [`CriterionResult::method`].
    pub method: &'static str,
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
    pub fn none(plan: pe_criteria::Plan) -> Estimated {
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

/// How many library cards a query actually matched.
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
    /// Lands whose tapped-ness this run decided for the pilot. Empty unless the
    /// run asked a mana question, because otherwise it decided nothing.
    pub assumed_tapped: Vec<String>,
    /// The priority that decided the land drop, where the file declared one.
    ///
    /// Beside the tapped-ness assumptions rather than anywhere else, because
    /// it is the same kind of fact: something that chose between two lands and
    /// moved every number that depended on which one was played.
    pub land_drop: Option<LandDropUse>,
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

/// What one answer says about where it came from.
const SAMPLED: &str = "sampled";
const EXACT: &str = "exact";

fn method_of(estimated: bool) -> &'static str {
    if estimated {
        SAMPLED
    } else {
        EXACT
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
    pub method: &'static str,
    /// Sampled runs only: `"requested"` or `"too_wide"`.
    ///
    /// A string rather than a bool, because there will be more ways to end up
    /// here than there are today and a `fell_back: false` would have to be
    /// reinterpreted rather than extended.
    #[facet(skip_serializing_if = Option::is_none)]
    pub sampled_because: Option<&'static str>,
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
            assumed_tapped,
            land_drop,
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
            .map(|((c, &p), &estimated)| {
                let how = sampled.filter(|_| estimated);
                CriterionResult {
                    name: c.name.clone(),
                    probability: round(p, 6),
                    percent: round(p * 100.0, 2),
                    standard_error: how.map(|s| round(pe_sim::standard_error(p, s.trials), 6)),
                    at_least: c.at_least,
                    // A criterion with no threshold is informational; it reports
                    // a number and cannot fail.
                    pass: c.at_least.is_none_or(|t| p >= t),
                    inconclusive: how.and_then(|s| {
                        let threshold = c.at_least?;
                        let se = pe_sim::standard_error(p, s.trials);
                        Some((p - threshold).abs() <= INCONCLUSIVE_ERRORS * se)
                    }),
                    method: method_of(estimated),
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
                    .map(|s| round(pe_sim::mean_standard_error(d, s.trials), 6)),
                method: method_of(estimated),
            })
            .collect();

        let asserted = results.iter().filter(|r| r.at_least.is_some()).count();
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
            // Three states, because a run can now be both. Narrowing means the
            // file is several enumerations, so the question that went over the
            // ceiling no longer takes its neighbours down with it — and a
            // reader has to be able to tell that from a run where everything
            // was sampled.
            method: match (answers.estimated.any(), answers.estimated.all()) {
                (false, _) => "exact",
                (true, true) => "sampled",
                (true, false) => "mixed",
            },
            sampled_because: sampled.map(|s| match s.why {
                WhySampled::Requested => "requested",
                WhySampled::TooWide { .. } => "too_wide",
            }),
            too_wide: sampled.and_then(|s| match s.why {
                WhySampled::Requested => None,
                WhySampled::TooWide { paths, groups } => Some(TooWide {
                    paths: paths as f64,
                    groups,
                    ceiling: pe_criteria::MAX_PATHS as f64,
                }),
            }),
            trials: sampled.map(|s| s.trials),
            seed: sampled.map(|s| s.seed),
            queries,
            zones,
            effects,
            assumed_tapped,
            land_drop,
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
                (None, _) => ", everything stays on top".to_string(),
                (Some(q), true) => format!(", {q} to the graveyard"),
                (Some(q), false) => format!(", {q} to the graveyard — which no card here matches"),
            };
            out.push_str(&format!(
                "note: effect {:?} (look {}, on {}{route})\n      applies to {} card{}: {}\n",
                e.matches,
                e.look,
                e.on,
                e.copies,
                if e.copies == 1 { "" } else { "s" },
                e.cards.join(", ")
            ));
        }
        // The same failure as an empty query, arriving by a different door. A
        // criterion asking about a zone no effect in this run routes into is
        // answered with a confident zero that reads exactly like a deck that
        // never gets there. It stays answered — silently refusing to print a
        // number is its own kind of lie — but it does not get to be mistaken
        // for a measurement. It switches off the moment an effect routes a card
        // there, and not before: a note that never fires is as useless as one
        // that always does.
        for z in &self.zones {
            if z.reachable {
                continue;
            }
            let fix = if z.zone == "battlefield" {
                "This deck holds no land, so nothing can be played."
            } else {
                "Declare `to_graveyard` on an [[effect]] to route one there."
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
            let status = match (c.at_least, c.pass) {
                (None, _) => "     ",
                (Some(_), true) => "PASS ",
                (Some(_), false) => "FAIL ",
            };
            let target = match c.at_least {
                Some(t) => format!("  (needs {:.1}%)", t * 100.0),
                None => String::new(),
            };
            // On the line rather than in a footnote. A sampled percentage
            // printed to two decimals looks exactly as certain as an enumerated
            // one, and the error bar is the only thing on the page that says it
            // is not.
            out.push_str(&format!(
                "{status}{:width$}  {:>6.2}%{}{target}\n",
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
            let (Some(true), Some(threshold), Some(se)) =
                (c.inconclusive, c.at_least, c.standard_error)
            else {
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
            .filter(|c| c.method == SAMPLED)
            .map(|c| c.name.as_str())
            .chain(
                self.expectations
                    .iter()
                    .filter(|e| e.method == SAMPLED)
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
         with: progress-engine sync"
            .to_string()
    })
}

/// Why a query naming an oracle tag this index cannot answer is refused.
///
/// Refused rather than answered, for the reason the whole tool exists: a count
/// of a tag nobody fetched is zero by construction, and it reads exactly like a
/// deck that plays no such card. The two halves of the message are the two
/// facts a reader needs — what this index knows, and which command changes it.
pub fn tag_gap_refusal(gap: &TagGap, library: &Library) -> String {
    let named: Vec<String> = gap.tags().iter().map(|t| format!("otag:{t}")).collect();
    let named = named.join(", ");
    match gap {
        TagGap::NotCarried(_) => format!(
            "this index does not carry {named}, so counting it would be zero by \
             construction\n      rather than by measurement. This index carries: {}.\n      \
             Check the spelling; a tag outside that list has to be added to \
             pe-scryfall's\n      standard tags and fetched by `progress-engine sync`.",
            library.index_tags.carried().join(", ")
        ),
        TagGap::NoneFetched(_) => format!(
            "this index carries no oracle tags at all, so {named} would match nothing \
             here\n      whether or not this deck plays such a card.\n      \
             `sync --from` builds an index like this one: tags come from Scryfall's search \
             API,\n      not from the bulk file. Fetch them with: progress-engine sync"
        ),
    }
}

/// Why a query naming a keyword no card in this index has is refused.
///
/// The precedent the tag refusal was built on, finally wired to something. The
/// index's keyword list is the whole card pool's, not this deck's, so a keyword
/// missing from it is missing from Magic as this index knows Magic — which
/// leaves exactly two readings, a typo or a set newer than the file, and one
/// command tells them apart. No list of what it does carry, unlike the tag
/// refusal: that one names six tags, this one would name two thousand keywords.
pub fn unknown_keyword_refusal(unknown: &[String]) -> String {
    let named: Vec<String> = unknown.iter().map(|k| format!("kw:{k}")).collect();
    format!(
        "no card in this index has {}, so counting it would be zero by construction\n      \
         rather than by measurement — which reads exactly like a deck that plays none. \
         The\n      index lists every keyword the whole card pool carries, so this is a \
         misspelling\n      unless it is newer than the index. Check the spelling; \
         rebuild with: progress-engine sync",
        named.join(", ")
    )
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
             Fetch the tags with: progress-engine sync   (--from cannot: they come from the \
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

fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}

/// Why a battlefield question about something that is not a land is refused.
///
/// The one approximation this would not be is "drawn", and it is wrong in the
/// direction that flatters the deck: an opening hand with one Island and a
/// three-drop has the three-drop in hand on turn 0 and on the battlefield on no
/// turn at all. A land is different in kind rather than in degree — it arrives
/// on a land drop, which is free, capped at one a turn, and something this
/// engine walks — so the zone opens for lands and stays shut for everything
/// else.
pub fn battlefield_refusal(query: &str, spells: &[String]) -> String {
    format!(
        "`zone = \"battlefield\"` in query {query:?} is only answerable for lands, and this \
         query matches {}\n      that {}: {}.\n      \
         A land arrives on a land drop, which is free and one a turn, so this engine knows when \
         it\n      got there. Everything else has to be cast, and which spells you cast when you \
         cannot cast\n      them all is the budget half of the mana model \
         (https://github.com/cramt/progress-engine/issues/10).\n      \
         Narrow the query to lands, or ask about `hand` and know that is what you asked.",
        spells.len(),
        if spells.len() == 1 {
            "is not"
        } else {
            "are not"
        },
        spells.join(", ")
    )
}

/// Why a mana question beside a live land-drop effect, with no priority
/// declared, is refused.
///
/// Both are answers to *which land did you play this turn*, and with nothing
/// declared they are different answers. The walk plays the deepest-looking land
/// you are holding, because there is nothing else to tell two drops apart; the
/// gate assumes you played whichever land pays. Running both would be two
/// policies over one resource, which is the failure VISION.md is written
/// against.
///
/// **What it does not do is pick one.** Which land you would have played is a
/// decision the pilot makes and the tool cannot derive, so the refusal names
/// the declaration that settles it — [`crate::landdrop`] — rather than choosing
/// a default and mentioning it in a note nobody reads. That is *ask, don't
/// guess* applied to a policy instead of to card data.
pub fn mana_beside_effects_refusal() -> String {
    "a mana question and a live land-drop effect are both answers to which land you played \
     this turn,\n      and this file declares no priority between them. With none declared \
     the effect plays the\n      deepest-looking land in hand and the mana question assumes \
     whichever land pays, which are\n      two answers to one drop — so this is refused \
     rather than arbitrated.\n      \
     Declare the priority and both read the same drop:\n\n      \
     [land_drop]\n      prefer = ['otag:surveil', 't:land -otag:tapland']\n\n      \
     The list is read in order, the first entry a land in hand matches wins, and any land the \
     list\n      does not name is played last."
        .to_string()
}

/// Why this index cannot price a cost, or `None` when it can.
///
/// Both halves are silent failures of the same shape as an unfetched oracle
/// tag: an index with no `produces` reports every land as making nothing and
/// answers 0.00%, and an index with no tapland tag reports every land as
/// untapped and answers a number the deck cannot reach. Neither looks like a
/// gap in the data from the outside.
pub fn cannot_price_mana(library: &Library) -> Option<String> {
    if library.index_is_stale {
        return Some(
            "this index was built before it recorded what a land produces, so every land in it \
             makes\n      no mana and every cost would be unpayable. \
             Rebuild it with: progress-engine sync"
                .to_string(),
        );
    }
    let missing: Vec<&str> = [crate::library::TAPLAND, crate::library::CONDITIONAL_TAPLAND]
        .into_iter()
        .filter(|tag| !library.index_tags.contains(tag))
        .collect();
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "this index does not carry {}, so it cannot say which lands enter tapped.\n      \
         A land that enters tapped makes no mana the turn it arrives, which is the difference \
         between\n      two lands and two mana — and without the tag every land here would \
         read as untapped, which\n      is the optimistic answer rather than the measured one. \
         This index carries: {}.\n      \
         Fetch them with: progress-engine sync   (--from cannot: they come from the search API)",
        missing
            .iter()
            .map(|t| format!("otag:{t}"))
            .collect::<Vec<_>>()
            .join(" or "),
        library.index_tags.carried().join(", ")
    ))
}
