//! The best mulligan for a weighted set of questions, and every number under it.
//!
//! [#7](https://github.com/cramt/progress-engine/issues/7) settles the mulligan
//! the pilot *declares*. This is the other half,
//! [#63](https://github.com/cramt/progress-engine/issues/63): given the
//! questions a deck is being built to answer, weighted by how much each one
//! matters, which hands should it keep and what should it put back — and how
//! much better is that than the rule the pilot wrote down?
//!
//! It is answered exactly, and out of pieces the engine already had.
//!
//! **A kept hand's value is linear.** For an opener `h` with `b` put back, the
//! objective scores `Σ w_i · P(C_i | h, b)`, and each of those conditionals is
//! the rest of a game dealt from the library `h` left behind — the walk this
//! engine already does, split at the opener. [`Conditionals`] computes them
//! once per class, on that class's own grouping, and keeps them.
//!
//! **The best strategy is a threshold per depth.** Under the London mulligan
//! every redraw is a fresh deal, so backward induction over the depths is the
//! whole search:
//!
//! ```text
//! V_last = E_h[ value(best_bottom(h)) ]            the floor keeps anything
//! keep h at depth d  ⇔  value(best_bottom(h)) ≥ V_{d+1}
//! V_d    = E_h[ max(value(best_bottom(h)), V_{d+1}) ]
//! ```
//!
//! **The strategy reads openers on one grouping, and every class plays on its
//! own.** The strategy's grouping is the join of what its objective's classes
//! tell apart; a class answering some other question reads the opener on the
//! join of its grouping and the strategy's — seven cards, so that join is small
//! — and plays the rest of the game on its own grouping, because after the
//! opener the library is the deck minus the projection of that hand. That is
//! [`run_chosen`], and it is what puts every number in a run under the chosen
//! strategy rather than only the ones it was chosen for
//! ([#64](https://github.com/cramt/progress-engine/issues/64)).
//!
//! **Ties are priced, not broken,** by the rule the declared mulligan already
//! follows: where two ways of putting cards back score the same, each card of
//! them goes back with the chance a uniform choice among them gives it. That
//! is the one rule that does not care how the groups were numbered.

use std::collections::HashMap;
use std::sync::Arc;

use chip_stats::{DistributionBuilder, KahanSum};

use crate::mulligan::{Back, Decider, Kept};
use crate::{
    compositions, feasible, settle, Answering, Board, Evaluator, Grouping, LandDetail, Outcomes,
    RunError, Schedule, Walking, MAX_PATHS,
};

/// What the rest of a game comes to from one kept hand, for one class.
#[derive(Debug, Clone, PartialEq)]
pub struct Continuation {
    /// One per criterion the class answers: the chance it holds, given the
    /// hand.
    pub held: Vec<f64>,
    /// One per expectation the class answers: P(value = k), given the hand.
    pub counted: Vec<Vec<f64>>,
}

/// Continuations already worked out for one class, by opener and by what went
/// back from it.
///
/// Owned, so it can outlive the evaluator borrow that filled it: the
/// optimiser fills one per class of its objective, reads them all at once, and
/// then hands each back to the run that plays the strategy it chose, which
/// would otherwise walk every one of them again.
#[derive(Debug, Clone, Default)]
pub struct Table {
    entries: HashMap<Vec<u8>, Continuation>,
}

impl Table {
    /// How many (opener, put back) pairs this has walked.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The continuation walked for `first` with `back` put back, if it has
    /// been walked.
    pub fn get(&self, first: &[u32], back: &[u32]) -> Option<&Continuation> {
        self.entries.get(&key(first, back))
    }
}

/// An opener and what went back from it, as one hashable key. A count in an
/// opener is at most the opener's size, which is seven.
fn key(first: &[u32], back: &[u32]) -> Vec<u8> {
    first
        .iter()
        .chain(back)
        .map(|&c| u8::try_from(c).expect("an opener holds at most 255 of a group"))
        .collect()
}

