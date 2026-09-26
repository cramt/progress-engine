//! Known-answer tests for the exact draw probabilities.
//!
//! The first fixture is deliberately the constant already documented in the
//! shuffler acceptance test of the tool this crate grew out of: "for 36 lands in
//! 99 cards, n=7: mean 2.5457, SD 1.2331". That number was derived out of band
//! and then hand-copied into two files. Now it is computed, so the new code and
//! the old acceptance test validate each other instead of both being magic.

use chip_stats::{self as h, Probability};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

#[test]
fn reproduces_the_documented_shuffler_constants() {
    // 36 lands in a 99-card library, opening seven.
    assert!(
        close(h::mean(99, 36, 7), 2.545455, 1e-6),
        "mean was {}",
        h::mean(99, 36, 7)
    );
    assert!(
        close(h::sd(99, 36, 7), 1.2331509, 1e-6),
        "sd was {}",
        h::sd(99, 36, 7)
    );
}

#[test]
fn the_distribution_sums_to_one() {
    let total: f64 = (0..=7).map(|k| h::pmf(99, 36, 7, k)).sum();
    assert!(close(total, 1.0, 1e-12), "summed to {total}");
}

#[test]
fn mean_and_sd_agree_with_the_enumerated_distribution() {
    // The closed forms and the pmf must not drift apart.
    let mean: f64 = (0..=7).map(|k| f64::from(k) * h::pmf(99, 36, 7, k)).sum();
    let ex2: f64 = (0..=7)
        .map(|k| f64::from(k * k) * h::pmf(99, 36, 7, k))
        .sum();
    assert!(close(mean, h::mean(99, 36, 7), 1e-9));
    assert!(close((ex2 - mean * mean).sqrt(), h::sd(99, 36, 7), 1e-9));
}

#[test]
fn at_least_one_of_six_in_eleven() {
    // Turn 5 on the play sees 11 cards. Six safe arming outlets in 99.
    let p = h::at_least(99, 6, 11, 1);
    assert!(close(p.percent(), 51.6, 0.05), "was {}", p.percent());
}

#[test]
fn at_least_one_of_eight_in_eleven() {
    let p = h::at_least(99, 8, 11, 1);
    assert!(close(p.percent(), 62.5, 0.05), "was {}", p.percent());
}

#[test]
fn edge_cases() {
    assert_eq!(h::at_least(99, 6, 11, 0), Probability::new(1.0));
    // Cannot draw more successes than exist.
    assert_eq!(h::pmf(99, 6, 11, 7), 0.0);
    // Cannot draw more cards than the deck holds.
    assert_eq!(h::pmf(10, 5, 11, 1), 0.0);
    // Every card is a success.
    assert!(close(h::at_least(10, 10, 1, 1).get(), 1.0, 1e-12));
    assert!(close(h::at_least(10, 0, 5, 1).get(), 0.0, 1e-12));
}

#[test]
fn compositions_are_a_probability_distribution() {
    let mut total = 0.0;
    h::for_each_composition(&[6, 8, 85], 11, |_, p| total += p);
    assert!(close(total, 1.0, 1e-12), "summed to {total}");
}

#[test]
fn composition_marginals_match_the_univariate_case() {
    // Marginalising the multivariate enumeration over one group must reproduce
    // the closed-form univariate answer, or the two engines disagree.
    let p = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1);
    assert!(
        close(p.get(), h::at_least(99, 6, 11, 1).get(), 1e-12),
        "{} vs {}",
        p.get(),
        h::at_least(99, 6, 11, 1).get()
    );
}

#[test]
fn the_thirty_one_versus_thirty_nine_percent_story() {
    // The disagreement that motivated this crate. Same deck, same question,
    // same turn: "do I have a safe arming outlet AND an evasive connector by
    // turn 5?" One analysis counted 8 connectors, another counted 12. Both
    // answers were defensible and neither definition was written down.
    let strict = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1 && c[1] >= 1);
    let wide = h::probability_that(&[6, 12, 81], 11, |c| c[0] >= 1 && c[1] >= 1);

    assert!(
        close(strict.percent(), 31.0, 0.05),
        "strict was {}",
        strict.percent()
    );
    assert!(
        close(wide.percent(), 39.0, 0.05),
        "wide was {}",
        wide.percent()
    );

    // And the wide figure matches the independent Monte Carlo run that produced
    // 38.5% by simulation, within its sampling error.
    assert!((wide.percent() - 38.5).abs() < 1.0);
}

