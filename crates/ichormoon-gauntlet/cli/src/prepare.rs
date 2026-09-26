//! Preparing a run: everything between a parsed criteria file and the first
//! hand enumerated.
//!
//! One entry point, [`prepare`], takes the deck's library, the criteria file
//! and the seat, and hands back either a [`PreparedRun`] or the reason there
//! is none. Behind it: every query checked against the index, each declared
//! priority resolved against the deck, the effect library resolved, every
//! refusal that can be made before anything is enumerated, the mana detail
//! chosen, the grouping and the schedule built, the run's feasibility checked,
//! its questions partitioned into classes, and — where the file declared an
//! objective — the strategy chosen that every answer is played under.
//!
//! The notes a run prints on the way are returned as data, in the order they
//! arose, beside the outcome rather than inside it: a refusal still has the
//! notes that came before it, and the caller prints both. Nothing here writes
//! to stderr.
//!
//! The file is data, so the whole query set and the whole turn horizon are
//! known before a single hand is enumerated. Nothing is discovered by running
//! anything, which is why a refusal here can say what the file asked for
//! rather than what it had learned before it gave up.
//!
//! Both engines get the same evaluator and the same grouping, so they cannot
//! drift apart in how they read a criteria file — only in how they compute the
//! answer.

use std::collections::HashMap;

use anyhow::Result;
use gauntlet_criteria::{Grouping, Plan, Schedule, Table};
use gauntlet_toml::Criteria;

use crate::library::{self, Library};
use crate::{answer, casting, effects, landdrop, mulligan, narrow, optimise, report};

/// What [`prepare`] came to: the notes it made, and the run or the reason
/// there is none.
///
/// The notes sit beside the outcome rather than inside it because a refused
/// run has notes too — a mulligan query matching nothing is said before the
/// refusal that follows it — and printing them is the caller's business.
pub struct Preparation {
    /// Everything worth telling a reader that is not a refusal, in the order
    /// it arose. One entry per `note:` block, each possibly several lines.
    pub notes: Vec<String>,
    /// The prepared run, or the refusal (or failure) that stopped it.
    pub run: Result<PreparedRun>,
}

/// A run ready to answer: the grouping and schedule every class is narrowed
/// from, the classes themselves, and what the report needs to know about each
/// declared priority and strategy.
pub struct PreparedRun {
    pub(crate) grouping: Grouping,
    pub(crate) schedule: Schedule,
    pub(crate) plan: Plan,
    pub(crate) classes: Vec<narrow::Class>,
    /// What the optimiser walked for each class holding an objective
    /// criterion, where its strategy is the one played. Taken by the run that
    /// answers, which would otherwise walk them again.
    pub(crate) tables: HashMap<usize, Table>,
    resolved: effects::Resolved,
    land_drop: Option<landdrop::Resolved>,
    casting: Option<casting::Resolved>,
    mulligan: Option<mulligan::Resolved>,
    chose: Option<optimise::Chose>,
    /// Whether the run priced mana, which is when it assumed anything about a
    /// conditional tapland.
    mana_modelled: bool,
}

impl PreparedRun {
    /// How many classes the run's questions were partitioned into.
    pub fn classes(&self) -> usize {
        self.classes.len()
    }
}

/// Prepare `criteria` to be answered against `library`, on the draw where
/// `on_the_draw`. `origin` is how refusals name the criteria file.
pub fn prepare(
    library: &Library,
    criteria: &mut Criteria,
    origin: &str,
    on_the_draw: bool,
) -> Preparation {
    let mut notes = Vec::new();
    let run = prepare_noting(library, criteria, origin, on_the_draw, &mut notes);
    Preparation { notes, run }
}

