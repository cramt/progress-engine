//! Choosing the mulligan for the objective a file declared
//! ([#63](https://github.com/cramt/progress-engine/issues/63)).
//!
//! The engine does the arithmetic; this decides what it is asked. Each
//! objective criterion is priced on the class the run already built for it —
//! its own grouping, its own turns — and the strategy reads openers on the
//! join of those classes, which is the coarsest grouping that can tell apart
//! everything its objective reads and nothing else.

use std::collections::HashMap;

use anyhow::Context;
use gauntlet_criteria::{
    Conditionals, Grouping, LandDetail, ManaSource, Objective, Optimised, Plan, RunError, Schedule,
    Table, MAX_OPTIMISE_PATHS,
};
use gauntlet_toml::{Criteria, MulliganDecl, Weighted};

use crate::narrow::Class;
use crate::prepare::Unprepared;
use crate::refusal::Refusal;

/// A strategy chosen for this run's objective, and what choosing it walked.
pub struct Chose {
    pub optimised: Optimised,
    /// The objective, in the file's order.
    pub objective: Vec<Weighted>,
    /// What was walked for each class that holds an objective criterion,
    /// keyed by the class's position. Handed to the run that plays the
    /// strategy, which would otherwise walk every one of them again.
    pub tables: HashMap<usize, Table>,
    /// Paths the conditionals walked, all classes together.
    pub width: u128,
}

/// Choose the strategy that maximises `declared`'s objective.
///
/// Refused by name where an objective criterion is estimated rather than
/// enumerated: an optimum found over sampled conditionals keeps the openers
/// that were lucky in the sample and reports an expected score that is too
/// high, and there is nothing in the number that would say so.
pub fn choose(
    declared: &MulliganDecl,
    classes: &[Class],
    grouping: &Grouping,
    schedule: &Schedule,
    plan: Plan,
    criteria: &mut Criteria,
    origin: &str,
) -> Result<Chose, Unprepared> {
    let opener = schedule.gaps().first().copied().unwrap_or(0);
    let deepest = opener.saturating_sub(declared.down_to);

    // Which class answers each objective criterion, and where in that class.
    let mut placed: Vec<(usize, usize)> = Vec::with_capacity(declared.optimise.len());
    for weighted in &declared.optimise {
        let found = classes.iter().enumerate().find_map(|(ci, class)| {
            let answering = class.answering(plan)?;
            answering
                .criteria()
                .iter()
                .position(|&i| i == weighted.criterion)
                .map(|position| (ci, position))
        });
        placed.push(found.context("every criterion is in exactly one class")?);
    }
    let mut objective_classes: Vec<usize> = placed.iter().map(|&(ci, _)| ci).collect();
    objective_classes.sort_unstable();
    objective_classes.dedup();

    let mut tables: HashMap<usize, Table> = HashMap::new();
    let mut width: u128 = 0;
    let mut groupings: HashMap<usize, Grouping> = HashMap::new();
    for &ci in &objective_classes {
        let class = &classes[ci];
        let narrowed = class.grouping(grouping);
        let walk = class.schedule_with_opener(schedule);
        let answering = class
            .answering(plan)
            .context("a class named a question this file does not hold")?;
        let weighed: Vec<String> = declared
            .optimise
            .iter()
            .zip(&placed)
            .filter(|(_, (c, _))| *c == ci)
            .map(|(w, _)| w.name.clone())
            .collect();
        let mut conditionals =
            match Conditionals::new(&narrowed, &walk, &answering, criteria, Table::default()) {
                Ok(c) => c,
                Err(RunError::TooWide { paths, groups, .. }) => {
                    return Err(Refusal::ObjectiveTooWide {
                        file: origin.to_string(),
                        weighed,
                        paths,
                        groups,
                    }
                    .into())
                }
                Err(e) => return Err(anyhow::Error::from(e).into()),
            };
        width = width.saturating_add(conditionals.fill_width(deepest));
        if width > MAX_OPTIMISE_PATHS {
            return Err(Refusal::ObjectiveOverBudget {
                file: origin.to_string(),
                weighed,
                width,
            }
            .into());
        }
        conditionals.fill(deepest).map_err(anyhow::Error::from)?;
        tables.insert(ci, conditionals.into_table());
        groupings.insert(ci, narrowed);
    }

    // The strategy's grouping: everything any objective class tells apart,
    // joined, and nothing else.
    let keep = objective_classes
        .iter()
        .fold(0u64, |bits, &ci| bits | classes[ci].keep());
    let detail = objective_classes
        .iter()
        .fold(LandDetail::Ignored, |d, &ci| d.join(classes[ci].mana()));
    let strategy_grouping = grouping.coarsened(keep, detail);
    let projections: HashMap<usize, Vec<usize>> = objective_classes
        .iter()
        .map(|&ci| {
            let to = strategy_grouping
                .projection(&groupings[&ci], classes[ci].keep(), classes[ci].mana())
                .expect("the strategy's grouping is the join of its classes'");
            (ci, to)
        })
        .collect();
    let objectives: Vec<Objective<'_>> = declared
        .optimise
        .iter()
        .zip(&placed)
        .map(|(weighted, &(ci, position))| Objective {
            weight: weighted.weight,
            table: &tables[&ci],
            to_class: &projections[&ci],
            class_groups: groupings[&ci].group_sizes().len(),
            position,
        })
        .collect();
    let optimised = gauntlet_criteria::optimise(
        strategy_grouping,
        keep,
        detail,
        opener,
        declared.down_to,
        &objectives,
    );
    drop(objectives);
    Ok(Chose {
        optimised,
        objective: declared.optimise.clone(),
        tables,
        width,
    })
}