#[test]
fn overlapping_categories_are_handled_by_grouping() {
    // A card counting for two roles at once is exactly what breaks naive
    // inclusion-exclusion. Group [both, only-a, only-b, neither] instead.
    // With 2 cards that are both, P(a and b) must exceed the disjoint case.
    let disjoint = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1 && c[1] >= 1);
    let overlapping =
        h::probability_that(&[2, 4, 6, 87], 11, |c| c[0] + c[1] >= 1 && c[0] + c[2] >= 1);
    assert!(overlapping.get() > 0.0 && overlapping.get() < 1.0);
    assert!(disjoint.get() > 0.0);
}

// --- checkpoint paths -------------------------------------------------------

#[test]
fn a_single_checkpoint_path_equals_a_plain_composition() {
    // The two enumerators must agree where they overlap, or turn-indexed
    // questions silently disagree with single-turn ones.
    let via_path = h::probability_that_path(&[6, 8, 85], &[11], |hist| hist[0][0] >= 1);
    let via_comp = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1);
    assert!(close(via_path.get(), via_comp.get(), 1e-12));
}

#[test]
fn path_probabilities_sum_to_one() {
    let mut total = 0.0;
    h::for_each_checkpoint_path(&[36, 10, 53], &[7, 1, 1], |_, p| total += p);
    assert!(close(total, 1.0, 1e-10), "summed to {total}");
}

#[test]
fn the_final_checkpoint_marginal_matches_drawing_that_many_at_once() {
    // Splitting 9 cards into 7+1+1 must not change the distribution at the end.
    let stepwise = h::probability_that_path(&[36, 10, 53], &[7, 1, 1], |hist| hist[2][0] >= 4);
    let at_once = h::probability_that(&[36, 10, 53], 9, |c| c[0] >= 4);
    assert!(
        close(stepwise.get(), at_once.get(), 1e-10),
        "{} vs {}",
        stepwise.get(),
        at_once.get()
    );
}

#[test]
fn the_curve_out_question() {
    // "A land and a mana dork on turn one, so the three-drop commander lands on
    // turn two" — 36 lands, 10 one-mana dorks, on the play.
    let groups = [36, 10, 53];
    let gaps = [7, 1];
    let p = h::probability_that_path(&groups, &gaps, |hist| {
        hist[0][0] >= 1 && hist[0][1] >= 1 && hist[1][0] >= 2
    });

    // Must be bounded by its own components: it cannot beat either the
    // opening-hand requirement alone or the turn-two land requirement alone.
    let opener_only =
        h::probability_that_path(&groups, &gaps, |hist| hist[0][0] >= 1 && hist[0][1] >= 1);
    let lands_only = h::probability_that_path(&groups, &gaps, |hist| hist[1][0] >= 2);
    assert!(p.get() <= opener_only.get());
    assert!(p.get() <= lands_only.get());
    assert!(p.get() > 0.0);

    // And the nesting must be respected: requiring 2 lands by turn 1 is strictly
    // harder than by turn 2, since turn 2 has seen one more card.
    let by_t1 = h::probability_that_path(&groups, &gaps, |hist| hist[0][0] >= 2);
    let by_t2 = h::probability_that_path(&groups, &gaps, |hist| hist[1][0] >= 2);
    assert!(
        by_t1.get() < by_t2.get(),
        "{} vs {}",
        by_t1.get(),
        by_t2.get()
    );
}

#[test]
fn drawing_more_cards_than_the_deck_holds_yields_nothing() {
    let mut called = false;
    h::for_each_checkpoint_path(&[3, 2], &[10], |_, _| called = true);
    assert!(!called);
}

// --- total probability mass -------------------------------------------------

#[test]
fn composition_mass_sums_to_one_across_group_shapes() {
    // The enumeration partitions the sample space, so whatever the shape of the
    // groups, the pieces must add back up to the whole.
    let shapes: [(&[u32], u32); 7] = [
        (&[99], 7),
        (&[36, 63], 7),
        (&[6, 8, 85], 11),
        (&[2, 4, 6, 87], 11),
        (&[1, 1, 1, 1, 1, 1, 1, 1, 1, 90], 7),
        (&[10, 10, 10, 10, 10, 10, 10, 10, 10, 9], 12),
        (&[5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5], 7),
    ];
    for (groups, draws) in shapes {
        let mut mass = h::KahanSum::new();
        h::for_each_composition(groups, draws, |_, p| mass.add(p));
        assert!(
            close(mass.total(), 1.0, 1e-12),
            "{groups:?} drawing {draws} summed to {}",
            mass.total()
        );
    }
}