fn prepare_noting(
    library: &Library,
    criteria: &mut Criteria,
    origin: &str,
    on_the_draw: bool,
    notes: &mut Vec<String>,
) -> Result<PreparedRun> {
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
                report::tag_gap_refusal(&gap, library)
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
    landdrop::check(criteria.land_drop(), library)?;
    let land_drop = match criteria.land_drop() {
        [] => None,
        prefer => Some(landdrop::resolve(prefer, library, criteria.queries())?),
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
    casting::check(criteria.casting(), library)?;
    let casting = match criteria.casting() {
        [] => None,
        prefer => Some(casting::resolve(prefer, library, &asked)?),
    };
    asked.extend(casting.iter().flat_map(|p| p.queries.iter().cloned()));
    // The mulligan next, on the same terms: its queries sit behind everything
    // already asked for, so no bit a clause or another priority holds moves.
    // Only a rule the pilot declared has queries of its own. An objective
    // names criteria, whose queries the file already asked for.
    let mulligan = match criteria.mulligan().filter(|m| m.declares_a_rule()) {
        None => None,
        Some(declared) => {
            mulligan::check(declared, library)?;
            Some(mulligan::resolve(declared, library, &asked)?)
        }
    };
    asked.extend(mulligan.iter().flat_map(|m| m.queries.iter().cloned()));
    if let Some(resolved) = &mulligan {
        for query in &resolved.unmatched {
            notes.push(format!(
                "note: mulligan query {query:?} matches no card in this deck"
            ));
        }
    }
    if let Some(policy) = &casting {
        for query in &policy.unmatched {
            notes.push(format!(
                "note: casting preference {query:?} matches no castable card in this deck"
            ));
        }
        for (query, lands) in &policy.lands {
            notes.push(format!(
                "note: casting preference {query:?} also matches {} you play rather than cast: \
                 {}.\n      A land arrives on a land drop, so the priority ignores them — \
                 [land_drop] is where that decision lives.",
                if lands.len() == 1 { "a land" } else { "lands" },
                lands.join(", ")
            ));
        }
    }
    // A `cast` clause with nobody to cast is the budget's version of the land
    // drop's two claimants, and it is refused for the same reason: which
    // spells you cast out of one turn's mana is a decision the pilot makes,
    // and a tool that picked would be reporting a line nobody chose.
    if let Some(asked_by) = criteria.counts_castings() {
        if casting.is_none() {
            anyhow::bail!(
                "{origin}: {asked_by}: {}",
                report::casting_without_priority()
            );
        }
    }
    if let Some(policy) = &land_drop {
        for query in &policy.unmatched {
            notes.push(format!(
                "note: land-drop preference {query:?} matches no land in this deck"
            ));
        }
        for (query, spells) in &policy.non_lands {
            notes.push(format!(
                "note: land-drop preference {query:?} also matches {} you cannot play as a land \
                 drop: {}.\n      A land drop plays lands, so the priority ignores them.",
                if spells.len() == 1 { "a card" } else { "cards" },
                spells.join(", ")
            ));
        }
    }

    // The standard library first, then the file's own, because last-wins is
    // what makes the prelude overridable without an override syntax.
    let effect_library = gauntlet_toml::EffectLibrary::parse(
        gauntlet_toml::STANDARD_LIBRARY,
        gauntlet_toml::STANDARD_LIBRARY_ORIGIN,
    )?
    .followed_by(criteria.effects().clone());
    let resolved = effects::resolve(&effect_library, library, &asked)?;
    for query in &resolved.unmatched {
        notes.push(format!(
            "note: effect {query:?} matched no cards in this deck"
        ));
    }
    // Not a refusal, unlike the same gap in a criteria query above: nobody
    // asked for these. The standard library autoloads, so refusing the run
    // would make a tagless index answer nothing at all rather than answer the
    // question that was actually asked — but it loads *and moves numbers*, so
    // it does not get to fall silent either.
    notes.extend(report::tag_blind_notes(&resolved, library));
    for (effect, query) in resolved
        .applied
        .iter()
        .flat_map(|a| a.fetch_misses.iter().map(move |q| (&a.matches, q)))
    {
        notes.push(format!(
            "note: effect {effect:?} would fetch {query:?}, which matches no card in this deck"
        ));
    }

    refuse_unfirable_tutors(
        library,
        origin,
        &resolved,
        land_drop.as_ref(),
        casting.as_ref(),
    )?;

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
        refuse_unmodelled_mana(
            library,
            criteria,
            origin,
            asked_by,
            &resolved,
            land_drop.as_ref(),
        )?;
    }
    let no_costs: [Option<gauntlet_criteria::Demand>; 0] = [];
    let mana = match criteria.casts().or_else(|| casting.as_ref().map(|_| table)) {
        None => library::ManaDetail::Ignored,
        Some(asked_by) => {
            refuse_unpriceable_mana(library, origin, asked_by, &resolved)?;
            library::ManaDetail::Modelled {
                castable: casting.as_ref().map_or(&no_costs, |c| c.costs.as_slice()),
            }
        }
    };
    let mana_modelled = matches!(mana, library::ManaDetail::Modelled { .. });

    let queries: Vec<String> = asked
        .iter()
        .cloned()
        .chain(resolved.queries.iter().cloned())
        .collect();
    let grouping = library.grouping_for(&queries, &resolved.marked, mana)?;
    let policies = gauntlet_criteria::Policies {
        land_drop: land_drop.as_ref().map(|p| p.policy.clone()),
        casting: casting.as_ref().map(|p| p.policy.clone()),
        mulligan: mulligan.as_ref().map(|m| m.policy.clone()),
        chosen: None,
    };
    let mut schedule = Schedule::build(
        criteria.horizon(),
        on_the_draw,
        resolved.effects.clone(),
        policies.clone(),
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
        mulligan: mulligan.as_ref().map(|m| m.bits),
    };
    let classes = narrow::partition(&criteria.reads(), &shared);

    // The strategy an objective asks for is chosen before anything is
    // answered, because where the file declared no rule of its own it is the
    // strategy every answer is played under.
    let declared = criteria.mulligan().cloned();
    let mut chose = match declared.as_ref().filter(|m| !m.optimise.is_empty()) {
        None => None,
        Some(declared) => Some(optimise::choose(
            declared, &classes, &grouping, &schedule, plan, criteria, origin,
        )?),
    };
    if let Some(chose) = chose.as_ref().filter(|_| mulligan.is_none()) {
        schedule = Schedule::build(
            criteria.horizon(),
            on_the_draw,
            resolved.effects.clone(),
            gauntlet_criteria::Policies {
                chosen: Some(gauntlet_criteria::Chosen(std::sync::Arc::new(
                    chose.optimised.strategy.clone(),
                ))),
                ..policies
            },
        );
    }
    let tables = match (&mut chose, mulligan.is_none()) {
        (Some(chose), true) => std::mem::take(&mut chose.tables),
        _ => HashMap::new(),
    };

    Ok(PreparedRun {
        grouping,
        schedule,
        plan,
        classes,
        tables,
        resolved,
        land_drop,
        casting,
        mulligan,
        chose,
        mana_modelled,
    })
}

