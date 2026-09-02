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

use pe_criteria::{Evaluator, Grouping, PathView};
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
    #[error("evaluating criteria: {0}")]
    Evaluator(E),
}

/// Deal `trials` hands and report how often each criterion held.
///
/// Returns proportions rather than `Probability` values, because these are
/// estimates: pair them with [`standard_error`] before believing a digit.
pub fn simulate<E>(
    grouping: &Grouping,
    gaps: &[u32],
    trials: u32,
    seed: u64,
    evaluator: &mut impl Evaluator<Error = E>,
) -> Result<Vec<f64>, SimError<E>> {
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
    let mut hits: Vec<u32> = Vec::new();

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
        if hits.is_empty() {
            hits = vec![0; results.len()];
        }
        for (h, r) in hits.iter_mut().zip(results) {
            if r {
                *h += 1;
            }
        }
    }

    Ok(hits
        .into_iter()
        .map(|h| f64::from(h) / f64::from(trials))
        .collect())
}

/// Standard error of a proportion estimated from `trials` samples.
///
/// Quoting a sampled figure without this is how a 38.5% and a 39.0% get
/// mistaken for a disagreement.
pub fn standard_error(proportion: f64, trials: u32) -> f64 {
    (proportion * (1.0 - proportion) / f64::from(trials)).sqrt()
}