#[test]
fn checkpoint_path_mass_sums_to_one_across_gap_vectors() {
    // Splitting the same draws into more checkpoints multiplies the path count
    // without changing the total, which is exactly the property worth checking:
    // the deepest case below is 122,880 paths and must still land on 1.
    let shapes: [(&[u32], &[u32]); 7] = [
        (&[36, 10, 53], &[7]),
        (&[36, 10, 53], &[7, 1]),
        (&[36, 10, 53], &[7, 1, 1]),
        (&[36, 10, 53], &[7, 2, 3]),
        (&[36, 10, 53], &[7, 1, 1, 1, 1, 1, 1, 1]),
        (&[20, 20, 20, 39], &[7, 1, 1, 1, 1, 1]),
        // Drawing the library down to nothing, one card at a time.
        (&[3, 2], &[1, 1, 1, 1, 1]),
    ];
    for (groups, gaps) in shapes {
        let mut mass = h::KahanSum::new();
        h::for_each_checkpoint_path(groups, gaps, |_, p| mass.add(p));
        assert!(
            close(mass.total(), 1.0, 1e-12),
            "{groups:?} over gaps {gaps:?} summed to {}",
            mass.total()
        );
    }
}

#[test]
fn a_dropped_branch_shows_up_as_missing_mass() {
    // The failure the mass check exists to catch: an enumerator that returns
    // early over part of the sample space. Nothing about the surviving answer
    // looks wrong — it is still a probability, still in range, still close to
    // the truth. Only the total gives it away.
    let groups = [12, 8, 79];
    let gaps = [7, 1, 1];

    let truth = h::probability_that_path(&groups, &gaps, |hist| hist[2][0] >= 1);

    let mut kept = h::KahanSum::new();
    let mut hits = h::KahanSum::new();
    h::for_each_checkpoint_path(&groups, &gaps, |hist, p| {
        // Stand-in for a bound that is off by one.
        if hist[0][0] >= 5 {
            return;
        }
        kept.add(p);
        if hist[2][0] >= 1 {
            hits.add(p);
        }
    });

    let silently_wrong = hits.total();
    assert!(silently_wrong > 0.0 && silently_wrong < 1.0);
    assert!(
        close(silently_wrong, truth.get(), 0.01),
        "the point is that this stays plausible: {silently_wrong} vs {}",
        truth.get()
    );

    let lost = 1.0 - kept.total();
    assert!(
        lost > 1e-9,
        "the mass check must be able to see this, but only {lost} went missing"
    );
}

#[test]
fn compensated_summation_keeps_what_naive_addition_throws_away() {
    // Every term here is below the spacing of f64 near 1, so naive addition
    // drops all hundred of them while their total is not negligible.
    let mut naive = 1.0f64;
    let mut kahan = h::KahanSum::new();
    kahan.add(1.0);
    for _ in 0..100 {
        naive += 1e-17;
        kahan.add(1e-17);
    }
    assert_eq!(naive, 1.0, "naive summation should have lost all of it");
    let recovered = kahan.total() - 1.0;
    assert!(
        (recovered - 1e-15).abs() <= f64::EPSILON,
        "recovered {recovered}"
    );
}

// --- removals ---------------------------------------------------------------
//
// A population that shrinks without being drawn from. Deliberately small
// enough that every answer below is worked out by hand in the comment beside
// it, and deliberately stated in this crate's own vocabulary — groups, draws,
// removals — with nothing about what a removal represents.

/// A walk that takes `each` more cards out of group `from` at every
/// checkpoint, up to `most` of them in total and never more of the group than
/// is still there.
struct TakesFrom<F> {
    sizes: &'static [u32],
    from: usize,
    each: u32,
    most: u32,
    seen: F,
}

impl<F: FnMut(h::Path<'_>, f64)> h::Walk for TakesFrom<F> {
    fn removals(&mut self, reached: h::Path<'_>, out: &mut [u32]) {
        let drawn = reached[reached.len() - 1][self.from];
        let want = reached.len() as u32 * self.each;
        out[self.from] = want.min(self.most).min(self.sizes[self.from] - drawn);
    }
    fn path(&mut self, reached: h::Path<'_>, p: f64) {
        (self.seen)(reached, p)
    }
}

