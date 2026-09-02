//! The `progress-engine` command line interface.
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.

mod library;
mod report;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use library::Library;

#[derive(Parser)]
#[command(name = "progress-engine", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a decklist and emit it as JSON.
    ///
    /// This is the canonical decklist parser; other tools shell out to it so
    /// there is exactly one definition of what a decklist is.
    Parse {
        /// Decklist file in Archidekt format.
        file: PathBuf,
    },
    /// Run a criteria file against a decklist.
    Test {
        /// Decklist file in Archidekt format.
        deck: PathBuf,
        /// JavaScript criteria file.
        criteria: PathBuf,
        /// Model being on the draw rather than on the play.
        #[arg(long)]
        draw: bool,
        /// Sample instead of enumerating. Slower and approximate; the exact
        /// engine is the default for good reason.
        #[arg(long)]
        simulate: bool,
        /// Hands to deal when sampling.
        #[arg(long, default_value_t = 200_000)]
        trials: u32,
        /// Seed, so a sampled run is reproducible.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Scryfall index, as built by `scryfall sync`.
        #[arg(long)]
        index: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Parse { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading decklist {}", file.display()))?;
            let entries = pe_decklist::parse(&text)?;
            // `commander` and `outside` are emitted rather than left for the
            // caller to re-derive. They are the fiddly part — a companion is a
            // 101st card, "Sticker Package" is not a sideboard, and a
            // multi-category line has to be tested per category — and a second
            // implementation of that is exactly what this tool exists to remove.
            let augmented: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut v = serde_json::to_value(e).expect("entry serialises");
                    v["commander"] = e.is_commander().into();
                    v["outside"] = e.is_outside().into();
                    v
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&augmented)?);
            Ok(())
        }
        Command::Test {
            deck,
            criteria,
            draw,
            simulate,
            trials,
            seed,
            index,
        } => run_test(
            &deck,
            &criteria,
            draw,
            index.as_deref(),
            simulate,
            trials,
            seed,
        ),
    }
}

fn run_test(
    deck: &std::path::Path,
    criteria_path: &std::path::Path,
    on_the_draw: bool,
    index_path: Option<&std::path::Path>,
    simulate: bool,
    trials: u32,
    seed: u64,
) -> Result<()> {
    let library = Library::load(deck, index_path)?;
    let source = std::fs::read_to_string(criteria_path)
        .with_context(|| format!("reading criteria {}", criteria_path.display()))?;

    let mut criteria = pe_js::Criteria::load(source)?;

    // Which queries the file uses and how deep into the game it looks are both
    // discovered by running it, so neither has to be declared. Both engines go
    // through the same loop, so they cannot drift apart in how they resolve a
    // criteria file — only in how they compute the answer.
    let probabilities: Vec<f64> = pe_js::with_discovery(
        &mut criteria,
        |queries| library.grouping_for(queries),
        |turns| draw_gaps(turns, on_the_draw),
        |grouping, gaps, criteria| -> Result<Vec<f64>> {
            if simulate {
                Ok(pe_sim::simulate(grouping, gaps, trials, seed, criteria)?)
            } else {
                let n = criteria.criteria().len();
                Ok(pe_criteria::run(grouping, gaps, n, criteria)?
                    .into_iter()
                    .map(|p| p.get())
                    .collect())
            }
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let queries = criteria
        .queries()
        .into_iter()
        .map(|q| {
            let cards = library.matching(&q).unwrap_or(0);
            report::QueryMatch { query: q, cards }
        })
        .collect();
    let report = report::Report::build(
        criteria.criteria(),
        &probabilities,
        simulate.then_some(trials),
        &library,
        queries,
        on_the_draw,
    );
    println!("{}", serde_json::to_string_pretty(&report)?);
    eprintln!("{}", report.human());
    if !report.ok {
        std::process::exit(1);
    }
    Ok(())
}

/// Cards drawn between successive checkpoints.
///
/// `t(0)` is the opening seven. On the play turn 1 draws nothing, so `t(1)` sees
/// the same seven — which is the honest answer, not an off-by-one.
fn draw_gaps(max_turn: u32, on_the_draw: bool) -> Vec<u32> {
    let seen = |n: u32| -> u32 {
        if n == 0 {
            7
        } else if on_the_draw {
            7 + n
        } else {
            7 + n - 1
        }
    };
    (0..=max_turn)
        .map(|n| {
            if n == 0 {
                seen(0)
            } else {
                seen(n) - seen(n - 1)
            }
        })
        .collect()
}
