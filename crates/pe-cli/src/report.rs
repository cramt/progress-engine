//! Turning results into a verdict.

use facet::Facet;
use pe_criteria::Criterion;

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

#[derive(Facet)]
pub struct Report {
    pub library_size: u32,
    pub commanders: Vec<String>,
    pub on_the_draw: bool,
    pub method: &'static str,
    #[facet(skip_serializing_if = Option::is_none)]
    pub trials: Option<u32>,
    pub queries: Vec<QueryMatch>,
    pub criteria: Vec<CriterionResult>,
    pub asserted: usize,
    pub failed: usize,
    pub ok: bool,
}

impl Report {
    pub fn build(
        criteria: &[Criterion],
        probabilities: &[f64],
        sampled: Option<u32>,
        library: &Library,
        queries: Vec<QueryMatch>,
        on_the_draw: bool,
    ) -> Self {
        let results: Vec<CriterionResult> = criteria
            .iter()
            .zip(probabilities)
            .map(|(c, &p)| CriterionResult {
                name: c.name.clone(),
                probability: round(p, 6),
                percent: round(p * 100.0, 2),
                standard_error: sampled.map(|t| round(pe_sim::standard_error(p, t), 6)),
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
            commanders: library.commanders.clone(),
            on_the_draw,
            method: if sampled.is_some() {
                "sampled"
            } else {
                "exact"
            },
            trials: sampled,
            queries,
            criteria: results,
            asserted,
            failed,
            ok: failed == 0,
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
        out
    }
}

fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}