#[test]
fn a_removal_changes_what_the_next_draw_is_drawn_from() {
    // Two groups of two. Draw one, remove one from group 0, draw one more.
    //
    //   drew group 0 (1/2): [0, 2] is left, so the second draw is group 1.
    //   drew group 1 (1/2): [1, 1] is left, so it is even.
    //
    // P(exactly one card of group 0 in hand) = 1/2 + 1/2 * 1/2 = 3/4.
    let mut held = h::KahanSum::new();
    let mut mass = h::KahanSum::new();
    h::for_each_checkpoint_path_removing(
        &[2, 2],
        &[1, 1],
        &mut TakesFrom {
            sizes: &[2, 2],
            from: 0,
            each: 1,
            most: 1,
            seen: |hist: h::Path<'_>, p: f64| {
                mass.add(p);
                if hist[1][0] == 1 {
                    held.add(p);
                }
            },
        },
    );
    assert!(close(mass.total(), 1.0, 1e-12), "mass {}", mass.total());
    assert!(close(held.total(), 0.75, 1e-12), "was {}", held.total());

    // The same two draws with nothing removed is a plain hypergeometric over
    // four cards, and it is a different number: C(2,1)C(2,1)/C(4,2) = 4/6.
    let plain = h::probability_that_path(&[2, 2], &[1, 1], |hist| hist[1][0] == 1);
    assert!(close(plain.get(), 4.0 / 6.0, 1e-12), "was {}", plain.get());
}

#[test]
fn removing_a_whole_group_first_is_the_same_as_never_having_had_it() {
    // A removal is a subtraction and nothing else, so taking every card of a
    // group out before anything is drawn has to leave exactly the walk over
    // the remaining groups. Checkpoint 0 draws nothing, which is where the
    // removal lands.
    let mut with = Vec::new();
    h::for_each_checkpoint_path_removing(
        &[5, 3, 7],
        &[0, 4, 2],
        &mut TakesFrom {
            sizes: &[5, 3, 7],
            from: 1,
            each: 3,
            most: 3,
            seen: |hist: h::Path<'_>, p: f64| with.push((hist[2][0], hist[2][2], p)),
        },
    );
    let mut without = Vec::new();
    h::for_each_checkpoint_path(&[5, 7], &[0, 4, 2], |hist, p| {
        without.push((hist[2][0], hist[2][1], p))
    });
    assert_eq!(with.len(), without.len(), "the same paths, group 1 aside");
    for (a, b) in with.iter().zip(&without) {
        assert_eq!((a.0, a.1), (b.0, b.1));
        assert!(close(a.2, b.2, 1e-12), "{} vs {}", a.2, b.2);
    }
}

#[test]
fn removals_that_never_fire_leave_every_path_untouched() {
    // The guarantee every number already in this repository rests on: a walk
    // that removes nothing is the walk that could not.
    let groups = [12, 8, 79];
    let gaps = [7, 1, 1];
    let mut removing = Vec::new();
    h::for_each_checkpoint_path_removing(
        &groups,
        &gaps,
        &mut TakesFrom {
            sizes: &[12, 8, 79],
            from: 0,
            each: 0,
            most: 0,
            seen: |hist: h::Path<'_>, p: f64| removing.push((hist.last().unwrap().clone(), p)),
        },
    );
    let mut plain = Vec::new();
    h::for_each_checkpoint_path(&groups, &gaps, |hist, p| {
        plain.push((hist.last().unwrap().clone(), p))
    });
    assert_eq!(removing.len(), plain.len());
    for (a, b) in removing.iter().zip(&plain) {
        assert_eq!(a.0, b.0);
        assert_eq!(a.1.to_bits(), b.1.to_bits(), "bit for bit, not close to");
    }
}

#[test]
fn a_removal_leaves_the_mass_at_one() {
    // Every gap is still one multivariate hypergeometric over whatever is
    // left, so the paths still partition the sample space — conditioned on the
    // removals, which is the whole claim.
    for each in 0..=2u32 {
        let mut mass = h::KahanSum::new();
        h::for_each_checkpoint_path_removing(
            &[9, 6, 11],
            &[5, 2, 2, 1],
            &mut TakesFrom {
                sizes: &[9, 6, 11],
                from: 2,
                each,
                most: 6,
                seen: |_: h::Path<'_>, p: f64| mass.add(p),
            },
        );
        assert!(
            close(mass.total(), 1.0, 1e-12),
            "removing {each} a checkpoint summed to {}",
            mass.total()
        );
    }
}