/// What a tutor needs of the run that declares it, refused by name before
/// anything is enumerated. Each of these would otherwise be answered — and in
/// the flattering direction, because a fetch that fires puts a card in your
/// hand.
fn refuse_unfirable_tutors(
    library: &Library,
    origin: &str,
    resolved: &effects::Resolved,
    land_drop: Option<&landdrop::Resolved>,
    casting: Option<&casting::Resolved>,
) -> Result<()> {
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
    Ok(())
}

/// What the mana model needs of a run that asks a mana question, checked
/// before anything is enumerated and refused by name where it is not there.
/// Every one of these would otherwise be answered — wrongly, and in the
/// flattering direction: an index with no tapland tag reports every land as
/// making mana the turn it lands, and a battlefield count of a spell reports
/// "drawn" under another name.
fn refuse_unmodelled_mana(
    library: &Library,
    criteria: &Criteria,
    origin: &str,
    asked_by: &str,
    resolved: &effects::Resolved,
    land_drop: Option<&landdrop::Resolved>,
) -> Result<()> {
    // What a delayed fetch puts onto the battlefield is on the battlefield,
    // and counted there: a Lantern off Urza's Saga's third chapter is the one
    // way a spell arrives that this walk models.
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
    // One land drop a turn is a decision, and a live effect already spends
    // it: with no declared priority the walk plays the deepest-looking land
    // you hold, because there was nothing else to choose by. Answering a mana
    // question beside that would be a second policy deciding the same drop,
    // and the two would disagree on exactly the hands that matter. Declaring
    // the priority makes them one decision, which is the remedy the refusal
    // names.
    // Only an effect that fires *on the drop* is a second claimant on it. A
    // tutor that fires when a spell is cast spends the mana, not the land
    // drop, and the budget has already said which spells those are.
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
    Ok(())
}

/// What pricing a cost needs of the run, refused by name where it is not
/// there.
fn refuse_unpriceable_mana(
    library: &Library,
    origin: &str,
    asked_by: &str,
    resolved: &effects::Resolved,
) -> Result<()> {
    if let Some(refusal) = report::cannot_price_mana(library) {
        anyhow::bail!("{origin}: {asked_by}: {refusal}");
    }
    // A fetched land is on the battlefield — countable, and counted — but
    // what it *taps for* on the turn it arrives is not readable from any tag
    // this index carries: a Scalding Tarn puts its Island down untapped and a
    // Terramorphic Expanse does not, and `otag:fetchland` holds both. So the
    // library it thinned is answered and the pool it filled is refused,
    // rather than answered in whichever direction happens to flatter.
    if let Some(named) = resolved
        .applied
        .iter()
        .filter(|a| a.live)
        .zip(&resolved.effects)
        // A delayed fetch was refused above if it could find a land, so what
        // it puts on the battlefield makes no mana and this question is not
        // about it.
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
    Ok(())
}