/// The conditionals of one class: for a kept hand, how often each of its
/// questions holds over the rest of the game.
///
/// Worked out on demand and remembered, because the same kept hand is reached
/// from many openers of a finer grouping — every opener a strategy reads that
/// projects onto it — and walking it again for each would multiply the work by
/// how much finer the strategy's grouping is than this class's.
pub struct Conditionals<'a, V> {
    grouping: &'a Grouping,
    schedule: &'a Schedule,
    answering: &'a Answering,
    evaluator: &'a mut V,
    board: Board<'a>,
    table: Table,
    threads: usize,
}

impl<'a, V: Evaluator> Conditionals<'a, V> {
    /// `schedule` has to keep the opener as its first checkpoint, which any
    /// schedule narrowed with turn 0 observed does. `seed` is anything
    /// already walked for this same class, schedule and set of questions.
    pub fn new(
        grouping: &'a Grouping,
        schedule: &'a Schedule,
        answering: &'a Answering,
        evaluator: &'a mut V,
        seed: Table,
    ) -> Result<Self, RunError<V::Error>> {
        feasible(grouping, schedule)?;
        let groups = grouping.dealt();
        let paths = compositions(groups, schedule.gaps());
        if paths > MAX_PATHS {
            return Err(RunError::TooWide {
                paths,
                groups,
                queries: grouping.queries().to_vec(),
            });
        }
        Ok(Conditionals {
            grouping,
            schedule,
            answering,
            evaluator,
            board: Board::new(grouping, schedule),
            table: seed,
            threads: crate::threads(),
        })
    }