#[test]
fn a_walk_resumed_after_its_first_checkpoint_is_the_same_walk() {
    // Splitting the walk at checkpoint 0 and weighting each resumed walk by the
    // chance of having reached that first checkpoint has to give back the full
    // walk's terms, one for one: the resumed histories are the same
    // histories, and the probabilities are the same conditioned on the start.
    let groups = [5, 3, 7];
    let gaps = [4, 1, 2];
    let mut full = Vec::new();
    h::for_each_checkpoint_path(&groups, &gaps, |hist, p| full.push((hist.to_vec(), p)));

    let mut split = Vec::new();
    h::for_each_composition(&groups, gaps[0], |first, p_first| {
        h::for_each_checkpoint_path_after(&groups, first, &gaps[1..], |hist, p| {
            split.push((hist.to_vec(), p_first * p))
        });
    });
    assert_eq!(full.len(), split.len());
    for (a, b) in full.iter().zip(&split) {
        assert_eq!(a.0, b.0, "the same history, in the same order");
        assert!(close(a.1, b.1, 1e-12), "{} vs {}", a.1, b.1);
    }
}

#[test]
fn a_resumed_walk_asks_for_its_removals_where_the_full_walk_does() {
    // The removing walk, split the same way: TakesFrom decides from the
    // history it is handed, so it has to be asked after checkpoint 0 exactly
    // as the full walk asks it, or the first gap is dealt out of the wrong
    // library.
    let mut full = Vec::new();
    h::for_each_checkpoint_path_removing(
        &[9, 6, 11],
        &[5, 2, 2],
        &mut TakesFrom {
            sizes: &[9, 6, 11],
            from: 2,
            each: 1,
            most: 6,
            seen: |hist: h::Path<'_>, p: f64| full.push((hist.to_vec(), p)),
        },
    );
    let mut split = Vec::new();
    h::for_each_composition(&[9, 6, 11], 5, |first, p_first| {
        h::for_each_checkpoint_path_removing_after(
            &[9, 6, 11],
            first,
            &[2, 2],
            &mut TakesFrom {
                sizes: &[9, 6, 11],
                from: 2,
                each: 1,
                most: 6,
                seen: |hist: h::Path<'_>, p: f64| split.push((hist.to_vec(), p_first * p)),
            },
        );
    });
    assert_eq!(full.len(), split.len());
    for (a, b) in full.iter().zip(&split) {
        assert_eq!(a.0, b.0);
        assert!(close(a.1, b.1, 1e-12), "{} vs {}", a.1, b.1);
    }
}

#[test]
fn a_walk_resumed_with_nothing_after_it_is_its_first_checkpoint() {
    let mut seen = Vec::new();
    h::for_each_checkpoint_path_after(&[3, 4], &[1, 2], &[], |hist, p| {
        seen.push((hist.to_vec(), p))
    });
    assert_eq!(seen, vec![(vec![vec![1, 2]], 1.0)]);
}

#[test]
fn a_resumed_walk_can_draw_every_card_left() {
    // Two cards left and two drawn: one path, certain. The guard against
    // over-drawing is about more than the deck holds, not as many as it holds,
    // and it counts what the first checkpoint already took as well as the gaps.
    let mut seen = Vec::new();
    h::for_each_checkpoint_path_after(&[2, 1], &[1, 0], &[2], |hist, p| {
        seen.push((hist.to_vec(), p))
    });
    assert_eq!(seen, vec![(vec![vec![1, 0], vec![2, 1]], 1.0)]);

    // Three taken, three more out of the four left: still a whole distribution,
    // and every path ends having drawn six of the seven.
    let mut mass = h::KahanSum::new();
    h::for_each_checkpoint_path_after(&[4, 3], &[3, 0], &[3], |hist, p| {
        assert_eq!(hist[1].iter().sum::<u32>(), 6);
        mass.add(p);
    });
    assert!(
        close(mass.total(), 1.0, 1e-12),
        "summed to {}",
        mass.total()
    );

    // One more than is left is nothing at all.
    let mut any = false;
    h::for_each_checkpoint_path_after(&[4, 3], &[3, 0], &[5], |_, _| any = true);
    assert!(!any);
}