/// What one of the strategy's groups is, for a reader: the queries its cards
/// match, and what they do for mana where the strategy can see it.
pub fn describe_group(grouping: &Grouping, group: usize) -> String {
    let mask = grouping.group_masks()[group];
    let queries: Vec<String> = grouping
        .queries()
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1u64 << i) != 0)
        .map(|(_, q)| q.clone())
        .collect();
    let named = if queries.is_empty() {
        "none of its queries".to_string()
    } else {
        queries.join(" and ")
    };
    match grouping.group_mana()[group] {
        ManaSource::Spell => named,
        ManaSource::Castable { cost } => format!("{named}; castable for {}", cost.total()),
        ManaSource::Land {
            enters_tapped,
            produces,
        } => format!(
            "{named}; a land making {}{}",
            match produces.symbols() {
                s if s.is_empty() => "none of the colours asked about".to_string(),
                s => s.join(""),
            },
            if enters_tapped { ", tapped" } else { "" }
        ),
    }
}

/// What a run says about the strategy it chose. `played` is whether it is the
/// strategy the run's numbers are under; where it is not, `answers` are under
/// the declared rule, and each objective criterion's number there is printed
/// beside the chosen strategy's.
pub fn report(
    chose: &Chose,
    played: bool,
    answers: &crate::report::Answers,
) -> crate::report::OptimisedUse {
    use crate::report::{
        round, BottomUse, DecisionUse, KeptAt, ObjectiveUse, OpenerUse, OptimisedUse,
        StrategyTable, Threshold,
    };
    let strategy = &chose.optimised.strategy;
    let declared = |criterion: usize| (!played).then(|| answers.probabilities[criterion]);
    let objective: Vec<ObjectiveUse> = chose
        .objective
        .iter()
        .enumerate()
        .map(|(k, w)| ObjectiveUse {
            criterion: w.name.clone(),
            weight: w.weight,
            probability: round(chose.optimised.under[k], 6),
            alone: round(chose.optimised.alone[k], 6),
            declared: declared(w.criterion).map(|p| round(p, 6)),
        })
        .collect();
    let score_declared = (!played).then(|| {
        chose
            .objective
            .iter()
            .map(|w| w.weight * answers.probabilities[w.criterion])
            .sum::<f64>()
    });
    let opener = strategy.opener();
    let grouping = strategy.grouping();
    OptimisedUse {
        played,
        objective,
        score: round(strategy.score(), 6),
        score_declared: score_declared.map(|s| round(s, 6)),
        thresholds: strategy
            .thresholds()
            .iter()
            .enumerate()
            .map(|(depth, &t)| Threshold {
                cards: opener - depth as u32,
                keep_at_least: round(t, 6),
            })
            .collect(),
        down_to: strategy.down_to(),
        kept: strategy
            .kept()
            .iter()
            .enumerate()
            .map(|(depth, &share)| KeptAt {
                cards: opener - depth as u32,
                share: round(share, 6),
            })
            .collect(),
        walked: chose.width as f64,
        tie_break: TIE_BREAK,
        strategy: StrategyTable {
            groups: (0..grouping.group_sizes().len())
                .map(|g| describe_group(grouping, g))
                .collect(),
            openers: strategy
                .openers()
                .iter()
                .zip(strategy.decisions())
                .map(|((hand, share), decisions)| OpenerUse {
                    hand: hand.clone(),
                    share: round(*share, 9),
                    decisions: decisions
                        .iter()
                        .map(|d| DecisionUse {
                            keep: d.keep,
                            value: round(d.value, 6),
                            bottom: d
                                .bottoms
                                .iter()
                                .map(|(cards, share)| BottomUse {
                                    cards: cards.clone(),
                                    share: round(*share, 6),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        },
    }
}

/// How a chosen strategy settles two ways of putting back that score the same.
pub const TIE_BREAK: &str = "ways that score the same are all taken, each card with the chance \
                             a uniform choice gives it";