    /// Walk on at most `threads` threads. The answers do not depend on it:
    /// each continuation is one thread's whole walk, and nothing is summed
    /// across threads.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// The class these continuations are played on.
    pub(crate) fn grouping(&self) -> &'a Grouping {
        self.grouping
    }

    /// The questions these continuations answer.
    pub(crate) fn answering(&self) -> &'a Answering {
        self.answering
    }

    /// The opener this class deals, which is its schedule's first gap.
    pub fn opener(&self) -> u32 {
        self.schedule.gaps().first().copied().unwrap_or(0)
    }

    /// The rest of the game from `first` with `back` put on the bottom.
    pub fn get(
        &mut self,
        first: &[u32],
        back: &[u32],
    ) -> Result<&Continuation, RunError<V::Error>> {
        let k = key(first, back);
        if !self.table.entries.contains_key(&k) {
            let walked = continue_from(
                &mut self.board,
                &mut *self.evaluator,
                self.grouping,
                self.schedule,
                self.answering,
                first,
                back,
            )?;
            self.table.entries.insert(k.clone(), walked);
        }
        Ok(&self.table.entries[&k])
    }

    /// Walk every one of `wanted` that has not been walked yet, spread over
    /// the threads this has.
    ///
    /// Each (opener, put back) pair is a walk of its own and shares nothing
    /// with another, so the threads take pairs off one queue until it is empty
    /// and each keeps its own board and its own fork of the evaluator. An
    /// evaluator that cannot fork walks them all here, one after another.
    ///
    /// Where more than one walk fails, the error reported is the one the
    /// single-threaded walk would have met first.
    pub fn prefill(&mut self, wanted: Vec<(Vec<u32>, Vec<u32>)>) -> Result<(), RunError<V::Error>> {
        let mut seen = std::collections::HashSet::new();
        let missing: Vec<(Vec<u32>, Vec<u32>)> = wanted
            .into_iter()
            .filter(|(first, back)| {
                let k = key(first, back);
                !self.table.entries.contains_key(&k) && seen.insert(k)
            })
            .collect();
        let threads = self.threads.min(missing.len());
        let forks: Option<Vec<V>> = if threads > 1 {
            (0..threads).map(|_| self.evaluator.fork()).collect()
        } else {
            None
        };
        let Some(forks) = forks else {
            for (first, back) in &missing {
                self.get(first, back)?;
            }
            return Ok(());
        };
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (grouping, schedule, answering) = (self.grouping, self.schedule, self.answering);
        let missing = &missing;
        let mut walked: Vec<Walked<V::Error>> = std::thread::scope(|scope| {
            let workers: Vec<_> = forks
                .into_iter()
                .map(|mut evaluator| {
                    let next = &next;
                    scope.spawn(move || {
                        let mut board = Board::new(grouping, schedule);
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some((first, back)) = missing.get(i) else {
                                break;
                            };
                            let result = continue_from(
                                &mut board,
                                &mut evaluator,
                                grouping,
                                schedule,
                                answering,
                                first,
                                back,
                            );
                            let failed = result.is_err();
                            done.push((i, result));
                            if failed {
                                break;
                            }
                        }
                        done
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().expect("a walking thread panicked"))
                .collect()
        });
        walked.sort_by_key(|(i, _)| *i);
        for (i, result) in walked {
            let (first, back) = &missing[i];
            self.table.entries.insert(key(first, back), result?);
        }
        Ok(())
    }

    /// Walk every opener this class can deal, with every way of putting back
    /// up to `deepest` cards from it. What an optimiser needs: it cannot know
    /// which hands are worth keeping until it has priced all of them.
    pub fn fill(&mut self, deepest: u32) -> Result<(), RunError<V::Error>> {
        let mut wanted: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
        chip_stats::for_each_composition(self.grouping.group_sizes(), self.opener(), |h, _| {
            for depth in 0..=deepest {
                chip_stats::for_each_composition(h, depth, |b, _| {
                    wanted.push((h.to_vec(), b.to_vec()))
                });
            }
        });
        self.prefill(wanted)
    }

    /// How many paths [`Conditionals::fill`] would walk: every (opener, put
    /// back) pair, times the width of the rest of the game dealt from it.
    ///
    /// The number an optimiser is refused on, because it is the one that
    /// decides how long it takes: each pair is a walk of its own.
    pub fn fill_width(&self, deepest: u32) -> u128 {
        let mut pairs: u128 = 0;
        chip_stats::for_each_composition(self.grouping.group_sizes(), self.opener(), |h, _| {
            for depth in 0..=deepest {
                chip_stats::for_each_composition(h, depth, |_, _| pairs += 1);
            }
        });
        let later = self.schedule.gaps().get(1..).unwrap_or(&[]);
        pairs.saturating_mul(compositions(self.grouping.dealt(), later))
    }

    /// Everything walked so far.
    pub fn into_table(self) -> Table {
        self.table
    }
}

/// One thread's answer for one (opener, put back) pair, filed under the
/// pair's position in the queue so the answers can be put back in order.
type Walked<E> = (usize, Result<Continuation, RunError<E>>);

/// The rest of one game, from `first` with `back` put on the bottom, walked
/// on `board` and answered by `evaluator`.
///
/// A free function rather than a method because the threads of
/// [`Conditionals::prefill`] each bring their own board and evaluator, and
/// this is the one walk every one of them does.
fn continue_from<'a, V: Evaluator>(
    board: &mut Board<'a>,
    evaluator: &mut V,
    grouping: &'a Grouping,
    schedule: &'a Schedule,
    answering: &Answering,
    first: &[u32],
    back: &[u32],
) -> Result<Continuation, RunError<V::Error>> {
    let plan = answering.plan();
    let later = schedule.gaps().get(1..).unwrap_or(&[]);
    let sizes = grouping.group_sizes();
    let fetches = board.fetches();
    board.bottom(back);
    let mut totals = vec![KahanSum::new(); answering.criteria().len()];
    let mut histograms = vec![DistributionBuilder::new(); answering.expectations().len()];
    let mut mass = KahanSum::new();
    let mut failure = None;
    let mut wrong_shape = None;
    let mut outcomes = crate::PathOutcomes::default();
    let mut walking = Walking {
        board,
        evaluator,
        plan,
        criteria: answering.criteria(),
        expectations: answering.expectations(),
        totals: &mut totals,
        histograms: &mut histograms,
        mass: &mut mass,
        failure: &mut failure,
        wrong_shape: &mut wrong_shape,
        outcomes: &mut outcomes,
    };
    if fetches {
        chip_stats::for_each_checkpoint_path_removing_after(sizes, first, later, &mut walking);
    } else {
        chip_stats::for_each_checkpoint_path_after(sizes, first, later, |h, p| {
            chip_stats::Walk::path(&mut walking, h, p)
        });
    }
    // Each continuation is a conditional distribution, so its own mass is
    // one: checked per hand, because a hand that lost mass would price itself
    // wrong and nothing downstream could tell.
    settle(failure, wrong_shape, plan, &mass)?;
    Ok(Continuation {
        held: totals.into_iter().map(KahanSum::total).collect(),
        counted: histograms
            .into_iter()
            .map(|h| h.build().probabilities().to_vec())
            .collect(),
    })
}

