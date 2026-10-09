//! The `gauntlet` command line interface.
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.
//!
//! This file is argument parsing and nothing else; what each command does lives
//! in the `gauntlet_cli` library beside it.

use std::path::PathBuf;

use anyhow::{Context, Result};
use facet::Facet;
use figue::{self as args, DriverError, Shell};

use gauntlet_cli::{sync, Engine};

/// Draw-probability tests for Magic: The Gathering decklists.
#[derive(Facet)]
struct Cli {
    #[facet(args::subcommand)]
    command: Command,
    #[facet(flatten)]
    _builtins: Builtins,
}

/// figue's `FigueBuiltins`, less the two this binary cannot honour.
///
/// `--export-jsonschemas` writes one schema per config root, and this binary
/// has none, so it printed "Wrote JSON Schema files:" over an empty list and
/// exited 1. A flag that writes nothing is a flag that lies, so it is not
/// offered; nor is `--html-help`, which wrote a page and reported that as a
/// failure.
#[derive(Facet, Default)]
struct Builtins {
    /// Show help message and exit.
    #[facet(args::named, args::short = 'h', args::help, default)]
    help: bool,
    /// Show version and exit.
    #[facet(args::named, args::short = 'V', args::version, default)]
    version: bool,
    /// Generate shell completions.
    #[facet(args::named, args::completions, default)]
    completions: Option<Shell>,
}

#[derive(Facet)]
#[repr(u8)]
enum Command {
    /// Parse a decklist and emit it as JSON.
    ///
    /// This is the canonical decklist parser; other tools shell out to it so
    /// there is exactly one definition of what a decklist is.
    Parse {
        /// Deck file: a `.deck.toml`, or Archidekt's text export.
        #[facet(args::positional)]
        file: PathBuf,
        /// Card index naming a `.deck.toml`'s printings (default: the synced one).
        #[facet(args::named, default)]
        index: Option<PathBuf>,
    },
    /// Convert Archidekt's text export into a `.deck.toml`, printed to stdout.
    ///
    /// Archidekt's categories become declared ones, typed by its naming
    /// conventions, and each printing is written with its name as a comment.
    Import {
        /// Decklist file in Archidekt format.
        #[facet(args::positional)]
        file: PathBuf,
    },
    /// Run a criteria file against a deck.
    Test {
        /// Deck file: a `.deck.toml`, or Archidekt's text export.
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
        /// Refuse questions too wide to enumerate instead of estimating them.
        ///
        /// The default answers them by sampling and says so. This is for
        /// anyone who would rather have no answer than an approximate one.
        #[facet(args::named, default)]
        exact: bool,
        /// Hands to deal when sampling, however the run got there.
        #[facet(args::named, default = DEFAULT_TRIALS)]
        trials: u32,
        /// Seed, so a sampled run is reproducible.
        #[facet(args::named, default = 0)]
        seed: u64,
        /// Scryfall index, as built by `gauntlet sync`.
        #[facet(args::named, default)]
        index: Option<PathBuf>,
    },
    /// Build the card index and the copy of Scryfall from Scryfall's bulk data.
    ///
    /// `test` needs to know what a card is, and this is where that comes from.
    /// Run it once, and again when you want newer cards; nothing else
    /// downloads.
    Sync {
        /// Where to write the index. Defaults to the path `test` reads.
        #[facet(args::named, default)]
        index: Option<PathBuf>,
        /// Build the index from an Oracle Cards file on disk, offline.
        ///
        /// Makes no copy unless `--cards-from` and `--tags-from` come too.
        #[facet(args::named, default)]
        from: Option<PathBuf>,
        /// Directory for the copy (default: `scryfall-copy/` by the index).
        #[facet(args::named, default)]
        copy: Option<PathBuf>,
        /// Make the copy from a Default Cards file already on disk.
        #[facet(args::named, default)]
        cards_from: Option<PathBuf>,
        /// Make the copy from an Oracle Tags file already on disk.
        #[facet(args::named, default)]
        tags_from: Option<PathBuf>,
        /// Also write the copy's text here, uncompressed, as `decks/` keeps it.
        #[facet(args::named, default)]
        snapshot: Option<PathBuf>,
        /// Rebuild even when Scryfall has nothing newer than what is kept.
        #[facet(args::named, default)]
        force: bool,
    },
}

