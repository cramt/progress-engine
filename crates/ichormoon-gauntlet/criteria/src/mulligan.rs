//! The London mulligan, walked once, whoever decides what it keeps.
//!
//! Under the London mulligan every redraw is a fresh deal of the whole
//! library, so depth `d` is the ordinary enumeration of openers with `d` cards
//! put back and a hand kept only if the strategy says so — and reaching depth
//! `d` at all is the product of having thrown back every hand before it:
//!
//! ```text
//! kept_d  = reach_d · Σ_h p(h) · P(keep h at d)
//! reach_0 = 1,   reach_{d+1} = reach_d · (1 - Σ_h p(h) · P(keep h at d))
//! ```
//!
//! Every kept hand's rest of the game is a [`Conditionals`] continuation,
//! walked on as many threads as there are and summed on this one in opener
//! order, so the thread count never reaches a digit. Beside the mulligan's
//! numbers the walk takes the keep-seven number, which is exactly depth 0 with
//! the keep rule ignored.
//!
//! **What varies is who decides, and nothing else.** A [`Decider`] says, for
//! one opener at one depth, whether it is kept and what goes back from it. The
//! pilot's declared mulligan is one ([`DeclaredRule`]); the strategy the
//! optimiser chose for an objective is the other
//! ([`crate::strategy::ChosenStrategy`]). A run with no mulligan declared is the
//! first of them at depth 0 alone, keeping every seven: the same walk, split at
//! the opener so its parts can be walked at once.

use chip_stats::{DistributionBuilder, KahanSum, Probability};

use crate::{
    settle, Board, Conditionals, Evaluator, Grouping, Mulliganed, Outcomes, RunError, Schedule,
};

/// Who decides, at every depth, whether an opener is kept and what goes back.
///
/// The walk hands it openers on the grouping it deals on — the one this
/// decider reads — and is handed back what went back in the groups of the
/// class whose questions are being answered.
pub(crate) trait Decider {
    /// The deepest mulligan taken: the floor, where every hand is kept.
    fn deepest(&self) -> u32;

    /// What happens to `opener` at `depth`: every share of its chance that is
    /// kept, each with the ways of putting back under it. Empty is a mulligan.
    fn decide(&self, opener: &[u32], depth: u32) -> Vec<Kept>;
}

/// A share of one opener's chance that is kept.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Kept {
    /// How much of the opener's chance keeps this way.
    pub(crate) chance: f64,
    /// Every way of putting back under it, which between them take all of
    /// `chance`.
    pub(crate) backs: Vec<Back>,
}

/// One way of putting back from a kept opener, in the class's groups.
///
/// Two chances rather than their product, because the product is taken in the
/// walk in one fixed order — `reach · p · kept · chance · spread` — and moving
/// a factor would move the last digit of a number that was exact before.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Back {
    pub(crate) counts: Vec<u32>,
    /// The chance the decider chose these counts on its own groups.
    pub(crate) chance: f64,
    /// The chance those counts fell on these cards of the class, where the
    /// decider's groups hold cards the class tells apart.
    pub(crate) spread: f64,
}

/// The pilot's declared mulligan, read off the board that plays it.
///
/// A tie inside one bottoming entry is a coin toss, and each side of the coin
/// is its own share of the opener's chance, kept or not on the hand it leaves.
/// A run that declared no mulligan keeps every seven.
pub(crate) struct DeclaredRule<'a> {
    board: Board<'a>,
    deepest: u32,
}

impl<'a> DeclaredRule<'a> {
    pub(crate) fn new(grouping: &'a Grouping, schedule: &'a Schedule) -> Self {
        let opener = schedule.gaps().first().copied().unwrap_or(0);
        DeclaredRule {
            board: Board::new(grouping, schedule),
            deepest: schedule
                .mulligan()
                .map_or(0, |policy| policy.deepest(opener)),
        }
    }
}

impl Decider for DeclaredRule<'_> {
    fn deepest(&self) -> u32 {
        self.deepest
    }

    fn decide(&self, opener: &[u32], depth: u32) -> Vec<Kept> {
        let mut kept = Vec::new();
        let mut hand = vec![0u32; opener.len()];
        self.board.bottomings(opener, depth, |back, chance| {
            for ((h, f), b) in hand.iter_mut().zip(opener).zip(back) {
                *h = f - b;
            }
            if depth == self.deepest || self.board.keeps(&hand) {
                kept.push(Kept {
                    chance,
                    backs: vec![Back {
                        counts: back.to_vec(),
                        chance: 1.0,
                        spread: 1.0,
                    }],
                });
            }
        });
        kept
    }
}