/// What a strategy does with one opener at one depth.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// Whether the hand is kept. Always, at the floor.
    pub keep: bool,
    /// What the hand scores, put back the best way.
    pub value: f64,
    /// Every way of putting back that scores `value`, as counts per strategy
    /// group, with the chance each is the one taken. One entry where nothing
    /// ties; card-uniform among them where something does.
    pub bottoms: Vec<(Vec<u32>, f64)>,
}

/// A mulligan strategy chosen for a weighted objective: for every opener it
/// can tell apart, and every depth, whether to keep it and what to put back.
#[derive(Debug, Clone)]
pub struct Strategy {
    grouping: Grouping,
    keep: u64,
    detail: LandDetail,
    opener: u32,
    down_to: u32,
    /// Every opener, in the order the enumeration deals them, with its chance.
    openers: Vec<(Vec<u32>, f64)>,
    /// Parallel to `openers`: one decision per depth.
    decisions: Vec<Vec<Decision>>,
    index: HashMap<Vec<u8>, usize>,
    thresholds: Vec<f64>,
    score: f64,
    kept: Vec<f64>,
}

impl Strategy {
    /// The grouping this strategy reads openers on.
    pub fn grouping(&self) -> &Grouping {
        &self.grouping
    }

    /// The query bits and the mana detail that grouping keeps.
    pub fn keep(&self) -> u64 {
        self.keep
    }

    pub fn detail(&self) -> LandDetail {
        self.detail
    }

    pub fn opener(&self) -> u32 {
        self.opener
    }

    pub fn down_to(&self) -> u32 {
        self.down_to
    }

    /// The deepest mulligan it takes: the floor, where anything is kept.
    pub fn deepest(&self) -> u32 {
        self.opener.saturating_sub(self.down_to)
    }

    /// For each depth above the floor, the score a hand has to reach to be
    /// kept there — which is the expected score of mulliganing it.
    pub fn thresholds(&self) -> &[f64] {
        &self.thresholds
    }

    /// The objective's expected score under this strategy.
    pub fn score(&self) -> f64 {
        self.score
    }

    /// The share of games kept at each depth, from the opener down.
    pub fn kept(&self) -> &[f64] {
        &self.kept
    }

    /// Every opener with its chance, parallel to [`Strategy::decisions`].
    pub fn openers(&self) -> &[(Vec<u32>, f64)] {
        &self.openers
    }

    /// One list per opener, one decision per depth.
    pub fn decisions(&self) -> &[Vec<Decision>] {
        &self.decisions
    }

    /// What this strategy does with `opener` (counts per strategy group) at
    /// `depth`.
    pub fn decide(&self, opener: &[u32], depth: u32) -> &Decision {
        let i = self.index[&key(opener, &[])];
        &self.decisions[i][depth as usize]
    }

    /// Where each group of `finer` falls in this strategy's grouping, for a
    /// grouping that tells apart at least what this one does.
    pub fn project(&self, finer: &Grouping) -> Option<Vec<usize>> {
        finer.projection(&self.grouping, self.keep, self.detail)
    }
}

