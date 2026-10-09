//! The count-only walk as one kernel.
//!
//! One thread walks every path below one prefix: the host deals the first
//! `known` checkpoints with the engine's own `for_each_checkpoint_path`, and
//! each thread deals the rest. `chip_stats::descend` is recursive and a kernel
//! cannot be, so each checkpoint's composition is an odometer instead: the
//! first composition fills the last groups first, and the next is the
//! lexicographic successor, which is the order the recursive walk visits them
//! in. Bounded by fixed-size local arrays, which is why groups and checkpoints
//! are comptime.

use cubecl::prelude::*;

/// Columns a thread writes after its criteria: the mass it walked, and how
/// many paths it walked to get there.
pub const TAIL: usize = 2;

#[cube(launch_unchecked)]
#[allow(clippy::too_many_arguments)]
pub fn walk<F: Float>(
    sizes: &Array<u32>,
    gaps: &Array<u32>,
    prefix_hist: &Array<u32>,
    prefix_p: &Array<F>,
    binom: &Array<F>,
    clauses: &Array<u32>,
    conj_end: &Array<u32>,
    crit_end: &Array<u32>,
    out: &mut Array<F>,
    prefixes: u32,
    #[comptime] groups: usize,
    #[comptime] checkpoints: usize,
    #[comptime] known: usize,
    #[comptime] criteria: usize,
    #[comptime] stride: usize,
) {
    let me = ABSOLUTE_POS;
    if me < prefixes as usize {
        let mut hist = Array::<u32>::new(comptime!(checkpoints * groups));
        let mut take = Array::<u32>::new(comptime!(checkpoints * groups));
        let mut prob = Array::<F>::new(checkpoints);
        let mut acc = Array::<F>::new(comptime!(criteria + TAIL));
        for c in 0..criteria + TAIL {
            acc[c] = F::new(0.0_f32);
        }
        for i in 0..known * groups {
            hist[i] = prefix_hist[me * known * groups + i];
        }
        let p0 = prefix_p[me];

        if known == checkpoints {
            leaf(
                &hist, clauses, conj_end, crit_end, &mut acc, p0, groups, criteria,
            );
        } else {
            // `.runtime()`, or the level is a comptime constant and every
            // `d += 1` below happens once, at expansion, instead of per step.
            let mut d = known.runtime();
            first(sizes, gaps, &hist, &mut take, d, groups);
            let mut done = false;
            while !done {
                // Checkpoint `d` is dealt: record it and its probability.
                let mut num = F::new(1.0_f32);
                let mut avail_total = 0u32;
                for g in 0..groups {
                    let left = avail(sizes, &hist, d, g, groups);
                    let t = take[d * groups + g];
                    avail_total += left;
                    num *= binom[left as usize * stride + t as usize];
                    hist[d * groups + g] = sizes[g] - left + t;
                }
                let mut parent = p0;
                if d > known {
                    parent = prob[d - 1];
                }
                prob[d] = parent * num / binom[avail_total as usize * stride + gaps[d] as usize];

                if d + 1 < checkpoints {
                    d += 1;
                    first(sizes, gaps, &hist, &mut take, d, groups);
                } else {
                    leaf(
                        &hist, clauses, conj_end, crit_end, &mut acc, prob[d], groups, criteria,
                    );
                    // Advance the deepest level that still has a successor.
                    let mut climbing = true;
                    while climbing {
                        if next(sizes, &hist, &mut take, d, groups) {
                            climbing = false;
                        } else if d == known {
                            climbing = false;
                            done = true;
                        } else {
                            d -= 1;
                        }
                    }
                }
            }
        }
        for c in 0..criteria + TAIL {
            out[me * (criteria + TAIL) + c] = acc[c];
        }
    }
}

