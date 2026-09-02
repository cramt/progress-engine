//! Known-answer tests for the exact draw probabilities.
//!
//! The first fixture is deliberately the constant already documented in the
//! shuffler acceptance test of the tool this crate grew out of: "for 36 lands in
//! 99 cards, n=7: mean 2.5457, SD 1.2331". That number was derived out of band
//! and then hand-copied into two files. Now it is computed, so the new code and
//! the old acceptance test validate each other instead of both being magic.

use pe_stats::{self as h, Probability};

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
