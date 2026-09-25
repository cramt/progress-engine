//! The `gauntlet` command line interface.
//!
//! Output discipline matches the tooling this grew alongside: JSON on stdout,
//! the human verdict on stderr, and an exit code that reflects it. A caller that
//! pipes stdout through `jq '{some,fields}'` can drop the failing field from the
//! JSON — that has happened — but stderr still lands in front of whoever is
//! reading.

mod casting;
mod effects;
mod landdrop;
mod library;
mod narrow;
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
        /// Scryfall index, as built by `gauntlet sync`.
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

/// The knobs that decide how a question gets answered, as opposed to what it
/// asks.
///
/// One struct rather than three arguments because two of them are only
/// meaningful together: a trial count without a seed does not identify the
/// hands, and a seed without one does not reproduce them.
#[derive(Clone, Copy)]
struct Run {
    engine: Engine,
    trials: u32,
    seed: u64,
}

/// Everything one file's questions came to: the answers, how they were
/// sampled where they were, and how the run enumerated to get them.
///
/// One struct rather than a tuple because the third one is about the first
/// two and a positional triple would leave a caller to remember which is
/// which.
struct Answered {
    answers: report::Answers,
    sampled: Option<report::Sampling>,
    enumerations: Vec<report::Enumeration>,
}

/// Answer every question in the file, each on the narrowest enumeration that
/// can answer it.
///
/// This is [#31](https://github.com/cramt/progress-engine/issues/31) at the
/// call site. The file used to be one enumeration, sized as the join of what
/// every question in it needs, so a `can_cast` clause over a Commander
/// manabase — seventeen groups, 1.2 billion compositions at turn four — took
/// every other criterion in the file over the ceiling with it and the whole
/// run was estimated. Each class is now enumerated on its own, and only the
/// classes still over the ceiling fall back.
///
/// The sampler runs **once**, over the whole file and the un-narrowed
/// grouping, and its answers are used only where enumeration refused. Keeping
/// it un-narrowed is what keeps it a second implementation: an oracle that
/// walked the same narrowed grouping as the engine it checks would be
/// agreeing with itself.
///
/// Every class also reports the width it cost, walked or not, which is how a
/// reader reproduces the figures this project quotes about its own narrowings
/// from a run they performed themselves.
fn answer(
    run: Run,
    classes: &[narrow::Class],
    grouping: &gauntlet_criteria::Grouping,
    schedule: &gauntlet_criteria::Schedule,
    plan: gauntlet_criteria::Plan,
    criteria: &mut gauntlet_toml::Criteria,
) -> Result<Answered> {
    let mut probabilities: Vec<Option<f64>> = vec![None; plan.criteria];
    let mut distributions: Vec<Option<chip_stats::Distribution>> = vec![None; plan.expectations];
    let mut estimated = report::Estimated::none(plan);
    let mut enumerations: Vec<report::Enumeration> = Vec::with_capacity(classes.len());
    // Taken before the loop, because the evaluator is borrowed mutably inside
    // it: the names are what a class's entry is filed under, and a position in
    // a plan is not something a reader can check against their own file.
    let criteria_names: Vec<String> = criteria.criteria().iter().map(|c| c.name.clone()).collect();
    let expectation_names: Vec<String> = criteria
        .expectations()
        .iter()
        .map(|e| e.name.clone())
        .collect();
    // Why the run sampled at all, and — where it was the ceiling — the widest
    // class that hit it, because that is the number a caller deciding whether
    // to narrow its question needs.
    let mut why: Option<report::WhySampled> = None;

    if run.engine == Engine::Sample {
        why = Some(report::WhySampled::Requested);
        estimated.criteria.fill(true);
        estimated.expectations.fill(true);
    }
    for class in classes {
        let answering = class
            .answering(plan)
            .context("a class named a question this file does not hold")?;
        let narrowed = class.grouping(grouping);
        let walk = class.schedule(schedule);
        let groups = narrowed.group_sizes().len();
        let mut enumerated = report::Enumeration {
            criteria: named(&criteria_names, answering.criteria()),
            expectations: named(&expectation_names, answering.expectations()),
            queries: class
                .queries(grouping)
                .into_iter()
                .map(str::to_string)
                .collect(),
            turns: class.turns().to_vec(),
            reading: match class.reading() {
                gauntlet_criteria::Reading::Cumulative => "cumulative",
                gauntlet_criteria::Reading::PerTurn => "per-turn",
            },
            pips: class.pips().map(gauntlet_criteria::Palette::symbols),
            groups,
            compositions: gauntlet_criteria::compositions(groups, walk.gaps()) as f64,
            method: "exact",
        };
        if run.engine == Engine::Sample {
            enumerated.method = "sampled";
            enumerations.push(enumerated);
            continue;
        }
        match gauntlet_criteria::run_answering(&narrowed, &walk, &answering, criteria) {
            Ok(exact) => {
                for (&i, p) in answering.criteria().iter().zip(exact.probabilities) {
                    probabilities[i] = Some(p.get());
                }
                for (&i, d) in answering.expectations().iter().zip(exact.distributions) {
                    distributions[i] = Some(d);
                }
            }
            // Only this one refusal falls back. Every other way a run can be
            // refused is a question the sampler would answer no better: an
            // empty library, a hand bigger than the deck, and a mass that did
            // not sum to one are all facts about what was asked rather than
            // about how expensive it was to enumerate.
            Err(gauntlet_criteria::RunError::TooWide { paths, groups, .. })
                if run.engine == Engine::ExactOrSample =>
            {
                let wider = !matches!(why, Some(report::WhySampled::TooWide { paths: p, .. }) if p >= paths);
                if wider {
                    why = Some(report::WhySampled::TooWide { paths, groups });
                }
                for &i in answering.criteria() {
                    estimated.criteria[i] = true;
                }
                for &i in answering.expectations() {
                    estimated.expectations[i] = true;
                }
                enumerated.method = "sampled";
            }
            Err(e) => return Err(e.into()),
        }
        enumerations.push(enumerated);
    }

    let sampled = match why {
        None => None,
        Some(why) => {
            let sampled =
                gauntlet_sim::simulate(grouping, schedule, run.trials, run.seed, plan, criteria)?;
            for (i, estimated) in estimated.criteria.iter().enumerate() {
                if *estimated {
                    probabilities[i] = Some(sampled.proportions[i]);
                }
            }
            for (i, estimated) in estimated.expectations.iter().enumerate() {
                if *estimated {
                    distributions[i] = Some(sampled.distributions[i].clone());
                }
            }
            Some(report::Sampling {
                trials: run.trials,
                seed: run.seed,
                why,
            })
        }
    };

    // A hole here would be a question no class claimed, and it would print as
    // a confident zero. The partition covers every question by construction,
    // so this is a bug rather than a state, and it says so.
    let missing = "a question this run answered with neither engine";
    Ok(Answered {
        answers: report::Answers {
            probabilities: probabilities
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .context(missing)?,
            distributions: distributions
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .context(missing)?,
            estimated,
        },
        sampled,
        enumerations,
    })
}

