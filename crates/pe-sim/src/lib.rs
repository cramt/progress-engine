//! Sampling, for criteria the exact engine cannot express.
//!
//! This exists as an escape hatch, not as the default. Everything the exact
//! engine can answer, it should — a sampled answer carries error bars that an
//! enumerated one does not.
//!
//! Randomness is `rand`'s ChaCha generator and a textbook partial Fisher-Yates,
//! never anything hand-rolled. The tooling this crate grew alongside rejected
//! two home-made PRNGs for producing confidently wrong numbers: an LCG that
//! dealt 82% of opening hands at exactly one land against a true 16.4%, and an
//! `awk srand()` keystream that biased the mean lands-in-seven by eight standard
//! errors. The acceptance test for touching any of this is the hypergeometric
//! distribution, and it is enforced in this crate's tests.

use pe_criteria::{Evaluator, Grouping, PathView, Plan};
use pe_stats::{Distribution, DistributionBuilder};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimError<E> {
    /// Asking for more cards than the library holds used to deal what it could
    /// and answer anyway, while the exact engine returned 0% for the same
    /// question. Two engines, a hundred points apart, neither complaining.
    #[error("this question draws {draws} cards from a library of {population}")]
    NotEnoughCards { population: u32, draws: u32 },
    #[error("no hands to deal: --trials must be greater than zero")]
    NoTrials,
    /// Worded identically to the exact engine's refusal of the same mistake.
    /// The two engines are supposed to be interchangeable behind one call site,
    /// and a shape mismatch is a fact about the evaluator rather than about how
    /// the answer is computed.
    #[error(
        "the criteria answered {held} criteria and {counted} expectations, but this run was \
         set up for {} and {}",
        .plan.criteria, .plan.expectations
    )]
    WrongShape {
        plan: Plan,
        held: usize,
        counted: usize,
    },
    #[error("evaluating criteria: {0}")]
    Evaluator(E),
}

/// What a sampled run measured. Estimates, every one of them.
///
/// Proportions rather than `Probability` values, and a sampled histogram rather
/// than an exact one: pair them with [`standard_error`] and
/// [`mean_standard_error`] before believing a digit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sampled {
    /// One per criterion: the fraction of hands in which it held.
    pub proportions: Vec<f64>,
    /// One per expectation: how often each value came up, as a fraction.
    pub distributions: Vec<Distribution>,
}

/// Deal `trials` hands and report how often each criterion held, and how the
/// values of each expectation were distributed.
///
/// A sampler computes a mean by averaging and a distribution by histogramming,
/// so the second kind of question costs it no more than the first — which
/// matters, because a question only one engine can answer is a question nothing
/// checks.
pub fn simulate<E>(
    grouping: &Grouping,
    gaps: &[u32],
    trials: u32,
    seed: u64,
    plan: Plan,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Sampled, SimError<E>> {
    let population = grouping.population();
    let total_draws: u32 = gaps.iter().sum();
    if total_draws > population {
        return Err(SimError::NotEnoughCards {
            population,
            draws: total_draws,
        });
    }
    if trials == 0 {
        return Err(SimError::NoTrials);
    }

    // The library as one card per slot, each holding the index of its group.
    let mut deck: Vec<u16> = Vec::with_capacity(grouping.population() as usize);
    for (group, &size) in grouping.group_sizes().iter().enumerate() {
        for _ in 0..size {
            deck.push(group as u16);
        }
    }

    // How many cards have been seen by each checkpoint.
    let reached_at: Vec<u32> = gaps
        .iter()
        .scan(0u32, |acc, gap| {
            *acc += gap;
            Some(*acc)
        })
        .collect();

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let groups = grouping.group_sizes().len();
    let mut hits: Vec<u32> = vec![0; plan.criteria];
    let mut histograms: Vec<DistributionBuilder> =
        vec![DistributionBuilder::new(); plan.expectations];
    // Every hand contributes the same share, so the buckets add up to 1 rather
    // than to a count that the caller would have to remember to divide.
    let share = 1.0 / f64::from(trials);

    for _ in 0..trials {
        // Partial Fisher-Yates: only shuffle as far as we actually draw.
        let n = deck.len();
        let mut cumulative = vec![0u32; groups];
        let mut history: Vec<Vec<u32>> = Vec::with_capacity(gaps.len());
        let mut checkpoint = 0usize;

        for i in 0..(total_draws as usize).min(n) {
            // Snapshot before dealing, because a checkpoint can be reached
            // before any card is: a leading gap of zero means "the hand as it
            // stands", and recording it after the next draw reports a card the
            // player has not seen yet.
            while checkpoint < gaps.len() && reached_at[checkpoint] as usize <= i {
                history.push(cumulative.clone());
                checkpoint += 1;
            }
            let j = rng.random_range(i..n);
            deck.swap(i, j);
            cumulative[deck[i] as usize] += 1;
        }
        // Whatever the last draw reached, plus any trailing gaps of zero.
        while checkpoint < gaps.len() {
            history.push(cumulative.clone());
            checkpoint += 1;
        }

        let view = PathView::new(grouping, &history);
        let results = evaluator.evaluate(&view).map_err(SimError::Evaluator)?;
        if results.held.len() != plan.criteria || results.counted.len() != plan.expectations {
            return Err(SimError::WrongShape {
                plan,
                held: results.held.len(),
                counted: results.counted.len(),
            });
        }
        for (h, r) in hits.iter_mut().zip(results.held) {
            if r {
                *h += 1;
            }
        }
        for (histogram, value) in histograms.iter_mut().zip(results.counted) {
            histogram.add(value.get(), share);
        }
    }

    Ok(Sampled {
        proportions: hits
            .into_iter()
            .map(|h| f64::from(h) / f64::from(trials))
            .collect(),
        distributions: histograms
            .into_iter()
            .map(DistributionBuilder::build)
            .collect(),
    })
}

/// Standard error of a proportion estimated from `trials` samples.
///
/// Quoting a sampled figure without this is how a 38.5% and a 39.0% get
/// mistaken for a disagreement.
pub fn standard_error(proportion: f64, trials: u32) -> f64 {
    (proportion * (1.0 - proportion) / f64::from(trials)).sqrt()
}

/// Standard error of a mean estimated from `trials` samples.
///
/// The proportion form above cannot serve here: a proportion's spread is fixed
/// by the proportion itself, and a mean's is not. Two expectations averaging 2.7
/// have wildly different error bars if one is always 2 or 3 and the other is
/// sometimes 0 and sometimes 7, so the spread has to come out of the sampled
/// distribution rather than out of its mean.
pub fn mean_standard_error(distribution: &Distribution, trials: u32) -> f64 {
    distribution.sd() / f64::from(trials).sqrt()
}