/// What checkpoint `d` can still deal from group `g`.
#[cube]
fn avail(sizes: &Array<u32>, hist: &Array<u32>, d: usize, g: usize, groups: usize) -> u32 {
    let mut before = 0u32;
    if d > 0 {
        before = hist[(d - 1) * groups + g];
    }
    sizes[g] - before
}

/// The first composition of checkpoint `d`'s gap: the last groups filled
/// first, so the first group takes as few as it can.
#[cube]
fn first(
    sizes: &Array<u32>,
    gaps: &Array<u32>,
    hist: &Array<u32>,
    take: &mut Array<u32>,
    d: usize,
    #[comptime] groups: usize,
) {
    let mut rem = gaps[d];
    for i in 0..groups {
        let g = groups - 1 - i;
        let t = min(avail(sizes, hist, d, g, groups), rem);
        take[d * groups + g] = t;
        rem -= t;
    }
}

/// Step checkpoint `d` to its next composition, or say it has none.
#[cube]
fn next(
    sizes: &Array<u32>,
    hist: &Array<u32>,
    take: &mut Array<u32>,
    d: usize,
    #[comptime] groups: usize,
) -> bool {
    // The rightmost group that can take one more while a group after it gives
    // one back; everything after it is then refilled from the end.
    let mut suffix = take[d * groups + groups - 1];
    let mut found = false;
    let mut i = comptime!(groups - 1).runtime();
    while i > 0 && !found {
        i -= 1;
        let t = take[d * groups + i];
        if suffix > 0 && t < avail(sizes, hist, d, i, groups) {
            found = true;
            take[d * groups + i] = t + 1;
            let mut rem = suffix - 1;
            for j in 0..groups - 1 - i {
                let g = groups - 1 - j;
                let a = min(avail(sizes, hist, d, g, groups), rem);
                take[d * groups + g] = a;
                rem -= a;
            }
        } else {
            suffix += t;
        }
    }
    found
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn leaf<F: Float>(
    hist: &Array<u32>,
    clauses: &Array<u32>,
    conj_end: &Array<u32>,
    crit_end: &Array<u32>,
    acc: &mut Array<F>,
    p: F,
    #[comptime] groups: usize,
    #[comptime] criteria: usize,
) {
    let mut conj = 0usize;
    let mut clause = 0usize;
    for c in 0..criteria {
        let mut any = false;
        let conj_stop = crit_end[c] as usize;
        while conj < conj_stop {
            let clause_stop = conj_end[conj] as usize;
            let mut all = true;
            while clause < clause_stop {
                // Every clause is read even once `all` fails: skipping costs
                // a branch per clause, and they are few.
                all = all && holds(hist, clauses, clause, groups);
                clause += 1;
            }
            any = any || all;
            conj += 1;
        }
        if any {
            acc[c] += p;
        }
    }
    acc[criteria] += p;
    acc[criteria + 1] += F::new(1.0_f32);
}

#[cube]
fn holds(hist: &Array<u32>, clauses: &Array<u32>, i: usize, #[comptime] groups: usize) -> bool {
    let turn = clauses[i * 6] as usize;
    let reads = clauses[i * 6 + 1];
    let mask = clauses[i * 6 + 2];
    let lo = clauses[i * 6 + 3];
    let hi = clauses[i * 6 + 4];
    let total = clauses[i * 6 + 5];
    let mut count = 0u32;
    if reads == 0 || reads == 1 {
        for g in 0..groups {
            if (mask >> g as u32) & 1 == 1 {
                count += hist[turn * groups + g];
            }
        }
        if reads == 1 {
            count = total - count;
        }
    } else if reads == 2 {
        // `Board::played_by` with nothing declared: one drop a turn, and a
        // drop that found no land is gone.
        for t in 1..turn + 1 {
            let mut drawn = 0u32;
            for g in 0..groups {
                if (mask >> g as u32) & 1 == 1 {
                    drawn += hist[t * groups + g];
                }
            }
            count = min(drawn, count + 1);
        }
    }
    count >= lo && count <= hi
}
