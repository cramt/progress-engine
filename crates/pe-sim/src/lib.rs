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
) -> Result<Vec<f64>, E> {
    // The library as one card per slot, each holding the index of its group.
    let mut deck: Vec<u16> = Vec::with_capacity(grouping.population() as usize);
    for (group, &size) in grouping.group_sizes().iter().enumerate() {
        for _ in 0..size {
            deck.push(group as u16);
        }
    }

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let groups = grouping.group_sizes().len();
    let total_draws: u32 = gaps.iter().sum();
    let mut hits: Vec<u32> = Vec::new();

    for _ in 0..trials {
        // Partial Fisher-Yates: only shuffle as far as we actually draw.
        let n = deck.len();
        let mut cumulative = vec![0u32; groups];
        let mut history: Vec<Vec<u32>> = Vec::with_capacity(gaps.len());
        let mut drawn = 0u32;
        let mut checkpoint = 0usize;

        for i in 0..(total_draws as usize).min(n) {
            let j = rng.random_range(i..n);
            deck.swap(i, j);
            cumulative[deck[i] as usize] += 1;
            drawn += 1;
            // Snapshot at every checkpoint boundary, including gaps of zero.
            while checkpoint < gaps.len() && drawn >= gaps[..=checkpoint].iter().sum::<u32>() {
                history.push(cumulative.clone());
                checkpoint += 1;
            }
        }
        // Gaps of zero at the very start leave checkpoints unfilled.
        while history.len() < gaps.len() {
            history.push(cumulative.clone());
        }

        let view = PathView::new(grouping, &history);
        let results = evaluator.evaluate(&view)?;
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
