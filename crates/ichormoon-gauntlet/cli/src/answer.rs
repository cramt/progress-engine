//! Answering a prepared run: every class on its own enumeration, the sampler
//! where a class was too wide, and how often a declared mulligan kept each
//! hand size.

use anyhow::{Context, Result};

use crate::{narrow, report};

/// Which engine a run may use, resolved from the flags before anything runs.
///
/// Three states rather than two booleans, because `--simulate --exact` is the
/// fourth state and it does not mean anything. Resolving it at the boundary
/// leaves the run itself with no contradiction to arbitrate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Engine {
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
pub(crate) struct Run {
    pub(crate) engine: Engine,
    pub(crate) trials: u32,
    pub(crate) seed: u64,
}

/// Everything one file's questions came to: the answers, how they were
/// sampled where they were, and how the run enumerated to get them.
///
/// One struct rather than a tuple because the third one is about the first
/// two and a positional triple would leave a caller to remember which is
/// which.
pub(crate) struct Answered {
    pub(crate) answers: report::Answers,
    pub(crate) sampled: Option<report::Sampling>,
    pub(crate) enumerations: Vec<report::Enumeration>,
    /// How often the mulligan kept each hand size, where the sampler
    /// answered everything and so is the engine that should say.
    pub(crate) kept: Option<(Vec<f64>, report::Method)>,
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
pub(crate) fn answer(
    run: Run,
    classes: &[narrow::Class],
    grouping: &gauntlet_criteria::Grouping,
    schedule: &gauntlet_criteria::Schedule,
    plan: gauntlet_criteria::Plan,
    criteria: &mut gauntlet_toml::Criteria,
    mut tables: std::collections::HashMap<usize, gauntlet_criteria::Table>,
) -> Result<Answered> {
    let mut probabilities: Vec<Option<f64>> = vec![None; plan.criteria];
    let mut distributions: Vec<Option<chip_stats::Distribution>> = vec![None; plan.expectations];
    // The keep-your-seven number beside each criterion, where a mulligan was
    // declared. Filled by whichever engine answered that criterion, so it is
    // always the same kind of number as the one it sits beside.
    let mut seven: Vec<Option<f64>> = vec![None; plan.criteria];
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
    for (position, class) in classes.iter().enumerate() {
        let answering = class
            .answering(plan)
            .context("a class named a question this file does not hold")?;
        let narrowed = class.grouping(grouping);
        let walk = class.schedule(schedule);
        // How this class was answered, decided before its entry is written
        // rather than corrected afterwards.
        let method = if run.engine == Engine::Sample {
            report::Method::Sampled
        } else {
            // A chosen strategy reads its openers on a grouping this class may
            // not tell apart, so it is played from the run's whole grouping,
            // joined with the class's own at the opener and nowhere else.
            let answered = if walk.chosen().is_some() {
                gauntlet_criteria::run_chosen(
                    grouping,
                    class.keep(),
                    class.mana(),
                    &walk,
                    &answering,
                    criteria,
                    tables.remove(&position).unwrap_or_default(),
                )
            } else {
                gauntlet_criteria::run_answering(&narrowed, &walk, &answering, criteria)
            };
            match answered {
                Ok(exact) => {
                    for (&i, p) in answering.criteria().iter().zip(exact.probabilities) {
                        probabilities[i] = Some(p.get());
                    }
                    if let Some(mulligan) = &exact.mulligan {
                        for (&i, p) in answering.criteria().iter().zip(&mulligan.seven) {
                            seven[i] = Some(p.get());
                        }
                    }
                    for (&i, d) in answering.expectations().iter().zip(exact.distributions) {
                        distributions[i] = Some(d);
                    }
                    report::Method::Exact
                }
                // Only this one refusal falls back. Every other way a run can
                // be refused is a question the sampler would answer no better:
                // an empty library, a hand bigger than the deck, and a mass
                // that did not sum to one are all facts about what was asked
                // rather than about how expensive it was to enumerate.
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
                    report::Method::Sampled
                }
                Err(e) => return Err(e.into()),
            }
        };
        let groups = narrowed.group_sizes().len();
        enumerations.push(report::Enumeration {
            criteria: named(&criteria_names, answering.criteria()),
            expectations: named(&expectation_names, answering.expectations()),
            queries: class
                .queries(grouping)
                .into_iter()
                .map(str::to_string)
                .collect(),
            turns: class.turns().to_vec(),
            reading: class.reading().into(),
            pips: class.pips().map(gauntlet_criteria::Palette::symbols),
            groups,
            compositions: gauntlet_criteria::compositions(groups, walk.gaps()) as f64,
            method,
            deals: match (walk.mulligan(), walk.chosen()) {
                (Some(policy), _) => {
                    Some(policy.deepest(walk.gaps().first().copied().unwrap_or(0)) + 1)
                }
                (None, Some(chosen)) => Some(chosen.0.deepest() + 1),
                (None, None) => None,
            },
        });
    }

    let mut kept = None;
    let sampled = match why {
        None => None,
        Some(why) => {
            let sampled =
                gauntlet_sim::simulate(grouping, schedule, run.trials, run.seed, plan, criteria)?;
            for (i, estimated) in estimated.criteria.iter().enumerate() {
                if *estimated {
                    probabilities[i] = Some(sampled.proportions[i]);
                    if let Some(mulligan) = &sampled.mulligan {
                        seven[i] = Some(mulligan.seven[i]);
                    }
                }
            }
            if run.engine == Engine::Sample {
                kept = sampled
                    .mulligan
                    .as_ref()
                    .map(|m| (m.kept.clone(), report::Method::Sampled));
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
            seven,
        },
        sampled,
        enumerations,
        kept,
    })
}

/// How often the declared mulligan keeps each hand size, largest first.
///
/// Enumerated on a grouping that can tell apart only what the mulligan reads,
/// over the opener alone: the keep decision is made at turn 0 and no later
/// draw can change it.
pub(crate) fn kept_at(
    grouping: &gauntlet_criteria::Grouping,
    schedule: &gauntlet_criteria::Schedule,
    bits: u64,
    plan: gauntlet_criteria::Plan,
    criteria: &mut gauntlet_toml::Criteria,
) -> Result<Vec<f64>> {
    let narrowed = grouping.coarsened(bits, gauntlet_criteria::LandDetail::Ignored);
    let opener = schedule.narrowed(&[0], gauntlet_criteria::Reading::Cumulative);
    let nothing = gauntlet_criteria::Answering::some(plan, Vec::new(), Vec::new())
        .context("an empty selection of questions is always in range")?;
    let outcomes = gauntlet_criteria::run_answering(&narrowed, &opener, &nothing, criteria)?;
    let kept = outcomes
        .mulligan
        .context("a run with a declared mulligan reports what it kept")?
        .kept;
    Ok(kept.into_iter().map(|p| p.get()).collect())
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