impl PreparedRun {
    /// How often the mulligan kept each hand size, and which engine said so.
    ///
    /// How often each hand size was kept is a question of its own, and a
    /// small one: it reads the mulligan's queries and the opener, nothing
    /// else, so it is enumerated exactly even where every class it describes
    /// was sampled. Asked separately rather than read off the first class,
    /// because every class answers it identically and the narrowest
    /// enumeration that can is the one that should. A chosen strategy already
    /// knows: the optimiser worked it out on the way to choosing it. `sampled`
    /// is what the sampler said, where it answered everything.
    pub(crate) fn kept(
        &self,
        sampled: Option<(Vec<f64>, &'static str)>,
        criteria: &mut Criteria,
    ) -> Result<Option<(Vec<f64>, &'static str)>> {
        Ok(match (sampled, &self.mulligan, &self.chose) {
            (Some(sampled), _, _) => Some(sampled),
            (None, Some(declared), _) => Some((
                answer::kept_at(
                    &self.grouping,
                    &self.schedule,
                    declared.bits,
                    self.plan,
                    criteria,
                )?,
                "exact",
            )),
            (None, None, Some(chose)) => Some((chose.optimised.strategy.kept().to_vec(), "exact")),
            (None, None, None) => None,
        })
    }

    /// What the report says about how this run was answered: the file's
    /// queries and zones, the effects, the enumerations, and every declared
    /// priority and strategy the numbers depended on.
    pub(crate) fn breakdown(
        &self,
        library: &Library,
        criteria: &Criteria,
        answers: &report::Answers,
        enumerations: Vec<report::Enumeration>,
        kept: Option<(Vec<f64>, &'static str)>,
    ) -> report::Breakdown {
        let schedule = &self.schedule;
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
        report::Breakdown {
            queries: query_matches,
            zones,
            effects: report::effects_applied(&self.resolved),
            enumerations,
            // Only where the run actually priced mana. A deck full of
            // shocklands answering a question about the graveyard assumed
            // nothing about any of them.
            assumed_tapped: if self.mana_modelled {
                library.conditional_taplands()
            } else {
                Vec::new()
            },
            // Read off the schedule the engine actually walked, like the zone
            // reachability above, so the note cannot claim a policy the
            // enumeration did not use.
            land_drop: schedule.land_drop().map(|_| report::LandDropUse {
                prefer: self
                    .land_drop
                    .as_ref()
                    .map_or_else(Vec::new, |p| p.prefer.clone()),
                then: "any other land",
                tie_break: gauntlet_criteria::LandDropPolicy::TIE_BREAK,
            }),
            // Read off the schedule for the same reason, and printed even
            // where no clause counts a casting: the line decides what is left
            // in hand, so it is an input to every number below it.
            casting: schedule.casting().map(|_| report::CastingUse {
                prefer: self
                    .casting
                    .as_ref()
                    .map_or_else(Vec::new, |p| p.prefer.clone()),
                then: gauntlet_criteria::CastingPolicy::THEN,
                tie_break: gauntlet_criteria::CastingPolicy::TIE_BREAK,
            }),
            // Read off the schedule too. A mulligan decides which hand every
            // other number is of, so it is printed above all of them.
            mulligan: schedule
                .mulligan()
                .zip(criteria.mulligan())
                .map(|(policy, declared)| {
                    let (shares, method) = kept.clone().unwrap_or_default();
                    let opener = schedule.gaps().first().copied().unwrap_or(0);
                    report::MulliganUse {
                        keep: declared.keep.iter().map(mulligan::describe).collect(),
                        bottom: declared.bottom.clone(),
                        then: gauntlet_criteria::MulliganPolicy::THEN,
                        tie_break: gauntlet_criteria::MulliganPolicy::TIE_BREAK,
                        down_to: policy.down_to(),
                        kept: shares
                            .iter()
                            .enumerate()
                            .map(|(depth, &share)| report::KeptAt {
                                cards: opener - depth as u32,
                                share: report::round(share, 6),
                            })
                            .collect(),
                        method,
                    }
                }),
            optimised: self
                .chose
                .as_ref()
                .map(|chose| optimise::report(chose, self.mulligan.is_none(), answers)),
        }
    }
}