#[cfg(test)]
impl Strategy {
    /// A strategy that decides whatever `decide` says, for a test that plays
    /// one against a declared rule. Nothing about it was optimised, so it
    /// carries no thresholds, score or kept shares.
    pub(crate) fn tabulated(
        grouping: Grouping,
        keep: u64,
        detail: LandDetail,
        (opener, down_to): (u32, u32),
        decide: impl Fn(&[u32], u32) -> Decision,
    ) -> Strategy {
        let mut openers: Vec<(Vec<u32>, f64)> = Vec::new();
        chip_stats::for_each_composition(grouping.group_sizes(), opener, |h, p| {
            openers.push((h.to_vec(), p))
        });
        let deepest = opener.saturating_sub(down_to);
        let decisions = openers
            .iter()
            .map(|(h, _)| (0..=deepest).map(|depth| decide(h, depth)).collect())
            .collect();
        let index = openers
            .iter()
            .enumerate()
            .map(|(i, (h, _))| (key(h, &[]), i))
            .collect();
        Strategy {
            grouping,
            keep,
            detail,
            opener,
            down_to,
            openers,
            decisions,
            index,
            thresholds: Vec::new(),
            score: 0.0,
            kept: Vec::new(),
        }
    }
}

/// A chosen strategy as a run carries it.
///
/// Shared rather than copied, because every class of a run plays the same
/// one; compared by identity, because two strategies are the same strategy
/// exactly when they are the same table.
#[derive(Debug, Clone)]
pub struct Chosen(pub Arc<Strategy>);

impl PartialEq for Chosen {
    fn eq(&self, other: &Chosen) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Chosen {}

/// One weighted question in an objective, with the conditionals that price
/// it.
pub struct Objective<'t> {
    pub weight: f64,
    /// Its class's conditionals, filled to the strategy's floor.
    pub table: &'t Table,
    /// For each strategy group, the group of its class it falls in.
    pub to_class: &'t [usize],
    /// How many groups its class has.
    pub class_groups: usize,
    /// Which of its class's criteria it is, by position in that class's
    /// answers.
    pub position: usize,
}

/// A chosen strategy and what it trades.
#[derive(Debug, Clone)]
pub struct Optimised {
    pub strategy: Strategy,
    /// Each objective question's chance under the strategy, parallel to the
    /// objective.
    pub under: Vec<f64>,
    /// Each objective question's chance under the strategy that optimises it
    /// alone: what its weight is being traded against.
    pub alone: Vec<f64>,
}

/// The most paths an optimiser's conditionals may walk, all its classes
/// together.
///
/// Twenty times [`MAX_PATHS`], because it is twenty times the work the
/// ceiling bounds for one enumeration and is still seconds rather than
/// minutes: a strategy is chosen once, and a question over this is refused by
/// name rather than estimated, because an optimum found over sampled
/// conditionals keeps the openers that were lucky in the sample.
pub const MAX_OPTIMISE_PATHS: u128 = 20 * MAX_PATHS;

/// The strategy that maximises the objective's expected score, over openers
/// read on `grouping` — which has to tell apart everything each objective's
/// class does. `keep` and `detail` say what that grouping kept.
pub fn optimise(
    grouping: Grouping,
    keep: u64,
    detail: LandDetail,
    opener: u32,
    down_to: u32,
    objectives: &[Objective<'_>],
) -> Optimised {
    let mut openers: Vec<(Vec<u32>, f64)> = Vec::new();
    chip_stats::for_each_composition(grouping.group_sizes(), opener, |h, p| {
        openers.push((h.to_vec(), p))
    });
    let deepest = opener.saturating_sub(down_to);
    let prices = Prices { objectives };
    let weights: Vec<f64> = objectives.iter().map(|o| o.weight).collect();
    let (decisions, thresholds, score) = induct(&openers, deepest, &prices, &weights);

    // Forward, under the strategy just chosen: how often each depth keeps,
    // and how often each question holds.
    let mut reach = 1.0;
    let mut kept = Vec::with_capacity(deepest as usize + 1);
    let mut under = vec![KahanSum::new(); objectives.len()];
    for depth in 0..=deepest {
        let mut keeps = KahanSum::new();
        for ((first, p), decided) in openers.iter().zip(&decisions) {
            let decision = &decided[depth as usize];
            if !decision.keep {
                continue;
            }
            keeps.add(*p);
            for (back, q) in &decision.bottoms {
                for (k, total) in under.iter_mut().enumerate() {
                    total.add(reach * p * q * prices.held(k, first, back));
                }
            }
        }
        let keeps = keeps.total();
        kept.push(reach * keeps);
        reach *= 1.0 - keeps;
    }
    let alone = (0..objectives.len())
        .map(|k| {
            let only: Vec<f64> = (0..objectives.len())
                .map(|j| if j == k { 1.0 } else { 0.0 })
                .collect();
            induct(&openers, deepest, &prices, &only).2
        })
        .collect();

    let index = openers
        .iter()
        .enumerate()
        .map(|(i, (h, _))| (key(h, &[]), i))
        .collect();
    Optimised {
        strategy: Strategy {
            grouping,
            keep,
            detail,
            opener,
            down_to,
            openers,
            decisions,
            index,
            thresholds,
            score,
            kept,
        },
        under: under.into_iter().map(KahanSum::total).collect(),
        alone,
    }
}

/// Reads an objective question's conditional off its class's table.
struct Prices<'o, 't> {
    objectives: &'o [Objective<'t>],
}

