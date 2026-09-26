//! What the `gauntlet` binary does, as a library.
//!
//! A test run is four steps: load the deck's [`Library`], prepare the criteria
//! file against it ([`prepare::prepare`]), answer the prepared run, and report.
//! Preparing is where every refusal that can be made before a hand is
//! enumerated lives, so it is public and testable in-process; the rest is
//! reached through [`run_test`].
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.

mod answer;
mod casting;
mod effects;
mod landdrop;
mod library;
mod mulligan;
mod narrow;
mod optimise;
pub mod prepare;
mod report;
pub mod sync;

use std::path::Path;

use anyhow::{Context, Result};

pub use answer::Engine;
pub use library::Library;

use answer::{Answered, Run};
use prepare::Preparation;

/// Run the criteria file at `criteria_path` against the deck at `deck`,
/// printing the report's JSON to stdout and its verdict to stderr.
///
/// Returns whether every bound held, which is the exit status the caller owes
/// its own caller.
pub fn run_test(
    deck: &Path,
    criteria_path: &Path,
    on_the_draw: bool,
    index_path: Option<&Path>,
    engine: Engine,
    trials: u32,
    seed: u64,
) -> Result<bool> {
    let library = Library::load(deck, index_path)?;
    for note in report::library_notes(&library) {
        eprintln!("{note}");
    }
    let source = std::fs::read_to_string(criteria_path)
        .with_context(|| format!("reading criteria {}", criteria_path.display()))?;
    // Hashed off the bytes that were actually about to run, before anything
    // gets a chance to normalise them.
    let criteria_sha256 = report::sha256_hex(source.as_bytes());
    let origin = criteria_path.display().to_string();
    let mut criteria = gauntlet_toml::Criteria::parse(&source, &origin)?;

    let Preparation { notes, run } =
        prepare::prepare(&library, &mut criteria, &origin, on_the_draw);
    for note in &notes {
        eprintln!("{note}");
    }
    let mut prepared = run?;

    let Answered {
        answers,
        sampled,
        enumerations,
        kept,
    } = answer::answer(
        Run {
            engine,
            trials,
            seed,
        },
        &prepared.classes,
        &prepared.grouping,
        &prepared.schedule,
        prepared.plan,
        &mut criteria,
        std::mem::take(&mut prepared.tables),
    )?;
    let kept = prepared.kept(kept, &mut criteria)?;

    let report = report::Report::build(
        report::Questions {
            criteria: criteria.criteria(),
            expectations: criteria.expectations(),
        },
        &answers,
        report::Scenario {
            on_the_draw,
            sampled,
        },
        &library,
        prepared.breakdown(&library, &criteria, &answers, enumerations, kept),
        report::Provenance {
            tool_version: env!("CARGO_PKG_VERSION"),
            index_updated_at: library.index_updated_at.clone(),
            deck_sha256: library.deck_sha256.clone(),
            criteria_sha256,
            effect_library_sha256: report::sha256_hex(gauntlet_toml::STANDARD_LIBRARY.as_bytes()),
        },
    );
    println!("{}", facet_json::to_string_pretty(&report)?);
    eprintln!("{}", report.human());
    Ok(report.ok)
}
