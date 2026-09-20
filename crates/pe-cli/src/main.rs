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
            run_test(
                &deck,
                &criteria,
                draw,
                index.as_deref(),
                engine,
                trials,
                seed,
            )
        }
        Command::Sync { index, from, force } => sync::run(index.as_deref(), from.as_deref(), force),
    }
}

/// Which engine a run may use, resolved from the flags before anything runs.
///
/// Three states rather than two booleans, because `--simulate --exact` is the
/// fourth state and it does not mean anything. Resolving it at the boundary
/// leaves the run itself with no contradiction to arbitrate.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Engine {
    /// Enumerate, and refuse what does not fit. Yesterday's behaviour, and what
    /// the cross-engine agreement tests need: an oracle that quietly became an
    /// estimate would be checking the sampler against itself.
    ExactOnly,
    /// Enumerate what fits, estimate what does not, and say loudly which
    /// happened. The default.
    ExactOrSample,
    /// Sample everything, because somebody asked.
    Sample,
}

fn run_test(
    deck: &std::path::Path,
    criteria_path: &std::path::Path,
    on_the_draw: bool,
    index_path: Option<&std::path::Path>,
    engine: Engine,
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
        let asked_by = criteria.asked_by(query).unwrap_or("this file");
        let parsed = match pe_scryfall::parse(query) {
            Ok(parsed) => parsed,
            Err(e) => anyhow::bail!("{asked_by}: in query {query:?}: {e}"),
        };
        // A query that parses can still be one this index cannot answer. Only
        // the index knows which oracle tags it carries, so this is the first
        // point where the question and the data are in the same place — and it
        // is still before anything is grouped, so the refusal names the
        // criterion that asked rather than a position in a query list.
        if let Some(gap) = parsed.tag_gap(&library.index_tags) {
            anyhow::bail!(
                "{asked_by}: in query {query:?}: {}",
                report::tag_gap_refusal(&gap, &library)
            );
        }
        // The same seam for `kw:`, and a separate check rather than a second
        // arm of the one above, because the two indexes are authoritative about
        // different things. An index carrying no tags is a fact it asserts about
        // itself; an index listing no keywords is a fact it never recorded, so
        // it refuses nothing — `unknown_keywords` already knows that and
        // returns nothing there rather than calling every keyword a typo.
        let unknown = parsed.unknown_keywords(&library.index_keywords);
        if !unknown.is_empty() {
            anyhow::bail!(
                "{asked_by}: in query {query:?}: {}",
                report::unknown_keyword_refusal(&unknown)
            );
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
    // Not a refusal, unlike the same gap in a criteria query above: nobody
    // asked for these. The standard library autoloads, so refusing the run
    // would make a tagless index answer nothing at all rather than answer the
    // question that was actually asked — but it loads *and moves numbers*, so
    // it does not get to fall silent either.
    for note in report::tag_blind_notes(&resolved, &library) {
        eprintln!("{note}");
    }

    // What the mana model needs of this run, checked before anything is
    // enumerated and refused by name where it is not there. Every one of these
    // would otherwise be answered — wrongly, and in the flattering direction:
    // an index with no tapland tag reports every land as making mana the turn
    // it lands, and a battlefield count of a spell reports "drawn" under
    // another name.
    // Named against the file as well as the question, the way a parse refusal
    // is: a caller running several criteria files needs to know which one it
    // was before it needs to know which criterion.
    let origin = criteria_path.display();
    let mana = match criteria.mana_question() {
        None => library::ManaDetail::Ignored,
        Some(asked_by) => {
            for (query, asked_by) in criteria.battlefield_queries() {
                let spells = library.non_lands_matching(query)?;
                if !spells.is_empty() {
                    anyhow::bail!(
                        "{origin}: {asked_by}: {}",
                        report::battlefield_refusal(query, &spells)
                    );
                }
            }
            // One land drop a turn is a decision, and a live effect already
            // spends it: the walk plays the deepest-looking land you hold
            // because with no mana model there was nothing else to choose by.
            // Answering a mana question beside it would be a second policy
            // deciding the same drop, and the two would disagree on exactly the
            // hands that matter.
            if !resolved.effects.is_empty() {
                anyhow::bail!(
                    "{origin}: {asked_by}: {}",
                    report::mana_beside_effects_refusal()
                );
            }
            match criteria.casts() {
                None => library::ManaDetail::Ignored,
                Some(asked_by) => {
                    if let Some(refusal) = report::cannot_price_mana(&library) {
                        anyhow::bail!("{origin}: {asked_by}: {refusal}");
                    }
                    library::ManaDetail::Modelled
                }
            }
        }
    };

    let queries: Vec<String> = criteria
        .queries()
        .iter()
        .cloned()
        .chain(resolved.queries.iter().cloned())
        .collect();
    let grouping = library.grouping_for(&queries, &resolved.marked, mana)?;
    let schedule =
        pe_criteria::Schedule::build(criteria.horizon(), on_the_draw, resolved.effects.clone());
    let plan = criteria.plan();
    // Exact first, always, for every question that fits: either enumeration
    // answered it, or here is why this run has to sample instead. There is no
    // third outcome and no way to reach the sampler without naming one of the
    // two reasons, which is what keeps the warning downstream from being
    // something a code path can forget to print.
    let enumerated: std::result::Result<pe_criteria::Outcomes, report::WhySampled> = match engine {
        Engine::Sample => Err(report::WhySampled::Requested),
        Engine::ExactOnly | Engine::ExactOrSample => {
            match pe_criteria::run(&grouping, &schedule, plan, &mut criteria) {
                Ok(exact) => Ok(exact),
                // Only this one refusal falls back. Every other way a run can
                // be refused is a question the sampler would answer no better:
                // an empty library, a hand bigger than the deck, and a mass
                // that did not sum to one are all facts about what was asked
                // rather than about how expensive it was to enumerate.
                Err(pe_criteria::RunError::TooWide { paths, groups, .. })
                    if engine == Engine::ExactOrSample =>
                {
                    Err(report::WhySampled::TooWide { paths, groups })
                }
                Err(e) => return Err(e.into()),
            }
        }
    };
    let (answers, sampled) = match enumerated {
        Ok(exact) => (
            report::Answers {
                probabilities: exact.probabilities.into_iter().map(|p| p.get()).collect(),
                distributions: exact.distributions,
            },
            None,
        ),
        Err(why) => {
            let sampled =
                pe_sim::simulate(&grouping, &schedule, trials, seed, plan, &mut criteria)?;
            (
                report::Answers {
                    probabilities: sampled.proportions,
                    distributions: sampled.distributions,
                },
                Some(report::Sampling { trials, seed, why }),
            )
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
        battlefield: library.has_lands(),
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
            sampled,
        },
        &library,
        report::Breakdown {
            queries: query_matches,
            zones,
            effects: report::effects_applied(&resolved),
            // Only where the run actually priced mana. A deck full of
            // shocklands answering a question about the graveyard assumed
            // nothing about any of them.
            assumed_tapped: match mana {
                library::ManaDetail::Ignored => Vec::new(),
                library::ManaDetail::Modelled => library.conditional_taplands(),
            },
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