impl Prices<'_, '_> {
    fn held(&self, k: usize, first: &[u32], back: &[u32]) -> f64 {
        let objective = &self.objectives[k];
        let project = |counts: &[u32]| {
            let mut out = vec![0u32; objective.class_groups];
            for (&c, &to) in counts.iter().zip(objective.to_class) {
                out[to] += c;
            }
            out
        };
        objective
            .table
            .get(&project(first), &project(back))
            .expect("the optimiser's tables are filled to its floor")
            .held[objective.position]
    }
}

/// Backward induction over the depths for one set of weights: every opener's
/// decision at every depth, the threshold at each depth above the floor, and
/// the expected score at the opener.
fn induct(
    openers: &[(Vec<u32>, f64)],
    deepest: u32,
    prices: &Prices<'_, '_>,
    weights: &[f64],
) -> (Vec<Vec<Decision>>, Vec<f64>, f64) {
    let mut decisions: Vec<Vec<Decision>> = vec![Vec::new(); openers.len()];
    let mut thresholds = vec![0.0; deepest as usize];
    let mut next: Option<f64> = None;
    for depth in (0..=deepest).rev() {
        let mut expected = KahanSum::new();
        for ((first, p), decided) in openers.iter().zip(decisions.iter_mut()) {
            let mut best = f64::NEG_INFINITY;
            let mut ties: Vec<(Vec<u32>, f64)> = Vec::new();
            chip_stats::for_each_composition(first, depth, |back, share| {
                // Summed in one fixed order, so two ways of putting back that
                // every objective question reads the same come out bit for bit
                // equal, and tie rather than being split by rounding.
                let value: f64 = weights
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| **w != 0.0)
                    .map(|(k, w)| w * prices.held(k, first, back))
                    .sum();
                if value > best {
                    best = value;
                    ties.clear();
                }
                if value == best {
                    ties.push((back.to_vec(), share));
                }
            });
            // Card-uniform among the ties: each way weighted by how many sets
            // of cards it is, which is the chance the enumeration handed it.
            let total: f64 = ties.iter().map(|(_, s)| s).sum();
            for (_, share) in &mut ties {
                *share /= total;
            }
            // A hand scoring exactly what a mulligan is expected to is kept:
            // a mulligan costs a card, and a tie is no reason to pay it.
            let keep = next.is_none_or(|v| best >= v);
            expected.add(p * if keep { best } else { next.unwrap_or(best) });
            decided.push(Decision {
                keep,
                value: best,
                bottoms: ties,
            });
        }
        if let Some(v) = next {
            thresholds[depth as usize] = v;
        }
        next = Some(expected.total());
    }
    for decided in &mut decisions {
        decided.reverse();
    }
    (decisions, thresholds, next.unwrap_or(0.0))
}