#[test]
fn a_distribution_is_the_histogram_it_was_built_from() {
    // Checkable by hand: a quarter on 0, three quarters on 2, and 1 never
    // taken. The mean is 1.5 and the variance is 0.25 * 2.25 + 0.75 * 0.25.
    let mut builder = h::DistributionBuilder::new();
    builder.add(0, 0.25);
    builder.add(2, 0.5);
    builder.add(2, 0.25);
    let d = builder.build();
    assert_eq!(d.probabilities(), &[0.25, 0.0, 0.75]);
    assert!(close(d.total(), 1.0, 1e-15), "total {}", d.total());
    assert!(close(d.mean(), 1.5, 1e-15), "mean {}", d.mean());
    assert!(close(d.sd(), 0.75f64.sqrt(), 1e-15), "sd {}", d.sd());

    // And an empty one is empty rather than a point mass somewhere.
    let empty = h::DistributionBuilder::new().build();
    assert!(empty.probabilities().is_empty());
    assert_eq!(empty.total(), 0.0);
}

#[test]
fn a_cached_ln_choose_is_the_computed_one_bit_for_bit() {
    // The table is a cache of lgamma, not an approximation of it: every
    // number the enumeration ever printed has to come out the same.
    for n in [0u32, 1, 7, 36, 99, 100, 4094, 4095, 4096, 5000] {
        for k in [0, 1, n / 3, n / 2, n] {
            let (nf, kf) = (f64::from(n), f64::from(k));
            let computed =
                libm::lgamma(nf + 1.0) - libm::lgamma(kf + 1.0) - libm::lgamma(nf - kf + 1.0);
            assert_eq!(
                h::ln_choose(n, k).to_bits(),
                computed.to_bits(),
                "C({n}, {k})"
            );
        }
    }
}

// --- sized gaps -------------------------------------------------------------
//
// A draw whose size the path decides. Stated, as removals are, in this crate's
// own words: a walk that asks for more cards when a checkpoint dealt a card of
// some group, and for none otherwise. Nothing here knows why it asks.

/// Deals `size` more cards after the first checkpoint when that checkpoint
/// dealt at least one card of group 0, and nothing anywhere else.
struct MoreAfterGroupZero<F> {
    size: u32,
    seen: F,
}

impl<F: FnMut(h::Path<'_>, f64)> h::Walk for MoreAfterGroupZero<F> {
    fn removals(&mut self, _reached: h::Path<'_>, _out: &mut [u32]) {}
    fn gap(&mut self, reached: h::Path<'_>) -> u32 {
        if reached.len() == 1 && reached[0][0] >= 1 {
            self.size
        } else {
            0
        }
    }
    fn path(&mut self, reached: h::Path<'_>, p: f64) {
        (self.seen)(reached, p)
    }
}

/// A walk recording every path it is handed, with the sized gap `size` decides
/// from the history so far and `remove` deciding the removals.
struct Recording<'a, S, R> {
    size: S,
    remove: R,
    paths: &'a mut Vec<(Vec<Vec<u32>>, f64)>,
}

impl<S: FnMut(h::Path<'_>) -> u32, R: FnMut(h::Path<'_>, &mut [u32])> h::Walk
    for Recording<'_, S, R>
{
    fn removals(&mut self, reached: h::Path<'_>, out: &mut [u32]) {
        (self.remove)(reached, out)
    }
    fn gap(&mut self, reached: h::Path<'_>) -> u32 {
        (self.size)(reached)
    }
    fn path(&mut self, reached: h::Path<'_>, p: f64) {
        self.paths.push((reached.to_vec(), p))
    }
}