/// Hands to deal when nobody said how many.
///
/// It is the default for `--simulate` and it is what a fallback run uses, which
/// is one number on purpose: a question that fell back was not asked to sample,
/// so nobody picked a trial count for it, and a second constant would let the
/// two drift apart for no reason a reader could reconstruct.
///
/// Chosen against the finest threshold a criteria file can state. Thresholds
/// are written and printed to one decimal place — `needs 70.0%` — so 0.1
/// percentage points is the smallest difference anybody can ask for. The
/// standard error of a proportion peaks at p = 0.5, where 200,000 hands put it
/// at 0.11pp, and it is under 0.09pp outside the 40-60% band where most
/// thresholds sit. So the error bar is about one threshold step wide rather
/// than a multiple of it. Ten times the hands would buy one more digit at ten
/// times the wait, on exactly the questions that landed here because they were
/// already the expensive ones.
///
/// Measured rather than assumed: 200,000 hands of a 99-card library to turn 6
/// is 0.7s in release, and 1.0s on a 60-card deck routing a card a turn to the
/// graveyard. The trial count is not what makes a wide question slow.
const DEFAULT_TRIALS: u32 = 200_000;

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
    entry: chip_decklist::Entry,
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
        matches!(a.as_str(), "-h" | "--help" | "-V" | "--version") || a.starts_with("--completions")
    });

    let config = match figue::builder::<Cli>() {
        Ok(builder) => builder
            .cli(|cli| cli.args(std::env::args().skip(1)))
            .help(|help| {
                help.program_name("gauntlet")
                    .version(env!("CARGO_PKG_VERSION"))
            })
            .build(),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    match figue::Driver::new(config).run().into_result() {
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
        // Asked for, so stdout and success, like help. Falling through to the
        // arm below printed them to stderr and exited 1.
        Err(DriverError::Version { text }) => {
            println!("{text}");
            std::process::exit(0);
        }
        Err(DriverError::Completions { script }) => {
            println!("{script}");
            std::process::exit(0);
        }
        Err(other) => {
            eprintln!("{other}");
            std::process::exit(other.exit_code().max(1));
        }
    }
}

fn main() -> Result<()> {
    match parse_args().command {
        Command::Parse { file, index } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading decklist {}", file.display()))?;
            let text = gauntlet_cli::archidekt_text(&file, &text, index.as_deref())?;
            let augmented: Vec<ParsedEntry> = chip_decklist::parse(&text)?
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
        Command::Import { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading decklist {}", file.display()))?;
            print!("{}", chip_decklist::deck::import_archidekt(&text)?);
            Ok(())
        }
        Command::Test {
            deck,
            criteria,
            draw,
            simulate,
            exact,
            trials,
            seed,
            index,
        } => {
            // The two flags are answers to the same question, and a run that
            // honoured both would have to pick one silently.
            if simulate && exact {
                anyhow::bail!(
                    "--simulate and --exact ask for opposite things: one samples every \
                     question, the other refuses\nto sample any. Pass neither to enumerate \
                     what fits and estimate what does not."
                );
            }
            let engine = if simulate {
                Engine::Sample
            } else if exact {
                Engine::ExactOnly
            } else {
                Engine::ExactOrSample
            };
            let ok = gauntlet_cli::run_test(
                &deck,
                &criteria,
                draw,
                index.as_deref(),
                engine,
                trials,
                seed,
            )?;
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Sync {
            index,
            from,
            copy,
            cards_from,
            tags_from,
            snapshot,
            force,
        } => sync::run(&sync::Args {
            index: index.as_deref(),
            from: from.as_deref(),
            copy: copy.as_deref(),
            cards_from: cards_from.as_deref(),
            tags_from: tags_from.as_deref(),
            snapshot: snapshot.as_deref(),
            force,
        }),
    }
}