/// Every question of one class answered under the chosen strategy the
/// schedule carries.
///
/// `full` is the run's grouping, and `keep` and `detail` are what this class
/// tells apart. The opener is read on the join of the class and the strategy,
/// which is what the strategy decides on, and the rest of the game is played
/// on the class's own grouping, which is all its questions read. What went
/// back is decided on the strategy's groups, and where one of those holds
/// cards the join tells apart, each of them goes back with the chance a
/// uniform choice gives it.
///
/// `seed` is whatever the optimiser already walked for this class.
pub fn run_chosen<V: Evaluator>(
    full: &Grouping,
    keep: u64,
    detail: LandDetail,
    schedule: &Schedule,
    answering: &Answering,
    evaluator: &mut V,
    seed: Table,
) -> Result<Outcomes, RunError<V::Error>> {
    let strategy = &schedule
        .chosen()
        .expect("only called for a run that plays a chosen strategy")
        .0;
    let class = full.coarsened(keep, detail);
    let join = full.coarsened(keep | strategy.keep(), detail.join(strategy.detail()));
    let to_class = join
        .projection(&class, keep, detail)
        .expect("a join is finer than either side of it");
    let to_strategy = strategy
        .project(&join)
        .expect("a join is finer than either side of it");
    let opener = strategy.opener();
    let joined = join.dealt();
    let openers_width = compositions(joined, &[opener]);
    if openers_width > MAX_PATHS {
        return Err(RunError::TooWide {
            paths: openers_width,
            groups: joined,
            queries: join.queries().to_vec(),
        });
    }
    let mut conditionals = Conditionals::new(&class, schedule, answering, evaluator, seed)?;
    debug_assert_eq!(
        conditionals.opener(),
        opener,
        "the class deals the same opener"
    );
    let decider = ChosenStrategy {
        strategy,
        to_strategy: &to_strategy,
        to_class: &to_class,
        classes: class.group_sizes().len(),
    };
    crate::mulligan::walk(&join, &to_class, &decider, &mut conditionals)
}

/// A chosen strategy, deciding for openers read on a grouping at least as
/// fine as its own: `to_strategy` says where each of that grouping's groups
/// falls in the strategy's, and `to_class` where it falls in the class being
/// answered.
pub(crate) struct ChosenStrategy<'s> {
    pub(crate) strategy: &'s Strategy,
    pub(crate) to_strategy: &'s [usize],
    pub(crate) to_class: &'s [usize],
    pub(crate) classes: usize,
}

impl Decider for ChosenStrategy<'_> {
    fn deepest(&self) -> u32 {
        self.strategy.deepest()
    }

    /// The whole opener is kept or none of it, and every way of putting back
    /// that the strategy ties between is spread over the cards the finer
    /// grouping tells apart.
    fn decide(&self, opener: &[u32], depth: u32) -> Vec<Kept> {
        let project = |counts: &[u32], to: &[usize], n: usize| {
            let mut out = vec![0u32; n];
            for (&c, &i) in counts.iter().zip(to) {
                out[i] += c;
            }
            out
        };
        let groups = self.strategy.grouping().group_sizes().len();
        let decision = self
            .strategy
            .decide(&project(opener, self.to_strategy, groups), depth);
        if !decision.keep {
            return Vec::new();
        }
        let mut backs = Vec::new();
        for (back, q) in &decision.bottoms {
            for (spread, r) in spread(back, opener, self.to_strategy) {
                backs.push(Back {
                    counts: project(&spread, self.to_class, self.classes),
                    chance: *q,
                    spread: r,
                });
            }
        }
        vec![Kept { chance: 1.0, backs }]
    }
}

/// Every way `back` — counts per strategy group — can fall across the finer
/// groups of `first` that each strategy group holds, with the chance of each:
/// the cards of one strategy group are interchangeable to the strategy, so
/// which of them go back is a uniform choice among the ones held.
fn spread(back: &[u32], first: &[u32], to_strategy: &[usize]) -> Vec<(Vec<u32>, f64)> {
    let mut ways: Vec<(Vec<u32>, f64)> = vec![(vec![0; first.len()], 1.0)];
    for (group, &count) in back.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let members: Vec<usize> = (0..first.len())
            .filter(|&j| to_strategy[j] == group)
            .collect();
        let held: Vec<u32> = members.iter().map(|&j| first[j]).collect();
        let mut next = Vec::new();
        chip_stats::for_each_composition(&held, count, |take, q| {
            for (partial, w) in &ways {
                let mut way = partial.clone();
                for (&j, &t) in members.iter().zip(take) {
                    way[j] = t;
                }
                next.push((way, w * q));
            }
        });
        ways = next;
    }
    ways
}