fn removes_nothing(_: h::Path<'_>, _: &mut [u32]) {}

#[test]
fn a_sized_gap_is_dealt_where_the_path_asks_for_it() {
    // Two groups of two, A and B. Deal one; if it was an A, deal two more;
    // then deal one.
    //
    //   A (1/2): two of [1 A, 2 B] is AB (2/3) or BB (1/3).
    //            AB leaves one B for the last card, BB leaves one A.
    //            [1,0] [2,1] [2,2]  1/2 * 2/3 = 1/3
    //            [1,0] [1,2] [2,2]  1/2 * 1/3 = 1/6
    //   B (1/2): nothing more is dealt, and the last card is out of [2 A, 1 B].
    //            [0,1] [1,1]        1/2 * 2/3 = 1/3
    //            [0,1] [0,2]        1/2 * 1/3 = 1/6
    let mut paths = Vec::new();
    h::for_each_checkpoint_path_sized(
        &[2, 2],
        &[1, 1],
        &mut MoreAfterGroupZero {
            size: 2,
            seen: |hist: h::Path<'_>, p: f64| paths.push((hist.to_vec(), p)),
        },
    );
    // In the walk's order: a group's smaller share first.
    let expected: Vec<(Vec<Vec<u32>>, f64)> = vec![
        (vec![vec![0, 1], vec![0, 2]], 1.0 / 6.0),
        (vec![vec![0, 1], vec![1, 1]], 1.0 / 3.0),
        (vec![vec![1, 0], vec![1, 2], vec![2, 2]], 1.0 / 6.0),
        (vec![vec![1, 0], vec![2, 1], vec![2, 2]], 1.0 / 3.0),
    ];
    assert_eq!(paths.len(), expected.len(), "{paths:?}");
    for (a, b) in paths.iter().zip(&expected) {
        assert_eq!(a.0, b.0);
        assert!(close(a.1, b.1, 1e-12), "{:?}: {} vs {}", a.0, a.1, b.1);
    }
    // At least one A by the end: 1/3 + 1/6 + 1/3.
    let held: f64 = paths
        .iter()
        .filter(|(hist, _)| hist.last().unwrap()[0] >= 1)
        .map(|(_, p)| p)
        .sum();
    assert!(close(held, 5.0 / 6.0, 1e-12), "was {held}");
}

#[test]
fn a_sized_gap_of_zero_leaves_every_path_untouched() {
    // A walk that never sizes a gap is the plain walk, bit for bit: a path on
    // which nothing fired pays nothing.
    let groups = [12, 8, 79];
    let gaps = [7, 1, 1];
    let mut sized = Vec::new();
    h::for_each_checkpoint_path_sized(
        &groups,
        &gaps,
        &mut MoreAfterGroupZero {
            size: 0,
            seen: |hist: h::Path<'_>, p: f64| sized.push((hist.to_vec(), p)),
        },
    );
    let mut plain = Vec::new();
    h::for_each_checkpoint_path(&groups, &gaps, |hist, p| plain.push((hist.to_vec(), p)));
    assert_eq!(sized.len(), plain.len());
    for (a, b) in sized.iter().zip(&plain) {
        assert_eq!(a.0, b.0);
        assert_eq!(a.1.to_bits(), b.1.to_bits(), "bit for bit, not close to");
    }
}

#[test]
fn a_sized_gap_every_path_takes_is_a_fixed_gap() {
    // Asking for two after the first checkpoint on every path is the fixed
    // schedule [7, 2, 1], term for term.
    let mut sized = Vec::new();
    h::for_each_checkpoint_path_sized(
        &[5, 3, 7],
        &[7, 1],
        &mut Recording {
            size: |reached: h::Path<'_>| if reached.len() == 1 { 2 } else { 0 },
            remove: removes_nothing,
            paths: &mut sized,
        },
    );
    let mut fixed = Vec::new();
    h::for_each_checkpoint_path(&[5, 3, 7], &[7, 2, 1], |hist, p| {
        fixed.push((hist.to_vec(), p))
    });
    assert_eq!(sized.len(), fixed.len());
    for (a, b) in sized.iter().zip(&fixed) {
        assert_eq!(a.0, b.0);
        assert!(close(a.1, b.1, 1e-12), "{} vs {}", a.1, b.1);
    }
}

#[test]
fn a_sized_gap_is_asked_for_again_after_it_is_dealt() {
    // A gap can follow a gap: deal one card at a time, from [1, 3], until the
    // one card of group 0 turns up. One path per position it can be in, each
    // 1/4, and the history is as long as the card was deep.
    let mut paths = Vec::new();
    h::for_each_checkpoint_path_sized(
        &[1, 3],
        &[0],
        &mut Recording {
            size: |reached: h::Path<'_>| u32::from(reached.last().unwrap()[0] == 0),
            remove: removes_nothing,
            paths: &mut paths,
        },
    );
    let lengths: Vec<usize> = paths.iter().map(|(hist, _)| hist.len()).collect();
    assert_eq!(lengths, vec![5, 4, 3, 2], "{paths:?}");
    for (_, p) in &paths {
        assert!(close(*p, 0.25, 1e-12), "was {p}");
    }
}

#[test]
fn a_sized_gap_is_dealt_after_the_removals_decided_at_the_same_checkpoint() {
    // Two groups of two. Deal one. If it was group 0, the other card of group
    // 0 is removed and one more card is dealt, which can then only be group 1.
    //
    //   group 0 (1/2): [1,0] [1,1]
    //   group 1 (1/2): [0,1]
    let mut paths = Vec::new();
    h::for_each_checkpoint_path_sized(
        &[2, 2],
        &[1],
        &mut Recording {
            size: |reached: h::Path<'_>| u32::from(reached.len() == 1 && reached[0][0] == 1),
            remove: |reached: h::Path<'_>, out: &mut [u32]| out[0] = reached[0][0],
            paths: &mut paths,
        },
    );
    let histories: Vec<&Vec<Vec<u32>>> = paths.iter().map(|(hist, _)| hist).collect();
    assert_eq!(
        histories,
        vec![&vec![vec![0, 1]], &vec![vec![1, 0], vec![1, 1]]]
    );
    for (_, p) in &paths {
        assert!(close(*p, 0.5, 1e-12), "was {p}");
    }
}

#[test]
fn a_sized_gap_never_deals_more_than_the_library_holds() {
    // Three cards, one dealt, and a gap of five asked for after it: the gap
    // deals the two that are left rather than losing the path.
    let mut paths = Vec::new();
    h::for_each_checkpoint_path_sized(
        &[1, 2],
        &[1],
        &mut Recording {
            size: |reached: h::Path<'_>| if reached.len() == 1 { 5 } else { 0 },
            remove: removes_nothing,
            paths: &mut paths,
        },
    );
    let mass: f64 = paths.iter().map(|(_, p)| p).sum();
    assert!(close(mass, 1.0, 1e-12), "mass {mass}");
    assert!(paths.iter().all(|(hist, _)| hist[1] == vec![1, 2]));
}

#[test]
fn a_sized_walk_resumed_after_its_first_checkpoint_is_the_same_walk() {
    let groups = [2, 2];
    let mut full = Vec::new();
    h::for_each_checkpoint_path_sized(
        &groups,
        &[1, 1],
        &mut MoreAfterGroupZero {
            size: 2,
            seen: |hist: h::Path<'_>, p: f64| full.push((hist.to_vec(), p)),
        },
    );
    let mut split = Vec::new();
    h::for_each_composition(&groups, 1, |first, p_first| {
        h::for_each_checkpoint_path_sized_after(
            &groups,
            first,
            &[1],
            &mut MoreAfterGroupZero {
                size: 2,
                seen: |hist: h::Path<'_>, p: f64| split.push((hist.to_vec(), p_first * p)),
            },
        );
    });
    assert_eq!(full.len(), split.len());
    for (a, b) in full.iter().zip(&split) {
        assert_eq!(a.0, b.0);
        assert!(close(a.1, b.1, 1e-12), "{} vs {}", a.1, b.1);
    }
}

#[test]
fn the_paths_of_a_sized_walk_are_counted_without_walking_them() {
    // The four paths of the first sized example, and a cap that stops the
    // count where a caller would refuse anyway.
    let mut walk = MoreAfterGroupZero {
        size: 2,
        seen: |_: h::Path<'_>, _: f64| panic!("counting hands out no path"),
    };
    assert_eq!(
        h::count_checkpoint_paths_sized(&[2, 2], &[1, 1], &mut walk, 100),
        4
    );
    assert_eq!(
        h::count_checkpoint_paths_sized(&[2, 2], &[1, 1], &mut walk, 3),
        3
    );
    // With nothing sized, it is the number of paths the plain walk hands out.
    let mut plain = 0u128;
    h::for_each_checkpoint_path(&[12, 8, 79], &[7, 1, 1], |_, _| plain += 1);
    let mut never = MoreAfterGroupZero {
        size: 0,
        seen: |_: h::Path<'_>, _: f64| {},
    };
    assert_eq!(
        h::count_checkpoint_paths_sized(&[12, 8, 79], &[7, 1, 1], &mut never, u128::MAX),
        plain
    );
}
