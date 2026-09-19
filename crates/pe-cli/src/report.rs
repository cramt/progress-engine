//! Turning results into a verdict.

use facet::Facet;
use pe_criteria::{Criterion, Expectation};
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
        } = breakdown;
        let Scenario {
            on_the_draw,
            sampled,
        } = scenario;
        let results: Vec<CriterionResult> = questions
            .criteria
            .iter()
            .zip(&answers.probabilities)
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

        let expected: Vec<ExpectationResult> = questions
            .expectations
            .iter()
            .zip(&answers.distributions)
            .map(|(e, d)| ExpectationResult {
                name: e.name.clone(),
                mean: round(d.mean(), 4),
                distribution: d.probabilities().iter().map(|p| round(*p, 6)).collect(),
                standard_error: sampled.map(|s| round(pe_sim::mean_standard_error(d, s.trials), 6)),
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
            method: if sampled.is_some() {
                "sampled"
            } else {
                "exact"
            },
            trials: sampled.map(|s| s.trials),
            seed: sampled.map(|s| s.seed),
            queries,
            zones,
            effects,
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
            if !z.reachable {
                out.push_str(&format!(
                    "note: nothing routes a card to the {} in this run, so every count in\n      \
                     it is zero by construction rather than by measurement.\n      \
                     Asked by: {:?}\n      \
                     Declare `to_graveyard` on an [[effect]] to route one there.\n",
                    z.zone, z.asked_by
                ));
            }
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
            out.push_str(&format!(
                "{status}{:width$}  {:>6.2}%{target}\n",
                c.name, c.percent
            ));
        }

        // The blank status column is not decoration: it is the same five
        // characters PASS and FAIL occupy, and it says that an expectation has
        // no verdict to give rather than that its verdict was omitted.
        let indent = 5 + width + 2;
        for e in &self.expectations {
            out.push_str(&format!("     {:width$}  mean {:.2}\n", e.name, e.mean));
            for line in histogram_lines(&e.distribution, indent) {
                out.push_str(&format!("{:indent$}{line}\n", ""));
            }
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
         with: progress-engine sync"
            .to_string()
    })
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
