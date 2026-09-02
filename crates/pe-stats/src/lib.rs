//! Exact draw probabilities.
//!
//! Everything here is closed-form. There is no shuffler and no sampling, so
//! there is no sampler bias to chase — which matters, because the tooling this
//! replaces rejected two PRNG prototypes for producing confidently wrong
//! numbers (an LCG that dealt 82% of opening hands at exactly one land, and an
//! `awk srand()` keystream that biased the mean by eight standard errors).
//!
//! Binomial coefficients go through log-gamma rather than factorials: C(99,17)
//! is about 1.3e19, so the naive form overflows u64 on perfectly ordinary
//! Commander questions.

use libm::lgamma;

/// A probability, guaranteed to be in [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Probability(f64);

impl Probability {
    /// Clamps rather than rejects: exact arithmetic in floating point can land
    /// a hair outside the interval, and 1.0000000000000002 is not an error
    /// worth propagating to a caller.
    pub fn new(v: f64) -> Self {
        debug_assert!(v.is_finite(), "probability must be finite, got {v}");
        Probability(v.clamp(0.0, 1.0))
    }

    pub fn get(self) -> f64 {
        self.0
    }

    pub fn percent(self) -> f64 {
        self.0 * 100.0
    }
}

/// ln C(n, k). Zero (as -inf in log space) outside 0 <= k <= n.
pub fn ln_choose(n: u32, k: u32) -> f64 {
    if k > n {
        return f64::NEG_INFINITY;
    }
    let (n, k) = (f64::from(n), f64::from(k));
    lgamma(n + 1.0) - lgamma(k + 1.0) - lgamma(n - k + 1.0)
}

/// P(exactly `k` successes) drawing `draws` from `population` containing
/// `successes` successes.
pub fn pmf(population: u32, successes: u32, draws: u32, k: u32) -> f64 {
    if k > successes || k > draws || draws > population {
        return 0.0;
    }
    if (draws - k) > (population - successes) {
        return 0.0;
    }
    let ln = ln_choose(successes, k) + ln_choose(population - successes, draws - k)
        - ln_choose(population, draws);
    ln.exp()
}

/// P(at least `k` successes).
pub fn at_least(population: u32, successes: u32, draws: u32, k: u32) -> Probability {
    if k == 0 {
        return Probability::new(1.0);
    }
    // Sum the short tail: below k is at most k terms, above is up to `draws`.
    let below: f64 = (0..k).map(|i| pmf(population, successes, draws, i)).sum();
    Probability::new(1.0 - below)
}

/// Mean successes in `draws`. Closed form: draws * successes / population.
pub fn mean(population: u32, successes: u32, draws: u32) -> f64 {
    f64::from(draws) * f64::from(successes) / f64::from(population)
}

/// Standard deviation of successes in `draws`, with the finite-population
/// correction that separates this from a binomial.
pub fn sd(population: u32, successes: u32, draws: u32) -> f64 {
    let (n, k, d) = (
        f64::from(population),
        f64::from(successes),
        f64::from(draws),
    );
    let p = k / n;
    (d * p * (1.0 - p) * ((n - d) / (n - 1.0))).sqrt()
}

/// Enumerate every way `draws` cards can be split across `groups`, calling `f`
/// with the per-group counts and the exact probability of that split.
///
/// This is the multivariate hypergeometric. Groups are formed by which queries
/// a card matches, so their number is bounded by the query count rather than by
/// the deck size — cards matching nothing collapse into a single group.
pub fn for_each_composition(groups: &[u32], draws: u32, mut f: impl FnMut(&[u32], f64)) {
    let population: u32 = groups.iter().sum();
    if draws > population {
        return;
    }
    // C(N,K) is the denominator, so it enters the log accumulator negated.
    let ln_total = ln_choose(population, draws);
    let mut counts = vec![0u32; groups.len()];
    walk(groups, draws, 0, -ln_total, &mut counts, &mut f);
}

fn walk(
    groups: &[u32],
    remaining: u32,
    idx: usize,
    ln_acc: f64,
    counts: &mut Vec<u32>,
    f: &mut impl FnMut(&[u32], f64),
) {
    if idx == groups.len() {
        if remaining == 0 {
            f(counts, ln_acc.exp());
        }
        return;
    }
    // The tail must be able to absorb whatever this level does not take.
    let tail: u32 = groups[idx + 1..].iter().sum();
    let lo = remaining.saturating_sub(tail);
    let hi = remaining.min(groups[idx]);
    for take in lo..=hi {
        counts[idx] = take;
        walk(
            groups,
            remaining - take,
            idx + 1,
            ln_acc + ln_choose(groups[idx], take),
            counts,
            f,
        );
    }
    counts[idx] = 0;
}

/// Probability that a composition of `draws` over `groups` satisfies `pred`.
pub fn probability_that(
    groups: &[u32],
    draws: u32,
    mut pred: impl FnMut(&[u32]) -> bool,
) -> Probability {
    let mut total = 0.0;
    for_each_composition(groups, draws, |counts, p| {
        if pred(counts) {
            total += p;
        }
    });
    Probability::new(total)
}

/// Cumulative per-group counts at each checkpoint: `[checkpoint][group]`.
pub type Path<'a> = &'a [Vec<u32>];

/// Enumerate draws across successive checkpoints, exactly.
///
/// `gaps[j]` is how many *additional* cards are drawn to reach checkpoint `j`,
/// so `[7, 1, 1]` is "opening hand, then one draw, then one more". `f` receives
/// the cumulative counts at every checkpoint and the joint probability of that
/// whole path.
///
/// This is what makes turn-indexed questions answerable. "A land and a dork by
/// turn one, and a second land by turn two" is not two independent events —
/// the prefixes are nested, so the answer needs the joint distribution. Because
/// each gap is drawn from what the previous checkpoints left behind, the chain
/// is Markov and stays exact.
pub fn for_each_checkpoint_path(groups: &[u32], gaps: &[u32], mut f: impl FnMut(Path<'_>, f64)) {
    let population: u32 = groups.iter().sum();
    if gaps.iter().sum::<u32>() > population {
        return;
    }
    let mut history: Vec<Vec<u32>> = Vec::with_capacity(gaps.len());
    let mut drawn = vec![0u32; groups.len()];
    descend(groups, gaps, 0, &mut drawn, &mut history, 1.0, &mut f);
}

fn descend(
    groups: &[u32],
    gaps: &[u32],
    depth: usize,
    drawn: &mut Vec<u32>,
    history: &mut Vec<Vec<u32>>,
    acc: f64,
    f: &mut impl FnMut(Path<'_>, f64),
) {
    if depth == gaps.len() {
        f(history, acc);
        return;
    }
    // What is still in the library, per group.
    let available: Vec<u32> = groups
        .iter()
        .zip(drawn.iter())
        .map(|(total, used)| total - used)
        .collect();

    for_each_composition(&available, gaps[depth], |take, p| {
        for (d, t) in drawn.iter_mut().zip(take.iter()) {
            *d += t;
        }
        history.push(drawn.clone());
        descend(groups, gaps, depth + 1, drawn, history, acc * p, f);
        history.pop();
        for (d, t) in drawn.iter_mut().zip(take.iter()) {
            *d -= t;
        }
    });
}

/// Probability that a checkpoint path satisfies `pred`.
pub fn probability_that_path(
    groups: &[u32],
    gaps: &[u32],
    mut pred: impl FnMut(Path<'_>) -> bool,
) -> Probability {
    let mut total = 0.0;
    for_each_checkpoint_path(groups, gaps, |history, p| {
        if pred(history) {
            total += p;
        }
    });
    Probability::new(total)
}
