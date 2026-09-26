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

/// Compensated (Kahan) summation.
///
/// Naive `f64` accumulation drifts with the number of terms, and this crate
/// enumerates a lot of them. Measured against an exact total of 1: naive
/// summation was off by 8.8e-12 over 4.3M compositions, 3.9e-11 over 17.9M
/// checkpoint paths and 2.9e-10 over 160M. Compensated summation held those
/// same enumerations to ~5e-14 whatever the term count, because what is left is
/// the log-gamma round trip inside each individual term rather than anything
/// that compounds. A flat error floor is what lets a caller check a total
/// against a tolerance tight enough to mean something.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KahanSum {
    sum: f64,
    lost: f64,
}

impl KahanSum {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, x: f64) {
        let corrected = x - self.lost;
        let next = self.sum + corrected;
        self.lost = (next - self.sum) - corrected;
        self.sum = next;
    }

    pub fn total(self) -> f64 {
        self.sum
    }
}

/// ln C(n, k). Zero (as -inf in log space) outside 0 <= k <= n.
pub fn ln_choose(n: u32, k: u32) -> f64 {
    if k > n {
        return f64::NEG_INFINITY;
    }
    ln_factorial(n) - ln_factorial(k) - ln_factorial(n - k)
}

/// How many ln n! are kept rather than recomputed: every count a library can
/// hold, with room to spare. A Commander library is 99 cards.
const LN_FACTORIALS: usize = 4096;

/// ln n!, read off a table where it can be.
///
/// The enumeration asks for the same handful of these millions of times — a
/// group of twelve cards has thirteen of them — and `lgamma` was a fifth of the
/// whole run. The table holds exactly what `lgamma(n + 1)` returns, computed
/// the same way, so a number read off it is the number computed without it,
/// bit for bit: this is a cache, not an approximation.
fn ln_factorial(n: u32) -> f64 {
    static TABLE: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| (0..LN_FACTORIALS).map(|n| lgamma(n as f64 + 1.0)).collect());
    match table.get(n as usize) {
        Some(&ln) => ln,
        None => lgamma(f64::from(n) + 1.0),
    }
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
    let mut total = KahanSum::new();
    for_each_composition(groups, draws, |counts, p| {
        if pred(counts) {
            total.add(p);
        }
    });
    Probability::new(total.total())
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
    struct Drawing<F>(F);
    impl<F: FnMut(Path<'_>, f64)> Walk for Drawing<F> {
        fn removals(&mut self, _reached: Path<'_>, _out: &mut [u32]) {}
        fn path(&mut self, reached: Path<'_>, p: f64) {
            (self.0)(reached, p)
        }
    }
    for_each_checkpoint_path_removing(groups, gaps, &mut Drawing(&mut f));
}

