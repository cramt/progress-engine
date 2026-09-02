//! Turning results into a verdict.

use facet::Facet;
use pe_criteria::Criterion;
use sha2::{Digest, Sha256};

use crate::legality::Violation;
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
    pub criteria_sha256: String,
}

/// The two knobs a sampled run has and an exact run has no honest answer for.
///
/// Kept together so neither can be reported without the other: trials without a
/// seed does not identify the hands that were dealt, and a seed without trials
/// does not reproduce them.
#[derive(Clone, Copy)]
pub struct Sampling {
    pub trials: u32,
    pub seed: u64,
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
    /// Everything about the list that is against the rules, empty when there is
    /// nothing to say. Never affects `ok`: see `legality.rs` for why a warning
    /// does not fail a run, and why this array exists so that a caller can
    /// decide otherwise for itself.
    pub legality: Vec<Violation>,
    pub on_the_draw: bool,
    pub method: &'static str,
    #[facet(skip_serializing_if = Option::is_none)]
    pub trials: Option<u32>,
    /// Present only for sampled runs, alongside `trials`: a sampled figure is
    /// only reproducible by whoever knows which seed dealt the hands.
    #[facet(skip_serializing_if = Option::is_none)]
    pub seed: Option<u64>,
    pub queries: Vec<QueryMatch>,
    pub criteria: Vec<CriterionResult>,
    pub asserted: usize,
    pub failed: usize,
    pub ok: bool,
    pub provenance: Provenance,
}

impl Report {
    pub fn build(
        criteria: &[Criterion],
        probabilities: &[f64],
        scenario: Scenario,
        library: &Library,
        queries: Vec<QueryMatch>,
        legality: Vec<Violation>,
        provenance: Provenance,
    ) -> Self {
        let Scenario {
            on_the_draw,
            sampled,
        } = scenario;
        let results: Vec<CriterionResult> = criteria
            .iter()
            .zip(probabilities)
            .map(|(c, &p)| CriterionResult {
                name: c.name.clone(),
                probability: round(p, 6),
                percent: round(p * 100.0, 2),
                standard_error: sampled.map(|s| round(pe_sim::standard_error(p, s.trials), 6)),
                at_least: c.at_least,
                // A criterion with no threshold is informational; it reports a
                // number and cannot fail.
                pass: c.at_least.is_none_or(|t| p >= t),
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
            legality,
            on_the_draw,
            method: if sampled.is_some() {
                "sampled"
            } else {
                "exact"
            },
            trials: sampled.map(|s| s.trials),
            seed: sampled.map(|s| s.seed),
            queries,
            criteria: results,
            asserted,
            failed,
            ok: failed == 0,
            provenance,
        }
    }

    pub fn human(&self) -> String {
        let mut out = String::new();
        for q in &self.queries {
            if q.cards == 0 {
                out.push_str(&format!(
                    "note: query {:?} matched no cards in this deck\n",
                    q.query
                ));
            }
        }
        let width = self
            .criteria
            .iter()
            .map(|c| c.name.chars().count())
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
            out.push_str(&format!(
                "{status}{:width$}  {:>6.2}%{target}\n",
                c.name, c.percent
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
        // Said twice on purpose. The detail is printed before the run so that a
        // run which then aborts still carries it, but the verdict line is what
        // a reader actually stops on, and a clean PASS sitting alone under an
        // illegal decklist is exactly the report this check exists to prevent.
        if !self.legality.is_empty() {
            out.push_str(&format!(
                "\nWARNING: {} legality problem{} above — this is not a legal Commander deck",
                self.legality.len(),
                if self.legality.len() == 1 { "" } else { "s" }
            ));
        }
        out
    }
}

/// What to tell a human about the cards that never reach the library.
///
/// Printed before the run rather than folded into `human()`, because a list that
/// is *all* stickers fails with "the library is empty" and the reader still
/// needs to know where their cards went.
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

/// What to tell a human about a list that is against the rules.
///
/// Printed before the run for the same reason the exclusion note is: a run that
/// then stops on an empty library or a question too wide to answer would take
/// the warning down with it, and "your deck is illegal" is worth having either
/// way. It does not stop the run — see `legality.rs`.
pub fn legality_note(violations: &[Violation]) -> Option<String> {
    if violations.is_empty() {
        return None;
    }
    let mut out = String::from("WARNING: this is not a legal Commander deck\n");
    for v in violations {
        out.push_str(&format!("  {}: {}\n", v.rule, v.detail));
    }
    out.push_str("The numbers below describe the list exactly as written.");
    Some(out)
}

fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}
