//! The `progress-engine` command line interface.
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.

mod effects;
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
        /// TOML criteria file.
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
    let source = std::fs::read_to_string(criteria_path)
        .with_context(|| format!("reading criteria {}", criteria_path.display()))?;
    // Hashed off the bytes that were actually about to run, before anything
    // gets a chance to normalise them.
    let criteria_sha256 = report::sha256_hex(source.as_bytes());

    let mut criteria = pe_toml::Criteria::parse(&source, &criteria_path.display().to_string())?;

    // The file is data, so the whole query set and the whole turn horizon are
    // known before a single hand is enumerated. Nothing is discovered by running
    // anything, which is why a refusal below can say what the file asked for
    // rather than what it had learned before it gave up.
    //
    // Both engines get the same evaluator and the same grouping, so they cannot
    // drift apart in how they read a criteria file — only in how they compute
    // the answer.

    // Checked before anything is grouped, so a refused query can name the
    // question that asked for it. The grouping sees a list of strings and has
    // no idea which criterion each one came from; the criteria file does.
    for query in criteria.queries() {
        if let Err(e) = pe_scryfall::parse(query) {
            let asked_by = criteria.asked_by(query).unwrap_or("this file");
            anyhow::bail!("{asked_by}: in query {query:?}: {e}");
        }
    }

    // The standard library first, then the file's own, because last-wins is
    // what makes the prelude overridable without an override syntax.
    let effect_library =
        pe_toml::EffectLibrary::parse(pe_toml::STANDARD_LIBRARY, pe_toml::STANDARD_LIBRARY_ORIGIN)?
            .followed_by(criteria.effects().clone());
    let resolved = effects::resolve(&effect_library, &library, criteria.queries())?;
    for query in &resolved.unmatched {
        eprintln!("note: effect {query:?} matched no cards in this deck");
    }

    let queries: Vec<String> = criteria
        .queries()
        .iter()
        .cloned()
        .chain(resolved.queries.iter().cloned())
        .collect();
    let grouping = library.grouping_for(&queries, &resolved.marked)?;
    let schedule =
        pe_criteria::Schedule::build(criteria.horizon(), on_the_draw, resolved.effects.clone());
    let plan = criteria.plan();
    let answers: report::Answers = if simulate {
        let sampled = pe_sim::simulate(&grouping, &schedule, trials, seed, plan, &mut criteria)?;
        report::Answers {
            probabilities: sampled.proportions,
            distributions: sampled.distributions,
        }
    } else {
        let exact = pe_criteria::run(&grouping, &schedule, plan, &mut criteria)?;
        report::Answers {
            probabilities: exact.probabilities.into_iter().map(|p| p.get()).collect(),
            distributions: exact.distributions,
        }
    };

    // The file's own queries, not the effect library's. A standard library
    // entry that matches nothing is the ordinary case and is not the user's
    // question, so it does not get to look like a typo in their file.
    let query_matches = criteria
        .queries()
        .iter()
        .map(|q| {
            let cards = library.matching(q).unwrap_or(0);
            report::QueryMatch {
                query: q.clone(),
                cards,
            }
        })
        .collect();
    // Known from the file rather than from the run, exactly like the queries
    // above: a zone nothing routes a card into has to be reported even though
    // the enumeration never noticed anything odd about it.
    //
    // Reachability is a fact about this run rather than about the zone: the
    // graveyard is a real destination exactly when some loaded effect routes a
    // card there, and it is the same confident zero as before when none does.
    // Asked of the schedule the engine actually ran rather than of the resolved
    // list beside it, so the note cannot disagree with the enumeration.
    let reachable = pe_criteria::Reachable {
        graveyard: schedule.routes_to_graveyard(),
    };
    let zones = criteria
        .zones()
        .iter()
        .map(|&zone| report::ZoneUse {
            zone: zone.as_str(),
            reachable: reachable.includes(zone),
            asked_by: criteria
                .zone_asked_by(zone)
                .unwrap_or("this file")
                .to_string(),
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
        report::Breakdown {
            queries: query_matches,
            zones,
            effects: report::effects_applied(&resolved),
        },
        report::Provenance {
            tool_version: env!("CARGO_PKG_VERSION"),
            index_updated_at: library.index_updated_at.clone(),
            deck_sha256: library.deck_sha256.clone(),
            criteria_sha256,
            effect_library_sha256: report::sha256_hex(pe_toml::STANDARD_LIBRARY.as_bytes()),
        },
    );
    println!("{}", facet_json::to_string_pretty(&report)?);
    eprintln!("{}", report.human());
    if !report.ok {
        std::process::exit(1);
    }
    Ok(())
}
