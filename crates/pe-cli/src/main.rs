//! The `progress-engine` command line interface.
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.

mod legality;
mod library;
mod report;
mod sync;

use std::path::PathBuf;

use anyhow::{Context, Result};
use facet::Facet;
use figue::{self as args, DriverError, FigueBuiltins};

use library::Library;

/// Draw-probability tests for Magic: The Gathering decklists.
#[derive(Facet)]
struct Cli {
    #[facet(args::subcommand)]
    command: Command,
    #[facet(flatten)]
    _builtins: FigueBuiltins,
}

#[derive(Facet)]
#[repr(u8)]
enum Command {
    /// Parse a decklist and emit it as JSON.
    ///
    /// This is the canonical decklist parser; other tools shell out to it so
    /// there is exactly one definition of what a decklist is.
    Parse {
        /// Decklist file in Archidekt format.
        #[facet(args::positional)]
        file: PathBuf,
    },
    /// Run a criteria file against a decklist.
    Test {
        /// Decklist file in Archidekt format.
        #[facet(args::positional)]
        deck: PathBuf,
        /// JavaScript criteria file.
        #[facet(args::positional)]
        criteria: PathBuf,
        /// Model being on the draw rather than on the play.
        #[facet(args::named, default)]
        draw: bool,
        /// Sample instead of enumerating. Slower and approximate; the exact
        /// engine is the default for good reason.
        #[facet(args::named, default)]
        simulate: bool,
        /// Hands to deal when sampling.
        #[facet(args::named, default = 200_000)]
        trials: u32,
        /// Seed, so a sampled run is reproducible.
        #[facet(args::named, default = 0)]
        seed: u64,
        /// Scryfall index, as built by `progress-engine sync`.
        #[facet(args::named, default)]
        index: Option<PathBuf>,
    },
    /// Build the card index from Scryfall's bulk data.
    ///
    /// `test` needs to know what a card is, and this is where that comes from.
    /// Run it once, and again when you want newer cards.
    Sync {
        /// Where to write the index. Defaults to the path `test` reads.
        #[facet(args::named, default)]
        index: Option<PathBuf>,
        /// Build from a bulk file already on disk instead of downloading one.
        #[facet(args::named, default)]
        from: Option<PathBuf>,
        /// Rebuild even when the index already has Scryfall's latest data.
        #[facet(args::named, default)]
        force: bool,
    },
}

/// A decklist entry as `parse` emits it.
///
/// `commander` and `outside` are emitted rather than left for the caller to
/// re-derive. They are the fiddly part — a companion is a 101st card, "Sticker
/// Package" is not a sideboard, and a multi-category line has to be tested per
/// category — and a second implementation of that is exactly what this tool
/// exists to remove.
#[derive(Facet)]
struct ParsedEntry {
    #[facet(flatten)]
    entry: pe_decklist::Entry,
    commander: bool,
    outside: bool,
}

/// Parse argv, with this tool's output discipline rather than figue's default.
///
/// figue renders a missing argument as a help request printed to stdout with
/// exit 0, which would hand a caller a success for a command that never ran and
/// put non-JSON on the stdout a caller pipes through `jq`. Both matter here. So
/// help goes to stdout and exits 0 only when it was actually asked for;
/// otherwise it is a usage error, and it goes to stderr with a non-zero status.
fn parse_args() -> Cli {
    let asked_for_help = std::env::args().skip(1).any(|a| {
        matches!(a.as_str(), "-h" | "--help" | "-V" | "--version")
            || a.starts_with("--completions")
            || a.starts_with("--html-help")
            || a.starts_with("--export-jsonschemas")
    });

    match figue::from_std_args::<Cli>().into_result() {
        Ok(output) => output.get(),
        Err(DriverError::Help { text, suggestion }) if asked_for_help => {
            println!("{text}");
            if let Some(s) = suggestion {
                println!("{}", s.render_pretty());
            }
            std::process::exit(0);
        }
        Err(DriverError::Help { text, suggestion }) => {
            eprintln!("{text}");
            if let Some(s) = suggestion {
                eprintln!("{}", s.render_pretty());
            }
            std::process::exit(2);
        }
        Err(other) => {
            eprintln!("{other}");
            std::process::exit(other.exit_code().max(1));
        }
    }
}

fn main() -> Result<()> {
    match parse_args().command {
        Command::Parse { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading decklist {}", file.display()))?;
            let augmented: Vec<ParsedEntry> = pe_decklist::parse(&text)?
                .into_iter()
                .map(|entry| ParsedEntry {
                    commander: entry.is_commander(),
                    outside: entry.is_outside(),
                    entry,
                })
                .collect();
            println!("{}", facet_json::to_string_pretty(&augmented)?);
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
        Command::Sync { index, from, force } => sync::run(index.as_deref(), from.as_deref(), force),
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
    if let Some(note) = report::stale_index_note(&library) {
        eprintln!("{note}");
    }
    if let Some(note) = report::exclusion_note(&library) {
        eprintln!("{note}");
    }
    // Before the run rather than after it: a list that is illegal is usually
    // also the list that goes on to fail on something else, and the warning has
    // to survive that.
    let violations = legality::check(&library);
    if let Some(note) = report::legality_note(&violations) {
        eprintln!("{note}");
    }
    let source = std::fs::read_to_string(criteria_path)
        .with_context(|| format!("reading criteria {}", criteria_path.display()))?;
    // Hashed off the bytes that were actually about to run, before anything
    // gets a chance to normalise them.
    let criteria_sha256 = report::sha256_hex(source.as_bytes());

    let mut criteria = pe_js::Criteria::load(source)?;

    // Which queries the file uses and how deep into the game it looks are both
    // discovered by running it, so neither has to be declared. Both engines go
    // through the same loop, so they cannot drift apart in how they resolve a
    // criteria file — only in how they compute the answer.
    let answers: report::Answers = pe_js::with_discovery(
        &mut criteria,
        |queries| library.grouping_for(queries),
        |turns| draw_gaps(turns, on_the_draw),
        |grouping, gaps, criteria| -> Result<report::Answers> {
            let plan = criteria.plan();
            if simulate {
                let sampled = pe_sim::simulate(grouping, gaps, trials, seed, plan, criteria)?;
                Ok(report::Answers {
                    probabilities: sampled.proportions,
                    distributions: sampled.distributions,
                })
            } else {
                let exact = pe_criteria::run(grouping, gaps, plan, criteria)?;
                Ok(report::Answers {
                    probabilities: exact.probabilities.into_iter().map(|p| p.get()).collect(),
                    distributions: exact.distributions,
                })
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
        report::Questions {
            criteria: criteria.criteria(),
            expectations: criteria.expectations(),
        },
        &answers,
        report::Scenario {
            on_the_draw,
            sampled: simulate.then_some(report::Sampling { trials, seed }),
        },
        &library,
        queries,
        violations,
        report::Provenance {
            tool_version: env!("CARGO_PKG_VERSION"),
            index_updated_at: library.index_updated_at.clone(),
            deck_sha256: library.deck_sha256.clone(),
            criteria_sha256,
        },
    );
    println!("{}", facet_json::to_string_pretty(&report)?);
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