/// A walk over checkpoint paths whose population does not stay fixed.
///
/// [`for_each_checkpoint_path`] computes what is left as `groups - drawn`, so
/// the population is a constant of the whole path by construction. Some
/// questions need it not to be: cards can leave a group without having been
/// drawn, and everything after that is against a smaller population with a
/// different composition.
///
/// Two methods rather than two closures because the caller needs one piece of
/// state answering both — the thing that decides a removal is the thing that
/// knows what the path did — and two closures cannot share a borrow of it.
///
/// **Removals are deterministic.** Given the checkpoints reached so far there
/// is one answer, so a removal costs no branch: it is a subtraction from the
/// pool the next gap draws out of, not a second distribution laid over it. A
/// removal that *were* a random sample is a draw, and a draw is what `gaps`
/// already says.
pub trait Walk {
    /// Cumulative removals from each group by the end of the checkpoints in
    /// `reached`, written into `out`.
    ///
    /// Cumulative rather than incremental, because the caller recomputes from
    /// the path and a total is what it naturally holds. Called once per
    /// prefix, in descent order, before the gap that follows it is dealt.
    ///
    /// It must not shrink as `reached` grows, and it must not claim more of a
    /// group than that group has left. A caller deciding removals from the
    /// same counts this walk hands it cannot break either, which is why
    /// neither is a runtime refusal.
    fn removals(&mut self, reached: Path<'_>, out: &mut [u32]);

    /// One complete path and the joint probability of it, as
    /// [`for_each_checkpoint_path`]'s callback.
    fn path(&mut self, reached: Path<'_>, p: f64);
}

/// [`for_each_checkpoint_path`], against a population that shrinks as the walk
/// removes from it.
///
/// Each gap is drawn from `groups - drawn - removed`, and `removed` is asked
/// for at every checkpoint. Nothing else changes: the chain is still Markov,
/// every gap is still one multivariate hypergeometric, and the path
/// probabilities still sum to 1 — over a sample space that is now conditioned
/// on the removals, which is the point.
pub fn for_each_checkpoint_path_removing(groups: &[u32], gaps: &[u32], walk: &mut impl Walk) {
    let population: u32 = groups.iter().sum();
    if gaps.iter().sum::<u32>() > population {
        return;
    }
    let mut history: Vec<Vec<u32>> = Vec::with_capacity(gaps.len());
    let mut drawn = vec![0u32; groups.len()];
    // One slot per depth, allocated once: the walk visits millions of nodes and
    // a fresh vector at each of them would cost more than the removals do.
    let mut removed = vec![vec![0u32; groups.len()]; gaps.len() + 1];
    descend(
        groups,
        gaps,
        0,
        &mut drawn,
        &mut removed,
        &mut history,
        1.0,
        walk,
    );
}

/// [`for_each_checkpoint_path`], resumed from a first checkpoint that has
/// already been reached.
///
/// `first` is the per-group counts at checkpoint 0, and `gaps` are the draws
/// *after* it, so the histories handed to `f` are exactly the ones the full
/// walk over `[first.sum(), gaps...]` would hand it for the paths that start at
/// `first` — and the probabilities are conditional on having started there.
/// Multiplying them by the probability of `first` itself gives back the full
/// walk's terms, one for one.
///
/// What this is for is a caller that has to do something *between* the first
/// checkpoint and the rest of the walk that the walk cannot express: branch on
/// a choice made about the cards already drawn, say, where each branch deals
/// the same later draws but takes different removals out of them. Splitting the
/// walk at its first checkpoint lets that caller enumerate the first
/// checkpoint itself, branch, and resume — without the walk having to know
/// what a branch is.
pub fn for_each_checkpoint_path_after(
    groups: &[u32],
    first: &[u32],
    gaps: &[u32],
    mut f: impl FnMut(Path<'_>, f64),
) {
    struct Drawing<F>(F);
    impl<F: FnMut(Path<'_>, f64)> Walk for Drawing<F> {
        fn removals(&mut self, _reached: Path<'_>, _out: &mut [u32]) {}
        fn path(&mut self, reached: Path<'_>, p: f64) {
            (self.0)(reached, p)
        }
    }
    for_each_checkpoint_path_removing_after(groups, first, gaps, &mut Drawing(&mut f));
}

/// [`for_each_checkpoint_path_removing`], resumed from a first checkpoint that
/// has already been reached. See [`for_each_checkpoint_path_after`].
///
/// `walk` is asked for its removals after `first` exactly as the full walk
/// would ask it, before the first of `gaps` is dealt.
pub fn for_each_checkpoint_path_removing_after(
    groups: &[u32],
    first: &[u32],
    gaps: &[u32],
    walk: &mut impl Walk,
) {
    debug_assert_eq!(groups.len(), first.len(), "one count per group");
    debug_assert!(
        groups.iter().zip(first).all(|(g, f)| f <= g),
        "the first checkpoint drew more of a group than it holds"
    );
    let population: u32 = groups.iter().sum();
    if first.iter().sum::<u32>() + gaps.iter().sum::<u32>() > population {
        return;
    }
    let mut history: Vec<Vec<u32>> = Vec::with_capacity(gaps.len() + 1);
    history.push(first.to_vec());
    let mut drawn = first.to_vec();
    let mut removed = vec![vec![0u32; groups.len()]; gaps.len() + 1];
    // The question the full walk asks after checkpoint 0, asked here for the
    // same reason: the first gap is dealt out of whatever it says.
    if !gaps.is_empty() {
        walk.removals(&history, &mut removed[0]);
    }
    descend(
        groups,
        gaps,
        0,
        &mut drawn,
        &mut removed,
        &mut history,
        1.0,
        walk,
    );
}

#[allow(clippy::too_many_arguments)]
fn descend(
    groups: &[u32],
    gaps: &[u32],
    depth: usize,
    drawn: &mut Vec<u32>,
    removed: &mut Vec<Vec<u32>>,
    history: &mut Vec<Vec<u32>>,
    acc: f64,
    walk: &mut impl Walk,
) {
    if depth == gaps.len() {
        walk.path(history, acc);
        return;
    }
    // What is still in the library, per group: what was never drawn, less what
    // left without being drawn. `saturating_sub` because over-removing is a
    // caller bug rather than a state this walk can be in, and wrapping it
    // would hand the enumeration a group of four billion cards.
    let available: Vec<u32> = groups
        .iter()
        .zip(drawn.iter())
        .zip(removed[depth].iter())
        .map(|((total, used), gone)| {
            debug_assert!(used + gone <= *total, "removed more than the group holds");
            total.saturating_sub(*used).saturating_sub(*gone)
        })
        .collect();

    for_each_composition(&available, gaps[depth], |take, p| {
        for (d, t) in drawn.iter_mut().zip(take.iter()) {
            *d += t;
        }
        history.push(drawn.clone());
        // Asked after this checkpoint is on the record and before the next gap
        // is dealt, so a removal decided here is one the rest of the path
        // cannot draw. It starts from what had already been removed, so a walk
        // that removes nothing writes nothing.
        //
        // Not asked at the last checkpoint, and that is worth a third of the
        // run rather than being tidiness: there is no gap after it for a
        // removal to shrink, and the last checkpoint is where all the leaves
        // are.
        if depth + 1 < gaps.len() {
            let (here, next) = removed.split_at_mut(depth + 1);
            next[0].copy_from_slice(&here[depth]);
            walk.removals(history, &mut next[0]);
        }
        descend(
            groups,
            gaps,
            depth + 1,
            drawn,
            removed,
            history,
            acc * p,
            walk,
        );
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
    let mut total = KahanSum::new();
    for_each_checkpoint_path(groups, gaps, |history, p| {
        if pred(history) {
            total.add(p);
        }
    });
    Probability::new(total.total())
}

/// A probability distribution over small non-negative integers.
///
/// Dense from zero, so the index *is* the value: `probabilities()[k]` is
/// P(value = k), and a value nothing ever took still occupies its bucket at
/// zero. That is what makes it a histogram rather than a sparse map, and a
/// histogram with holes in it reads as missing data rather than as an unlikely
/// outcome.
///
/// The exact engine gets one of these for nothing. It already walks every
/// composition with that composition's exact probability, so bucketing by the
/// value a question took costs the same loop as testing whether it held — and
/// answers a strictly larger question, because a mean of 2.71 lands cannot tell
/// you whether you are flooding or screwing and the shape can.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Distribution {
    p: Vec<f64>,
}

impl Distribution {
    /// P(value = k) for every k from 0 to the largest value observed.
    pub fn probabilities(&self) -> &[f64] {
        &self.p
    }

    /// Total mass. 1 for a complete enumeration, which is what the caller
    /// checks rather than assumes.
    pub fn total(&self) -> f64 {
        let mut sum = KahanSum::new();
        for p in &self.p {
            sum.add(*p);
        }
        sum.total()
    }

    /// Expected value, summed over the same buckets that are reported.
    ///
    /// Deliberately not accumulated separately during the walk. A mean computed
    /// beside a histogram is a second answer to the same question, and the two
    /// can drift; computed *from* the histogram it cannot disagree with the
    /// numbers printed underneath it.
    pub fn mean(&self) -> f64 {
        let mut sum = KahanSum::new();
        for (k, p) in self.p.iter().enumerate() {
            sum.add(k as f64 * p);
        }
        sum.total()
    }

    /// Standard deviation, from the same buckets.
    pub fn sd(&self) -> f64 {
        let mean = self.mean();
        let mut sum = KahanSum::new();
        for (k, p) in self.p.iter().enumerate() {
            let d = k as f64 - mean;
            sum.add(d * d * p);
        }
        sum.total().max(0.0).sqrt()
    }
}

/// Accumulates a [`Distribution`] one weighted observation at a time.
///
/// One compensated sum per bucket rather than one for the whole thing: the
/// enumeration visits the buckets interleaved and in no useful order, and a
/// bucket holding a millionth of the mass would otherwise lose its low bits to
/// whichever bucket holds most of it.
#[derive(Debug, Clone, Default)]
pub struct DistributionBuilder {
    buckets: Vec<KahanSum>,
}

impl DistributionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, value: u32, p: f64) {
        let idx = value as usize;
        if idx >= self.buckets.len() {
            self.buckets.resize(idx + 1, KahanSum::new());
        }
        self.buckets[idx].add(p);
    }

    pub fn build(self) -> Distribution {
        Distribution {
            p: self.buckets.into_iter().map(KahanSum::total).collect(),
        }
    }
}