/// A class's questions, by name, in the order the report prints them.
///
/// Nothing is dropped here: `Answering::some` refused any index outside the
/// same plan these names were taken from, so every one of them lands.
fn named(names: &[String], which: &[usize]) -> Vec<String> {
    which
        .iter()
        .filter_map(|&i| names.get(i).cloned())
        .collect()
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

    let mut criteria =
        gauntlet_toml::Criteria::parse(&source, &criteria_path.display().to_string())?;

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
        let parsed = match chip_scryfall::parse(query) {
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

    // The land-drop priority, resolved before the effects so that its queries
    // sit directly behind the criteria file's own and the effect library's
    // grouping bits still land where `effects::resolve` puts them.
    landdrop::check(criteria.land_drop(), &library)?;
    let land_drop = match criteria.land_drop() {
        [] => None,
        prefer => Some(landdrop::resolve(prefer, &library, criteria.queries())?),
    };
    let mut asked: Vec<String> = criteria
        .queries()
        .iter()
        .cloned()
        .chain(land_drop.iter().flat_map(|p| p.queries.iter().cloned()))
        .collect();
    // The casting priority next, on the same terms and for the same reason:
    // its queries sit behind everything already asked for, so no bit a clause
    // holds moves. It is resolved here rather than later because pricing it is
    // where a cost this engine cannot pay gets refused, and that has to happen
    // before anything is grouped.
    casting::check(criteria.casting(), &library)?;
    let casting = match criteria.casting() {
        [] => None,
        prefer => Some(casting::resolve(prefer, &library, &asked)?),
    };
    asked.extend(casting.iter().flat_map(|p| p.queries.iter().cloned()));
    if let Some(policy) = &casting {
        for query in &policy.unmatched {
            eprintln!("note: casting preference {query:?} matches no castable card in this deck");
        }
        for (query, lands) in &policy.lands {
            eprintln!(
                "note: casting preference {query:?} also matches {} you play rather than cast: \
                 {}.\n      A land arrives on a land drop, so the priority ignores them — \
                 [land_drop] is where that decision lives.",
                if lands.len() == 1 { "a land" } else { "lands" },
                lands.join(", ")
            );
        }
    }
    // A `cast` clause with nobody to cast is the budget's version of the land
    // drop's two claimants, and it is refused for the same reason: which
    // spells you cast out of one turn's mana is a decision the pilot makes,
    // and a tool that picked would be reporting a line nobody chose.
    if let Some(asked_by) = criteria.counts_castings() {
        if casting.is_none() {
            anyhow::bail!(
                "{}: {asked_by}: {}",
                criteria_path.display(),
                report::casting_without_priority()
            );
        }
    }
    if let Some(policy) = &land_drop {
        for query in &policy.unmatched {
            eprintln!("note: land-drop preference {query:?} matches no land in this deck");
        }
        for (query, spells) in &policy.non_lands {
            eprintln!(
                "note: land-drop preference {query:?} also matches {} you cannot play as a land \
                 drop: {}.\n      A land drop plays lands, so the priority ignores them.",
                if spells.len() == 1 { "a card" } else { "cards" },
                spells.join(", ")
            );
        }
    }

    // The standard library first, then the file's own, because last-wins is
    // what makes the prelude overridable without an override syntax.
    let effect_library = gauntlet_toml::EffectLibrary::parse(
        gauntlet_toml::STANDARD_LIBRARY,
        gauntlet_toml::STANDARD_LIBRARY_ORIGIN,
    )?
    .followed_by(criteria.effects().clone());
    let resolved = effects::resolve(&effect_library, &library, &asked)?;
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
    for (effect, query) in resolved
        .applied
        .iter()
        .flat_map(|a| a.fetch_misses.iter().map(move |q| (&a.matches, q)))
    {
        eprintln!(
            "note: effect {effect:?} would fetch {query:?}, which matches no card in this deck"
        );
    }

    // What a tutor needs of the run that declares it, refused by name before
    // anything is enumerated. Each of these would otherwise be answered — and
    // in the flattering direction, because a fetch that fires puts a card in
    // your hand.
    let origin = criteria_path.display();
    for (applied, effect) in resolved
        .applied
        .iter()
        .filter(|a| a.live)
        .zip(&resolved.effects)
    {
        let Some(fetch) = &effect.fetch else { continue };
        match effect.trigger {
            // A land-drop fetch happens *on* the drop and replaces the land
            // that made it, so a run that cannot say which land it played
            // cannot say what it fetched either. Same shape of refusal as a
            // mana question beside a live effect, and the same remedy.
            gauntlet_criteria::Trigger::LandDrop if land_drop.is_none() => anyhow::bail!(
                "{origin}: {}",
                report::fetch_without_land_drop(&applied.matches)
            ),
            // And a cast fetch fires when the declared line casts the card, so
            // with no line there is nothing to fire it.
            gauntlet_criteria::Trigger::Cast if casting.is_none() => anyhow::bail!(
                "{origin}: {}",
                report::fetch_without_casting(&applied.matches)
            ),
            _ => {}
        }
        // A delayed fetch is the other way onto the battlefield, and it is
        // refused the opposite half: a Saga puts an artifact beside itself,
        // and a land arriving that way is a land nobody knows the tapped-ness
        // of. It is checked here, before the fetchland's rule below, because
        // the two are about different cards arriving for different reasons.
        if fetch.to == gauntlet_criteria::Fetched::Battlefield && effect.delay.is_some() {
            for query in applied.fetch.iter().flat_map(|(prefer, _)| prefer) {
                let lands = library.lands_matching(query)?;
                if lands > 0 {
                    anyhow::bail!(
                        "{origin}: effect {:?}: {}",
                        applied.matches,
                        report::delayed_fetch_land_refusal(query, lands)
                    );
                }
            }
        } else if fetch.to == gauntlet_criteria::Fetched::Battlefield {
            for query in applied.fetch.iter().flat_map(|(prefer, _)| prefer) {
                let spells = library.non_lands_matching(query)?;
                if !spells.is_empty() {
                    anyhow::bail!(
                        "{origin}: effect {:?}: {}",
                        applied.matches,
                        report::fetch_battlefield_refusal(query, &spells)
                    );
                }
            }
        }
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
    // A declared casting priority is a mana question whether or not any clause
    // asks one, because the budget spends the pool: what was cast decides what
    // is left in hand, and every count in the file reads that.
    let table = "[casting]";
    if let Some(asked_by) = criteria
        .mana_question()
        .or_else(|| casting.as_ref().map(|_| table))
    {
        // What a delayed fetch puts onto the battlefield is on the
        // battlefield, and counted there: a Lantern off Urza's Saga's third
        // chapter is the one way a spell arrives that this walk models.
        let delivered: Vec<&str> = resolved
            .applied
            .iter()
            .filter(|a| a.live && a.delay.is_some())
            .filter_map(|a| a.fetch.as_ref())
            .filter(|(_, to)| {
                *to == gauntlet_toml::fetched_name(gauntlet_criteria::Fetched::Battlefield)
            })
            .flat_map(|(prefer, _)| prefer.iter().map(String::as_str))
            .collect();
        for (query, asked_by) in criteria.battlefield_queries() {
            let spells = library.stranded_matching(query, &delivered, criteria.casting())?;
            if !spells.is_empty() {
                anyhow::bail!(
                    "{origin}: {asked_by}: {}",
                    report::battlefield_refusal(query, &spells, &library.back_face_lands(query)?)
                );
            }
        }
        // One land drop a turn is a decision, and a live effect already
        // spends it: with no declared priority the walk plays the
        // deepest-looking land you hold, because there was nothing else to
        // choose by. Answering a mana question beside that would be a
        // second policy deciding the same drop, and the two would disagree
        // on exactly the hands that matter. Declaring the priority makes
        // them one decision, which is the remedy the refusal names.
        // Only an effect that fires *on the drop* is a second claimant on it.
        // A tutor that fires when a spell is cast spends the mana, not the
        // land drop, and the budget has already said which spells those are.
        let on_the_drop = resolved
            .effects
            .iter()
            .any(|e| e.trigger == gauntlet_criteria::Trigger::LandDrop);
        if on_the_drop && land_drop.is_none() {
            anyhow::bail!(
                "{origin}: {asked_by}: {}",
                report::mana_beside_effects_refusal()
            );
        }
    }
    let no_costs: [Option<gauntlet_criteria::Demand>; 0] = [];
    let mana = match criteria.casts().or_else(|| casting.as_ref().map(|_| table)) {
        None => library::ManaDetail::Ignored,
        Some(asked_by) => {
            if let Some(refusal) = report::cannot_price_mana(&library) {
                anyhow::bail!("{origin}: {asked_by}: {refusal}");
            }
            // A fetched land is on the battlefield — countable, and counted —
            // but what it *taps for* on the turn it arrives is not readable
            // from any tag this index carries: a Scalding Tarn puts its Island
            // down untapped and a Terramorphic Expanse does not, and
            // `otag:fetchland` holds both. So the library it thinned is
            // answered and the pool it filled is refused, rather than answered
            // in whichever direction happens to flatter.
            if let Some(named) = resolved
                .applied
                .iter()
                .filter(|a| a.live)
                .zip(&resolved.effects)
                // A delayed fetch was refused above if it could find a land,
                // so what it puts on the battlefield makes no mana and this
                // question is not about it.
                .find(|(_, e)| {
                    e.delay.is_none()
                        && e.fetch
                            .as_ref()
                            .is_some_and(|f| f.to == gauntlet_criteria::Fetched::Battlefield)
                })
                .map(|(a, _)| a.matches.as_str())
            {
                anyhow::bail!(
                    "{origin}: {asked_by}: {}",
                    report::mana_beside_a_fetched_land(named)
                );
            }
            library::ManaDetail::Modelled {
                castable: casting.as_ref().map_or(&no_costs, |c| c.costs.as_slice()),
            }
        }
    };

    let queries: Vec<String> = asked
        .iter()
        .cloned()
        .chain(resolved.queries.iter().cloned())
        .collect();
    let grouping = library.grouping_for(&queries, &resolved.marked, mana)?;
    let schedule = gauntlet_criteria::Schedule::build(
        criteria.horizon(),
        on_the_draw,
        resolved.effects.clone(),
        gauntlet_criteria::Policies {
            land_drop: land_drop.as_ref().map(|p| p.policy.clone()),
            casting: casting.as_ref().map(|p| p.policy.clone()),
        },
    );
    let plan = criteria.plan();
    // Refused about the run rather than about one of its classes. Narrowing
    // asks a smaller question than the file did, so a class about turn 2 would
    // happily answer against a library the file's own horizon could never be
    // dealt from — turning a refusal into a number by changing the question.
    gauntlet_criteria::feasible::<gauntlet_toml::EvalError>(&grouping, &schedule)?;

    // What the walk reads for itself, so every class keeps it however little
    // its own clauses care: a live effect decides which zone a card ends up
    // in, and a declared priority decides which land was played.
    let shared = narrow::Shared {
        effects: (!resolved.effects.is_empty()).then(|| narrow::Effects {
            queries: resolved.effects.iter().fold(0u64, |bits, effect| {
                let destination = match effect.route {
                    gauntlet_criteria::Route::Matching(query) => 1u64 << query,
                    gauntlet_criteria::Route::Everything | gauntlet_criteria::Route::Nowhere => 0,
                };
                // And what a tutor would go and get, for the same reason: it
                // decides which card left the library, so every count in the
                // run depends on which groups the priority can tell apart.
                let fetched = effect
                    .fetch
                    .iter()
                    .flat_map(|f| &f.prefer)
                    .fold(0u64, |b, &q| b | 1u64 << q);
                bits | 1u64 << effect.matched_by | destination | fetched
            }),
            on_the_drop: resolved
                .effects
                .iter()
                .any(|e| e.trigger == gauntlet_criteria::Trigger::LandDrop),
        }),
        land_drop: land_drop
            .as_ref()
            .map(|p| p.policy.tiers().fold(0u64, |bits, q| bits | 1u64 << q)),
        // The budget is the widest of the three: it reads which spells the
        // line names *and* what the manabase makes, on every class, because a
        // spell it paid for is one that left the hand.
        casting: casting.as_ref().map(|p| narrow::Casting {
            queries: p.policy.tiers().fold(0u64, |bits, q| bits | 1u64 << q),
            demands: p.demands,
        }),
    };
    let classes = narrow::partition(&criteria.reads(), &shared);

    let Answered {
        answers,
        sampled,
        enumerations,
    } = answer(
        Run {
            engine,
            trials,
            seed,
        },
        &classes,
        &grouping,
        &schedule,
        plan,
        &mut criteria,
    )?;

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
    let reachable = gauntlet_criteria::Reachable {
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
            enumerations,
            // Only where the run actually priced mana. A deck full of
            // shocklands answering a question about the graveyard assumed
            // nothing about any of them.
            assumed_tapped: match mana {
                library::ManaDetail::Ignored => Vec::new(),
                library::ManaDetail::Modelled { .. } => library.conditional_taplands(),
            },
            // Read off the schedule the engine actually walked, like the zone
            // reachability above, so the note cannot claim a policy the
            // enumeration did not use.
            land_drop: schedule.land_drop().map(|_| report::LandDropUse {
                prefer: land_drop
                    .as_ref()
                    .map_or_else(Vec::new, |p| p.prefer.clone()),
                then: "any other land",
                tie_break: gauntlet_criteria::LandDropPolicy::TIE_BREAK,
            }),
            // Read off the schedule for the same reason, and printed even
            // where no clause counts a casting: the line decides what is left
            // in hand, so it is an input to every number below it.
            casting: schedule.casting().map(|_| report::CastingUse {
                prefer: casting.as_ref().map_or_else(Vec::new, |p| p.prefer.clone()),
                then: gauntlet_criteria::CastingPolicy::THEN,
                tie_break: gauntlet_criteria::CastingPolicy::TIE_BREAK,
            }),
        },
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
    if !report.ok {
        std::process::exit(1);
    }
    Ok(())
}