/// Every question `conditionals` answers, under the mulligan `decider` plays.
///
/// Openers are dealt on `dealt`, which is what the decider reads, and each is
/// projected onto the class through `to_class` to be played out. The outcomes
/// always carry what the mulligan did; a caller whose run declared none drops
/// it.
pub(crate) fn walk<V: Evaluator>(
    dealt: &Grouping,
    to_class: &[usize],
    decider: &impl Decider,
    conditionals: &mut Conditionals<'_, V>,
) -> Result<Outcomes, RunError<V::Error>> {
    let answering = conditionals.answering();
    let plan = answering.plan();
    let classes = conditionals.grouping().group_sizes().len();
    let deepest = decider.deepest();

    let mut openers: Vec<(Vec<u32>, f64)> = Vec::new();
    chip_stats::for_each_composition(dealt.group_sizes(), conditionals.opener(), |h, p| {
        openers.push((h.to_vec(), p))
    });
    let mut mass = KahanSum::new();
    for (_, p) in &openers {
        mass.add(*p);
    }
    settle::<V::Error>(None, None, plan, &mass)?;

    // Every decision, taken before anything is walked.
    let nothing = vec![0u32; classes];
    let firsts: Vec<Vec<u32>> = openers
        .iter()
        .map(|(first, _)| {
            let mut out = vec![0u32; classes];
            for (&c, &to) in first.iter().zip(to_class) {
                out[to] += c;
            }
            out
        })
        .collect();
    let decided: Vec<Vec<Vec<Kept>>> = (0..=deepest)
        .map(|depth| {
            openers
                .iter()
                .map(|(first, _)| decider.decide(first, depth))
                .collect()
        })
        .collect();

    // Everything the pass below will read, walked first and all at once: the
    // continuations are independent, so they are what the threads share out,
    // and the sums over them stay on this thread in one order. A hand thrown
    // back at depth `d > 0` is not walked past its opener, because nothing
    // about its later turns is asked; at depth 0 every hand is, for the
    // keep-seven number.
    let mut wanted: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
    for (depth, here) in decided.iter().enumerate() {
        for (first, kept) in firsts.iter().zip(here) {
            if depth == 0 {
                wanted.push((first.clone(), nothing.clone()));
            }
            for back in kept.iter().flat_map(|k| &k.backs) {
                wanted.push((first.clone(), back.counts.clone()));
            }
        }
    }
    conditionals.prefill(wanted)?;

    let mut totals = vec![KahanSum::new(); answering.criteria().len()];
    let mut seven = vec![KahanSum::new(); answering.criteria().len()];
    let mut histograms = vec![DistributionBuilder::new(); answering.expectations().len()];
    let mut kept_at: Vec<Probability> = Vec::with_capacity(deepest as usize + 1);
    let mut reach = 1.0;
    for (depth, here) in decided.iter().enumerate() {
        let mut keeps = KahanSum::new();
        for (((_, p), first), kept) in openers.iter().zip(&firsts).zip(here) {
            if depth == 0 {
                // The seven's own probability, with no mulligan weight on it:
                // it is the number had this hand been kept.
                let rest = conditionals.get(first, &nothing)?;
                for (total, held) in seven.iter_mut().zip(&rest.held) {
                    total.add(p * held);
                }
            }
            for share in kept {
                keeps.add(p * share.chance);
                let weight = reach * p * share.chance;
                for back in &share.backs {
                    let weight = weight * back.chance * back.spread;
                    let rest = conditionals.get(first, &back.counts)?;
                    for (total, held) in totals.iter_mut().zip(&rest.held) {
                        total.add(weight * held);
                    }
                    for (histogram, counted) in histograms.iter_mut().zip(&rest.counted) {
                        for (value, share) in counted.iter().enumerate() {
                            if *share > 0.0 {
                                histogram.add(value as u32, weight * share);
                            }
                        }
                    }
                }
            }
        }
        let keeps = keeps.total();
        kept_at.push(Probability::new(reach * keeps));
        reach *= 1.0 - keeps;
    }

    Ok(Outcomes {
        probabilities: totals
            .into_iter()
            .map(|t| Probability::new(t.total()))
            .collect(),
        distributions: histograms
            .into_iter()
            .map(DistributionBuilder::build)
            .collect(),
        mulligan: Some(Mulliganed {
            kept: kept_at,
            seven: seven
                .into_iter()
                .map(|t| Probability::new(t.total()))
                .collect(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::ChosenStrategy;
    use crate::{
        Answering, Count, Counted, Decision, Keep, LandDetail, MulliganPolicy, PathOutcomes,
        PathView, Plan, Policies, Strategy, Table, Zone,
    };

    /// Two criteria and an expectation, reading the tied cards, the lands and
    /// a later turn.
    struct Questions;

    impl Evaluator for Questions {
        type Error = std::convert::Infallible;

        fn evaluate(&mut self, v: &PathView<'_>) -> Result<PathOutcomes, Self::Error> {
            let hand = |turn, query| v.count_at(turn, query, Counted::In(Zone::Hand));
            Ok(PathOutcomes {
                held: vec![hand(0, 0) >= 1, hand(2, 3) >= 3],
                counted: vec![Count::new(hand(1, 2)).unwrap()],
            })
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn a_chosen_strategy_that_is_the_declared_rule_deals_the_same_numbers() {
        // Two cards A and B that one bottoming entry cannot tell apart, lands,
        // and filler. The pilot keeps two to four lands, puts back A-or-B
        // first and lands second, and stops at four.
        let full = Grouping::build(
            ["a", "b", "either", "land"].map(String::from).to_vec(),
            [(0b0101, 2), (0b0110, 2), (0b1000, 6), (0, 6)],
        )
        .unwrap();
        let rule = MulliganPolicy::new(
            vec![Keep {
                query: 3,
                min: 2,
                max: Some(4),
            }],
            vec![2, 3],
            4,
        );
        let schedule = Schedule::plain_with(
            &[7, 1, 1],
            Policies {
                mulligan: Some(rule),
                ..Policies::default()
            },
        );
        let declared = DeclaredRule::new(&full, &schedule);

        // The same rule as a chosen strategy, read on a grouping where A and
        // B are one group: there the tie is not a coin toss but a spread,
        // which is the other adapter's arithmetic.
        let keep = 0b1100;
        let coarse = full.coarsened(keep, LandDetail::Ignored);
        let on_coarse = DeclaredRule::new(&coarse, &schedule);
        let strategy = Strategy::tabulated(
            coarse.clone(),
            keep,
            LandDetail::Ignored,
            (7, 4),
            |opener, depth| {
                let kept = on_coarse.decide(opener, depth);
                Decision {
                    keep: !kept.is_empty(),
                    value: 0.0,
                    bottoms: kept
                        .iter()
                        .flat_map(|k| k.backs.iter().map(|b| (b.counts.clone(), k.chance)))
                        .collect(),
                }
            },
        );
        let identity: Vec<usize> = (0..full.group_sizes().len()).collect();
        let to_strategy = full.projection(&coarse, keep, LandDetail::Ignored).unwrap();
        let chosen = ChosenStrategy {
            strategy: &strategy,
            to_strategy: &to_strategy,
            to_class: &identity,
            classes: identity.len(),
        };
        assert_eq!(chosen.deepest(), declared.deepest());
        // The tie is exercised: the declared rule splits some opener in two.
        assert!(
            (0..=declared.deepest()).any(|d| declared.decide(&[1, 1, 3, 2], d).len() == 2),
            "no tie was priced"
        );

        let answering = Answering::all(Plan {
            criteria: 2,
            expectations: 1,
        });
        let mut questions = Questions;
        let mut conditionals = Conditionals::new(
            &full,
            &schedule,
            &answering,
            &mut questions,
            Table::default(),
        )
        .unwrap();
        let by_rule = walk(&full, &identity, &declared, &mut conditionals).unwrap();
        let by_strategy = walk(&full, &identity, &chosen, &mut conditionals).unwrap();

        for (a, b) in by_rule.probabilities.iter().zip(&by_strategy.probabilities) {
            assert!(close(a.get(), b.get()), "{a:?} vs {b:?}");
        }
        for (a, b) in by_rule.distributions.iter().zip(&by_strategy.distributions) {
            assert_eq!(a.probabilities().len(), b.probabilities().len());
            for (x, y) in a.probabilities().iter().zip(b.probabilities()) {
                assert!(close(*x, *y), "{a:?} vs {b:?}");
            }
        }
        let (m, n) = (by_rule.mulligan.unwrap(), by_strategy.mulligan.unwrap());
        assert_eq!(m.kept.len(), 4);
        for (a, b) in m
            .kept
            .iter()
            .zip(&n.kept)
            .chain(m.seven.iter().zip(&n.seven))
        {
            assert!(close(a.get(), b.get()), "{m:?} vs {n:?}");
        }
        // And the mulligan did something: the number is not the first seven's.
        assert!(!close(by_rule.probabilities[1].get(), m.seven[1].get()));
    }
}
